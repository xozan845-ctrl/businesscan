//! Proveedores de datos. Cada fuente devuelve observaciones normalizadas.

pub mod categories;
pub mod foursquare;
pub mod google;
pub mod import;
pub mod overpass;
pub mod overture;
pub mod overture_pmtiles;

use anyhow::{bail, Result};
use async_trait::async_trait;

use crate::config::Config;
use crate::http::HttpClient;
use crate::location::ResolvedLocation;
use crate::model::BusinessRecord;

pub const SOURCE_OSM: &str = "osm";
pub const SOURCE_GOOGLE: &str = "google";
pub const SOURCE_FOURSQUARE: &str = "foursquare";
pub const SOURCE_OVERTURE: &str = "overture";

#[derive(Clone)]
pub struct ProviderContext {
    pub http: HttpClient,
    pub config: Config,
}

#[derive(Debug, Clone)]
pub struct ProviderRequest {
    pub location: ResolvedLocation,
    pub query: Option<String>,
    pub category: Option<String>,
    pub limit: Option<usize>,
    pub include_unnamed: bool,
    pub language: crate::i18n::Language,
    pub page_size: usize,
    pub max_pages: usize,
}

impl ProviderRequest {
    pub fn categories(&self) -> Vec<String> {
        self.category
            .as_deref()
            .unwrap_or("")
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(ToString::to_string)
            .collect()
    }

    pub fn text_query(&self) -> Option<&str> {
        self.query
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .or_else(|| {
                self.category
                    .as_deref()
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
            })
    }
}

#[async_trait]
pub trait Provider: Send + Sync {
    fn name(&self) -> &'static str;
    fn description(&self) -> &'static str;
    fn requires_key(&self) -> bool;
    fn key_env(&self) -> Option<&'static str> {
        None
    }
    fn is_available(&self, config: &Config) -> bool;
    /// Si se incluye cuando no se pide `--source` explícitamente.
    fn default_enabled(&self) -> bool {
        true
    }
    async fn fetch(
        &self,
        ctx: &ProviderContext,
        request: &ProviderRequest,
    ) -> Result<Vec<BusinessRecord>>;
}

pub fn provider_by_name(name: &str) -> Option<Box<dyn Provider>> {
    match name {
        SOURCE_OSM => Some(Box::new(overpass::OverpassProvider)),
        SOURCE_GOOGLE => Some(Box::new(google::GooglePlacesProvider)),
        SOURCE_FOURSQUARE => Some(Box::new(foursquare::FoursquareProvider)),
        SOURCE_OVERTURE => Some(Box::new(overture::OvertureProvider)),
        _ => None,
    }
}

pub fn all_providers() -> Vec<Box<dyn Provider>> {
    vec![
        Box::new(overpass::OverpassProvider),
        Box::new(google::GooglePlacesProvider),
        Box::new(foursquare::FoursquareProvider),
        Box::new(overture::OvertureProvider),
    ]
}

pub fn parse_sources(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| match s {
            "openstreetmap" | "open-street-map" | "overpass" => SOURCE_OSM.to_string(),
            "google-places" | "google_places" | "places" => SOURCE_GOOGLE.to_string(),
            "fsq" | "4sq" => SOURCE_FOURSQUARE.to_string(),
            "overture-maps" | "overturemaps" => SOURCE_OVERTURE.to_string(),
            other => other.to_lowercase(),
        })
        .collect()
}

pub fn resolve_sources(requested: &[String], config: &Config) -> Result<Vec<Box<dyn Provider>>> {
    let names: Vec<String> = if requested.is_empty() {
        all_providers()
            .into_iter()
            .filter(|p| p.is_available(config) && p.default_enabled())
            .map(|p| p.name().to_string())
            .collect()
    } else {
        requested.to_vec()
    };

    let mut providers = Vec::new();
    for name in names {
        let Some(provider) = provider_by_name(&name) else {
            bail!(
                "Fuente desconocida: «{name}». Usa `businesscan sources` para ver las disponibles"
            );
        };
        if !provider.is_available(config) {
            if provider.requires_key() {
                let env = provider.key_env().unwrap_or("la variable de entorno");
                bail!(
                    "La fuente «{name}» requiere una clave de API. Define {env} y vuelve a intentar"
                );
            }
            bail!(
                "La fuente «{name}» no está configurada. Para Overture usa --overture-file <archivo.geojson>"
            );
        }
        providers.push(provider);
    }

    if providers.is_empty() {
        bail!("No hay fuentes disponibles con la configuración actual");
    }
    Ok(providers)
}
