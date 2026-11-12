//! Fuente Foursquare Places API v3. Requiere `FOURSQUARE_API_KEY`.
//!
//! Aporta categorías, teléfono, sitio web, redes sociales, horarios, rating y
//! popularidad. Tiene un plan gratuito con cuota; revisa los términos de uso.

use anyhow::{bail, Context, Result};
use async_trait::async_trait;
use chrono::Utc;
use serde_json::{json, Value};

use crate::config::Config;
use crate::location::GeoFilter;
use crate::model::{Address, BusinessRecord, Contact, ContactKind};
use crate::providers::{Provider, ProviderContext, ProviderRequest};

const ENDPOINT: &str = "https://api.foursquare.com/v3/places/search";
const FIELDS: &str = "fsq_id,name,aliases,categories,distance,geocodes,location,tel,website,social_media,hours,rating,price,popularity,description,tips,photos,chains,attributes,closed_bucket,link";

pub struct FoursquareProvider;

#[async_trait]
impl Provider for FoursquareProvider {
    fn name(&self) -> &'static str {
        "foursquare"
    }

    fn description(&self) -> &'static str {
        "Foursquare Places: categorías, contacto, redes sociales y horarios (requiere API key)"
    }

    fn requires_key(&self) -> bool {
        true
    }

    fn key_env(&self) -> Option<&'static str> {
        Some("FOURSQUARE_API_KEY")
    }

    fn is_available(&self, config: &Config) -> bool {
        config.foursquare_key.is_some()
    }

    async fn fetch(
        &self,
        ctx: &ProviderContext,
        request: &ProviderRequest,
    ) -> Result<Vec<BusinessRecord>> {
        let key = ctx
            .config
            .foursquare_key
            .as_deref()
            .context("Falta FOURSQUARE_API_KEY")?;

        let text_query = request
            .text_query()
            .map(ToString::to_string)
            .unwrap_or_else(|| "negocios y servicios".to_string());

        let mut cursor: Option<String> = None;
        let mut records = Vec::new();

        for _ in 0..request.max_pages.max(1) {
            let near = request.location.provider_query();
            let mut params: Vec<(String, String)> = vec![
                ("query".to_string(), text_query.clone()),
                ("limit".to_string(), "50".to_string()),
                ("sort".to_string(), "RELEVANCE".to_string()),
                ("fields".to_string(), FIELDS.to_string()),
            ];
            match &request.location.filter {
                GeoFilter::Around { lat, lon, radius_m } => {
                    params.push(("ll".to_string(), format!("{lat},{lon}")));
                    params.push((
                        "radius".to_string(),
                        format!("{:.0}", radius_m.clamp(100.0, 100_000.0)),
                    ));
                }
                GeoFilter::Bbox {
                    south,
                    west,
                    north,
                    east,
                } => {
                    params.push(("sw".to_string(), format!("{south},{west}")));
                    params.push(("ne".to_string(), format!("{north},{east}")));
                }
                _ => {
                    params.push(("near".to_string(), near));
                }
            }
            if let Some(cursor) = cursor.as_deref() {
                params.push(("cursor".to_string(), cursor.to_string()));
            }

            let key_owned = key.to_string();
            let response = ctx
                .http
                .json::<Value, _>(|| {
                    ctx.http
                        .raw()
                        .get(ENDPOINT)
                        .header("Authorization", &key_owned)
                        .header("Accept", "application/json")
                        .query(&params)
                })
                .await
                .context("Foursquare Places falló")?;

            if let Some(error) = response.get("error") {
                bail!(
                    "Foursquare: {}",
                    error
                        .get("message")
                        .and_then(Value::as_str)
                        .unwrap_or("error desconocido")
                );
            }

            if let Some(results) = response.get("results").and_then(Value::as_array) {
                for place in results {
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

            cursor = response
                .pointer("/context/next_cursor")
                .or_else(|| response.get("next_cursor"))
                .and_then(Value::as_str)
                .map(ToString::to_string);
            if cursor.is_none() {
                break;
            }
        }

        Ok(records)
    }
}

pub fn map_place(place: &Value, request: &ProviderRequest) -> Option<BusinessRecord> {
    let name = place
        .get("name")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())?
        .to_string();
    let id = place.get("fsq_id").and_then(Value::as_str)?.to_string();

    let categories: Vec<String> = place
        .get("categories")
        .and_then(Value::as_array)
        .map(|values| {
            values
                .iter()
                .filter_map(|c| c.get("name").and_then(Value::as_str))
                .map(ToString::to_string)
                .collect()
        })
        .unwrap_or_default();
    let primary_category = categories.first().cloned();

    let aliases = place
        .get("aliases")
        .and_then(Value::as_array)
        .map(|values| {
            values
                .iter()
                .filter_map(Value::as_str)
                .map(ToString::to_string)
                .collect()
        })
        .unwrap_or_default();

    let mut contacts = Vec::new();
    if let Some(tel) = place.get("tel").and_then(Value::as_str) {
        let mut first = true;
        for value in tel
            .split([';', ','])
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            let mut contact = Contact::new(ContactKind::Phone, value).with_source("foursquare");
            if first {
                contact = contact.primary();
                first = false;
            }
            contacts.push(contact);
        }
    }
    if let Some(website) = place.get("website").and_then(Value::as_str) {
        contacts.push(
            Contact::new(ContactKind::Website, website)
                .with_source("foursquare")
                .primary(),
        );
    }
    if let Some(social) = place.get("social_media").and_then(Value::as_object) {
        for (key, kind) in [
            ("facebook_id", ContactKind::Facebook),
            ("instagram", ContactKind::Instagram),
            ("twitter", ContactKind::Twitter),
        ] {
            if let Some(value) = social.get(key).and_then(Value::as_str) {
                contacts.push(Contact::new(kind, value).with_source("foursquare"));
            }
        }
    }

    let location = place.get("location").unwrap_or(&Value::Null);
    let mut address = Address {
        line: location
            .get("formatted_address")
            .or_else(|| location.get("address"))
            .and_then(Value::as_str)
            .map(ToString::to_string),
        street: location
            .get("address")
            .and_then(Value::as_str)
            .map(ToString::to_string),
        house_number: None,
        barrio: location
            .get("neighborhood")
            .and_then(Value::as_str)
            .map(ToString::to_string),
        comarca: None,
        municipio: location
            .get("locality")
            .and_then(Value::as_str)
            .map(ToString::to_string),
        departamento: location
            .get("region")
            .and_then(Value::as_str)
            .map(ToString::to_string),
        postal_code: location
            .get("postcode")
            .and_then(Value::as_str)
            .map(ToString::to_string),
        country_code: location
            .get("country")
            .and_then(Value::as_str)
            .map(|s| s.to_uppercase())
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

    let latitude = place
        .pointer("/geocodes/main/latitude")
        .and_then(Value::as_f64);
    let longitude = place
        .pointer("/geocodes/main/longitude")
        .and_then(Value::as_f64);

    let opening_hours = place
        .pointer("/hours/display")
        .and_then(Value::as_str)
        .map(ToString::to_string)
        .or_else(|| {
            place
                .pointer("/hours/regular")
                .and_then(Value::as_array)
                .map(|days| {
                    days.iter()
                        .filter_map(|day| {
                            let label = day.get("days").and_then(Value::as_str)?;
                            let open = day.get("open").and_then(Value::as_str).unwrap_or("");
                            let close = day.get("close").and_then(Value::as_str).unwrap_or("");
                            Some(format!("{label}: {open}-{close}"))
                        })
                        .collect::<Vec<_>>()
                        .join("; ")
                })
                .filter(|s| !s.is_empty())
        });

    let rating_10 = place.get("rating").and_then(Value::as_f64);
    let rating = rating_10.map(|value| (value / 2.0 * 10.0).round() / 10.0);
    let price = place.get("price").and_then(Value::as_i64);
    let price_range = price.map(|value| "$".repeat(value.clamp(1, 4) as usize));
    let popularity = place.get("popularity").and_then(Value::as_f64);
    let review_count = place
        .get("tips")
        .and_then(Value::as_array)
        .map(|tips| tips.len() as i64);

    let status = place
        .get("closed_bucket")
        .and_then(Value::as_str)
        .map(|value| match value {
            "VeryLikelyClosed" | "LikelyClosed" => "closed",
            "VeryLikelyOpen" | "LikelyOpen" => "active",
            _ => "unknown",
        })
        .map(ToString::to_string);

    let mut tags = std::collections::BTreeMap::new();
    tags.insert("foursquare.id".to_string(), id.clone());
    if let Some(categories) = place.get("categories").and_then(Value::as_array) {
        let ids = categories
            .iter()
            .filter_map(|c| c.get("id").and_then(Value::as_str))
            .collect::<Vec<_>>()
            .join(",");
        if !ids.is_empty() {
            tags.insert("foursquare.category_ids".to_string(), ids);
        }
    }
    if let Some(value) = place.get("closed_bucket").and_then(Value::as_str) {
        tags.insert("foursquare.closed_bucket".to_string(), value.to_string());
    }
    if let Some(value) = popularity {
        tags.insert("foursquare.popularity".to_string(), format!("{value:.2}"));
    }
    if let Some(value) = rating_10 {
        tags.insert("foursquare.rating_10".to_string(), format!("{value:.1}"));
    }
    if let Some(value) = price {
        tags.insert("foursquare.price".to_string(), value.to_string());
    }
    if let Some(distance) = place.get("distance").and_then(Value::as_i64) {
        tags.insert("foursquare.distance_m".to_string(), distance.to_string());
    }
    if let Some(chains) = place.get("chains").and_then(Value::as_array) {
        let names = chains
            .iter()
            .filter_map(|c| c.get("name").and_then(Value::as_str))
            .collect::<Vec<_>>()
            .join(",");
        if !names.is_empty() {
            tags.insert("foursquare.chains".to_string(), names);
        }
    }
    if let Some(attributes) = place.get("attributes").and_then(Value::as_object) {
        for (key, value) in attributes {
            if let Some(value) = value.as_str() {
                tags.insert(format!("foursquare.{key}"), value.to_string());
            } else if let Some(value) = value.as_bool() {
                tags.insert(format!("foursquare.{key}"), value.to_string());
            }
        }
    }

    let source_url = place
        .get("link")
        .and_then(Value::as_str)
        .map(ToString::to_string)
        .or_else(|| Some(format!("https://foursquare.com/v/{id}")));

    Some(BusinessRecord {
        source: "foursquare".to_string(),
        external_id: Some(id),
        source_url,
        name,
        aliases,
        legal_name: None,
        description: place
            .get("description")
            .and_then(Value::as_str)
            .map(ToString::to_string),
        primary_category,
        business_type: Some("foursquare.category".to_string()),
        categories,
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
        confidence: 0.8,
        fetched_at: Utc::now(),
    })
}

/// El endpoint de búsqueda también acepta filtros por categoría de Foursquare.
pub fn search_body(query: &str) -> Value {
    json!({
        "query": query,
        "limit": 50,
        "sort": "RELEVANCE"
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::location::{GeoPoint, ResolvedLocation};

    fn request() -> ProviderRequest {
        ProviderRequest {
            location: ResolvedLocation {
                query: "Granada".into(),
                display_name: "Granada, Nicaragua".into(),
                departamento: Some("Granada".into()),
                municipio: Some("Granada".into()),
                barrio: None,
                comarca: None,
                filter: GeoFilter::Area { relation_id: 1 },
                center: Some(GeoPoint {
                    lat: 11.93,
                    lon: -85.95,
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
            page_size: 50,
            max_pages: 1,
        }
    }

    #[test]
    fn maps_foursquare_place() {
        let place: Value = serde_json::from_str(
            r#"{
              "fsq_id": "abc123",
              "name": "Hotel Colonial",
              "aliases": ["Colonial"],
              "categories": [{"id": 19014, "name": "Hotel"}],
              "distance": 120,
              "geocodes": {"main": {"latitude": 11.929, "longitude": -85.954}},
              "location": {
                "address": "Calle La Calzada",
                "locality": "Granada",
                "region": "Granada",
                "country": "NI",
                "formatted_address": "Calle La Calzada, Granada, Nicaragua"
              },
              "tel": "+505 2552 1234",
              "website": "https://hotelcolonial.ni",
              "social_media": {"instagram": "hotelcolonial"},
              "hours": {"display": "Lun-Dom 8:00-22:00"},
              "rating": 9.2,
              "price": 3,
              "popularity": 0.82,
              "closed_bucket": "LikelyOpen",
              "link": "https://foursquare.com/v/abc123"
            }"#,
        )
        .unwrap();
        let record = map_place(&place, &request()).unwrap();
        assert_eq!(record.name, "Hotel Colonial");
        assert_eq!(record.primary_category.as_deref(), Some("Hotel"));
        assert_eq!(record.rating, Some(4.6));
        assert_eq!(record.price_range.as_deref(), Some("$$$"));
        assert_eq!(record.address.municipio.as_deref(), Some("Granada"));
        assert!(record
            .contacts
            .iter()
            .any(|c| c.kind == ContactKind::Instagram));
    }
}
