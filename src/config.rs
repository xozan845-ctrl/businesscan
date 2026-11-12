//! Configuración global: rutas, claves de API y endpoints.

use std::env;
use std::path::PathBuf;
use std::time::Duration;

use anyhow::{Context, Result};
use directories::ProjectDirs;

pub const APP_NAME: &str = "nicascan";
pub const DEFAULT_OVERPASS: &[&str] = &[
    "https://overpass-api.de/api/interpreter",
    "https://overpass.kumi.systems/api/interpreter",
];
pub const DEFAULT_NOMINATIM: &str = "https://nominatim.openstreetmap.org";

#[derive(Debug, Clone)]
pub struct Config {
    pub db_path: PathBuf,
    pub overpass_urls: Vec<String>,
    pub nominatim_url: String,
    pub google_places_key: Option<String>,
    pub foursquare_key: Option<String>,
    pub user_agent: String,
    pub language: String,
    pub http_timeout: Duration,
    pub overpass_timeout: Duration,
    pub nominatim_interval: Duration,
    pub offline: bool,
}

impl Config {
    pub fn load(db_override: Option<PathBuf>) -> Result<Self> {
        let db_path = match db_override.or_else(|| env::var_os("NICASCAN_DB").map(PathBuf::from)) {
            Some(path) => path,
            None => default_db_path()?,
        };

        let overpass_urls = env::var("NICASCAN_OVERPASS_URL")
            .ok()
            .map(|value| {
                value
                    .split(',')
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
            })
            .filter(|urls| !urls.is_empty())
            .unwrap_or_else(|| DEFAULT_OVERPASS.iter().map(|s| s.to_string()).collect());

        let nominatim_url =
            env::var("NICASCAN_NOMINATIM_URL").unwrap_or_else(|_| DEFAULT_NOMINATIM.to_string());

        let user_agent = env::var("NICASCAN_USER_AGENT").unwrap_or_else(|_| {
            format!(
                "nicascan/{} (+https://github.com/xozan845-ctrl/nicascan)",
                crate::version::VERSION
            )
        });

        let google_places_key = env::var("GOOGLE_PLACES_API_KEY")
            .ok()
            .or_else(|| env::var("GOOGLE_MAPS_API_KEY").ok())
            .filter(|s| !s.trim().is_empty());

        let foursquare_key = env::var("FOURSQUARE_API_KEY")
            .ok()
            .filter(|s| !s.trim().is_empty());

        Ok(Self {
            db_path,
            overpass_urls,
            nominatim_url,
            google_places_key,
            foursquare_key,
            user_agent,
            language: env::var("NICASCAN_LANGUAGE").unwrap_or_else(|_| "es".to_string()),
            http_timeout: Duration::from_secs(180),
            overpass_timeout: Duration::from_secs(240),
            nominatim_interval: Duration::from_millis(1_100),
            offline: env::var("NICASCAN_OFFLINE")
                .map(|v| matches!(v.as_str(), "1" | "true" | "yes"))
                .unwrap_or(false),
        })
    }

    pub fn default_db_path() -> Result<PathBuf> {
        default_db_path()
    }
}

fn default_db_path() -> Result<PathBuf> {
    if let Some(dirs) = ProjectDirs::from("ni", "nicascan", APP_NAME) {
        return Ok(dirs.data_dir().join("nicascan.db"));
    }
    let home = env::var_os("HOME")
        .or_else(|| env::var_os("USERPROFILE"))
        .context("No se pudo determinar el directorio home; usa --db <ruta>")?;
    Ok(PathBuf::from(home)
        .join(".local")
        .join("share")
        .join(APP_NAME)
        .join("nicascan.db"))
}
