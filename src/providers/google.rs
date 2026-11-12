//! Fuente Google Places API (New). Requiere `GOOGLE_PLACES_API_KEY`.
//!
//! Es la fuente con datos más ricos (rating, reseñas, horarios, tipos y
//! atributos), pero es de pago y tiene cuotas. Úsala con `--limit` y `--category`
//! para controlar el costo.

use anyhow::{bail, Context, Result};
use async_trait::async_trait;
use chrono::Utc;
use serde_json::{json, Value};
use url::Url;

use crate::config::Config;
use crate::model::{Address, BusinessRecord, Contact, ContactKind};
use crate::providers::{Provider, ProviderContext, ProviderRequest};

const ENDPOINT: &str = "https://places.googleapis.com/v1/places:searchText";

pub struct GooglePlacesProvider;

#[async_trait]
impl Provider for GooglePlacesProvider {
    fn name(&self) -> &'static str {
        "google"
    }

    fn description(&self) -> &'static str {
        "Google Places: rating, reseñas, horarios y atributos (requiere API key y facturación)"
    }

    fn requires_key(&self) -> bool {
        true
    }

    fn key_env(&self) -> Option<&'static str> {
        Some("GOOGLE_PLACES_API_KEY")
    }

    fn is_available(&self, config: &Config) -> bool {
        config.google_places_key.is_some()
    }

    async fn fetch(
        &self,
        ctx: &ProviderContext,
        request: &ProviderRequest,
    ) -> Result<Vec<BusinessRecord>> {
        let key = ctx
            .config
            .google_places_key
            .as_deref()
            .context("Falta GOOGLE_PLACES_API_KEY")?;

        let mut queries = Vec::new();
        if let Some(query) = request
            .query
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            queries.push(query.to_string());
        } else {
            let categories = request.categories();
            if categories.is_empty() {
                queries.push("negocios y servicios".to_string());
            } else {
                queries.extend(categories);
            }
        }

        let mut records = Vec::new();
        for query in queries {
            let text_query = format!("{query} en {}", request.location.provider_query());
            let mut page_token: Option<String> = None;
            for _ in 0..request.max_pages.max(1) {
                let body = build_body(&text_query, request, page_token.as_deref());
                let key_owned = key.to_string();
                let response = ctx
                    .http
                    .json::<Value, _>(|| {
                        ctx.http
                            .raw()
                            .post(ENDPOINT)
                            .header("X-Goog-Api-Key", &key_owned)
                            .header("X-Goog-FieldMask", "*")
                            .json(&body)
                    })
                    .await
                    .with_context(|| format!("Google Places falló para «{text_query}»"))?;

                if let Some(error) = response.get("error") {
                    bail!(
                        "Google Places: {}",
                        error
                            .get("message")
                            .and_then(Value::as_str)
                            .unwrap_or("error desconocido")
                    );
                }

                if let Some(places) = response.get("places").and_then(Value::as_array) {
                    for place in places {
                        if let Some(record) = map_place(place, request) {
                            records.push(record);
                            if let Some(limit) = request.limit {
                                if records.len() >= limit {
                                    return Ok(records);
                                }
                            }
                        }
                    }
                }

                page_token = response
                    .get("nextPageToken")
                    .and_then(Value::as_str)
                    .map(ToString::to_string);
                if page_token.is_none() {
                    break;
                }
            }
        }

        Ok(records)
    }
}

fn build_body(text_query: &str, request: &ProviderRequest, page_token: Option<&str>) -> Value {
    let mut body = json!({
        "textQuery": text_query,
        "languageCode": request.language.code(),
        "pageSize": 20,
        "rankPreference": "RELEVANCE"
    });
    if let Some(country) = request.location.country_code.as_deref() {
        body["regionCode"] = json!(country.to_uppercase());
    }

    if let Some(token) = page_token {
        body["pageToken"] = json!(token);
    }

    match &request.location.filter {
        crate::location::GeoFilter::Around { lat, lon, radius_m } => {
            body["locationRestriction"] = json!({
                "circle": {
                    "center": { "latitude": lat, "longitude": lon },
                    "radius": radius_m.clamp(100.0, 50_000.0)
                }
            });
        }
        _ => {
            if let Some(bbox) = request.location.bbox {
                let area = (bbox.north - bbox.south) * (bbox.east - bbox.west);
                if area <= 25.0 {
                    body["locationBias"] = json!({
                        "rectangle": {
                            "low": { "latitude": bbox.south, "longitude": bbox.west },
                            "high": { "latitude": bbox.north, "longitude": bbox.east }
                        }
                    });
                }
            }
        }
    }
    body
}

pub fn map_place(place: &Value, request: &ProviderRequest) -> Option<BusinessRecord> {
    let name = place
        .pointer("/displayName/text")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())?
        .to_string();

    let id = place
        .get("id")
        .and_then(Value::as_str)
        .or_else(|| {
            place
                .get("name")
                .and_then(Value::as_str)
                .map(|n| n.trim_start_matches("places/"))
        })
        .map(ToString::to_string)?;

    let types: Vec<String> = place
        .get("types")
        .and_then(Value::as_array)
        .map(|values| {
            values
                .iter()
                .filter_map(Value::as_str)
                .map(ToString::to_string)
                .collect()
        })
        .unwrap_or_default();

    let primary_type = place
        .get("primaryType")
        .and_then(Value::as_str)
        .map(ToString::to_string)
        .or_else(|| types.first().cloned());
    let primary_category = place
        .pointer("/primaryTypeDisplayName/text")
        .and_then(Value::as_str)
        .map(ToString::to_string)
        .or_else(|| primary_type.as_deref().map(humanize_type));

    let status = place
        .get("businessStatus")
        .and_then(Value::as_str)
        .map(|status| match status {
            "OPERATIONAL" => "active".to_string(),
            "CLOSED_TEMPORARILY" => "closed_temporarily".to_string(),
            "CLOSED_PERMANENTLY" => "closed".to_string(),
            other => other.to_lowercase(),
        });

    let mut contacts = Vec::new();
    if let Some(phone) = place.get("nationalPhoneNumber").and_then(Value::as_str) {
        contacts.push(
            Contact::new(ContactKind::Phone, phone)
                .with_source("google")
                .primary(),
        );
    }
    if let Some(phone) = place
        .get("internationalPhoneNumber")
        .and_then(Value::as_str)
    {
        if !contacts.iter().any(|c| {
            crate::normalize::phone_key_for(&c.value, request.location.country_code.as_deref())
                == crate::normalize::phone_key_for(phone, request.location.country_code.as_deref())
        }) {
            contacts.push(Contact::new(ContactKind::Phone, phone).with_source("google"));
        }
    }
    if let Some(website) = place.get("websiteUri").and_then(Value::as_str) {
        contacts.push(
            Contact::new(ContactKind::Website, website)
                .with_source("google")
                .primary(),
        );
    }

    let mut address = Address {
        line: place
            .get("formattedAddress")
            .and_then(Value::as_str)
            .map(ToString::to_string),
        country_code: request.location.country_code.clone(),
        ..Default::default()
    };
    if let Some(components) = place.get("addressComponents").and_then(Value::as_array) {
        for component in components {
            let long = component.get("longText").and_then(Value::as_str);
            let short = component.get("shortText").and_then(Value::as_str);
            let types: Vec<&str> = component
                .get("types")
                .and_then(Value::as_array)
                .map(|types| types.iter().filter_map(Value::as_str).collect())
                .unwrap_or_default();
            if types.contains(&"sublocality_level_1")
                || types.contains(&"sublocality")
                || types.contains(&"neighborhood")
            {
                address.barrio = address.barrio.or_else(|| long.map(ToString::to_string));
            } else if types.contains(&"locality")
                || types.contains(&"postal_town")
                || types.contains(&"administrative_area_level_2")
            {
                address.municipio = address.municipio.or_else(|| long.map(ToString::to_string));
            } else if types.contains(&"administrative_area_level_1") {
                address.departamento = address
                    .departamento
                    .or_else(|| long.map(ToString::to_string));
            } else if types.contains(&"route") {
                address.street = address.street.or_else(|| long.map(ToString::to_string));
            } else if types.contains(&"street_number") {
                address.house_number = address
                    .house_number
                    .or_else(|| long.map(ToString::to_string));
            } else if types.contains(&"postal_code") {
                address.postal_code = address
                    .postal_code
                    .or_else(|| long.map(ToString::to_string));
            } else if types.contains(&"country") {
                address.country_code = short
                    .and_then(|value| {
                        crate::gazetteer::Gazetteer::global().normalize_country_code(value)
                    })
                    .or_else(|| {
                        long.and_then(|value| {
                            crate::gazetteer::Gazetteer::global().normalize_country_code(value)
                        })
                    });
            }
        }
    }
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

    let latitude = place.pointer("/location/latitude").and_then(Value::as_f64);
    let longitude = place.pointer("/location/longitude").and_then(Value::as_f64);

    let opening_hours = place
        .pointer("/regularOpeningHours/weekdayDescriptions")
        .and_then(Value::as_array)
        .map(|days| {
            days.iter()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>()
                .join("; ")
        })
        .filter(|s| !s.is_empty());

    let price_range = place
        .get("priceLevel")
        .and_then(Value::as_str)
        .map(|level| match level {
            "PRICE_LEVEL_FREE" => "Gratis".to_string(),
            "PRICE_LEVEL_INEXPENSIVE" => "$".to_string(),
            "PRICE_LEVEL_MODERATE" => "$$".to_string(),
            "PRICE_LEVEL_EXPENSIVE" => "$$$".to_string(),
            "PRICE_LEVEL_VERY_EXPENSIVE" => "$$$$".to_string(),
            other => other.to_string(),
        });

    let rating = place.get("rating").and_then(Value::as_f64);
    let review_count = place.get("userRatingCount").and_then(Value::as_i64);

    let description = place
        .pointer("/editorialSummary/text")
        .and_then(Value::as_str)
        .or_else(|| {
            place
                .pointer("/generativeSummary/overview/text")
                .and_then(Value::as_str)
        })
        .map(ToString::to_string);

    let mut tags = std::collections::BTreeMap::new();
    tags.insert(
        "google.primary_type".to_string(),
        primary_type.clone().unwrap_or_default(),
    );
    tags.insert("google.types".to_string(), types.join(","));
    if let Some(status) = &status {
        tags.insert("google.business_status".to_string(), status.clone());
    }
    if let Some(price) = &price_range {
        tags.insert("google.price_level".to_string(), price.clone());
    }
    if let Some(code) = place
        .pointer("/plusCode/globalCode")
        .and_then(Value::as_str)
    {
        tags.insert("google.plus_code".to_string(), code.to_string());
    }
    if let Some(url) = place.get("googleMapsUri").and_then(Value::as_str) {
        tags.insert("google.maps_url".to_string(), url.to_string());
    }
    for key in [
        "takeout",
        "delivery",
        "dineIn",
        "reservable",
        "servesBreakfast",
        "servesLunch",
        "servesDinner",
        "servesBeer",
        "servesWine",
        "servesCocktails",
        "servesVegetarianFood",
        "outdoorSeating",
        "goodForChildren",
        "goodForGroups",
        "goodForWatchingSports",
        "allowsDogs",
        "restroom",
    ] {
        if let Some(value) = place.get(key).and_then(Value::as_bool) {
            tags.insert(format!("google.{key}"), value.to_string());
        }
    }
    for group in ["paymentOptions", "parkingOptions", "accessibilityOptions"] {
        if let Some(object) = place.get(group).and_then(Value::as_object) {
            for (key, value) in object {
                if let Some(value) = value.as_bool() {
                    tags.insert(format!("google.{group}.{key}"), value.to_string());
                }
            }
        }
    }

    let source_url = place
        .get("googleMapsUri")
        .and_then(Value::as_str)
        .map(ToString::to_string)
        .or_else(|| {
            Some(format!(
                "https://www.google.com/maps/place/?q=place_id:{id}"
            ))
        });

    Some(BusinessRecord {
        source: "google".to_string(),
        external_id: Some(id),
        source_url,
        name,
        aliases: Vec::new(),
        legal_name: None,
        description,
        primary_category,
        business_type: primary_type,
        categories: types,
        status,
        contacts,
        address,
        latitude,
        longitude,
        opening_hours,
        price_range,
        rating,
        review_count,
        tags,
        raw: place.clone(),
        confidence: 0.9,
        fetched_at: Utc::now(),
    })
}

fn humanize_type(value: &str) -> String {
    let spaced = value.replace('_', " ");
    let mut chars = spaced.chars();
    match chars.next() {
        Some(first) => format!("{}{}", first.to_uppercase(), chars.as_str()),
        None => spaced,
    }
}

/// Extrae el dominio de un sitio web de Google, útil para completar datos.
pub fn website_domain(place: &Value) -> Option<String> {
    let raw = place.get("websiteUri").and_then(Value::as_str)?;
    Url::parse(raw).ok()?.host_str().map(ToString::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::location::{GeoFilter, GeoPoint, ResolvedLocation};

    fn request() -> ProviderRequest {
        ProviderRequest {
            location: ResolvedLocation {
                query: "Managua".into(),
                display_name: "Managua, Nicaragua".into(),
                country_code: Some("NI".into()),
                country_name: Some("Nicaragua".into()),
                continent: Some("North America".into()),
                departamento: Some("Managua".into()),
                municipio: Some("Managua".into()),
                barrio: None,
                comarca: None,
                filter: GeoFilter::Area { relation_id: 1 },
                center: Some(GeoPoint {
                    lat: 12.13,
                    lon: -86.25,
                }),
                bbox: None,
                radius_m: None,
                osm_type: None,
                osm_id: None,
                source: "test".into(),
            },
            query: None,
            category: None,
            limit: None,
            include_unnamed: false,
            language: crate::i18n::Language::Es,
            page_size: 20,
            max_pages: 1,
        }
    }

    #[test]
    fn maps_google_place() {
        let place: Value = serde_json::from_str(
            r#"{
              "id": "ChIJtest",
              "displayName": {"text": "Café Las Flores", "languageCode": "es"},
              "formattedAddress": "Calle Colón, Managua, Nicaragua",
              "location": {"latitude": 12.136, "longitude": -86.251},
              "types": ["cafe", "food", "establishment"],
              "primaryType": "cafe",
              "primaryTypeDisplayName": {"text": "Cafetería"},
              "nationalPhoneNumber": "2222 3333",
              "internationalPhoneNumber": "+505 2222 3333",
              "websiteUri": "https://lasflores.ni",
              "googleMapsUri": "https://maps.google.com/?cid=1",
              "businessStatus": "OPERATIONAL",
              "rating": 4.6,
              "userRatingCount": 120,
              "priceLevel": "PRICE_LEVEL_MODERATE",
              "regularOpeningHours": {"weekdayDescriptions": ["lunes: 8:00–18:00"]},
              "addressComponents": [
                {"longText": "Managua", "shortText": "Managua", "types": ["locality"]},
                {"longText": "Managua", "shortText": "Managua", "types": ["administrative_area_level_1"]},
                {"longText": "Nicaragua", "shortText": "NI", "types": ["country"]}
              ],
              "paymentOptions": {"acceptsCreditCards": true}
            }"#,
        )
        .unwrap();
        let record = map_place(&place, &request()).unwrap();
        assert_eq!(record.name, "Café Las Flores");
        assert_eq!(record.primary_category.as_deref(), Some("Cafetería"));
        assert_eq!(record.rating, Some(4.6));
        assert_eq!(record.review_count, Some(120));
        assert_eq!(record.address.municipio.as_deref(), Some("Managua"));
        assert!(record
            .contacts
            .iter()
            .any(|c| c.kind == ContactKind::Website));
        assert_eq!(record.price_range.as_deref(), Some("$$"));
    }

    #[test]
    fn builds_circle_restriction() {
        let mut req = request();
        req.location.filter = GeoFilter::Around {
            lat: 12.1,
            lon: -86.2,
            radius_m: 500.0,
        };
        let body = build_body("cafés", &req, None);
        assert_eq!(
            body.pointer("/locationRestriction/circle/radius")
                .and_then(Value::as_f64),
            Some(500.0)
        );
    }
}
