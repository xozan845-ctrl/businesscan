//! Caché de geocodificación (Nominatim) para no repetir consultas.

use anyhow::{Context, Result};
use rusqlite::OptionalExtension;
use serde::Serialize;

use crate::db::business::now;
use crate::db::Database;
use crate::location::{GeoFilter, GeoPoint, ResolvedLocation};

#[derive(Debug, Clone, Serialize)]
pub struct PlaceRow {
    pub query: String,
    pub display_name: String,
    pub lat: Option<f64>,
    pub lon: Option<f64>,
    pub osm_type: Option<String>,
    pub osm_id: Option<i64>,
    pub fetched_at: String,
}

impl Database {
    pub fn cached_place(&self, key: &str) -> Result<Option<ResolvedLocation>> {
        let raw: Option<String> = self
            .conn
            .query_row(
                "SELECT resolved_json FROM places WHERE query_norm=?1",
                [key],
                |row| row.get(0),
            )
            .optional()?;
        match raw {
            Some(raw) => Ok(serde_json::from_str(&raw).ok()),
            None => Ok(None),
        }
    }

    pub fn save_place(&self, key: &str, location: &ResolvedLocation) -> Result<()> {
        let center = location.center.unwrap_or(GeoPoint { lat: 0.0, lon: 0.0 });
        let bbox = location.bbox;
        let place_type = match &location.filter {
            GeoFilter::Area { .. } => "area",
            GeoFilter::Around { .. } => "around",
            GeoFilter::Bbox { .. } => "bbox",
            GeoFilter::Country => "country",
            GeoFilter::OverpassArea { .. } => "overpass_area",
        };
        self.conn
            .execute(
                "INSERT INTO places(
                query_norm, display_name, place_type, osm_type, osm_id, lat, lon,
                bbox_south, bbox_north, bbox_west, bbox_east, resolved_json, fetched_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)
             ON CONFLICT(query_norm) DO UPDATE SET
                display_name=excluded.display_name,
                place_type=excluded.place_type,
                osm_type=excluded.osm_type,
                osm_id=excluded.osm_id,
                lat=excluded.lat,
                lon=excluded.lon,
                bbox_south=excluded.bbox_south,
                bbox_north=excluded.bbox_north,
                bbox_west=excluded.bbox_west,
                bbox_east=excluded.bbox_east,
                resolved_json=excluded.resolved_json,
                fetched_at=excluded.fetched_at",
                rusqlite::params![
                    key,
                    location.display_name,
                    place_type,
                    location.osm_type,
                    location.osm_id,
                    center.lat,
                    center.lon,
                    bbox.map(|b| b.south),
                    bbox.map(|b| b.north),
                    bbox.map(|b| b.west),
                    bbox.map(|b| b.east),
                    serde_json::to_string(location)?,
                    now(),
                ],
            )
            .context("No se pudo guardar la ubicación en caché")?;
        Ok(())
    }

    pub fn list_places(&self, limit: usize) -> Result<Vec<PlaceRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT COALESCE(json_extract(resolved_json, '$.query'), query_norm),
                    display_name, lat, lon, osm_type, osm_id, fetched_at
             FROM places ORDER BY fetched_at DESC LIMIT ?1",
        )?;
        let rows = stmt
            .query_map([limit as i64], |row| {
                Ok(PlaceRow {
                    query: row.get(0)?,
                    display_name: row.get(1)?,
                    lat: row.get(2)?,
                    lon: row.get(3)?,
                    osm_type: row.get(4)?,
                    osm_id: row.get(5)?,
                    fetched_at: row.get(6)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn clear_places(&self) -> Result<usize> {
        Ok(self.conn.execute("DELETE FROM places", [])?)
    }
}
