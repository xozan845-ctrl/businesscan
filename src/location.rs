//! Resolución internacional de ubicaciones: continente, país, departamento,
//! ciudad/barrio, radio, bbox o área de OSM. Usa el gacetero de países y
//! Nominatim con caché.

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};

use crate::config::Config;
use crate::gazetteer::{Country, Gazetteer};
use crate::http::{HttpClient, RateLimiter};
use crate::i18n::Language;
use crate::normalize::{normalize_text, Bbox};

#[derive(Debug, Clone, Default)]
pub struct LocationInput {
    pub continent: Option<String>,
    pub country: Option<String>,
    pub place: Option<String>,
    pub department: Option<String>,
    pub city: Option<String>,
    pub neighborhood: Option<String>,
    pub comarca: Option<String>,
    pub lat: Option<f64>,
    pub lon: Option<f64>,
    pub radius_m: Option<f64>,
    pub bbox: Option<String>,
    pub osm_area_id: Option<i64>,
}

impl LocationInput {
    pub fn is_empty(&self) -> bool {
        self.continent.is_none() && self.country.is_none() && self.is_empty_place()
    }

    fn is_empty_place(&self) -> bool {
        self.place.is_none()
            && self.department.is_none()
            && self.city.is_none()
            && self.neighborhood.is_none()
            && self.comarca.is_none()
            && self.lat.is_none()
            && self.bbox.is_none()
            && self.osm_area_id.is_none()
    }

    pub fn cache_key(&self) -> String {
        let parts: Vec<String> = vec![
            self.continent.clone().unwrap_or_default(),
            self.country.clone().unwrap_or_default(),
            self.place.clone().unwrap_or_default(),
            self.department.clone().unwrap_or_default(),
            self.city.clone().unwrap_or_default(),
            self.neighborhood.clone().unwrap_or_default(),
            self.comarca.clone().unwrap_or_default(),
            self.lat.map(|v| format!("{v:.6}")).unwrap_or_default(),
            self.lon.map(|v| format!("{v:.6}")).unwrap_or_default(),
            self.radius_m.map(|v| format!("{v:.0}")).unwrap_or_default(),
            self.bbox.clone().unwrap_or_default(),
            self.osm_area_id.map(|v| v.to_string()).unwrap_or_default(),
        ];
        format!("v2|{}", normalize_text(&parts.join("|")))
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
        if self.continent.is_some() && !self.is_empty_place() {
            bail!(
                "--continent no se combina con departamento, ciudad, barrio, --place, coordenadas ni bbox; usa --country"
            );
        }
        if self.is_empty() {
            bail!(
                "Indica una ubicación: --continent, --country, --department, --city, --place, --lat/--lon, --bbox o --osm-area-id"
            );
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
    /// ID de área de Overpass explícito (avanzado).
    OverpassArea {
        area_id: i64,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolvedLocation {
    pub query: String,
    pub display_name: String,
    pub country_code: Option<String>,
    pub country_name: Option<String>,
    pub continent: Option<String>,
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
        let base = self.query.trim();
        let country = self.country_name.as_deref().unwrap_or("");
        if country.is_empty() {
            return base.to_string();
        }
        if normalize_text(base).contains(&normalize_text(country)) {
            base.to_string()
        } else if base.is_empty() {
            country.to_string()
        } else {
            format!("{base}, {country}")
        }
    }

    pub fn short_label(&self) -> String {
        self.display_name.clone()
    }

    pub fn overpass_area_id(&self) -> Option<i64> {
        match self.filter {
            GeoFilter::Area { relation_id } => Some(3_600_000_000 + relation_id),
            GeoFilter::OverpassArea { area_id } => Some(area_id),
            _ => None,
        }
    }

    pub fn is_country_level(&self) -> bool {
        self.departamento.is_none() && self.municipio.is_none() && self.barrio.is_none()
    }
}

pub struct LocationResolver {
    http: HttpClient,
    rate: RateLimiter,
    offline: bool,
    language: Language,
    nominatim_url: String,
}

impl LocationResolver {
    pub fn new(cfg: &Config, http: HttpClient) -> Self {
        Self {
            http,
            rate: RateLimiter::new(cfg.nominatim_interval),
            offline: cfg.offline,
            language: cfg.language,
            nominatim_url: cfg.nominatim_url.trim_end_matches('/').to_string(),
        }
    }

    /// Resuelve una o varias ubicaciones. Un continente devuelve un país por
    /// elemento; el resto devuelve una sola ubicación.
    pub async fn resolve_many(&self, input: &LocationInput) -> Result<Vec<ResolvedLocation>> {
        input.validate()?;
        let gaz = Gazetteer::global();

        if let Some(continent) = input.continent.as_deref() {
            let mut countries = gaz.countries_in_group(continent).with_context(|| {
                format!(
                    "Continente o región «{continent}» no reconocido. Usa: {}",
                    gaz.group_names().join(", ")
                )
            })?;
            if let Some(country_input) = input.country.as_deref() {
                let country = canonical_country(gaz, country_input)?;
                countries.retain(|candidate| candidate.iso2 == country.iso2);
                if countries.is_empty() {
                    bail!(
                        "«{}» no está dentro del grupo «{continent}»",
                        country.name(self.language)
                    );
                }
            }
            let mut resolved = Vec::new();
            let precise = countries.len() <= 20 && !self.offline;
            for country in countries {
                if precise {
                    resolved.push(self.resolve_country(country).await?);
                } else {
                    resolved.push(country_bbox_location(country, self.language));
                }
            }
            return Ok(resolved);
        }

        let country = match input.country.as_deref() {
            Some(value) => Some(canonical_country(gaz, value)?),
            None => None,
        };

        if let (Some(lat), Some(lon)) = (input.lat, input.lon) {
            return Ok(vec![
                self.resolve_coordinates(input, country, lat, lon).await?,
            ]);
        }
        if let Some(raw) = input.bbox.as_deref() {
            return Ok(vec![self.resolve_bbox(input, country, raw)?]);
        }
        if let Some(area_id) = input.osm_area_id {
            return Ok(vec![self.resolve_area_id(input, country, area_id)?]);
        }

        if let Some(country) = country {
            if input.place.is_none()
                && input.department.is_none()
                && input.city.is_none()
                && input.neighborhood.is_none()
                && input.comarca.is_none()
            {
                return Ok(vec![self.resolve_country(country).await?]);
            }
        }

        if self.offline {
            bail!(
                "Modo offline: no puedo geocodificar esta ubicación. Usa --lat/--lon, --bbox, --osm-area-id o un país con bbox conocido"
            );
        }

        let query = build_query(input, country, self.language);
        let mut candidates = self
            .nominatim(&query, country.map(|c| c.iso2.as_str()))
            .await?;
        let wants_country = country.is_some()
            && input.place.is_none()
            && input.department.is_none()
            && input.city.is_none()
            && input.neighborhood.is_none();
        if wants_country
            && !candidates
                .iter()
                .any(|candidate| candidate.addresstype == "country")
        {
            if let Some(country) = country {
                let retry = country.name(self.language).to_string();
                if let Ok(more) = self.nominatim(&retry, Some(country.iso2.as_str())).await {
                    candidates = more;
                }
            }
        }
        if candidates.is_empty() {
            bail!("No se encontró «{query}» en el mundo. Prueba con un nombre más específico.");
        }
        let chosen = choose_candidate(&candidates, input, country);
        Ok(vec![
            self.location_from_candidate(chosen, input, country, &query)
        ])
    }

    pub async fn resolve(&self, input: &LocationInput) -> Result<ResolvedLocation> {
        let mut locations = self.resolve_many(input).await?;
        if locations.len() == 1 {
            Ok(locations.remove(0))
        } else {
            bail!(
                "La ubicación abarca {} países; usa una operación que acepte múltiples ubicaciones",
                locations.len()
            )
        }
    }

    async fn resolve_country(&self, country: &Country) -> Result<ResolvedLocation> {
        let mut resolved = country_bbox_location(country, self.language);
        if self.offline {
            return Ok(resolved);
        }
        let label = country.name(self.language).to_string();
        if let Ok(candidates) = self.nominatim(&label, Some(country.iso2.as_str())).await {
            if let Some(candidate) = candidates.iter().find(|candidate| {
                candidate.addresstype == "country" || candidate.kind == "administrative"
            }) {
                if candidate.osm_type == "relation" {
                    resolved.filter = GeoFilter::Area {
                        relation_id: candidate.osm_id,
                    };
                    resolved.osm_type = Some(candidate.osm_type.clone());
                    resolved.osm_id = Some(candidate.osm_id);
                    resolved.source = "nominatim-country".to_string();
                }
                resolved.center = Some(candidate.point());
                if let Some(bbox) = candidate.geo_box() {
                    resolved.bbox = Some(bbox.into());
                }
            }
        }
        Ok(resolved)
    }

    async fn resolve_coordinates(
        &self,
        input: &LocationInput,
        country: Option<&Country>,
        lat: f64,
        lon: f64,
    ) -> Result<ResolvedLocation> {
        let radius = input.radius_m.unwrap_or(1_000.0);
        let bbox = Bbox::around(lat, lon, radius);
        let mut hints = AdminHints {
            country_code: country.map(|c| c.iso2.clone()),
            country_name: country.map(|c| c.name(self.language).to_string()),
            departamento: input.department.clone(),
            municipio: input.city.clone(),
            barrio: input.neighborhood.clone(),
            comarca: input.comarca.clone(),
        };
        let mut reverse_name = None;
        if !self.offline
            && (hints.country_code.is_none()
                || hints.departamento.is_none()
                || hints.municipio.is_none())
        {
            if let Ok(Some(place)) = self.reverse(lat, lon).await {
                apply_address_hints(&mut hints, &place, self.language);
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
        Ok(ResolvedLocation {
            query,
            display_name,
            country_code: hints.country_code,
            country_name: hints.country_name,
            continent: None,
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
        })
    }

    fn resolve_bbox(
        &self,
        input: &LocationInput,
        country: Option<&Country>,
        raw: &str,
    ) -> Result<ResolvedLocation> {
        let bbox = Bbox::parse(raw).context("bbox inválido")?;
        let label = label_from_input(input, country, self.language)
            .unwrap_or_else(|| format!("bbox {raw}"));
        Ok(ResolvedLocation {
            query: label.clone(),
            display_name: label,
            country_code: country.map(|c| c.iso2.clone()),
            country_name: country.map(|c| c.name(self.language).to_string()),
            continent: None,
            departamento: input.department.clone(),
            municipio: input.city.clone(),
            barrio: input.neighborhood.clone(),
            comarca: input.comarca.clone(),
            filter: GeoFilter::Bbox {
                south: bbox.south,
                west: bbox.west,
                north: bbox.north,
                east: bbox.east,
            },
            center: Some(GeoPoint {
                lat: (bbox.south + bbox.north) / 2.0,
                lon: (bbox.west + bbox.east) / 2.0,
            }),
            bbox: Some(bbox.into()),
            radius_m: None,
            osm_type: None,
            osm_id: None,
            source: "bbox".to_string(),
        })
    }

    fn resolve_area_id(
        &self,
        input: &LocationInput,
        country: Option<&Country>,
        area_id: i64,
    ) -> Result<ResolvedLocation> {
        let label = label_from_input(input, country, self.language)
            .unwrap_or_else(|| format!("área Overpass {area_id}"));
        Ok(ResolvedLocation {
            query: label.clone(),
            display_name: label,
            country_code: country.map(|c| c.iso2.clone()),
            country_name: country.map(|c| c.name(self.language).to_string()),
            continent: None,
            departamento: input.department.clone(),
            municipio: input.city.clone(),
            barrio: input.neighborhood.clone(),
            comarca: input.comarca.clone(),
            filter: GeoFilter::OverpassArea { area_id },
            center: None,
            bbox: None,
            radius_m: None,
            osm_type: None,
            osm_id: None,
            source: "overpass-area".to_string(),
        })
    }

    async fn nominatim(
        &self,
        query: &str,
        country_code: Option<&str>,
    ) -> Result<Vec<NominatimPlace>> {
        self.rate.wait().await;
        let url = format!("{}/search", self.nominatim_url);
        let query_owned = query.to_string();
        let language = self.language.code().to_string();
        let country_code = country_code.map(|value| value.to_lowercase());
        let result = self
            .http
            .json::<Vec<NominatimPlace>, _>(|| {
                let mut request = self
                    .http
                    .raw()
                    .get(&url)
                    .header(reqwest::header::ACCEPT_LANGUAGE, &language)
                    .query(&[
                        ("format", "jsonv2"),
                        ("q", query_owned.as_str()),
                        ("addressdetails", "1"),
                        ("extratags", "1"),
                        ("dedupe", "1"),
                        ("limit", "10"),
                        ("accept-language", language.as_str()),
                    ]);
                if let Some(country_code) = country_code.as_deref() {
                    request = request.query(&[("countrycodes", country_code)]);
                }
                request
            })
            .await
            .with_context(|| format!("Error consultando Nominatim para «{query}»"))?;
        Ok(result)
    }

    async fn reverse(&self, lat: f64, lon: f64) -> Result<Option<NominatimPlace>> {
        self.rate.wait().await;
        let url = format!("{}/reverse", self.nominatim_url);
        let language = self.language.code().to_string();
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
        input: &LocationInput,
        country: Option<&Country>,
        query: &str,
    ) -> ResolvedLocation {
        let gaz = Gazetteer::global();
        let center = place.point();
        let bbox = place.geo_box();
        let mut country_code = country.map(|c| c.iso2.clone());
        if country_code.is_none() {
            country_code = place
                .address
                .country_code
                .as_deref()
                .and_then(|value| gaz.normalize_country_code(value));
        }
        let country_name = country
            .map(|c| c.name(self.language).to_string())
            .or_else(|| {
                country_code
                    .as_deref()
                    .and_then(|code| gaz.country_by_iso2(code))
                    .map(|c| c.name(self.language).to_string())
            })
            .or_else(|| place.address.country.clone());

        let mut departamento = input.department.clone();
        let mut municipio = input.city.clone();
        let mut barrio = input.neighborhood.clone();
        let mut comarca = input.comarca.clone();
        if departamento.is_none() {
            departamento = place
                .address
                .state
                .as_deref()
                .or(place.address.region.as_deref())
                .or(place.address.province.as_deref())
                .map(clean_admin_name);
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
        if comarca.is_none() {
            comarca = place.address.hamlet.clone();
        }

        let kind = place.addresstype.as_str();
        if barrio.is_none() && is_neighbourhood_kind(kind) {
            barrio = Some(place.name.clone());
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

        ResolvedLocation {
            query: query.to_string(),
            display_name: place.display_name.clone(),
            country_code,
            country_name,
            continent: country.and_then(|c| c.continent.clone()),
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

fn country_bbox_location(country: &Country, language: Language) -> ResolvedLocation {
    let label = country.name(language).to_string();
    let bbox = country.bbox_or_point();
    ResolvedLocation {
        query: label.clone(),
        display_name: label.clone(),
        country_code: Some(country.iso2.clone()),
        country_name: Some(label),
        continent: country.continent.clone(),
        departamento: None,
        municipio: None,
        barrio: None,
        comarca: None,
        filter: GeoFilter::Bbox {
            south: bbox[1],
            west: bbox[0],
            north: bbox[3],
            east: bbox[2],
        },
        center: Some(GeoPoint {
            lat: country.lat,
            lon: country.lon,
        }),
        bbox: Some(GeoBox {
            south: bbox[1],
            west: bbox[0],
            north: bbox[3],
            east: bbox[2],
        }),
        radius_m: None,
        osm_type: None,
        osm_id: None,
        source: "country-bbox".to_string(),
    }
}

fn canonical_country<'a>(gaz: &'a Gazetteer, raw: &str) -> Result<&'a Country> {
    if let Some(country) = gaz.country(raw) {
        return Ok(country);
    }
    if let Some(suggestion) = gaz.suggest_country(raw) {
        bail!(
            "País «{raw}» no reconocido. ¿Quisiste decir «{}» ({})?",
            suggestion.name(Language::Es),
            suggestion.iso2
        );
    }
    bail!("País «{raw}» no reconocido. Usa un nombre o código ISO de dos letras (ej. NI, GT, PA)")
}

fn build_query(input: &LocationInput, country: Option<&Country>, language: Language) -> String {
    let mut parts: Vec<String> = Vec::new();
    if let Some(place) = input.place.as_deref() {
        push_unique(&mut parts, place);
    }
    if let Some(value) = input.neighborhood.as_deref() {
        push_unique(&mut parts, value);
    }
    if let Some(value) = input.comarca.as_deref() {
        push_unique(&mut parts, value);
    }
    if let Some(value) = input.city.as_deref() {
        push_unique(&mut parts, value);
    }
    if let Some(value) = input.department.as_deref() {
        push_unique(&mut parts, value);
    }
    if let Some(country) = country {
        push_unique(&mut parts, country.name(language));
    }
    if parts.is_empty() {
        "world".to_string()
    } else {
        parts.join(", ")
    }
}

fn label_from_input(
    input: &LocationInput,
    country: Option<&Country>,
    language: Language,
) -> Option<String> {
    let mut parts: Vec<String> = Vec::new();
    for value in [
        input.neighborhood.as_deref(),
        input.comarca.as_deref(),
        input.city.as_deref(),
        input.department.as_deref(),
    ]
    .into_iter()
    .flatten()
    {
        push_unique(&mut parts, value);
    }
    if let Some(country) = country {
        push_unique(&mut parts, country.name(language));
    }
    if parts.is_empty() {
        None
    } else {
        Some(parts.join(", "))
    }
}

fn push_unique(parts: &mut Vec<String>, value: &str) {
    if value.trim().is_empty() {
        return;
    }
    if !parts
        .iter()
        .any(|part| normalize_text(part) == normalize_text(value))
    {
        parts.push(value.trim().to_string());
    }
}

#[derive(Debug, Clone, Default)]
struct AdminHints {
    country_code: Option<String>,
    country_name: Option<String>,
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
            self.country_name.as_deref(),
        ]
        .into_iter()
        .flatten()
        {
            push_unique(&mut parts, value);
        }
        if parts.is_empty() {
            None
        } else {
            Some(parts.join(", "))
        }
    }
}

fn apply_address_hints(hints: &mut AdminHints, place: &NominatimPlace, language: Language) {
    let gaz = Gazetteer::global();
    if hints.country_code.is_none() {
        hints.country_code = place
            .address
            .country_code
            .as_deref()
            .and_then(|value| gaz.normalize_country_code(value));
    }
    if hints.country_name.is_none() {
        hints.country_name = hints
            .country_code
            .as_deref()
            .and_then(|code| gaz.country_by_iso2(code))
            .map(|country| country.name(language).to_string())
            .or_else(|| place.address.country.clone());
    }
    if hints.departamento.is_none() {
        hints.departamento = place
            .address
            .state
            .as_deref()
            .or(place.address.region.as_deref())
            .or(place.address.province.as_deref())
            .map(clean_admin_name);
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

fn choose_candidate<'a>(
    candidates: &'a [NominatimPlace],
    input: &LocationInput,
    country: Option<&Country>,
) -> &'a NominatimPlace {
    let wants_country = country.is_some()
        && input.place.is_none()
        && input.department.is_none()
        && input.city.is_none()
        && input.neighborhood.is_none()
        && input.comarca.is_none();
    let wants_department = input.department.is_some() && input.city.is_none();
    let wants_city = input.city.is_some();
    let wants_neighbourhood = input.neighborhood.is_some() || input.comarca.is_some();

    let mut best = &candidates[0];
    let mut best_score = i32::MIN;
    for candidate in candidates {
        let mut score = 0;
        let kind = candidate.addresstype.as_str();
        if wants_country {
            if kind == "country" {
                score += 120;
            }
            if candidate.osm_type == "relation" {
                score += 10;
            }
        } else if wants_department {
            if matches!(kind, "state" | "region" | "department" | "province") {
                score += 100;
            }
        } else if wants_city {
            if is_city_kind(kind) {
                score += 80;
            }
            if user_name_matches(candidate, input.city.as_deref()) {
                score += 30;
            }
        } else if wants_neighbourhood {
            if is_neighbourhood_kind(kind) || kind == "hamlet" {
                score += 90;
            }
            if user_name_matches(candidate, input.neighborhood.as_deref())
                || user_name_matches(candidate, input.comarca.as_deref())
            {
                score += 25;
            }
        } else {
            if matches!(
                kind,
                "city" | "town" | "village" | "municipality" | "county" | "state"
            ) {
                score += 40;
            }
            if is_neighbourhood_kind(kind) {
                score += 30;
            }
        }
        if matches!(
            candidate.category.as_str(),
            "boundary" | "place" | "administrative"
        ) {
            score += 15;
        }
        if country
            .map(|country| candidate.address.country_code.as_deref() == Some(country.iso2.as_str()))
            .unwrap_or(false)
        {
            score += 8;
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

fn is_city_kind(kind: &str) -> bool {
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
        "country"
            | "state"
            | "region"
            | "department"
            | "province"
            | "city"
            | "town"
            | "village"
            | "municipality"
            | "county"
    )
}

fn clean_admin_name(raw: &str) -> String {
    let mut value = raw.trim().to_string();
    for prefix in [
        "Departamento de ",
        "departamento de ",
        "Provincia de ",
        "provincia de ",
        "Región de ",
        "region de ",
        "Municipio de ",
        "municipio de ",
        "Department of ",
        "Province of ",
        "Region of ",
        "Municipality of ",
    ] {
        if let Some(rest) = value.strip_prefix(prefix) {
            value = rest.trim().to_string();
        }
    }
    value
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
            .filter_map(|value| value.parse().ok())
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
            "state" | "region" | "department" | "province" => 50_000.0,
            "country" => 100_000.0,
            _ => 1_500.0,
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
struct NominatimAddress {
    #[serde(default)]
    country_code: Option<String>,
    #[serde(default)]
    country: Option<String>,
    #[serde(default)]
    state: Option<String>,
    #[serde(default)]
    region: Option<String>,
    #[serde(default)]
    province: Option<String>,
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
            city: Some("León".into()),
            department: Some("León".into()),
            country: Some("NI".into()),
            ..Default::default()
        };
        let country = Gazetteer::global().country("NI").unwrap();
        assert_eq!(
            build_query(&input, Some(country), Language::Es),
            "León, Nicaragua"
        );
    }

    #[test]
    fn cache_key_is_stable() {
        let input = LocationInput {
            continent: Some("central-america".into()),
            country: Some("NI".into()),
            ..Default::default()
        };
        assert_eq!(input.cache_key(), input.cache_key());
        assert!(input.cache_key().starts_with("v2|"));
    }

    #[test]
    fn bbox_parse_accepts_both_orders() {
        let b = Bbox::parse("12.0,-86.3,12.2,-86.1").unwrap();
        assert!(b.south < b.north && b.west < b.east);
    }

    #[tokio::test]
    async fn country_location_has_bbox() {
        let country = Gazetteer::global().country("NI").unwrap();
        let mut config = Config::load(Some(std::path::PathBuf::from(":memory:"))).unwrap();
        config.offline = true;
        let http = HttpClient::new(&config).unwrap();
        let resolver = LocationResolver::new(&config, http);
        let location = resolver.resolve_country(country).await.unwrap();
        assert_eq!(location.country_code.as_deref(), Some("NI"));
        assert!(location.bbox.is_some());
    }
}
