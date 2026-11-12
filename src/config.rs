//! Configuración global: rutas, claves, idioma y endpoints.

use std::env;
use std::path::PathBuf;
use std::time::Duration;

use anyhow::{Context, Result};
use directories::ProjectDirs;

use crate::i18n::Language;

pub const APP_NAME: &str = "businesscan";
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
    pub overture_file: Option<PathBuf>,
    pub user_agent: String,
    pub language: Language,
    pub http_timeout: Duration,
    pub overpass_timeout: Duration,
    pub nominatim_interval: Duration,
    pub enrich_delay_ms: u64,
    pub offline: bool,
}

impl Config {
    pub fn load(db_override: Option<PathBuf>) -> Result<Self> {
        let db_path = match db_override
            .or_else(|| env_any(&["BUSINESSCAN_DB", "NICASCAN_DB"]).map(PathBuf::from))
        {
            Some(path) => path,
            None => default_db_path()?,
        };

        let overpass_urls = env_any(&["BUSINESSCAN_OVERPASS_URL", "NICASCAN_OVERPASS_URL"])
            .map(|value| {
                value
                    .split(',')
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
            })
            .filter(|urls| !urls.is_empty())
            .unwrap_or_else(|| DEFAULT_OVERPASS.iter().map(|s| s.to_string()).collect());

        let nominatim_url = env_any(&["BUSINESSCAN_NOMINATIM_URL", "NICASCAN_NOMINATIM_URL"])
            .unwrap_or_else(|| DEFAULT_NOMINATIM.to_string());

        let user_agent = env_any(&["BUSINESSCAN_USER_AGENT", "NICASCAN_USER_AGENT"])
            .unwrap_or_else(|| {
                format!(
                    "businesscan/{} (+https://github.com/xozan845-ctrl/businesscan)",
                    crate::version::VERSION
                )
            });

        let google_places_key = env_any(&["GOOGLE_PLACES_API_KEY", "GOOGLE_MAPS_API_KEY"])
            .filter(|value| !value.trim().is_empty());
        let foursquare_key =
            env_any(&["FOURSQUARE_API_KEY"]).filter(|value| !value.trim().is_empty());

        Ok(Self {
            db_path,
            overpass_urls,
            nominatim_url,
            google_places_key,
            foursquare_key,
            overture_file: env_any(&["BUSINESSCAN_OVERTURE_FILE", "NICASCAN_OVERTURE_FILE"])
                .map(PathBuf::from),
            user_agent,
            language: Language::from_env(),
            http_timeout: Duration::from_secs(180),
            overpass_timeout: Duration::from_secs(240),
            nominatim_interval: Duration::from_millis(1_100),
            enrich_delay_ms: env_any(&["BUSINESSCAN_ENRICH_DELAY_MS", "NICASCAN_ENRICH_DELAY_MS"])
                .and_then(|value| value.parse().ok())
                .unwrap_or(2_000),
            offline: env_any(&["BUSINESSCAN_OFFLINE", "NICASCAN_OFFLINE"])
                .map(|value| matches!(value.as_str(), "1" | "true" | "yes"))
                .unwrap_or(false),
        })
    }

    pub fn default_db_path() -> Result<PathBuf> {
        default_db_path()
    }
}

fn env_any(names: &[&str]) -> Option<String> {
    for name in names {
        if let Ok(value) = env::var(name) {
            if !value.trim().is_empty() {
                return Some(value);
            }
        }
    }
    None
}

fn env_any_os(names: &[&str]) -> Option<std::ffi::OsString> {
    for name in names {
        if let Some(value) = env::var_os(name) {
            if !value.is_empty() {
                return Some(value);
            }
        }
    }
    None
}

fn default_db_path() -> Result<PathBuf> {
    if let Some(dirs) = ProjectDirs::from("com", "businesscan", APP_NAME) {
        return Ok(dirs.data_dir().join("businesscan.db"));
    }
    let home = env_any_os(&["HOME", "USERPROFILE"])
        .context("No se pudo determinar el directorio home; usa --db <ruta>")?;
    Ok(PathBuf::from(home)
        .join(".local")
        .join("share")
        .join(APP_NAME)
        .join("businesscan.db"))
}
