//! Lectura directa de Overture Places vía PMTiles (HTTP Range).
//!
//! Overture publica un archivo PMTiles de lugares. No hace falta descargar el
//! archivo completo: se leen el directorio y solo los tiles que cubren el bbox
//! solicitado. Así funciona incluso en Termux y sin DuckDB.

use std::collections::HashSet;
use std::io::{Cursor, Read};
use std::time::Instant;

use anyhow::{bail, Context, Result};
use bytes::Bytes;
use flate2::read::GzDecoder;
use mapbox_vector_tile::Tile;
use serde_json::{json, Map, Value};
use url::Url;

use crate::http::HttpClient;
use crate::location::{GeoFilter, ResolvedLocation};
use crate::model::BusinessRecord;
use crate::normalize::Bbox;
use crate::providers::overture::{feature_to_record, in_location};

const STAC_CATALOG: &str = "https://stac.overturemaps.org/catalog.json";
const PMTILES_BASE: &str = "https://tiles.overturemaps.org";
const MAX_DIRECTORY_BYTES: u64 = 32 * 1024 * 1024;
const MAX_TILE_BYTES: u64 = 64 * 1024 * 1024;

#[derive(Debug, Clone)]
pub struct FetchOptions {
    pub location: ResolvedLocation,
    pub bbox: Bbox,
    pub zoom: Option<u8>,
    pub max_tiles: usize,
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct FetchStats {
    pub zoom: u8,
    pub tiles: usize,
    pub fetched_tiles: usize,
    pub features: usize,
    pub records: usize,
    pub duration_ms: u64,
}

pub async fn latest_release(http: &HttpClient) -> Result<String> {
    let catalog = http
        .json::<Value, _>(|| {
            http.raw()
                .get(STAC_CATALOG)
                .header(reqwest::header::ACCEPT, "application/json")
        })
        .await
        .context("No se pudo consultar el catálogo STAC de Overture")?;
    let mut latest: Option<String> = None;
    if let Some(links) = catalog.get("links").and_then(Value::as_array) {
        for link in links {
            if link.get("rel").and_then(Value::as_str) != Some("child") {
                continue;
            }
            if link.get("latest").and_then(Value::as_bool) != Some(true) {
                continue;
            }
            if let Some(title) = link.get("title").and_then(Value::as_str) {
                latest = title.split_whitespace().next().map(ToString::to_string);
            }
            if latest.is_none() {
                if let Some(href) = link.get("href").and_then(Value::as_str) {
                    latest = href
                        .trim_end_matches('/')
                        .split('/')
                        .nth(3)
                        .map(ToString::to_string);
                }
            }
        }
    }
    latest.context("No se encontró una release marcada como latest en Overture")
}

pub fn pmtiles_url(release: &str) -> String {
    format!("{PMTILES_BASE}/{release}/places.pmtiles")
}

pub async fn fetch_records(
    http: &HttpClient,
    pmtiles_url: &str,
    options: &FetchOptions,
) -> Result<(Vec<BusinessRecord>, FetchStats)> {
    let started = Instant::now();
    let mut archive = PmtilesArchive::open(http.raw().clone(), pmtiles_url).await?;
    let zoom = choose_zoom(
        &options.bbox,
        options.zoom,
        archive.header.max_zoom,
        options.max_tiles,
    )?;
    let (min_x, max_x, min_y, max_y) = tile_range(&options.bbox, zoom);
    let tiles: Vec<(u32, u32)> = (min_y..=max_y)
        .flat_map(|y| (min_x..=max_x).map(move |x| (x, y)))
        .collect();

    if tiles.len() > options.max_tiles {
        bail!(
            "El área requiere {} tiles en zoom {zoom} y el máximo es {}. Usa un zoom menor, un área más pequeña o sube --max-tiles",
            tiles.len(),
            options.max_tiles
        );
    }

    let mut records = Vec::new();
    let mut seen = HashSet::new();
    let mut features = 0usize;
    let mut fetched_tiles = 0usize;

    for (x, y) in &tiles {
        let Some(tile_bytes) = archive.get_tile(zoom, *x, *y).await? else {
            continue;
        };
        fetched_tiles += 1;
        let tile = match Tile::from_uncompressed_bytes(&tile_bytes) {
            Ok(tile) => tile,
            Err(error) => {
                log::warn!("Tile {zoom}/{x}/{y} inválido: {error}");
                continue;
            }
        };
        let has_place_layer = tile.layers.iter().any(|layer| layer.name == "place");
        for layer in &tile.layers {
            if has_place_layer && layer.name != "place" {
                continue;
            }
            let extent = layer.extent.max(1) as f64;
            for feature in &layer.features {
                features += 1;
                let Some(point) = feature.get_point() else {
                    continue;
                };
                let (lon, lat) = tile_point_to_lonlat(zoom, *x, *y, point.x(), point.y(), extent);
                if !in_location(lon, lat, &options.location) {
                    continue;
                }
                let properties = properties_to_json(feature.properties.0.iter());
                let feature_json = json!({
                    "type": "Feature",
                    "geometry": {"type": "Point", "coordinates": [lon, lat]},
                    "properties": properties,
                });
                let Some(mut record) = feature_to_record(&feature_json, lon, lat) else {
                    continue;
                };
                if record.address.country_code.is_none() {
                    record.address.country_code = options.location.country_code.clone();
                }
                let key = record
                    .external_id
                    .clone()
                    .unwrap_or_else(|| record.name.clone());
                if !seen.insert(key) {
                    continue;
                }
                records.push(record);
                if let Some(limit) = options.limit {
                    if records.len() >= limit {
                        break;
                    }
                }
            }
            if let Some(limit) = options.limit {
                if records.len() >= limit {
                    break;
                }
            }
        }
        if let Some(limit) = options.limit {
            if records.len() >= limit {
                break;
            }
        }
    }

    let stats = FetchStats {
        zoom,
        tiles: tiles.len(),
        fetched_tiles,
        features,
        records: records.len(),
        duration_ms: started.elapsed().as_millis() as u64,
    };
    Ok((records, stats))
}

/// Filtra un bbox exacto cuando el usuario dio coordenadas; para un área usa
/// el bbox resuelto por Nominatim.
pub fn exact_bbox(location: &ResolvedLocation) -> Option<Bbox> {
    match &location.filter {
        GeoFilter::Around { lat, lon, radius_m } => Some(Bbox::around(*lat, *lon, *radius_m)),
        _ => location.bbox.map(|bbox| bbox.to_bbox()),
    }
}

fn choose_zoom(bbox: &Bbox, requested: Option<u8>, max_zoom: u8, max_tiles: usize) -> Result<u8> {
    let zoom = requested.unwrap_or(max_zoom).clamp(1, max_zoom.max(1));
    let (min_x, max_x, min_y, max_y) = tile_range(bbox, zoom);
    let count = ((max_x - min_x + 1) as usize) * ((max_y - min_y + 1) as usize);
    if count > max_tiles {
        let estimated_mb = (count * 300) / 1024;
        bail!(
            "El área necesita {count} tiles en zoom {zoom} (~{estimated_mb} MB) y --max-tiles es {max_tiles}.              Reduce el área, sube --max-tiles o usa un extracto local con `scan --overture-file`.              El archivo PMTiles de Overture solo tiene datos en zoom 14."
        );
    }
    Ok(zoom)
}

fn tile_range(bbox: &Bbox, z: u8) -> (u32, u32, u32, u32) {
    let n = (1u32 << z).max(1);
    let min_x = lon_to_tile_x(bbox.west, z).min(n - 1);
    let max_x = lon_to_tile_x(bbox.east, z).min(n - 1);
    let min_y = lat_to_tile_y(bbox.north, z).min(n - 1);
    let max_y = lat_to_tile_y(bbox.south, z).min(n - 1);
    (min_x, max_x, min_y, max_y)
}

fn lon_to_tile_x(lon: f64, z: u8) -> u32 {
    let n = (1u32 << z) as f64;
    (((lon + 180.0) / 360.0) * n).floor().clamp(0.0, n - 1.0) as u32
}

fn lat_to_tile_y(lat: f64, z: u8) -> u32 {
    let n = (1u32 << z) as f64;
    let lat = lat.to_radians().clamp(-85.051_128_78, 85.051_128_78);
    let y = ((1.0 - ((lat.tan() + 1.0 / lat.cos()).ln() / std::f64::consts::PI)) / 2.0) * n;
    y.floor().clamp(0.0, n - 1.0) as u32
}

fn tile_point_to_lonlat(z: u8, x: u32, y: u32, px: i32, py: i32, extent: f64) -> (f64, f64) {
    let n = (1u32 << z) as f64;
    let tx = (x as f64 + (px as f64 / extent)) / n;
    let ty = (y as f64 + (py as f64 / extent)) / n;
    let lon = tx * 360.0 - 180.0;
    let lat = (std::f64::consts::PI * (1.0 - 2.0 * ty))
        .sinh()
        .atan()
        .to_degrees();
    (lon, lat)
}

fn properties_to_json<'a>(
    values: impl Iterator<Item = (&'a std::rc::Rc<String>, &'a mapbox_vector_tile::Value)>,
) -> Map<String, Value> {
    let mut map = Map::new();
    for (key, value) in values {
        map.insert(key.to_string(), mvt_value_to_json(value));
    }
    map
}

fn mvt_value_to_json(value: &mapbox_vector_tile::Value) -> Value {
    use mapbox_vector_tile::Value as MvtValue;
    match value {
        MvtValue::String(value) => {
            let text = value.trim();
            if (text.starts_with('{') && text.ends_with('}'))
                || (text.starts_with('[') && text.ends_with(']'))
            {
                serde_json::from_str(text).unwrap_or_else(|_| Value::String(text.to_string()))
            } else {
                Value::String(text.to_string())
            }
        }
        MvtValue::Float(value) => json!(value),
        MvtValue::Double(value) => json!(value),
        MvtValue::Int(value) | MvtValue::SInt(value) => json!(value),
        MvtValue::UInt(value) => json!(value),
        MvtValue::Boolean(value) => json!(value),
        MvtValue::Unknown => Value::Null,
    }
}

#[derive(Debug, Clone)]
struct PmtilesHeader {
    root_offset: u64,
    root_length: u64,
    leaf_offset: u64,
    tile_offset: u64,
    internal_compression: u8,
    tile_compression: u8,
    min_zoom: u8,
    max_zoom: u8,
}

struct PmtilesArchive {
    client: reqwest::Client,
    url: String,
    header: PmtilesHeader,
    root: Vec<DirEntry>,
}

impl PmtilesArchive {
    async fn open(client: reqwest::Client, url: &str) -> Result<Self> {
        let parsed = Url::parse(url).context("URL de PMTiles inválida")?;
        if parsed.scheme() != "https" && parsed.scheme() != "http" {
            bail!("PMTiles debe ser HTTP/HTTPS");
        }
        let header_bytes = read_range(&client, url, 0, 127).await?;
        let header = parse_header(&header_bytes)?;
        if header.root_length == 0 || header.root_length > MAX_DIRECTORY_BYTES {
            bail!("Directorio raíz PMTiles inválido");
        }
        let root_bytes = read_range(&client, url, header.root_offset, header.root_length).await?;
        let root = parse_directory(&decompress(&root_bytes, header.internal_compression)?)?;
        log::debug!(
            "PMTiles {}: {} entradas raíz, zoom {}–{}",
            url,
            root.len(),
            header.min_zoom,
            header.max_zoom
        );
        Ok(Self {
            client,
            url: url.to_string(),
            header,
            root,
        })
    }

    async fn get_tile(&mut self, z: u8, x: u32, y: u32) -> Result<Option<Vec<u8>>> {
        let tile_id = tile_id(z, x, y);
        let mut entries = self.root.clone();
        for _ in 0..4 {
            let Some(entry) = find_entry(&entries, tile_id) else {
                return Ok(None);
            };
            if entry.run == 0 {
                let start = self
                    .header
                    .leaf_offset
                    .checked_add(entry.offset)
                    .context("Offset de directorio desbordado")?;
                let length = entry.length;
                if length == 0 || length > MAX_DIRECTORY_BYTES {
                    bail!("Directorio hoja PMTiles inválido");
                }
                let bytes = read_range(&self.client, &self.url, start, length).await?;
                entries = parse_directory(&decompress(&bytes, self.header.internal_compression)?)?;
                continue;
            }
            let start = self
                .header
                .tile_offset
                .checked_add(entry.offset)
                .context("Offset de tile desbordado")?;
            if entry.length == 0 || entry.length > MAX_TILE_BYTES {
                bail!("Tile PMTiles inválido");
            }
            let bytes = read_range(&self.client, &self.url, start, entry.length).await?;
            return Ok(Some(decompress(&bytes, self.header.tile_compression)?));
        }
        bail!("Demasiados niveles de directorios PMTiles")
    }
}

async fn read_range(
    client: &reqwest::Client,
    url: &str,
    start: u64,
    length: u64,
) -> Result<Vec<u8>> {
    if length == 0 {
        return Ok(Vec::new());
    }
    let end = start
        .checked_add(length - 1)
        .context("Rango PMTiles desbordado")?;
    let response = client
        .get(url)
        .header(reqwest::header::RANGE, format!("bytes={start}-{end}"))
        .timeout(std::time::Duration::from_secs(60))
        .send()
        .await
        .with_context(|| format!("Error leyendo PMTiles {url}"))?;
    if response.status() != reqwest::StatusCode::PARTIAL_CONTENT {
        bail!(
            "El servidor PMTiles no soporta Range (HTTP {})",
            response.status()
        );
    }
    let bytes: Bytes = response.bytes().await?;
    if bytes.len() as u64 != length {
        bail!(
            "Rango PMTiles incompleto: esperaba {length} bytes, recibió {}",
            bytes.len()
        );
    }
    Ok(bytes.to_vec())
}

fn parse_header(bytes: &[u8]) -> Result<PmtilesHeader> {
    if bytes.len() < 127 || &bytes[..7] != b"PMTiles" || bytes[7] != 3 {
        bail!("Cabecera PMTiles inválida");
    }
    let val = |offset: usize| -> u64 {
        let mut data = [0u8; 8];
        data.copy_from_slice(&bytes[offset..offset + 8]);
        u64::from_le_bytes(data)
    };
    Ok(PmtilesHeader {
        root_offset: val(8),
        root_length: val(16),
        leaf_offset: val(40),
        tile_offset: val(56),
        internal_compression: bytes[97],
        tile_compression: bytes[98],
        min_zoom: bytes[100],
        max_zoom: bytes[101],
    })
}

#[derive(Debug, Clone, Copy)]
struct DirEntry {
    tile_id: u64,
    run: u64,
    length: u64,
    offset: u64,
}

fn parse_directory(bytes: &[u8]) -> Result<Vec<DirEntry>> {
    let mut index = 0usize;
    let count = read_varint(bytes, &mut index)? as usize;
    if count == 0 || count > 1_000_000 {
        bail!("Directorio PMTiles con {count} entradas");
    }
    let mut ids = Vec::with_capacity(count);
    let mut last = 0u64;
    for _ in 0..count {
        let delta = read_varint(bytes, &mut index)?;
        last = last
            .checked_add(delta)
            .context("TileID PMTiles desbordado")?;
        ids.push(last);
    }
    let mut runs = Vec::with_capacity(count);
    for _ in 0..count {
        runs.push(read_varint(bytes, &mut index)?);
    }
    let mut lengths = Vec::with_capacity(count);
    for _ in 0..count {
        lengths.push(read_varint(bytes, &mut index)?);
    }
    let mut offsets = Vec::with_capacity(count);
    let mut previous_offset = 0u64;
    let mut previous_length = 0u64;
    for _ in 0..count {
        let encoded = read_varint(bytes, &mut index)?;
        let offset = if encoded == 0 {
            previous_offset
                .checked_add(previous_length)
                .context("Offset PMTiles desbordado")?
        } else {
            encoded - 1
        };
        offsets.push(offset);
        previous_offset = offset;
        previous_length = lengths[offsets.len() - 1];
    }
    Ok((0..count)
        .map(|i| DirEntry {
            tile_id: ids[i],
            run: runs[i],
            length: lengths[i],
            offset: offsets[i],
        })
        .collect())
}

fn read_varint(bytes: &[u8], index: &mut usize) -> Result<u64> {
    let mut value = 0u64;
    let mut shift = 0u32;
    loop {
        let byte = *bytes.get(*index).context("Varint PMTiles truncado")?;
        *index += 1;
        value |= u64::from(byte & 0x7f) << shift;
        if byte & 0x80 == 0 {
            return Ok(value);
        }
        shift += 7;
        if shift > 63 {
            bail!("Varint PMTiles demasiado largo");
        }
    }
}

fn decompress(data: &[u8], compression: u8) -> Result<Vec<u8>> {
    match compression {
        0 | 1 => Ok(data.to_vec()),
        2 => {
            let mut decoder = GzDecoder::new(data);
            let mut out = Vec::new();
            decoder.read_to_end(&mut out)?;
            Ok(out)
        }
        3 => bail!("Compresión brotli no soportada en PMTiles"),
        4 => Ok(zstd::decode_all(Cursor::new(data))?),
        other => bail!("Compresión PMTiles desconocida: {other}"),
    }
}

fn find_entry(entries: &[DirEntry], tile_id: u64) -> Option<DirEntry> {
    let mut leaf: Option<DirEntry> = None;
    for entry in entries {
        if entry.tile_id > tile_id {
            break;
        }
        if entry.run > 0 {
            if tile_id < entry.tile_id.saturating_add(entry.run) {
                return Some(*entry);
            }
        } else {
            leaf = Some(*entry);
        }
    }
    leaf
}

fn tile_id(z: u8, x: u32, y: u32) -> u64 {
    let base = ((1u64 << (2 * z)) - 1) / 3;
    base + hilbert_index(x as u64, y as u64, z)
}

fn hilbert_index(mut x: u64, mut y: u64, z: u8) -> u64 {
    let mut d = 0u64;
    let mut s = 1u64 << z.saturating_sub(1);
    while s > 0 {
        let rx = u64::from((x & s) > 0);
        let ry = u64::from((y & s) > 0);
        d += s * s * ((3 * rx) ^ ry);
        if ry == 0 {
            if rx == 1 {
                x = s.wrapping_sub(1).wrapping_sub(x);
                y = s.wrapping_sub(1).wrapping_sub(y);
            }
            std::mem::swap(&mut x, &mut y);
        }
        s >>= 1;
    }
    d
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hilbert_matches_known_ids() {
        assert_eq!(tile_id(0, 0, 0), 0);
        assert_eq!(tile_id(1, 0, 0), 1);
        assert_eq!(tile_id(1, 0, 1), 2);
        assert_eq!(tile_id(1, 1, 1), 3);
        assert_eq!(tile_id(1, 1, 0), 4);
        assert_eq!(tile_id(14, 4266, 7635), 138_867_488);
    }

    #[test]
    fn parses_directory_layout() {
        // 2 entradas: id 5/42, run 1/3, len 42/7, offsets 1337/0 (contiguo)
        let bytes = [
            2, // count
            5, 37, // ids
            1, 3, // runs
            42, 7, // lengths
            0, 0, // offsets: 0 => previous+length (1336? first previous 0)
        ];
        let entries = parse_directory(&bytes).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].tile_id, 5);
        assert_eq!(entries[1].tile_id, 42);
        assert_eq!(entries[0].offset, 0);
        assert_eq!(entries[1].offset, 42);
    }

    #[test]
    fn maps_tile_point_to_managua() {
        let (lon, lat) = tile_point_to_lonlat(14, 4266, 7635, 2048, 2048, 4096.0);
        assert!((lon - -86.25).abs() < 0.1, "lon {lon}");
        assert!((lat - 12.13).abs() < 0.1, "lat {lat}");
    }

    #[test]
    fn converts_mvt_json_strings() {
        let value =
            mapbox_vector_tile::Value::String(std::rc::Rc::new(r#"["+50588887777"]"#.to_string()));
        assert_eq!(mvt_value_to_json(&value), json!(["+50588887777"]));
    }
}
