//! API HTTP local para que un agente de IA consulte el directorio.

use std::net::SocketAddr;
use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};
use tokio::sync::Mutex;
use tower_http::cors::CorsLayer;

use crate::config::Config;
use crate::db::business::{get_full, merge_businesses, search, search_full};
use crate::db::Database;
use crate::model::{SearchQuery, SortBy};
use crate::version::VERSION;

#[derive(Clone)]
pub struct AppState {
    pub db: Arc<Mutex<Database>>,
    pub config: Config,
    pub token: Option<String>,
}

pub fn build_router(state: AppState) -> Router {
    Router::new()
        .route("/", get(root))
        .route("/v1/health", get(health))
        .route("/v1/businesses", get(list_businesses))
        .route("/v1/businesses/{id}", get(show_business))
        .route("/v1/search", post(search_post))
        .route("/v1/export", get(export_handler))
        .route("/v1/stats", get(stats_handler))
        .route("/v1/sources", get(sources_handler))
        .route("/v1/duplicates", get(duplicates_handler))
        .route("/v1/merge", post(merge_handler))
        .route("/panel", get(panel))
        .route("/openapi.json", get(openapi))
        .layer(CorsLayer::permissive())
        .with_state(state)
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct SearchParams {
    #[serde(default)]
    pub q: Option<String>,
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub departamento: Option<String>,
    #[serde(default)]
    pub municipio: Option<String>,
    #[serde(default)]
    pub barrio: Option<String>,
    #[serde(default)]
    pub category: Option<String>,
    #[serde(default)]
    pub source: Option<String>,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub has_phone: Option<bool>,
    #[serde(default)]
    pub has_email: Option<bool>,
    #[serde(default)]
    pub has_website: Option<bool>,
    #[serde(default)]
    pub min_confidence: Option<f64>,
    #[serde(default)]
    pub min_quality: Option<f64>,
    #[serde(default)]
    pub updated_since: Option<String>,
    #[serde(default)]
    pub lat: Option<f64>,
    #[serde(default)]
    pub lon: Option<f64>,
    #[serde(default)]
    pub radius: Option<f64>,
    #[serde(default)]
    pub sort: Option<String>,
    #[serde(default)]
    pub limit: Option<usize>,
    #[serde(default)]
    pub offset: Option<usize>,
    #[serde(default)]
    pub full: Option<bool>,
    #[serde(default)]
    pub raw: Option<bool>,
}

impl SearchParams {
    fn to_query(&self) -> Result<SearchQuery, ApiError> {
        if self.lat.is_some() != self.lon.is_some() {
            return Err(ApiError::bad_request("lat y lon deben usarse juntos"));
        }
        if let Some(value) = self.min_confidence {
            if !(0.0..=1.0).contains(&value) {
                return Err(ApiError::bad_request(
                    "min_confidence debe estar entre 0 y 1",
                ));
            }
        }
        if let Some(value) = self.min_quality {
            if !(0.0..=1.0).contains(&value) {
                return Err(ApiError::bad_request("min_quality debe estar entre 0 y 1"));
            }
        }
        let sort = match self.sort.as_deref().unwrap_or("relevance") {
            "relevance" => SortBy::Relevance,
            "name" => SortBy::Name,
            "confidence" => SortBy::Confidence,
            "quality" => SortBy::Quality,
            "updated" => SortBy::Updated,
            "distance" => SortBy::Distance,
            other => {
                return Err(ApiError::bad_request(format!(
                    "sort inválido: {other}; usa relevance, name, confidence, quality, updated o distance"
                )))
            }
        };
        let text = self
            .q
            .clone()
            .or_else(|| self.text.clone())
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty());
        Ok(SearchQuery {
            text,
            departamento: self.departamento.clone(),
            municipio: self.municipio.clone(),
            barrio: self.barrio.clone(),
            category: self.category.clone(),
            source: self.source.clone(),
            status: self.status.clone(),
            has_phone: self.has_phone.unwrap_or(false),
            has_email: self.has_email.unwrap_or(false),
            has_website: self.has_website.unwrap_or(false),
            min_confidence: self.min_confidence,
            min_quality: self.min_quality,
            updated_since: self.updated_since.clone(),
            lat: self.lat,
            lon: self.lon,
            radius_m: self.radius.or(Some(1_000.0)).filter(|_| self.lat.is_some()),
            sort,
            limit: self.limit.unwrap_or(25).min(500),
            offset: self.offset.unwrap_or(0),
            include_raw: self.raw.unwrap_or(false),
        })
    }
}

#[derive(Debug)]
pub struct ApiError {
    status: StatusCode,
    message: String,
}

impl ApiError {
    pub fn bad_request(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            message: message.into(),
        }
    }

    fn unauthorized() -> Self {
        Self {
            status: StatusCode::UNAUTHORIZED,
            message: "Token de API inválido o ausente".to_string(),
        }
    }

    fn not_found(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::NOT_FOUND,
            message: message.into(),
        }
    }

    fn internal(error: anyhow::Error) -> Self {
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: format!("{error:#}"),
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.status, Json(json!({ "error": self.message }))).into_response()
    }
}

fn authorize(state: &AppState, headers: &HeaderMap) -> Result<(), ApiError> {
    let Some(token) = state.token.as_deref() else {
        return Ok(());
    };
    let expected = format!("Bearer {token}");
    let authorized = headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .map(|value| value == expected)
        .unwrap_or(false);
    if authorized {
        Ok(())
    } else {
        Err(ApiError::unauthorized())
    }
}

async fn root() -> Json<Value> {
    Json(json!({
        "name": "NicaScan",
        "version": VERSION,
        "docs": "/openapi.json",
        "health": "/v1/health",
        "businesses": "/v1/businesses",
        "search": "POST /v1/search",
        "export": "/v1/export?q=...",
        "panel": "/panel",
    }))
}

async fn health() -> Json<Value> {
    Json(json!({ "ok": true, "service": "nicascan", "version": VERSION }))
}

async fn list_businesses(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(params): Query<SearchParams>,
) -> Result<Json<Value>, ApiError> {
    authorize(&state, &headers)?;
    run_search(&state, &params).await
}

async fn search_post(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(params): Json<SearchParams>,
) -> Result<Json<Value>, ApiError> {
    authorize(&state, &headers)?;
    run_search(&state, &params).await
}

async fn run_search(state: &AppState, params: &SearchParams) -> Result<Json<Value>, ApiError> {
    let query = params.to_query()?;
    let full = params.full.unwrap_or(true);
    let db = state.db.lock().await;
    let json = if full {
        let results = search_full(&db, &query).map_err(ApiError::internal)?;
        json!({
            "count": results.len(),
            "limit": query.limit,
            "offset": query.offset,
            "results": results,
        })
    } else {
        let results = search(&db, &query).map_err(ApiError::internal)?;
        json!({
            "count": results.len(),
            "limit": query.limit,
            "offset": query.offset,
            "results": results,
        })
    };
    Ok(Json(json))
}

async fn show_business(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Query(params): Query<SearchParams>,
) -> Result<Json<Value>, ApiError> {
    authorize(&state, &headers)?;
    let db = state.db.lock().await;
    let full = get_full(&db, &id, params.raw.unwrap_or(false))
        .map_err(|error| ApiError::not_found(format!("{error:#}")))?;
    Ok(Json(serde_json::to_value(full).map_err(|error| {
        ApiError::internal(anyhow::Error::new(error))
    })?))
}

async fn stats_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Value>, ApiError> {
    authorize(&state, &headers)?;
    let db = state.db.lock().await;
    let stats = db.stats().map_err(ApiError::internal)?;
    Ok(Json(serde_json::to_value(stats).map_err(|error| {
        ApiError::internal(anyhow::Error::new(error))
    })?))
}

async fn sources_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Value>, ApiError> {
    authorize(&state, &headers)?;
    let config = &state.config;
    Ok(Json(json!({
        "osm": { "available": true, "requires_key": false },
        "google": {
            "available": config.google_places_key.is_some(),
            "requires_key": true,
            "env": "GOOGLE_PLACES_API_KEY"
        },
        "foursquare": {
            "available": config.foursquare_key.is_some(),
            "requires_key": true,
            "env": "FOURSQUARE_API_KEY"
        },
        "overture": {
            "available": config.overture_file.is_some(),
            "requires_key": false,
            "file": config.overture_file.as_ref().map(|path| path.display().to_string())
        },
        "web": { "available": true, "requires_key": false }
    })))
}

async fn export_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(params): Query<SearchParams>,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let mut query = params.to_query()?;
    let total = params.limit.unwrap_or(1_000).min(10_000);
    query.offset = 0;
    let mut offset = 0;
    let mut written = 0usize;
    let mut body = String::new();
    let db = state.db.lock().await;

    loop {
        let remaining = total.saturating_sub(written).min(500);
        if remaining == 0 {
            break;
        }
        query.limit = remaining;
        query.offset = offset;
        let page = search_full(&db, &query).map_err(ApiError::internal)?;
        if page.is_empty() {
            break;
        }
        let page_len = page.len();
        for item in page {
            let line = serde_json::to_string(&item)
                .map_err(|error| ApiError::internal(anyhow::Error::new(error)))?;
            body.push_str(&line);
            body.push('\n');
            written += 1;
        }
        offset += page_len;
        if page_len < remaining {
            break;
        }
    }

    let mut response = body.into_response();
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/x-ndjson; charset=utf-8"),
    );
    Ok(response)
}

#[derive(Debug, Deserialize, Default)]
struct DuplicateParams {
    #[serde(default)]
    min_score: Option<f64>,
    #[serde(default)]
    max_distance: Option<f64>,
    #[serde(default)]
    limit: Option<usize>,
}

async fn duplicates_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(params): Query<DuplicateParams>,
) -> Result<Json<Value>, ApiError> {
    authorize(&state, &headers)?;
    let min_score = params.min_score.unwrap_or(0.75);
    let max_distance = params.max_distance.unwrap_or(5_000.0);
    let limit = params.limit.unwrap_or(50).min(500);
    if !(0.0..=1.0).contains(&min_score) {
        return Err(ApiError::bad_request("min_score debe estar entre 0 y 1"));
    }
    let db = state.db.lock().await;
    let pairs = db
        .find_duplicates(min_score, max_distance, limit)
        .map_err(ApiError::internal)?;
    Ok(Json(json!({ "count": pairs.len(), "results": pairs })))
}

#[derive(Debug, Deserialize)]
struct MergeBody {
    keep: String,
    other: String,
    #[serde(default)]
    confirm: bool,
}

async fn merge_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<MergeBody>,
) -> Result<Json<Value>, ApiError> {
    authorize(&state, &headers)?;
    if !body.confirm {
        return Err(ApiError::bad_request("confirm debe ser true para fusionar"));
    }
    let mut db = state.db.lock().await;
    let kept = merge_businesses(&mut db, &body.keep, &body.other)
        .map_err(|error| ApiError::bad_request(format!("{error:#}")))?;
    Ok(Json(json!({ "ok": true, "kept": kept })))
}

async fn panel() -> Html<&'static str> {
    Html(include_str!("web/panel.html"))
}

async fn openapi() -> Json<Value> {
    Json(json!({
        "openapi": "3.0.3",
        "info": {
            "title": "NicaScan API",
            "version": VERSION,
            "description": "Directorio de negocios de Nicaragua para agentes de IA."
        },
        "servers": [{"url": "http://127.0.0.1:8787"}],
        "paths": {
            "/v1/health": {"get": {"summary": "Estado del servicio"}},
            "/v1/businesses": {"get": {"summary": "Lista negocios con filtros"}},
            "/v1/businesses/{id}": {"get": {"summary": "Ficha completa por ID o prefijo"}},
            "/v1/search": {"post": {"summary": "Búsqueda por JSON"}},
            "/v1/export": {"get": {"summary": "Exporta JSONL"}},
            "/v1/stats": {"get": {"summary": "Estadísticas y cobertura"}},
            "/v1/sources": {"get": {"summary": "Fuentes y disponibilidad"}},
            "/v1/duplicates": {"get": {"summary": "Posibles duplicados"}},
            "/v1/merge": {"post": {"summary": "Fusiona dos negocios (confirm=true)"}},
            "/panel": {"get": {"summary": "Panel web de revisión"}}
        }
    }))
}

pub async fn serve(
    db: Database,
    config: Config,
    address: SocketAddr,
    token: Option<String>,
) -> anyhow::Result<()> {
    let listener = tokio::net::TcpListener::bind(address).await?;
    let state = AppState {
        db: Arc::new(Mutex::new(db)),
        config,
        token,
    };
    let app = build_router(state);
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;
    Ok(())
}

async fn shutdown_signal() {
    let _ = tokio::signal::ctrl_c().await;
}
