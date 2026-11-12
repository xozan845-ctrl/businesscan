//! Esquema SQLite y migraciones.
//!
//! Todo el modelo vive en un solo archivo `.db` que se puede copiar entre
//! Termux, Linux, Windows y macOS.

use anyhow::{Context, Result};
use rusqlite::Connection;

pub const SCHEMA_VERSION: i64 = 1;

const BASE_SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS meta (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS businesses (
    id                  TEXT PRIMARY KEY,
    name                TEXT NOT NULL,
    name_norm           TEXT NOT NULL,
    match_name          TEXT NOT NULL,
    legal_name          TEXT,
    description         TEXT,
    primary_category    TEXT,
    business_type       TEXT,
    status              TEXT,
    phone_primary       TEXT,
    email_primary       TEXT,
    website             TEXT,
    website_domain      TEXT,
    address_line        TEXT,
    street              TEXT,
    house_number        TEXT,
    barrio              TEXT,
    barrio_norm         TEXT,
    comarca             TEXT,
    comarca_norm        TEXT,
    municipio           TEXT,
    municipio_norm      TEXT,
    departamento        TEXT,
    departamento_norm   TEXT,
    country_code        TEXT NOT NULL DEFAULT 'NI',
    postal_code         TEXT,
    latitude            REAL,
    longitude           REAL,
    geohash             TEXT,
    opening_hours       TEXT,
    price_range         TEXT,
    rating              REAL,
    review_count        INTEGER,
    source_count        INTEGER NOT NULL DEFAULT 0,
    confidence          REAL NOT NULL DEFAULT 0.5,
    first_seen_at       TEXT NOT NULL,
    last_seen_at        TEXT NOT NULL,
    updated_at          TEXT NOT NULL,
    extra_json          TEXT NOT NULL DEFAULT '{}'
);

CREATE INDEX IF NOT EXISTS idx_businesses_name ON businesses(name_norm);
CREATE INDEX IF NOT EXISTS idx_businesses_match_name ON businesses(match_name);
CREATE INDEX IF NOT EXISTS idx_businesses_geo ON businesses(latitude, longitude);
CREATE INDEX IF NOT EXISTS idx_businesses_dept ON businesses(departamento_norm);
CREATE INDEX IF NOT EXISTS idx_businesses_muni ON businesses(municipio_norm);
CREATE INDEX IF NOT EXISTS idx_businesses_barrio ON businesses(barrio_norm);
CREATE INDEX IF NOT EXISTS idx_businesses_category ON businesses(primary_category);
CREATE INDEX IF NOT EXISTS idx_businesses_website_domain ON businesses(website_domain);
CREATE INDEX IF NOT EXISTS idx_businesses_phone ON businesses(phone_primary);
CREATE INDEX IF NOT EXISTS idx_businesses_updated ON businesses(updated_at DESC);
CREATE INDEX IF NOT EXISTS idx_businesses_confidence ON businesses(confidence DESC);

CREATE TABLE IF NOT EXISTS contacts (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    business_id TEXT NOT NULL REFERENCES businesses(id) ON DELETE CASCADE,
    kind        TEXT NOT NULL,
    value       TEXT NOT NULL,
    normalized  TEXT,
    match_key   TEXT,
    label       TEXT,
    is_primary  INTEGER NOT NULL DEFAULT 0,
    source      TEXT,
    created_at  TEXT NOT NULL,
    UNIQUE (business_id, kind, value)
);

CREATE INDEX IF NOT EXISTS idx_contacts_business ON contacts(business_id);
CREATE INDEX IF NOT EXISTS idx_contacts_match ON contacts(match_key);
CREATE INDEX IF NOT EXISTS idx_contacts_kind ON contacts(kind);

CREATE TABLE IF NOT EXISTS business_aliases (
    business_id TEXT NOT NULL REFERENCES businesses(id) ON DELETE CASCADE,
    alias       TEXT NOT NULL,
    alias_norm  TEXT NOT NULL,
    source      TEXT,
    UNIQUE (business_id, alias_norm)
);
CREATE INDEX IF NOT EXISTS idx_aliases_norm ON business_aliases(alias_norm);

CREATE TABLE IF NOT EXISTS business_categories (
    business_id   TEXT NOT NULL REFERENCES businesses(id) ON DELETE CASCADE,
    category      TEXT NOT NULL,
    category_norm TEXT NOT NULL,
    source        TEXT,
    PRIMARY KEY (business_id, category_norm)
);
CREATE INDEX IF NOT EXISTS idx_categories_norm ON business_categories(category_norm);

CREATE TABLE IF NOT EXISTS business_tags (
    business_id TEXT NOT NULL REFERENCES businesses(id) ON DELETE CASCADE,
    key         TEXT NOT NULL,
    value       TEXT NOT NULL,
    source      TEXT NOT NULL DEFAULT '',
    PRIMARY KEY (business_id, key, source)
);
CREATE INDEX IF NOT EXISTS idx_tags_key ON business_tags(key);

CREATE TABLE IF NOT EXISTS business_sources (
    business_id   TEXT NOT NULL REFERENCES businesses(id) ON DELETE CASCADE,
    source        TEXT NOT NULL,
    external_id   TEXT NOT NULL,
    source_url    TEXT,
    raw_json      TEXT NOT NULL DEFAULT 'null',
    confidence    REAL NOT NULL DEFAULT 0.5,
    fetched_at    TEXT NOT NULL,
    first_seen_at TEXT NOT NULL,
    last_seen_at  TEXT NOT NULL,
    PRIMARY KEY (business_id, source, external_id)
);
CREATE INDEX IF NOT EXISTS idx_sources_external ON business_sources(source, external_id);
CREATE INDEX IF NOT EXISTS idx_sources_business ON business_sources(business_id);

CREATE TABLE IF NOT EXISTS places (
    query_norm    TEXT PRIMARY KEY,
    display_name  TEXT NOT NULL,
    place_type    TEXT,
    osm_type      TEXT,
    osm_id        INTEGER,
    lat           REAL,
    lon           REAL,
    bbox_south    REAL,
    bbox_north    REAL,
    bbox_west     REAL,
    bbox_east     REAL,
    resolved_json TEXT NOT NULL,
    fetched_at    TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS scans (
    id          TEXT PRIMARY KEY,
    started_at  TEXT NOT NULL,
    finished_at TEXT,
    location    TEXT NOT NULL,
    sources     TEXT NOT NULL,
    query       TEXT,
    fetched     INTEGER NOT NULL DEFAULT 0,
    inserted    INTEGER NOT NULL DEFAULT 0,
    merged      INTEGER NOT NULL DEFAULT 0,
    skipped     INTEGER NOT NULL DEFAULT 0,
    errors      TEXT
);
CREATE INDEX IF NOT EXISTS idx_scans_started ON scans(started_at DESC);
"#;

const FTS_SCHEMA: &str = r#"
CREATE VIRTUAL TABLE IF NOT EXISTS businesses_fts USING fts5(
    business_id UNINDEXED,
    name,
    aliases,
    categories,
    address,
    description,
    municipio,
    departamento,
    tags,
    tokenize = 'unicode61 remove_diacritics 2'
);
"#;

pub fn migrate(conn: &mut Connection) -> Result<()> {
    let current: i64 = conn.pragma_query_value(None, "user_version", |row| row.get(0))?;
    if current > SCHEMA_VERSION {
        anyhow::bail!(
            "La base fue creada con un esquema más nuevo (v{current}); actualiza nicascan"
        );
    }

    if current < SCHEMA_VERSION {
        let tx = conn.transaction()?;
        tx.execute_batch(BASE_SCHEMA)
            .context("No se pudo crear el esquema de la base")?;
        tx.pragma_update(None, "user_version", SCHEMA_VERSION)?;
        tx.execute(
            "INSERT INTO meta(key, value) VALUES ('app', 'nicascan')
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            [],
        )?;
        tx.commit()?;
    }

    // FTS5 puede no estar disponible en SQLite compilado a medida. Si falla,
    // la búsqueda usa LIKE como respaldo.
    let fts_ok = conn.execute_batch(FTS_SCHEMA).is_ok();
    conn.execute(
        "INSERT INTO meta(key, value) VALUES ('fts5', ?1)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        [if fts_ok { "1" } else { "0" }],
    )?;
    if !fts_ok {
        log::warn!("FTS5 no disponible; la búsqueda de texto usará LIKE (más lenta)");
    }

    Ok(())
}

pub fn fts_available(conn: &Connection) -> bool {
    conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='businesses_fts')",
        [],
        |row| row.get::<_, i64>(0),
    )
    .map(|v| v == 1)
    .unwrap_or(false)
}
