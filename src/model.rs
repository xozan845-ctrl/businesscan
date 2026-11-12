//! Modelos de dominio compartidos por proveedores, base de datos y exportadores.

use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Tipo de dato de contacto. Se guarda en SQLite como texto estable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContactKind {
    Phone,
    Mobile,
    Whatsapp,
    Email,
    Website,
    Facebook,
    Instagram,
    TikTok,
    Twitter,
    Telegram,
    YouTube,
    LinkedIn,
    Other,
}

impl ContactKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Phone => "phone",
            Self::Mobile => "mobile",
            Self::Whatsapp => "whatsapp",
            Self::Email => "email",
            Self::Website => "website",
            Self::Facebook => "facebook",
            Self::Instagram => "instagram",
            Self::TikTok => "tiktok",
            Self::Twitter => "twitter",
            Self::Telegram => "telegram",
            Self::YouTube => "youtube",
            Self::LinkedIn => "linkedin",
            Self::Other => "other",
        }
    }

    pub fn from_str_lossy(s: &str) -> Self {
        match s {
            "phone" => Self::Phone,
            "mobile" => Self::Mobile,
            "whatsapp" => Self::Whatsapp,
            "email" => Self::Email,
            "website" => Self::Website,
            "facebook" => Self::Facebook,
            "instagram" => Self::Instagram,
            "tiktok" => Self::TikTok,
            "twitter" => Self::Twitter,
            "telegram" => Self::Telegram,
            "youtube" => Self::YouTube,
            "linkedin" => Self::LinkedIn,
            _ => Self::Other,
        }
    }

    pub fn label_es(self) -> &'static str {
        match self {
            Self::Phone => "Teléfono",
            Self::Mobile => "Celular",
            Self::Whatsapp => "WhatsApp",
            Self::Email => "Correo",
            Self::Website => "Sitio web",
            Self::Facebook => "Facebook",
            Self::Instagram => "Instagram",
            Self::TikTok => "TikTok",
            Self::Twitter => "X / Twitter",
            Self::Telegram => "Telegram",
            Self::YouTube => "YouTube",
            Self::LinkedIn => "LinkedIn",
            Self::Other => "Otro",
        }
    }

    /// Los contactos que se pueden deduplicar por valor normalizado.
    pub fn is_phone_like(self) -> bool {
        matches!(self, Self::Phone | Self::Mobile | Self::Whatsapp)
    }

    pub fn is_url_like(self) -> bool {
        matches!(
            self,
            Self::Website
                | Self::Facebook
                | Self::Instagram
                | Self::TikTok
                | Self::Twitter
                | Self::Telegram
                | Self::YouTube
                | Self::LinkedIn
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Contact {
    pub kind: ContactKind,
    pub value: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub normalized: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(default)]
    pub is_primary: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
}

impl Contact {
    pub fn new(kind: ContactKind, value: impl Into<String>) -> Self {
        Self {
            kind,
            value: value.into(),
            normalized: None,
            label: None,
            is_primary: false,
            source: None,
        }
    }

    pub fn with_source(mut self, source: impl Into<String>) -> Self {
        self.source = Some(source.into());
        self
    }

    pub fn primary(mut self) -> Self {
        self.is_primary = true;
        self
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Address {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub street: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub house_number: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub barrio: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub comarca: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub municipio: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub departamento: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub postal_code: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub country_code: Option<String>,
}

impl Address {
    pub fn is_empty(&self) -> bool {
        self.line.is_none()
            && self.street.is_none()
            && self.house_number.is_none()
            && self.barrio.is_none()
            && self.comarca.is_none()
            && self.municipio.is_none()
            && self.departamento.is_none()
            && self.postal_code.is_none()
    }
}

/// Observación normalizada de un negocio, producida por una fuente.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BusinessRecord {
    /// Identificador de la fuente: `osm`, `google`, `foursquare`, `import`, `web`.
    pub source: String,
    /// Identificador dentro de la fuente (ej. `node/123`, `ChIJ...`, `fsq_id`).
    pub external_id: Option<String>,
    pub source_url: Option<String>,
    pub name: String,
    #[serde(default)]
    pub aliases: Vec<String>,
    pub legal_name: Option<String>,
    pub description: Option<String>,
    pub primary_category: Option<String>,
    pub business_type: Option<String>,
    #[serde(default)]
    pub categories: Vec<String>,
    pub status: Option<String>,
    #[serde(default)]
    pub contacts: Vec<Contact>,
    #[serde(default)]
    pub address: Address,
    pub latitude: Option<f64>,
    pub longitude: Option<f64>,
    pub opening_hours: Option<String>,
    pub price_range: Option<String>,
    pub rating: Option<f64>,
    pub review_count: Option<i64>,
    #[serde(default)]
    pub tags: BTreeMap<String, String>,
    #[serde(default)]
    pub raw: serde_json::Value,
    /// Confianza base de la fuente, 0..1.
    pub confidence: f64,
    pub fetched_at: DateTime<Utc>,
}

impl BusinessRecord {
    pub fn new(source: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            source: source.into(),
            external_id: None,
            source_url: None,
            name: name.into(),
            aliases: Vec::new(),
            legal_name: None,
            description: None,
            primary_category: None,
            business_type: None,
            categories: Vec::new(),
            status: None,
            contacts: Vec::new(),
            address: Address::default(),
            latitude: None,
            longitude: None,
            opening_hours: None,
            price_range: None,
            rating: None,
            review_count: None,
            tags: BTreeMap::new(),
            raw: serde_json::Value::Null,
            confidence: 0.5,
            fetched_at: Utc::now(),
        }
    }

    pub fn phone_values(&self) -> impl Iterator<Item = &str> {
        self.contacts
            .iter()
            .filter(|c| c.kind.is_phone_like())
            .map(|c| c.value.as_str())
    }
}

/// Negocio canónico tal como vive en SQLite.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Business {
    pub id: String,
    pub name: String,
    #[serde(skip_serializing)]
    pub name_norm: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub legal_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub primary_category: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub business_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub phone_primary: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub email_primary: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub website: Option<String>,
    pub address: Address,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latitude: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub longitude: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub geohash: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub opening_hours: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub price_range: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rating: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub review_count: Option<i64>,
    pub source_count: i64,
    pub confidence: f64,
    pub quality_score: f64,
    pub first_seen_at: String,
    pub last_seen_at: String,
    pub updated_at: String,
    #[serde(default, skip_serializing_if = "json_is_empty")]
    pub extra: serde_json::Value,
    /// Distancia en metros cuando la búsqueda usó un centro geográfico.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub distance_m: Option<f64>,
}

fn json_is_empty(value: &serde_json::Value) -> bool {
    value.is_null()
        || value
            .as_object()
            .map(|object| object.is_empty())
            .unwrap_or(false)
}

impl Business {
    pub fn location_label(&self) -> String {
        let mut parts = Vec::new();
        if let Some(v) = self.address.barrio.as_deref().filter(|s| !s.is_empty()) {
            parts.push(v);
        }
        if let Some(v) = self.address.comarca.as_deref().filter(|s| !s.is_empty()) {
            if !parts.iter().any(|p| p.eq_ignore_ascii_case(v)) {
                parts.push(v);
            }
        }
        if let Some(v) = self.address.municipio.as_deref().filter(|s| !s.is_empty()) {
            parts.push(v);
        }
        if let Some(v) = self
            .address
            .departamento
            .as_deref()
            .filter(|s| !s.is_empty())
        {
            parts.push(v);
        }
        if parts.is_empty() {
            self.address
                .country_code
                .clone()
                .unwrap_or_else(|| "Sin ubicación".to_string())
        } else {
            parts.join(", ")
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BusinessFull {
    #[serde(flatten)]
    pub business: Business,
    #[serde(default)]
    pub contacts: Vec<Contact>,
    #[serde(default)]
    pub aliases: Vec<String>,
    #[serde(default)]
    pub categories: Vec<String>,
    #[serde(default)]
    pub tags: BTreeMap<String, String>,
    #[serde(default)]
    pub sources: Vec<SourceRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceRecord {
    pub source: String,
    pub external_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    pub confidence: f64,
    pub first_seen_at: String,
    pub last_seen_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub raw: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpsertAction {
    Inserted,
    Merged,
    Skipped,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MatchReason {
    ExternalId,
    Phone,
    Website,
    NameAndProximity,
    NameAndAddress,
}

impl MatchReason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ExternalId => "external_id",
            Self::Phone => "phone",
            Self::Website => "website",
            Self::NameAndProximity => "name+proximity",
            Self::NameAndAddress => "name+address",
        }
    }
}

#[derive(Debug, Clone)]
pub struct UpsertResult {
    pub id: String,
    pub action: UpsertAction,
    pub match_reason: Option<MatchReason>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SortBy {
    #[default]
    Relevance,
    Name,
    Confidence,
    Quality,
    Updated,
    Distance,
}

#[derive(Debug, Clone, Default)]
pub struct SearchQuery {
    pub text: Option<String>,
    pub countries: Option<Vec<String>>,
    pub departamento: Option<String>,
    pub municipio: Option<String>,
    pub barrio: Option<String>,
    pub category: Option<String>,
    pub source: Option<String>,
    pub status: Option<String>,
    pub has_phone: bool,
    pub has_email: bool,
    pub has_website: bool,
    pub min_confidence: Option<f64>,
    pub min_quality: Option<f64>,
    pub updated_since: Option<String>,
    pub lat: Option<f64>,
    pub lon: Option<f64>,
    pub radius_m: Option<f64>,
    pub sort: SortBy,
    pub limit: usize,
    pub offset: usize,
    pub include_raw: bool,
}

impl SearchQuery {
    pub fn bounded_limit(&self, cap: usize) -> usize {
        if self.limit == 0 {
            cap
        } else {
            self.limit.min(cap)
        }
    }
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct ScanSourceSummary {
    pub source: String,
    pub fetched: usize,
    pub inserted: usize,
    pub merged: usize,
    pub skipped: usize,
    pub errors: Vec<String>,
    pub duration_ms: u64,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct ScanSummary {
    pub scan_id: String,
    pub location: String,
    pub sources: Vec<ScanSourceSummary>,
    pub total_fetched: usize,
    pub total_inserted: usize,
    pub total_merged: usize,
    pub total_skipped: usize,
    pub duration_ms: u64,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct Stats {
    pub total_businesses: i64,
    pub with_phone: i64,
    pub with_email: i64,
    pub with_website: i64,
    pub with_coordinates: i64,
    pub with_address: i64,
    pub avg_quality: f64,
    pub by_country: Vec<CountRow>,
    pub by_departamento: Vec<CountRow>,
    pub by_category: Vec<CountRow>,
    pub by_source: Vec<CountRow>,
    pub by_status: Vec<CountRow>,
    pub last_scans: Vec<ScanLogRow>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CountRow {
    pub label: String,
    pub count: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct ScanLogRow {
    pub id: String,
    pub started_at: String,
    pub finished_at: Option<String>,
    pub location: String,
    pub sources: String,
    pub query: Option<String>,
    pub fetched: i64,
    pub inserted: i64,
    pub merged: i64,
    pub errors: Option<String>,
}

/// Vista resumida para la revisión de duplicados.
#[derive(Debug, Clone, Serialize)]
pub struct DuplicateBusiness {
    pub id: String,
    pub name: String,
    pub primary_category: Option<String>,
    pub phone_primary: Option<String>,
    pub website: Option<String>,
    pub municipio: Option<String>,
    pub departamento: Option<String>,
    pub quality_score: f64,
    pub confidence: f64,
    pub source_count: i64,
}

impl From<&Business> for DuplicateBusiness {
    fn from(business: &Business) -> Self {
        Self {
            id: business.id.clone(),
            name: business.name.clone(),
            primary_category: business.primary_category.clone(),
            phone_primary: business.phone_primary.clone(),
            website: business.website.clone(),
            municipio: business.address.municipio.clone(),
            departamento: business.address.departamento.clone(),
            quality_score: business.quality_score,
            confidence: business.confidence,
            source_count: business.source_count,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct DuplicatePair {
    pub left: DuplicateBusiness,
    pub right: DuplicateBusiness,
    pub score: f64,
    pub distance_m: Option<f64>,
    pub reasons: Vec<String>,
}
