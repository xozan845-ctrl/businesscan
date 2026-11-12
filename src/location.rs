//! Resolución de ubicaciones (departamento, municipio, barrio, comarca, radio,
//! bbox o país) a un filtro que Overpass y los demás proveedores entienden.

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};

use crate::config::Config;
use crate::gazetteer::Gazetteer;
use crate::http::{HttpClient, RateLimiter};
use crate::normalize::{normalize_text, Bbox};

const NICARAGUA_RELATION_ID: i64 = 287_666;
const NICARAGUA_AREA_ID: i64 = 3_600_287_666;

#[derive(Debug, Clone, Default)]
pub struct LocationInput {
    pub place: Option<String>,
    pub departamento: Option<String>,
    pub municipio: Option<String>,
    pub barrio: Option<String>,
    pub comarca: Option<String>,
    pub lat: Option<f64>,
    pub lon: Option<f64>,
    pub radius_m: Option<f64>,
    pub bbox: Option<String>,
    pub all_nicaragua: bool,
    pub osm_area_id: Option<i64>,
}

impl LocationInput {
    pub fn is_empty(&self) -> bool {
        self.place.is_none()
            && self.departamento.is_none()
            && self.municipio.is_none()
            && self.barrio.is_none()
            && self.comarca.is_none()
            && self.lat.is_none()
            && self.bbox.is_none()
            && !self.all_nicaragua
            && self.osm_area_id.is_none()
    }

    pub fn cache_key(&self) -> String {
        let parts: Vec<String> = vec![
            self.place.clone().unwrap_or_default(),
            self.departamento.clone().unwrap_or_default(),
            self.municipio.clone().unwrap_or_default(),
            self.barrio.clone().unwrap_or_default(),
            self.comarca.clone().unwrap_or_default(),
            self.lat.map(|v| format!("{v:.6}")).unwrap_or_default(),
            self.lon.map(|v| format!("{v:.6}")).unwrap_or_default(),
            self.radius_m.map(|v| format!("{v:.0}")).unwrap_or_default(),
            self.bbox.clone().unwrap_or_default(),
            if self.all_nicaragua {
                "all".to_string()
            } else {
                String::new()
            },
            self.osm_area_id.map(|v| v.to_string()).unwrap_or_default(),
        ];
        format!("v1|{}", normalize_text(&parts.join("|")))
    }

    pub fn validate(&self) -> Result<()> {
        if self.lat.is_some() != self.lon.is_some() {
            bail!("--lat y --lon deben usarse juntos");
        }
        if let Some(lat) = self.lat {
            if !(-90.0..=90.0).contains(&lat) {
                bail!("--lat fuera de rango (-90..90)");
            }
        }
        if let Some(lon) = self.lon {
            if !(-180.0..=180.0).contains(&lon) {
                bail!("--lon fuera de rango (-180..180)");
            }
        }
        if let Some(radius) = self.radius_m {
            if radius <= 0.0 {
                bail!("--radius debe ser mayor que cero");
            }
        }
        if self.bbox.is_some() && Bbox::parse(self.bbox.as_deref().unwrap()).is_none() {
            bail!("--bbox inválido; usa minLat,minLon,maxLat,maxLon");
        }
        if self.all_nicaragua
            && (self.place.is_some()
                || self.departamento.is_some()
                || self.municipio.is_some()
                || self.barrio.is_some()
                || self.comarca.is_some()
                || self.lat.is_some()
                || self.bbox.is_some())
        {
            bail!("--all no se combina con --place, --departamento, coordenadas ni bbox");
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub struct GeoPoint {
    pub lat: f64,
    pub lon: f64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub struct GeoBox {
    pub south: f64,
    pub west: f64,
    pub north: f64,
    pub east: f64,
}

impl From<Bbox> for GeoBox {
    fn from(value: Bbox) -> Self {
        Self {
            south: value.south,
            west: value.west,
            north: value.north,
            east: value.east,
        }
    }
}

impl GeoBox {
    pub fn to_bbox(self) -> Bbox {
        Bbox {
            south: self.south,
            west: self.west,
            north: self.north,
            east: self.east,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum GeoFilter {
    /// Área administrativa de OSM (`area(3600000000 + relation_id)`).
    Area {
        relation_id: i64,
    },
    Around {
        lat: f64,
        lon: f64,
        radius_m: f64,
    },
    Bbox {
        south: f64,
        west: f64,
        north: f64,
        east: f64,
    },
    Country,
    /// ID de área de Overpass explícito (avanzado).
    OverpassArea {
        area_id: i64,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolvedLocation {
    /// Texto canónico que se envía a proveedores que aceptan lenguaje natural.
    pub query: String,
    pub display_name: String,
    pub departamento: Option<String>,
    pub municipio: Option<String>,
    pub barrio: Option<String>,
    pub comarca: Option<String>,
    pub filter: GeoFilter,
    pub center: Option<GeoPoint>,
    pub bbox: Option<GeoBox>,
    pub radius_m: Option<f64>,
    pub osm_type: Option<String>,
    pub osm_id: Option<i64>,
    pub source: String,
}

impl ResolvedLocation {
    pub fn provider_query(&self) -> String {
        if normalize_text(&self.query).contains("nicaragua") {
            self.query.clone()
        } else {
            format!("{}, Nicaragua", self.query)
        }
    }

    pub fn short_label(&self) -> String {
        self.display_name.clone()
    }

    pub fn overpass_area_id(&self) -> Option<i64> {
        match self.filter {
            GeoFilter::Area { relation_id } => Some(3_600_000_000 + relation_id),
            GeoFilter::OverpassArea { area_id } => Some(area_id),
            GeoFilter::Country => Some(NICARAGUA_AREA_ID),
            _ => None,
        }
    }
}

pub struct LocationResolver {
    http: HttpClient,
    rate: RateLimiter,
    offline: bool,
    language: String,
    nominatim_url: String,
}

impl LocationResolver {
    pub fn new(cfg: &Config, http: HttpClient) -> Self {
        Self {
            http,
            rate: RateLimiter::new(cfg.nominatim_interval),
            offline: cfg.offline,
            language: cfg.language.clone(),
            nominatim_url: cfg.nominatim_url.trim_end_matches('/').to_string(),
        }
    }

    pub async fn resolve(&self, input: &LocationInput) -> Result<ResolvedLocation> {
        input.validate()?;
        let gaz = Gazetteer::global();

        let departamento = canonical_department(gaz, input.departamento.as_deref())?;
        let municipio =
            canonical_municipality(gaz, input.municipio.as_deref(), departamento.as_deref())?;

        let mut hints = AdminHints {
            departamento,
            municipio,
            barrio: input.barrio.clone(),
            comarca: input.comarca.clone(),
        };

        // 1. Coordenadas explícitas: completa la administración con reverse
        // geocoding una sola vez (y queda en caché).
        if let (Some(lat), Some(lon)) = (input.lat, input.lon) {
            let radius = input.radius_m.unwrap_or(1_000.0);
            let bbox = Bbox::around(lat, lon, radius);
            let mut reverse_name: Option<String> = None;
            if !self.offline
                && (hints.departamento.is_none()
                    || hints.municipio.is_none()
                    || hints.barrio.is_none())
            {
                if let Ok(Some(place)) = self.reverse(lat, lon).await {
                    apply_address_hints(&mut hints, &place);
                    reverse_name = Some(place.display_name);
                }
            }
            let query = hints
                .display_label()
                .unwrap_or_else(|| format!("{lat:.5}, {lon:.5}"));
            let display_name = match reverse_name {
                Some(name) => format!("{name} · radio {radius:.0} m"),
                None => format!("{lat:.5}, {lon:.5} · radio {radius:.0} m"),
            };
            return Ok(ResolvedLocation {
                query,
                display_name,
                departamento: hints.departamento,
                municipio: hints.municipio,
                barrio: hints.barrio,
                comarca: hints.comarca,
                filter: GeoFilter::Around {
                    lat,
                    lon,
                    radius_m: radius,
                },
                center: Some(GeoPoint { lat, lon }),
                bbox: Some(bbox.into()),
                radius_m: Some(radius),
                osm_type: None,
                osm_id: None,
                source: "coordenadas".to_string(),
            });
        }

        // 2. Bounding box explícito.
        if let Some(raw) = input.bbox.as_deref() {
            let bbox = Bbox::parse(raw).context("bbox inválido")?;
            let center = GeoPoint {
                lat: (bbox.south + bbox.north) / 2.0,
                lon: (bbox.west + bbox.east) / 2.0,
            };
            let label = hints
                .display_label()
                .unwrap_or_else(|| format!("bbox {raw}"));
            return Ok(ResolvedLocation {
                query: label.clone(),
                display_name: label,
                departamento: hints.departamento,
                municipio: hints.municipio,
                barrio: hints.barrio,
                comarca: hints.comarca,
                filter: GeoFilter::Bbox {
                    south: bbox.south,
                    west: bbox.west,
                    north: bbox.north,
                    east: bbox.east,
                },
                center: Some(center),
                bbox: Some(bbox.into()),
                radius_m: None,
                osm_type: None,
                osm_id: None,
                source: "bbox".to_string(),
            });
        }

        // 3. ID de área de Overpass explícito.
        if let Some(area_id) = input.osm_area_id {
            let label = hints
                .display_label()
                .unwrap_or_else(|| format!("área Overpass {area_id}"));
            return Ok(ResolvedLocation {
                query: label.clone(),
                display_name: label,
                departamento: hints.departamento,
                municipio: hints.municipio,
                barrio: hints.barrio,
                comarca: hints.comarca,
                filter: GeoFilter::OverpassArea { area_id },
                center: None,
                bbox: None,
                radius_m: None,
                osm_type: None,
                osm_id: None,
                source: "overpass-area".to_string(),
            });
        }

        // 4. Todo el país.
        if input.all_nicaragua {
            return Ok(ResolvedLocation {
                query: "Nicaragua".to_string(),
                display_name: "Nicaragua (todo el país)".to_string(),
                departamento: None,
                municipio: None,
                barrio: None,
                comarca: None,
                filter: GeoFilter::Country,
                center: Some(GeoPoint {
                    lat: 12.865_416,
                    lon: -85.207_229,
                }),
                bbox: Some(GeoBox {
                    south: 10.707_394,
                    west: -87.895_817,
                    north: 15.090_278,
                    east: -82.476_725,
                }),
                radius_m: None,
                osm_type: Some("relation".to_string()),
                osm_id: Some(NICARAGUA_RELATION_ID),
                source: "país".to_string(),
            });
        }

        if input.is_empty() {
            bail!(
                "Indica una ubicación: --place, --departamento, --municipio, --lat/--lon, --bbox, --osm-area-id o --all"
            );
        }

        if normalize_text(input.place.as_deref().unwrap_or("")) == "nicaragua" {
            return Ok(country_location());
        }

        let query = build_query(input, &hints);
        if self.offline {
            bail!(
                "Modo offline: no puedo geocodificar «{query}». Usa --lat/--lon, --bbox o --osm-area-id"
            );
        }

        let mut candidates = self.nominatim(&query).await?;
        let wants_department = hints.departamento.is_some()
            && hints.municipio.is_none()
            && hints.barrio.is_none()
            && hints.comarca.is_none()
            && input.place.is_none();

        // Si se pidió un departamento pero el primer resultado es una ciudad,
        // reintenta con el nombre administrativo completo.
        if wants_department
            && !candidates
                .iter()
                .any(|c| c.addresstype == "state" || c.addresstype == "region")
        {
            if let Some(dept) = hints.departamento.as_deref() {
                let retry = format!("Departamento de {dept}, Nicaragua");
                log::debug!("Reintentando geocodificación: {retry}");
                if let Ok(more) = self.nominatim(&retry).await {
                    candidates = more;
                }
            }
        }

        if candidates.is_empty() {
            bail!("No se encontró «{query}» en Nicaragua. Prueba con un nombre más específico.");
        }

        let chosen = choose_candidate(&candidates, &hints, input);
        let mut resolved = self.location_from_candidate(chosen, &hints, input);
        // El nombre consultado suele ser más útil para el proveedor que el
        // display_name completo de Nominatim.
        if input.place.is_none() {
            resolved.query = hints.display_label().unwrap_or_else(|| query.clone());
        }
        Ok(resolved)
    }

    async fn nominatim(&self, query: &str) -> Result<Vec<NominatimPlace>> {
        self.rate.wait().await;
        let url = format!("{}/search", self.nominatim_url);
        let query_owned = query.to_string();
        let language = self.language.clone();
        let result = self
            .http
            .json::<Vec<NominatimPlace>, _>(|| {
                self.http
                    .raw()
                    .get(&url)
                    .header(reqwest::header::ACCEPT_LANGUAGE, &language)
                    .query(&[
                        ("format", "jsonv2"),
                        ("q", query_owned.as_str()),
                        ("addressdetails", "1"),
                        ("extratags", "1"),
                        ("countrycodes", "ni"),
                        ("dedupe", "1"),
                        ("limit", "8"),
                        ("accept-language", language.as_str()),
                    ])
            })
            .await
            .with_context(|| format!("Error consultando Nominatim para «{query}»"))?;
        Ok(result)
    }

    async fn reverse(&self, lat: f64, lon: f64) -> Result<Option<NominatimPlace>> {
        self.rate.wait().await;
        let url = format!("{}/reverse", self.nominatim_url);
        let language = self.language.clone();
        let value = self
            .http
            .json::<serde_json::Value, _>(|| {
                self.http
                    .raw()
                    .get(&url)
                    .header(reqwest::header::ACCEPT_LANGUAGE, &language)
                    .query(&[
                        ("format", "jsonv2"),
                        ("addressdetails", "1"),
                        ("zoom", "18"),
                        ("accept-language", language.as_str()),
                    ])
                    .query(&[("lat", lat), ("lon", lon)])
            })
            .await
            .context("Error consultando Nominatim reverse")?;
        if value.get("error").is_some() {
            return Ok(None);
        }
        Ok(serde_json::from_value(value).ok())
    }

    fn location_from_candidate(
        &self,
        place: &NominatimPlace,
        hints: &AdminHints,
        input: &LocationInput,
    ) -> ResolvedLocation {
        let gaz = Gazetteer::global();
        let center = place.point();
        let bbox = place.geo_box();

        let mut departamento = hints.departamento.clone();
        let mut municipio = hints.municipio.clone();
        let mut barrio = hints.barrio.clone();
        let mut comarca = hints.comarca.clone();

        if departamento.is_none() {
            departamento = place
                .address
                .state
                .as_deref()
                .or(place.address.region.as_deref())
                .and_then(|v| canonical_admin_department(gaz, v));
        }
        if municipio.is_none() {
            municipio = place
                .address
                .city
                .as_deref()
                .or(place.address.town.as_deref())
                .or(place.address.village.as_deref())
                .or(place.address.municipality.as_deref())
                .or(place.address.county.as_deref())
                .map(clean_admin_name);
        }
        if barrio.is_none() {
            barrio = place
                .address
                .neighbourhood
                .clone()
                .or_else(|| place.address.suburb.clone())
                .or_else(|| place.address.quarter.clone())
                .or_else(|| place.address.borough.clone())
                .or_else(|| place.address.city_district.clone());
        }
        if comarca.is_none() && place.address.hamlet.is_some() {
            comarca = place.address.hamlet.clone();
        }

        // Si el resultado es exactamente un barrio/comarca consultado, úsalo
        // como tal aunque Nominatim lo clasifique de otra forma.
        let kind = place.addresstype.as_str();
        if barrio.is_none() && is_neighbourhood_kind(kind) {
            barrio = Some(place.name.clone());
        }
        if comarca.is_none() && kind == "hamlet" {
            comarca = Some(place.name.clone());
        }

        let radius = place.radius_for_kind();

        let filter = if place.osm_type == "relation"
            && matches!(
                place.category.as_str(),
                "boundary" | "place" | "administrative"
            )
            && !matches!(place.kind.as_str(), "neighbourhood" | "suburb" | "quarter")
        {
            GeoFilter::Area {
                relation_id: place.osm_id,
            }
        } else if is_large_kind(kind) {
            match bbox {
                Some(b) => GeoFilter::Bbox {
                    south: b.south,
                    west: b.west,
                    north: b.north,
                    east: b.east,
                },
                None => GeoFilter::Around {
                    lat: center.lat,
                    lon: center.lon,
                    radius_m: radius,
                },
            }
        } else {
            GeoFilter::Around {
                lat: center.lat,
                lon: center.lon,
                radius_m: radius,
            }
        };

        let display_name = place.display_name.clone();
        let query = if input.place.is_some() {
            input.place.clone().unwrap_or_else(|| display_name.clone())
        } else {
            hints
                .display_label()
                .unwrap_or_else(|| display_name.clone())
        };

        ResolvedLocation {
            query,
            display_name,
            departamento,
            municipio,
            barrio,
            comarca,
            filter,
            center: Some(center),
            bbox: bbox.map(Into::into),
            radius_m: Some(radius),
            osm_type: Some(place.osm_type.clone()),
            osm_id: Some(place.osm_id),
            source: "nominatim".to_string(),
        }
    }
}

fn country_location() -> ResolvedLocation {
    ResolvedLocation {
        query: "Nicaragua".to_string(),
        display_name: "Nicaragua (todo el país)".to_string(),
        departamento: None,
        municipio: None,
        barrio: None,
        comarca: None,
        filter: GeoFilter::Country,
        center: Some(GeoPoint {
            lat: 12.865_416,
            lon: -85.207_229,
        }),
        bbox: Some(GeoBox {
            south: 10.707_394,
            west: -87.895_817,
            north: 15.090_278,
            east: -82.476_725,
        }),
        radius_m: None,
        osm_type: Some("relation".to_string()),
        osm_id: Some(NICARAGUA_RELATION_ID),
        source: "país".to_string(),
    }
}

#[derive(Debug, Clone, Default)]
struct AdminHints {
    departamento: Option<String>,
    municipio: Option<String>,
    barrio: Option<String>,
    comarca: Option<String>,
}

impl AdminHints {
    fn display_label(&self) -> Option<String> {
        let mut parts: Vec<String> = Vec::new();
        for value in [
            self.barrio.as_deref(),
            self.comarca.as_deref(),
            self.municipio.as_deref(),
            self.departamento.as_deref(),
        ]
        .into_iter()
        .flatten()
        {
            if !parts
                .iter()
                .any(|p| normalize_text(p) == normalize_text(value))
            {
                parts.push(value.to_string());
            }
        }
        if parts.is_empty() {
            None
        } else {
            Some(parts.join(", "))
        }
    }
}

fn apply_address_hints(hints: &mut AdminHints, place: &NominatimPlace) {
    let gaz = Gazetteer::global();
    if hints.departamento.is_none() {
        hints.departamento = place
            .address
            .state
            .as_deref()
            .or(place.address.region.as_deref())
            .and_then(|value| canonical_admin_department(gaz, value));
    }
    if hints.municipio.is_none() {
        hints.municipio = place
            .address
            .city
            .as_deref()
            .or(place.address.town.as_deref())
            .or(place.address.village.as_deref())
            .or(place.address.municipality.as_deref())
            .or(place.address.county.as_deref())
            .map(clean_admin_name);
    }
    if hints.barrio.is_none() {
        hints.barrio = place
            .address
            .neighbourhood
            .clone()
            .or_else(|| place.address.suburb.clone())
            .or_else(|| place.address.quarter.clone())
            .or_else(|| place.address.borough.clone())
            .or_else(|| place.address.city_district.clone());
    }
    if hints.comarca.is_none() {
        hints.comarca = place.address.hamlet.clone();
    }
}

fn build_query(input: &LocationInput, hints: &AdminHints) -> String {
    let mut parts: Vec<String> = Vec::new();

    if let Some(place) = input.place.as_deref() {
        push_unique(&mut parts, place);
    }
    if let Some(v) = hints.barrio.as_deref() {
        push_unique(&mut parts, v);
    }
    if let Some(v) = hints.comarca.as_deref() {
        push_unique(&mut parts, v);
    }
    if let Some(v) = hints.municipio.as_deref() {
        push_unique(&mut parts, v);
    }
    if let Some(v) = hints.departamento.as_deref() {
        push_unique(&mut parts, v);
    }
    if parts.is_empty() {
        parts.push("Nicaragua".to_string());
    }
    if normalize_text(parts.last().map(String::as_str).unwrap_or("")) != "nicaragua" {
        parts.push("Nicaragua".to_string());
    }
    parts.join(", ")
}

fn push_unique(parts: &mut Vec<String>, value: &str) {
    if value.trim().is_empty() {
        return;
    }
    if !parts
        .iter()
        .any(|p| normalize_text(p) == normalize_text(value))
    {
        parts.push(value.trim().to_string());
    }
}

fn canonical_department(gaz: &Gazetteer, raw: Option<&str>) -> Result<Option<String>> {
    let Some(raw) = raw else { return Ok(None) };
    if let Some(canonical) = canonical_admin_department(gaz, raw) {
        return Ok(Some(canonical));
    }
    if let Some(suggestion) = gaz.suggest_department(raw) {
        bail!("Departamento «{raw}» no reconocido. ¿Quisiste decir «{suggestion}»?");
    }
    bail!(
        "Departamento «{raw}» no reconocido. Usa uno de: {}",
        gaz.department_names().join(", ")
    );
}

fn canonical_admin_department(gaz: &Gazetteer, raw: &str) -> Option<String> {
    let cleaned = clean_admin_name(raw);
    gaz.canonical_department(&cleaned)
        .map(ToString::to_string)
        .or_else(|| gaz.canonical_department(raw).map(ToString::to_string))
}

fn canonical_municipality(
    gaz: &Gazetteer,
    raw: Option<&str>,
    department: Option<&str>,
) -> Result<Option<String>> {
    let Some(raw) = raw else { return Ok(None) };
    let cleaned = clean_admin_name(raw);
    if let Some((_, municipality)) = gaz.canonical_municipality(&cleaned, department) {
        return Ok(Some(municipality.to_string()));
    }
    if let Some((dept, municipality)) = gaz.suggest_municipality(&cleaned) {
        bail!("Municipio «{raw}» no reconocido. ¿Quisiste decir «{municipality}» ({dept})?");
    }
    bail!("Municipio «{raw}» no reconocido en Nicaragua");
}

fn clean_admin_name(raw: &str) -> String {
    let mut value = raw.trim().to_string();
    for prefix in [
        "Departamento de ",
        "departamento de ",
        "Región Autónoma de la ",
        "Region Autonoma de la ",
        "Municipio de ",
        "municipio de ",
    ] {
        if let Some(rest) = value.strip_prefix(prefix) {
            value = rest.trim().to_string();
        }
    }
    value
}

fn choose_candidate<'a>(
    candidates: &'a [NominatimPlace],
    hints: &AdminHints,
    input: &LocationInput,
) -> &'a NominatimPlace {
    let wants_department = hints.departamento.is_some()
        && hints.municipio.is_none()
        && hints.barrio.is_none()
        && hints.comarca.is_none()
        && input.place.is_none();
    let wants_municipality = hints.municipio.is_some();
    let wants_neighbourhood = hints.barrio.is_some() || hints.comarca.is_some();

    let mut best = &candidates[0];
    let mut best_score = i32::MIN;

    for candidate in candidates {
        let mut score = 0;
        let kind = candidate.addresstype.as_str();
        if wants_department {
            if kind == "state" || kind == "region" || kind == "department" {
                score += 100;
            }
            if candidate.osm_type == "relation" {
                score += 10;
            }
        } else if wants_municipality {
            if is_municipality_kind(kind) {
                score += 80;
            }
            if user_name_matches(candidate, hints.municipio.as_deref()) {
                score += 30;
            }
        } else if wants_neighbourhood {
            if is_neighbourhood_kind(kind) || kind == "hamlet" {
                score += 90;
            }
            if user_name_matches(candidate, hints.barrio.as_deref())
                || user_name_matches(candidate, hints.comarca.as_deref())
            {
                score += 25;
            }
        } else {
            if matches!(
                kind,
                "city" | "town" | "village" | "municipality" | "county" | "state" | "region"
            ) {
                score += 40;
            }
            if is_neighbourhood_kind(kind) {
                score += 30;
            }
        }
        // Preferir resultados administrativos sobre POIs con nombre parecido.
        if matches!(
            candidate.category.as_str(),
            "boundary" | "place" | "administrative"
        ) {
            score += 15;
        }
        if candidate.address.country_code.as_deref() == Some("ni") {
            score += 5;
        }
        score += (candidate.place_rank as i32).min(30) / 3;
        if score > best_score {
            best_score = score;
            best = candidate;
        }
    }
    best
}

fn user_name_matches(candidate: &NominatimPlace, wanted: Option<&str>) -> bool {
    let Some(wanted) = wanted else { return false };
    let wanted = normalize_text(&clean_admin_name(wanted));
    normalize_text(&candidate.name) == wanted
        || normalize_text(&candidate.display_name).starts_with(&wanted)
}

fn is_municipality_kind(kind: &str) -> bool {
    matches!(
        kind,
        "city" | "town" | "village" | "municipality" | "county" | "city_district"
    )
}

fn is_neighbourhood_kind(kind: &str) -> bool {
    matches!(
        kind,
        "neighbourhood" | "suburb" | "quarter" | "borough" | "city_district"
    )
}

fn is_large_kind(kind: &str) -> bool {
    matches!(
        kind,
        "state"
            | "region"
            | "department"
            | "country"
            | "city"
            | "town"
            | "village"
            | "municipality"
            | "county"
    )
}

#[derive(Debug, Clone, Deserialize)]
struct NominatimPlace {
    osm_type: String,
    osm_id: i64,
    lat: String,
    lon: String,
    #[serde(default, alias = "class")]
    category: String,
    #[serde(rename = "type", default)]
    kind: String,
    #[serde(default)]
    place_rank: u32,
    #[serde(default)]
    addresstype: String,
    #[serde(default)]
    name: String,
    display_name: String,
    #[serde(default)]
    address: NominatimAddress,
    #[serde(default)]
    boundingbox: Vec<String>,
}

impl NominatimPlace {
    fn point(&self) -> GeoPoint {
        GeoPoint {
            lat: self.lat.parse().unwrap_or(0.0),
            lon: self.lon.parse().unwrap_or(0.0),
        }
    }

    fn geo_box(&self) -> Option<Bbox> {
        if self.boundingbox.len() != 4 {
            return None;
        }
        let values: Vec<f64> = self
            .boundingbox
            .iter()
            .filter_map(|v| v.parse().ok())
            .collect();
        if values.len() != 4 {
            return None;
        }
        Some(Bbox {
            south: values[0],
            west: values[2],
            north: values[1],
            east: values[3],
        })
    }

    fn radius_for_kind(&self) -> f64 {
        match self.addresstype.as_str() {
            "neighbourhood" | "quarter" | "borough" | "city_district" => 1_200.0,
            "suburb" => 1_800.0,
            "hamlet" => 2_500.0,
            "village" => 3_000.0,
            "town" => 5_000.0,
            "city" => 8_000.0,
            "municipality" | "county" => 15_000.0,
            "state" | "region" | "department" => 50_000.0,
            _ => 1_500.0,
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
struct NominatimAddress {
    #[serde(default)]
    country_code: Option<String>,
    #[serde(default)]
    state: Option<String>,
    #[serde(default)]
    region: Option<String>,
    #[serde(default)]
    county: Option<String>,
    #[serde(default)]
    city: Option<String>,
    #[serde(default)]
    town: Option<String>,
    #[serde(default)]
    village: Option<String>,
    #[serde(default)]
    municipality: Option<String>,
    #[serde(default)]
    city_district: Option<String>,
    #[serde(default)]
    suburb: Option<String>,
    #[serde(default)]
    neighbourhood: Option<String>,
    #[serde(default)]
    quarter: Option<String>,
    #[serde(default)]
    borough: Option<String>,
    #[serde(default)]
    hamlet: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_queries_without_duplicates() {
        let input = LocationInput {
            municipio: Some("León".into()),
            departamento: Some("León".into()),
            ..Default::default()
        };
        let hints = AdminHints {
            departamento: Some("León".into()),
            municipio: Some("León".into()),
            ..Default::default()
        };
        assert_eq!(build_query(&input, &hints), "León, Nicaragua");
    }

    #[test]
    fn cache_key_is_stable() {
        let input = LocationInput {
            place: Some("Barrio Martha Quezada".into()),
            municipio: Some("Managua".into()),
            ..Default::default()
        };
        assert_eq!(input.cache_key(), input.cache_key());
        assert!(input.cache_key().starts_with("v1|"));
    }

    #[test]
    fn bbox_parse_accepts_both_orders() {
        let b = Bbox::parse("12.0,-86.3,12.2,-86.1").unwrap();
        assert!(b.south < b.north && b.west < b.east);
    }

    #[test]
    fn country_location_has_overpass_area() {
        let loc = country_location();
        assert_eq!(loc.overpass_area_id(), Some(NICARAGUA_AREA_ID));
    }
}
