//! Capa de base de datos SQLite.

pub mod business;
pub mod places;
pub mod schema;

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{bail, Context, Result};
use rusqlite::Connection;
use serde::Serialize;
use ulid::Ulid;

use crate::model::{ScanSummary, Stats};

pub struct Database {
    pub(crate) conn: Connection,
    path: Option<PathBuf>,
    fts: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct DbInfo {
    pub path: String,
    pub size_bytes: u64,
    pub schema_version: i64,
    pub journal_mode: String,
    pub fts5: bool,
    pub businesses: i64,
    pub contacts: i64,
    pub sources: i64,
}

impl Database {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).with_context(|| {
                format!(
                    "No se pudo crear el directorio de la base: {}",
                    parent.display()
                )
            })?;
        }
        let conn = Connection::open(&path)
            .with_context(|| format!("No se pudo abrir la base {}", path.display()))?;
        configure(&conn, true)?;
        let mut db = Self {
            conn,
            path: Some(path),
            fts: false,
        };
        schema::migrate(&mut db.conn)?;
        db.fts = schema::fts_available(&db.conn);
        Ok(db)
    }

    pub fn open_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        configure(&conn, false)?;
        let mut db = Self {
            conn,
            path: None,
            fts: false,
        };
        schema::migrate(&mut db.conn)?;
        db.fts = schema::fts_available(&db.conn);
        Ok(db)
    }

    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    pub fn fts_enabled(&self) -> bool {
        self.fts
    }

    pub fn meta_get(&self, key: &str) -> Result<Option<String>> {
        let mut stmt = self.conn.prepare("SELECT value FROM meta WHERE key = ?1")?;
        let value = stmt
            .query_row([key], |row| row.get::<_, String>(0))
            .optional()?;
        Ok(value)
    }

    pub fn meta_set(&self, key: &str, value: &str) -> Result<()> {
        self.conn.execute(
            "INSERT INTO meta(key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            rusqlite::params![key, value],
        )?;
        Ok(())
    }

    pub fn info(&self) -> Result<DbInfo> {
        let schema_version: i64 = self
            .conn
            .pragma_query_value(None, "user_version", |row| row.get(0))?;
        let journal_mode: String = self
            .conn
            .pragma_query_value(None, "journal_mode", |row| row.get(0))?;
        let size_bytes = self
            .path
            .as_ref()
            .and_then(|p| fs::metadata(p).ok())
            .map(|m| m.len())
            .unwrap_or(0);

        let businesses = count(&self.conn, "businesses")?;
        let contacts = count(&self.conn, "contacts")?;
        let sources = count(&self.conn, "business_sources")?;

        Ok(DbInfo {
            path: self
                .path
                .as_ref()
                .map(|p| p.display().to_string())
                .unwrap_or_else(|| ":memory:".to_string()),
            size_bytes,
            schema_version,
            journal_mode,
            fts5: self.fts,
            businesses,
            contacts,
            sources,
        })
    }

    pub fn stats(&self) -> Result<Stats> {
        business::stats(&self.conn)
    }

    /// Pares sospechosos de duplicado para revisión manual.
    pub fn find_duplicates(
        &self,
        min_score: f64,
        max_distance_m: f64,
        limit: usize,
    ) -> Result<Vec<crate::model::DuplicatePair>> {
        business::find_duplicates(&self.conn, min_score, max_distance_m, limit)
    }

    pub fn checkpoint(&self) -> Result<()> {
        self.conn
            .execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")?;
        Ok(())
    }

    pub fn optimize(&self) -> Result<()> {
        self.conn.execute_batch("PRAGMA optimize;")?;
        Ok(())
    }

    pub fn vacuum(&self) -> Result<()> {
        self.conn.execute_batch("VACUUM;")?;
        Ok(())
    }

    /// Copia consistente a un archivo nuevo usando `VACUUM INTO`.
    pub fn backup_to(&self, target: impl AsRef<Path>) -> Result<()> {
        let target = target.as_ref();
        if target.exists() {
            bail!("El destino ya existe: {}", target.display());
        }
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        self.conn
            .execute("VACUUM INTO ?1", [target.to_string_lossy().as_ref()])?;
        Ok(())
    }

    pub fn begin_scan(
        &self,
        location: &str,
        sources: &[String],
        query: Option<&str>,
    ) -> Result<String> {
        let id = Ulid::new().to_string();
        self.conn.execute(
            "INSERT INTO scans(id, started_at, location, sources, query)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params![
                id,
                crate::db::business::now(),
                location,
                sources.join(","),
                query
            ],
        )?;
        Ok(id)
    }

    pub fn finish_scan(&self, summary: &ScanSummary) -> Result<()> {
        let errors: Vec<String> = summary
            .sources
            .iter()
            .flat_map(|source| {
                source
                    .errors
                    .iter()
                    .map(move |error| format!("{}: {error}", source.source))
            })
            .collect();
        self.conn.execute(
            "UPDATE scans SET finished_at=?1, fetched=?2, inserted=?3, merged=?4,
                    skipped=?5, errors=?6
             WHERE id=?7",
            rusqlite::params![
                business::now(),
                summary.total_fetched as i64,
                summary.total_inserted as i64,
                summary.total_merged as i64,
                summary.total_skipped as i64,
                if errors.is_empty() {
                    None
                } else {
                    Some(serde_json::to_string(&errors)?)
                },
                summary.scan_id
            ],
        )?;
        Ok(())
    }
}

fn configure(conn: &Connection, file: bool) -> Result<()> {
    conn.busy_timeout(Duration::from_secs(15))?;
    conn.execute_batch(
        "PRAGMA foreign_keys = ON;
         PRAGMA synchronous = NORMAL;
         PRAGMA temp_store = MEMORY;
         PRAGMA cache_size = -32000;",
    )?;
    if file {
        // WAL permite lecturas concurrentes. `nicascan db checkpoint` consolida
        // los archivos auxiliares antes de copiar la base a mano.
        let _ = conn
            .pragma_update_and_check(None, "journal_mode", "WAL", |row| row.get::<_, String>(0));
    }
    Ok(())
}

fn count(conn: &Connection, table: &str) -> Result<i64> {
    let sql = format!("SELECT COUNT(*) FROM {table}");
    Ok(conn.query_row(&sql, [], |row| row.get(0))?)
}

trait OptionalExt<T> {
    fn optional(self) -> rusqlite::Result<Option<T>>;
}

impl<T> OptionalExt<T> for rusqlite::Result<T> {
    fn optional(self) -> rusqlite::Result<Option<T>> {
        match self {
            Ok(value) => Ok(Some(value)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(error) => Err(error),
        }
    }
}
