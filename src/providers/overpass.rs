//! Fuente OpenStreetMap vía Overpass API.
//!
//! Es la fuente base: no requiere clave, cubre todo Nicaragua y trae etiquetas
//! de contacto, horarios, dirección, redes sociales y categorías.

use std::collections::{BTreeMap, BTreeSet};

use anyhow::Result;
use async_trait::async_trait;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;

use crate::config::Config;
use crate::location::{GeoFilter, ResolvedLocation};
use crate::model::{Address, BusinessRecord, Contact, ContactKind};
use crate::normalize::normalize_text;
use crate::providers::categories::{
    category_label, matches_category_text, osm_constraint, pick_category, AMENITY_REGEX,
    BUSINESS_KEYS, LEISURE_REGEX, TOURISM_REGEX,
};
use crate::providers::{Provider, ProviderContext, ProviderRequest};

pub struct OverpassProvider;

#[async_trait]
impl Provider for OverpassProvider {
    fn name(&self) -> &'static str {
        "osm"
    }

    fn description(&self) -> &'static str {
        "OpenStreetMap vía Overpass: amplia cobertura de negocios, contactos y direcciones (sin clave)"
    }

    fn requires_key(&self) -> bool {
        false
    }

    fn is_available(&self, _config: &Config) -> bool {
        true
    }

    async fn fetch(
        &self,
        ctx: &ProviderContext,
        request: &ProviderRequest,
    ) -> Result<Vec<BusinessRecord>> {
        let categories = request.categories();
        let query = build_query(
            &request.location,
            &categories,
            request.include_unnamed,
            ctx.config.overpass_timeout.as_secs(),
            request.limit,
        );

        log::debug!("Consulta Overpass ({} bytes):\n{query}", query.len());

        let mut last_error = None;
        let mut elements: Option<Vec<OverpassElement>> = None;
        for url in &ctx.config.overpass_urls {
            let query_ref = query.clone();
            let client = ctx.http.raw();
            match ctx
                .http
                .text(|| client.post(url).form(&[("data", query_ref.as_str())]))
                .await
            {
                Ok(body) => match serde_json::from_str::<OverpassResponse>(&body) {
                    Ok(response) => {
                        elements = Some(response.elements);
                        break;
                    }
                    Err(error) => {
                        last_error = Some(anyhow::anyhow!(
                            "Respuesta Overpass inválida desde {url}: {error}"
                        ));
                    }
                },
                Err(error) => {
                    log::warn!("Overpass falló en {url}: {error}");
                    last_error = Some(error);
                }
            }
        }

        let Some(elements) = elements else {
            return Err(last_error.unwrap_or_else(|| {
                anyhow::anyhow!("No se pudo consultar Overpass en ningún espejo")
            }));
        };

        let mut records = Vec::new();
        let mut unknown_category = false;
        for category in &categories {
            if osm_constraint(category).is_none() {
                unknown_category = true;
            }
        }
        if !categories.is_empty() && unknown_category {
            log::debug!("Categorías no reconocidas por Overpass; se filtran en el cliente");
        }

        for element in elements {
            let Some(record) = map_element(&element, request) else {
                continue;
            };
            if !categories.is_empty()
                && !categories.iter().any(|term| {
                    matches_category_text(
                        record.primary_category.as_deref(),
                        record.business_type.as_deref(),
                        &record.categories,
                        &record.name,
                        term,
                    )
                })
            {
                continue;
            }
            if let Some(query) = request.query.as_deref() {
                if !matches_free_query(&record, query) {
                    continue;
                }
            }
            records.push(record);
            if let Some(limit) = request.limit {
                if records.len() >= limit {
                    break;
                }
            }
        }

        Ok(records)
    }
}

pub fn build_query(
    location: &ResolvedLocation,
    categories: &[String],
    include_unnamed: bool,
    timeout_secs: u64,
    limit: Option<usize>,
) -> String {
    let (area_statement, geo_suffix) = geo_clause(location);
    let name_filter = if include_unnamed { "" } else { "[\"name\"]" };

    let mut filters: BTreeSet<String> = BTreeSet::new();
    let mut constraints: Vec<(String, Option<String>)> = categories
        .iter()
        .filter_map(|category| osm_constraint(category))
        .collect();
    let has_unknown = categories.len() > constraints.len();

    if constraints.is_empty() || has_unknown {
        for key in BUSINESS_KEYS {
            let condition = match *key {
                "amenity" => format!("[\"amenity\"~\"{AMENITY_REGEX}\"]"),
                "tourism" => format!("[\"tourism\"~\"{TOURISM_REGEX}\"]"),
                "leisure" => format!("[\"leisure\"~\"{LEISURE_REGEX}\"]"),
                _ => format!("[\"{key}\"]"),
            };
            filters.insert(format!("nwr{name_filter}{condition}{geo_suffix};"));
        }
    }

    for (key, value) in constraints.drain(..) {
        let condition = match value {
            Some(value) if value != "*" => format!("[\"{key}\"=\"{value}\"]"),
            _ => format!("[\"{key}\"]"),
        };
        filters.insert(format!("nwr{name_filter}{condition}{geo_suffix};"));
    }

    let mut query = format!("[out:json][timeout:{timeout_secs}][maxsize:1073741824];\n");
    if let Some(area) = area_statement {
        query.push_str(&area);
        query.push('\n');
    }
    query.push_str("(\n");
    for filter in filters {
        query.push_str("  ");
        query.push_str(&filter);
        query.push('\n');
    }
    query.push_str(");\nout center tags");
    if let Some(limit) = limit {
        query.push(' ');
        query.push_str(&limit.to_string());
    }
    query.push_str(";\n");
    query
}

fn geo_clause(location: &ResolvedLocation) -> (Option<String>, String) {
    match &location.filter {
        GeoFilter::Area { relation_id } => {
            let area_id = 3_600_000_000i64 + relation_id;
            (
                Some(format!("area({area_id})->.search_area;")),
                "(area.search_area)".to_string(),
            )
        }
        GeoFilter::Country => (
            Some("area(3600287666)->.search_area;".to_string()),
            "(area.search_area)".to_string(),
        ),
        GeoFilter::OverpassArea { area_id } => (
            Some(format!("area({area_id})->.search_area;")),
            "(area.search_area)".to_string(),
        ),
        GeoFilter::Around { lat, lon, radius_m } => (
            None,
            format!("(around:{:.0},{lat:.7},{lon:.7})", radius_m.max(1.0)),
        ),
        GeoFilter::Bbox {
            south,
            west,
            north,
            east,
        } => (None, format!("({south},{west},{north},{east})")),
    }
}

fn map_element(element: &OverpassElement, request: &ProviderRequest) -> Option<BusinessRecord> {
    let tags = element.tags.as_ref()?;
    let name = tags
        .get("name")
        .or_else(|| tags.get("name:es"))
        .or_else(|| tags.get("official_name"))
        .or_else(|| tags.get("brand"))
        .or_else(|| tags.get("operator"))
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());

    let name = match name {
        Some(name) => name,
        None if request.include_unnamed => {
            format!("Sin nombre ({}/{})", element.kind, element.id)
        }
        None => return None,
    };

    let (category_key, category_value) = pick_category(tags)?;
    let business_type = format!("{category_key}={category_value}");
    let primary_category = category_label(&category_key, &category_value);

    let mut aliases = Vec::new();
    for key in [
        "name:es",
        "alt_name",
        "official_name",
        "short_name",
        "loc_name",
        "old_name",
        "brand",
        "operator",
    ] {
        if let Some(value) = tags.get(key) {
            for value in split_values(value) {
                if normalize_text(&value) != normalize_text(&name) && !aliases.contains(&value) {
                    aliases.push(value);
                }
            }
        }
    }

    let mut categories = Vec::new();
    for key in BUSINESS_KEYS {
        if let Some(value) = tags.get(*key) {
            let label = category_label(key, value);
            if !label.is_empty() && !categories.contains(&label) {
                categories.push(label);
            }
        }
    }
    if let Some(cuisine) = tags.get("cuisine") {
        for value in split_values(cuisine) {
            let label = format!("Cocina: {}", value.replace('_', " "));
            if !categories.contains(&label) {
                categories.push(label);
            }
        }
    }

    let mut contacts = Vec::new();
    let mut first_phone = true;
    let mut first_email = true;
    let mut first_website = true;

    for key in ["contact:phone", "phone", "contact:mobile", "mobile"] {
        if let Some(value) = tags.get(key) {
            for value in split_values(value) {
                let kind = if key.contains("mobile") {
                    ContactKind::Mobile
                } else {
                    ContactKind::Phone
                };
                let mut contact = Contact::new(kind, value).with_source("osm");
                if first_phone {
                    contact = contact.primary();
                    first_phone = false;
                }
                contact.label = Some(if kind == ContactKind::Mobile {
                    "Celular".to_string()
                } else {
                    "Teléfono".to_string()
                });
                contacts.push(contact);
            }
        }
    }
    for key in ["contact:whatsapp", "whatsapp"] {
        if let Some(value) = tags.get(key) {
            for value in split_values(value) {
                contacts.push(
                    Contact::new(ContactKind::Whatsapp, value)
                        .with_source("osm")
                        .primary(),
                );
            }
        }
    }
    for key in ["contact:email", "email"] {
        if let Some(value) = tags.get(key) {
            for value in split_values(value) {
                let mut contact = Contact::new(ContactKind::Email, value).with_source("osm");
                if first_email {
                    contact = contact.primary();
                    first_email = false;
                }
                contacts.push(contact);
            }
        }
    }
    for key in ["contact:website", "website", "url", "contact:url"] {
        if let Some(value) = tags.get(key) {
            for value in split_values(value) {
                let mut contact = Contact::new(ContactKind::Website, value).with_source("osm");
                if first_website {
                    contact = contact.primary();
                    first_website = false;
                }
                contacts.push(contact);
            }
        }
    }

    for (keys, kind) in [
        (&["contact:facebook", "facebook"][..], ContactKind::Facebook),
        (
            &["contact:instagram", "instagram"][..],
            ContactKind::Instagram,
        ),
        (&["contact:tiktok", "tiktok"][..], ContactKind::TikTok),
        (
            &["contact:twitter", "contact:x", "twitter"][..],
            ContactKind::Twitter,
        ),
        (&["contact:telegram", "telegram"][..], ContactKind::Telegram),
        (&["contact:youtube", "youtube"][..], ContactKind::YouTube),
        (&["contact:linkedin", "linkedin"][..], ContactKind::LinkedIn),
    ] {
        for key in keys {
            if let Some(value) = tags.get(*key) {
                for value in split_values(value) {
                    contacts.push(Contact::new(kind, value).with_source("osm"));
                }
            }
        }
    }

    let mut address = Address {
        line: tags.get("addr:full").cloned(),
        street: tags
            .get("addr:street")
            .or_else(|| tags.get("addr:place"))
            .cloned(),
        house_number: tags.get("addr:housenumber").cloned(),
        barrio: tags
            .get("addr:neighbourhood")
            .or_else(|| tags.get("addr:suburb"))
            .cloned(),
        comarca: tags
            .get("addr:hamlet")
            .or_else(|| tags.get("addr:locality"))
            .cloned(),
        municipio: tags
            .get("addr:city")
            .or_else(|| tags.get("addr:town"))
            .or_else(|| tags.get("addr:village"))
            .or_else(|| tags.get("addr:municipality"))
            .cloned(),
        departamento: tags.get("addr:state").cloned(),
        postal_code: tags.get("addr:postcode").cloned(),
        country_code: tags
            .get("addr:country")
            .cloned()
            .or_else(|| Some("NI".to_string())),
    };

    if address.departamento.is_none() {
        address.departamento = request.location.departamento.clone();
    }
    if address.municipio.is_none() {
        address.municipio = request.location.municipio.clone();
    }
    if address.barrio.is_none() {
        address.barrio = request.location.barrio.clone();
    }
    if address.comarca.is_none() {
        address.comarca = request.location.comarca.clone();
    }

    let (latitude, longitude) = match (element.lat, element.lon) {
        (Some(lat), Some(lon)) => (Some(lat), Some(lon)),
        _ => element
            .center
            .as_ref()
            .map(|center| (Some(center.lat), Some(center.lon)))
            .unwrap_or((None, None)),
    };

    let status = if tags.keys().any(|key| {
        key.starts_with("disused:") || key.starts_with("abandoned:") || key.starts_with("was:")
    }) || tags.get("shop").map(String::as_str) == Some("vacant")
    {
        Some("closed".to_string())
    } else {
        Some("active".to_string())
    };

    let mut confidence: f64 = 0.62;
    if !contacts.is_empty() {
        confidence += 0.06;
    }
    if contacts.iter().any(|c| c.kind == ContactKind::Website) {
        confidence += 0.04;
    }
    if contacts.iter().any(|c| c.kind == ContactKind::Email) {
        confidence += 0.04;
    }
    if latitude.is_some() {
        confidence += 0.04;
    }
    if !address.is_empty() {
        confidence += 0.04;
    }

    let source_url = Some(format!(
        "https://www.openstreetmap.org/{}/{}",
        element.kind, element.id
    ));
    let raw = serde_json::to_value(element).unwrap_or(JsonValue::Null);

    Some(BusinessRecord {
        source: "osm".to_string(),
        external_id: Some(format!("{}/{}", element.kind, element.id)),
        source_url,
        name,
        aliases,
        legal_name: tags.get("official_name").cloned(),
        description: tags
            .get("description")
            .or_else(|| tags.get("note"))
            .cloned(),
        primary_category: Some(primary_category),
        business_type: Some(business_type),
        categories,
        status,
        contacts,
        address,
        latitude,
        longitude,
        opening_hours: tags.get("opening_hours").cloned(),
        price_range: tags
            .get("price_range")
            .or_else(|| tags.get("price"))
            .cloned(),
        rating: None,
        review_count: None,
        tags: tags.clone(),
        raw,
        confidence: confidence.min(0.9),
        fetched_at: Utc::now(),
    })
}

fn split_values(value: &str) -> Vec<String> {
    value
        .split([';', '|'])
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(ToString::to_string)
        .collect()
}

fn matches_free_query(record: &BusinessRecord, query: &str) -> bool {
    let haystack = normalize_text(&format!(
        "{} {} {} {} {} {}",
        record.name,
        record.description.as_deref().unwrap_or(""),
        record.primary_category.as_deref().unwrap_or(""),
        record.business_type.as_deref().unwrap_or(""),
        record.categories.join(" "),
        record
            .tags
            .iter()
            .map(|(k, v)| format!("{k} {v}"))
            .collect::<Vec<_>>()
            .join(" ")
    ));
    normalize_text(query)
        .split_whitespace()
        .all(|token| haystack.contains(token))
}

#[derive(Debug, Clone, Deserialize)]
pub struct OverpassResponse {
    #[serde(default)]
    pub elements: Vec<OverpassElement>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct OverpassElement {
    #[serde(rename = "type")]
    pub kind: String,
    pub id: i64,
    #[serde(default)]
    pub lat: Option<f64>,
    #[serde(default)]
    pub lon: Option<f64>,
    #[serde(default)]
    pub center: Option<OverpassCenter>,
    #[serde(default)]
    pub tags: Option<BTreeMap<String, String>>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct OverpassCenter {
    pub lat: f64,
    pub lon: f64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::location::{GeoFilter, GeoPoint, ResolvedLocation};

    fn location() -> ResolvedLocation {
        ResolvedLocation {
            query: "León".into(),
            display_name: "León, Nicaragua".into(),
            departamento: Some("León".into()),
            municipio: Some("León".into()),
            barrio: None,
            comarca: None,
            filter: GeoFilter::Area { relation_id: 123 },
            center: Some(GeoPoint {
                lat: 12.43,
                lon: -86.87,
            }),
            bbox: None,
            radius_m: None,
            osm_type: Some("relation".into()),
            osm_id: Some(123),
            source: "test".into(),
        }
    }

    #[test]
    fn query_contains_area_and_business_keys() {
        let q = build_query(&location(), &[], false, 120, None);
        assert!(q.contains("area(3600000123)->.search_area;"));
        assert!(q.contains("[\"shop\"]"));
        assert!(q.contains("[\"amenity\"~\""));
        assert!(q.contains("out center tags;"));
    }

    #[test]
    fn category_constraint_narrows_query() {
        let q = build_query(&location(), &["ferreteria".into()], false, 120, None);
        assert!(q.contains("[\"shop\"=\"hardware\"]"));
        assert!(!q.contains("[\"amenity\"~"));
    }

    #[test]
    fn maps_osm_element_with_contacts_and_address() {
        let json = r#"{
          "type": "node",
          "id": 42,
          "lat": 12.435,
          "lon": -86.878,
          "tags": {
            "name": "Ferretería El Tornillo",
            "shop": "hardware",
            "phone": "+505 2222-3333; +505 8888-7777",
            "contact:email": "ventas@eltornillo.ni",
            "contact:website": "eltornillo.ni",
            "contact:facebook": "eltornillo",
            "addr:street": "Calle Central",
            "addr:housenumber": "123",
            "addr:neighbourhood": "Barrio San Juan",
            "addr:city": "León",
            "addr:state": "León",
            "opening_hours": "Mo-Sa 08:00-18:00"
          }
        }"#;
        let element: OverpassElement = serde_json::from_str(json).unwrap();
        let request = ProviderRequest {
            location: location(),
            query: None,
            category: None,
            limit: None,
            include_unnamed: false,
            page_size: 20,
            max_pages: 1,
        };
        let record = map_element(&element, &request).unwrap();
        assert_eq!(record.name, "Ferretería El Tornillo");
        assert_eq!(record.business_type.as_deref(), Some("shop=hardware"));
        assert_eq!(record.primary_category.as_deref(), Some("Ferretería"));
        assert_eq!(record.contacts.len(), 5);
        assert_eq!(record.address.municipio.as_deref(), Some("León"));
        assert_eq!(record.address.barrio.as_deref(), Some("Barrio San Juan"));
        assert_eq!(record.external_id.as_deref(), Some("node/42"));
    }

    #[test]
    fn unknown_category_falls_back_to_base_query() {
        let q = build_query(&location(), &["categoría rara".into()], false, 120, None);
        assert!(q.contains("[\"shop\"]"));
    }

    #[test]
    fn around_query_has_coordinates() {
        let mut loc = location();
        loc.filter = GeoFilter::Around {
            lat: 12.1,
            lon: -86.2,
            radius_m: 900.0,
        };
        let q = build_query(&loc, &[], false, 120, None);
        assert!(q.contains("(around:900,12.1000000,-86.2000000)"));
        assert!(!q.contains("area("));
    }
}
