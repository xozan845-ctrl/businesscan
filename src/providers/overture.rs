//! Fuente Overture Maps desde un archivo GeoJSON/JSONL local.
//!
//! Overture publica Places como GeoParquet en S3. Para mantener NicaScan
//! portable (Termux incluido), esta fuente lee un extracto GeoJSON producido
//! por DuckDB o cualquier herramienta GIS. Ver docs/SOURCES.md.

use std::path::Path;
use std::time::Instant;

use anyhow::{bail, Context, Result};
use async_trait::async_trait;
use chrono::Utc;
use serde_json::Value;

use crate::config::Config;
use crate::location::{GeoFilter, ResolvedLocation};
use crate::model::{Address, BusinessRecord, Contact, ContactKind};
use crate::normalize::{
    domain_of, haversine_m, normalize_email, normalize_phone, normalize_social, normalize_text,
    normalize_url, stable_hash,
};
use crate::providers::categories::matches_category_text;
use crate::providers::{Provider, ProviderContext, ProviderRequest};

pub struct OvertureProvider;

#[async_trait]
impl Provider for OvertureProvider {
    fn name(&self) -> &'static str {
        "overture"
    }

    fn description(&self) -> &'static str {
        "Overture Maps desde un GeoJSON local extraído con DuckDB (--overture-file)"
    }

    fn requires_key(&self) -> bool {
        false
    }

    fn is_available(&self, config: &Config) -> bool {
        config.overture_file.is_some()
    }

    fn default_enabled(&self) -> bool {
        false
    }

    async fn fetch(
        &self,
        ctx: &ProviderContext,
        request: &ProviderRequest,
    ) -> Result<Vec<BusinessRecord>> {
        let path = ctx
            .config
            .overture_file
            .as_deref()
            .context("Falta --overture-file o NICASCAN_OVERTURE_FILE")?;
        fetch_file(path, request).await
    }
}

pub async fn fetch_file(path: &Path, request: &ProviderRequest) -> Result<Vec<BusinessRecord>> {
    let started = Instant::now();
    let text = tokio::fs::read_to_string(path)
        .await
        .with_context(|| format!("No se pudo leer {}", path.display()))?;
    let features = parse_features(&text, path)?;
    log::debug!(
        "Overture: {} features leídas de {} en {} ms",
        features.len(),
        path.display(),
        started.elapsed().as_millis()
    );

    let categories = request.categories();
    let mut records = Vec::new();
    for feature in &features {
        let Some((lon, lat)) = feature_point(feature) else {
            continue;
        };
        if !in_location(lon, lat, &request.location) {
            continue;
        }
        let Some(record) = feature_to_record(feature, lon, lat) else {
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
            if !matches_query(&record, query) {
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

pub fn parse_features(text: &str, path: &Path) -> Result<Vec<Value>> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Ok(Vec::new());
    }
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("")
        .to_lowercase();

    if extension == "jsonl"
        || extension == "ndjson"
        || (!matches!(extension.as_str(), "json" | "geojson") && trimmed.lines().count() > 1)
    {
        let mut features = Vec::new();
        for (index, line) in text.lines().enumerate() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let value: Value = serde_json::from_str(line)
                .with_context(|| format!("JSONL inválido en la línea {}", index + 1))?;
            features.push(value);
        }
        return Ok(features);
    }

    let value: Value = serde_json::from_str(trimmed)
        .with_context(|| format!("GeoJSON inválido en {}", path.display()))?;
    Ok(match value {
        Value::Array(values) => values,
        Value::Object(object) => {
            if object.get("type").and_then(Value::as_str) == Some("FeatureCollection") {
                object
                    .get("features")
                    .and_then(Value::as_array)
                    .cloned()
                    .unwrap_or_default()
            } else if object.get("type").and_then(Value::as_str) == Some("Feature") {
                vec![Value::Object(object)]
            } else if let Some(features) = object.get("features").and_then(Value::as_array) {
                features.clone()
            } else if object.contains_key("properties") {
                vec![Value::Object(object)]
            } else {
                bail!("El archivo no contiene features de Overture")
            }
        }
        _ => bail!("El archivo no contiene features de Overture"),
    })
}

fn feature_point(feature: &Value) -> Option<(f64, f64)> {
    let geometry = feature.get("geometry")?;
    let kind = geometry.get("type").and_then(Value::as_str)?;
    if kind != "Point" {
        return None;
    }
    let coordinates = geometry.get("coordinates")?.as_array()?;
    let lon = coordinates.first()?.as_f64()?;
    let lat = coordinates.get(1)?.as_f64()?;
    if !(-180.0..=180.0).contains(&lon) || !(-90.0..=90.0).contains(&lat) {
        return None;
    }
    Some((lon, lat))
}

fn in_location(lon: f64, lat: f64, location: &ResolvedLocation) -> bool {
    match &location.filter {
        GeoFilter::Around {
            lat: center_lat,
            lon: center_lon,
            radius_m,
        } => haversine_m(lat, lon, *center_lat, *center_lon) <= *radius_m,
        _ => match location.bbox {
            Some(bbox) => bbox.to_bbox().contains(lat, lon),
            None => true,
        },
    }
}

pub fn feature_to_record(feature: &Value, lon: f64, lat: f64) -> Option<BusinessRecord> {
    let properties = feature.get("properties").unwrap_or(&Value::Null);
    let names = properties.get("names");

    let mut name = names
        .and_then(|names| names.get("primary"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string);
    let mut aliases = Vec::new();
    if let Some(common) = names
        .and_then(|names| names.get("common"))
        .and_then(Value::as_object)
    {
        for value in common.values() {
            if let Some(value) = value.as_str().map(str::trim).filter(|v| !v.is_empty()) {
                if name.is_none() {
                    name = Some(value.to_string());
                } else if normalize_text(value) != normalize_text(name.as_deref().unwrap_or("")) {
                    aliases.push(value.to_string());
                }
            }
        }
    }
    if name.is_none() {
        name = properties
            .get("name")
            .and_then(Value::as_str)
            .map(ToString::to_string);
    }
    let name = name?;

    if let Some(brand) = properties
        .pointer("/brand/names/primary")
        .and_then(Value::as_str)
    {
        if normalize_text(brand) != normalize_text(&name) {
            aliases.push(brand.to_string());
        }
    }

    let mut categories = Vec::new();
    if let Some(primary) = properties
        .pointer("/categories/primary")
        .and_then(Value::as_str)
    {
        categories.push(primary.to_string());
    }
    for value in json_strings(properties.pointer("/categories/alternate")) {
        if !categories
            .iter()
            .any(|item| normalize_text(item) == normalize_text(&value))
        {
            categories.push(value);
        }
    }
    let primary_raw = categories.first().cloned();
    let primary_category = primary_raw.as_deref().map(category_label);
    let business_type = primary_raw
        .as_deref()
        .map(|value| format!("overture.category={value}"));

    let mut contacts = Vec::new();
    let mut first_phone = true;
    for value in json_strings(properties.get("phones")) {
        if let Some(phone) = normalize_phone(&value) {
            let mut contact = Contact::new(ContactKind::Phone, phone).with_source("overture");
            if first_phone {
                contact = contact.primary();
                first_phone = false;
            }
            contacts.push(contact);
        }
    }
    let mut first_email = true;
    for value in json_strings(properties.get("emails")) {
        if let Some(email) = normalize_email(&value) {
            let mut contact = Contact::new(ContactKind::Email, email).with_source("overture");
            if first_email {
                contact = contact.primary();
                first_email = false;
            }
            contacts.push(contact);
        }
    }
    let mut first_website = true;
    for value in json_strings(properties.get("websites")) {
        if let Some(url) = normalize_url(&value) {
            let mut contact = Contact::new(ContactKind::Website, url).with_source("overture");
            if first_website {
                contact = contact.primary();
                first_website = false;
            }
            contacts.push(contact);
        }
    }
    for value in social_urls(properties.get("socials")) {
        if let Some(kind) = social_kind(&value) {
            if let Some(url) = normalize_social(kind.as_str(), &value) {
                contacts.push(Contact::new(kind, url).with_source("overture"));
            }
        }
    }

    let mut address = Address::default();
    if let Some(addresses) = properties.get("addresses").and_then(Value::as_array) {
        if let Some(address_value) = addresses.first() {
            address.line = address_value
                .get("freeform")
                .and_then(Value::as_str)
                .map(ToString::to_string);
            address.municipio = address_value
                .get("locality")
                .and_then(Value::as_str)
                .map(ToString::to_string);
            address.departamento = address_value
                .get("region")
                .and_then(Value::as_str)
                .map(ToString::to_string);
            address.postal_code = address_value
                .get("postcode")
                .and_then(Value::as_str)
                .map(ToString::to_string);
            address.country_code =
                address_value
                    .get("country")
                    .and_then(Value::as_str)
                    .map(|value| {
                        if value.eq_ignore_ascii_case("nicaragua") {
                            "NI".to_string()
                        } else {
                            value.to_uppercase()
                        }
                    });
        }
    }
    if address.line.is_none() {
        address.line = properties
            .get("address")
            .and_then(Value::as_str)
            .map(ToString::to_string);
    }

    let status = properties
        .get("operating_status")
        .or_else(|| properties.get("status"))
        .and_then(Value::as_str)
        .map(
            |value| match value.to_lowercase().replace(' ', "_").as_str() {
                "open" | "operational" | "active" => "active".to_string(),
                "closed" | "permanently_closed" => "closed".to_string(),
                "temporarily_closed" => "closed_temporarily".to_string(),
                other => other.to_string(),
            },
        );

    let confidence = properties
        .get("confidence")
        .and_then(Value::as_f64)
        .unwrap_or(0.7)
        .clamp(0.3, 0.95);

    let id = properties
        .get("id")
        .and_then(Value::as_str)
        .map(ToString::to_string)
        .unwrap_or_else(|| {
            format!(
                "overture:{}",
                stable_hash(&format!("{}|{lat:.6}|{lon:.6}", normalize_text(&name)))
            )
        });

    let mut tags = std::collections::BTreeMap::new();
    tags.insert("overture.id".to_string(), id.clone());
    tags.insert(
        "overture.confidence".to_string(),
        format!("{confidence:.2}"),
    );
    if let Some(category) = primary_raw.as_deref() {
        tags.insert("overture.category".to_string(), category.to_string());
    }
    if let Some(release) = properties.get("version").and_then(Value::as_str) {
        tags.insert("overture.version".to_string(), release.to_string());
    }

    let description = properties
        .get("description")
        .and_then(Value::as_str)
        .map(ToString::to_string);

    let mut record = BusinessRecord::new("overture", name);
    record.external_id = Some(id);
    record.source_url = Some("https://overturemaps.org/".to_string());
    record.aliases = aliases;
    record.description = description;
    record.primary_category = primary_category;
    record.business_type = business_type;
    record.categories = categories;
    record.status = status;
    record.contacts = contacts;
    record.address = address;
    record.latitude = Some(lat);
    record.longitude = Some(lon);
    record.tags = tags;
    record.raw = feature.clone();
    record.confidence = confidence;
    record.fetched_at = Utc::now();
    Some(record)
}

fn json_strings(value: Option<&Value>) -> Vec<String> {
    match value {
        Some(Value::String(value)) => vec![value.trim().to_string()],
        Some(Value::Array(values)) => values
            .iter()
            .flat_map(|value| match value {
                Value::String(value) => vec![value.trim().to_string()],
                Value::Object(object) => object
                    .get("value")
                    .or_else(|| object.get("url"))
                    .or_else(|| object.get("phone"))
                    .and_then(Value::as_str)
                    .map(|value| vec![value.trim().to_string()])
                    .unwrap_or_default(),
                _ => Vec::new(),
            })
            .filter(|value| !value.is_empty())
            .collect(),
        _ => Vec::new(),
    }
}

fn social_urls(value: Option<&Value>) -> Vec<String> {
    match value {
        Some(Value::Array(values)) => values
            .iter()
            .flat_map(|value| match value {
                Value::String(value) => vec![value.clone()],
                Value::Object(object) => object
                    .get("url")
                    .or_else(|| object.get("value"))
                    .and_then(Value::as_str)
                    .map(|value| vec![value.to_string()])
                    .unwrap_or_default(),
                _ => Vec::new(),
            })
            .collect(),
        Some(Value::String(value)) => vec![value.clone()],
        _ => Vec::new(),
    }
}

fn social_kind(url: &str) -> Option<ContactKind> {
    let domain = domain_of(url)?;
    if domain.ends_with("facebook.com") {
        Some(ContactKind::Facebook)
    } else if domain.ends_with("instagram.com") {
        Some(ContactKind::Instagram)
    } else if domain.ends_with("tiktok.com") {
        Some(ContactKind::TikTok)
    } else if domain.ends_with("twitter.com") || domain == "x.com" {
        Some(ContactKind::Twitter)
    } else if domain.ends_with("t.me") {
        Some(ContactKind::Telegram)
    } else if domain.ends_with("linkedin.com") {
        Some(ContactKind::LinkedIn)
    } else if domain.ends_with("youtube.com") || domain.ends_with("youtu.be") {
        Some(ContactKind::YouTube)
    } else {
        None
    }
}

fn category_label(value: &str) -> String {
    let mapping = [
        ("restaurant", "Restaurante"),
        ("cafe", "Cafetería"),
        ("coffee_shop", "Cafetería"),
        ("bar", "Bar"),
        ("bakery", "Panadería"),
        ("pharmacy", "Farmacia"),
        ("hardware_store", "Ferretería"),
        ("supermarket", "Supermercado"),
        ("convenience_store", "Tienda de conveniencia"),
        ("hotel", "Hotel"),
        ("lodging", "Hospedaje"),
        ("clothing_store", "Ropa"),
        ("shoe_store", "Zapatería"),
        ("beauty_salon", "Salón de belleza"),
        ("hair_salon", "Barbería / peluquería"),
        ("car_repair", "Taller mecánico"),
        ("auto_parts_store", "Repuestos"),
        ("bank", "Banco"),
        ("gas_station", "Gasolinera"),
        ("dentist", "Dentista"),
        ("doctor", "Consultorio médico"),
        ("veterinarian", "Veterinaria"),
        ("school", "Colegio"),
        ("gym", "Gimnasio"),
        ("travel_agency", "Agencia de viajes"),
        ("real_estate_agency", "Inmobiliaria"),
        ("lawyer", "Abogado"),
        ("accountant", "Contador"),
    ];
    let key = value.to_lowercase();
    for (needle, label) in mapping {
        if key.contains(needle) {
            return label.to_string();
        }
    }
    let pretty = value.replace('_', " ");
    let mut chars = pretty.chars();
    match chars.next() {
        Some(first) => format!("{}{}", first.to_uppercase(), chars.as_str()),
        None => value.to_string(),
    }
}

fn matches_query(record: &BusinessRecord, query: &str) -> bool {
    let haystack = normalize_text(&format!(
        "{} {} {} {}",
        record.name,
        record.primary_category.as_deref().unwrap_or(""),
        record.categories.join(" "),
        record.tags.values().cloned().collect::<Vec<_>>().join(" ")
    ));
    normalize_text(query)
        .split_whitespace()
        .all(|token| haystack.contains(token))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::location::{GeoFilter, GeoPoint, ResolvedLocation};
    use std::path::PathBuf;

    fn request() -> ProviderRequest {
        ProviderRequest {
            location: ResolvedLocation {
                query: "Managua".into(),
                display_name: "Managua, Nicaragua".into(),
                departamento: Some("Managua".into()),
                municipio: Some("Managua".into()),
                barrio: None,
                comarca: None,
                filter: GeoFilter::Around {
                    lat: 12.1364,
                    lon: -86.2514,
                    radius_m: 5_000.0,
                },
                center: Some(GeoPoint {
                    lat: 12.1364,
                    lon: -86.2514,
                }),
                bbox: None,
                radius_m: Some(5_000.0),
                osm_type: None,
                osm_id: None,
                source: "test".into(),
            },
            query: None,
            category: None,
            limit: None,
            include_unnamed: false,
            page_size: 20,
            max_pages: 1,
        }
    }

    fn sample() -> Value {
        serde_json::json!({
            "type": "Feature",
            "geometry": {"type": "Point", "coordinates": [-86.2514, 12.1364]},
            "properties": {
                "id": "overture-1",
                "names": {"primary": "Café Las Flores"},
                "categories": {"primary": "coffee_shop", "alternate": ["restaurant"]},
                "confidence": 0.82,
                "phones": ["+505 8888-1111"],
                "emails": ["hola@lasflores.ni"],
                "websites": ["https://lasflores.ni"],
                "socials": ["https://instagram.com/lasflores"],
                "addresses": [{
                    "freeform": "Calle Colón 123",
                    "locality": "Managua",
                    "region": "Managua",
                    "country": "NI"
                }],
                "operating_status": "open"
            }
        })
    }

    #[test]
    fn parses_feature_collection_and_maps_fields() {
        let collection = serde_json::json!({"type": "FeatureCollection", "features": [sample()]});
        let features =
            parse_features(&collection.to_string(), &PathBuf::from("overture.geojson")).unwrap();
        assert_eq!(features.len(), 1);
        let record = feature_to_record(&features[0], -86.2514, 12.1364).unwrap();
        assert_eq!(record.name, "Café Las Flores");
        assert_eq!(record.primary_category.as_deref(), Some("Cafetería"));
        assert_eq!(record.contacts.len(), 4);
        assert_eq!(record.address.municipio.as_deref(), Some("Managua"));
        assert_eq!(record.status.as_deref(), Some("active"));
        assert_eq!(record.confidence, 0.82);
    }

    #[tokio::test]
    async fn filters_by_radius_and_category() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("overture.geojson");
        let collection = serde_json::json!({"type": "FeatureCollection", "features": [sample()]});
        std::fs::write(&path, collection.to_string()).unwrap();
        let mut req = request();
        req.category = Some("cafeteria".into());
        let records = fetch_file(&path, &req).await.unwrap();
        assert_eq!(records.len(), 1);

        req.category = Some("ferreteria".into());
        let records = fetch_file(&path, &req).await.unwrap();
        assert!(records.is_empty());
    }
}
