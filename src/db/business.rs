//! Repositorio de negocios: inserción, fusión de duplicados y búsqueda.

use std::collections::BTreeMap;

use anyhow::{bail, Context, Result};
use chrono::{SecondsFormat, Utc};
use rusqlite::types::Value;
use rusqlite::{params, params_from_iter, Connection, OptionalExtension, Transaction};
use serde_json::Value as JsonValue;
use ulid::Ulid;

use crate::db::schema;
use crate::db::Database;
use crate::model::{
    Address, Business, BusinessFull, BusinessRecord, Contact, ContactKind, CountRow, MatchReason,
    ScanLogRow, SearchQuery, SortBy, SourceRecord, Stats, UpsertAction, UpsertResult,
};
use crate::normalize::{
    domain_of, fts_query, geohash, haversine_m, normalize_email, normalize_name_for_match,
    normalize_phone, normalize_social, normalize_text, normalize_url, phone_key, similarity,
    stable_hash,
};

pub fn now() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true)
}

const BUSINESS_COLUMNS: &str = "b.id, b.name, b.name_norm, b.legal_name, b.description, \
b.primary_category, b.business_type, b.status, b.phone_primary, b.email_primary, b.website, \
b.address_line, b.street, b.house_number, b.barrio, b.comarca, b.municipio, b.departamento, \
b.country_code, b.postal_code, b.latitude, b.longitude, b.geohash, b.opening_hours, \
b.price_range, b.rating, b.review_count, b.source_count, b.confidence, b.first_seen_at, \
b.last_seen_at, b.updated_at, b.extra_json";

/// Importa un lote dentro de una sola transacción.
pub fn import_batch(db: &mut Database, records: &[BusinessRecord]) -> Result<Vec<UpsertResult>> {
    let tx = db.conn.transaction()?;
    let mut results = Vec::with_capacity(records.len());
    for record in records {
        results.push(upsert_tx(&tx, record)?);
    }
    tx.commit()?;
    Ok(results)
}

fn upsert_tx(tx: &Transaction<'_>, record: &BusinessRecord) -> Result<UpsertResult> {
    if record.name.trim().is_empty() {
        return Ok(UpsertResult {
            id: String::new(),
            action: UpsertAction::Skipped,
            match_reason: None,
        });
    }
    let conn: &Connection = tx;
    if let Some((business_id, reason)) = find_match(conn, record)? {
        merge_into(conn, &business_id, record)?;
        return Ok(UpsertResult {
            id: business_id,
            action: UpsertAction::Merged,
            match_reason: Some(reason),
        });
    }
    let id = insert_new(conn, record)?;
    Ok(UpsertResult {
        id,
        action: UpsertAction::Inserted,
        match_reason: None,
    })
}

fn find_match(conn: &Connection, record: &BusinessRecord) -> Result<Option<(String, MatchReason)>> {
    let external_id = external_id_for(record);
    if let Some(id) = conn
        .query_row(
            "SELECT business_id FROM business_sources WHERE source=?1 AND external_id=?2",
            params![record.source, external_id],
            |row| row.get::<_, String>(0),
        )
        .optional()?
    {
        return Ok(Some((id, MatchReason::ExternalId)));
    }

    for contact in &record.contacts {
        if !contact.kind.is_phone_like() {
            continue;
        }
        let Some(key) = phone_key(&contact.value) else {
            continue;
        };
        if let Some(id) = conn
            .query_row(
                "SELECT business_id FROM contacts
                 WHERE match_key=?1 AND kind IN ('phone','mobile','whatsapp')
                 LIMIT 1",
                [&key],
                |row| row.get::<_, String>(0),
            )
            .optional()?
        {
            return Ok(Some((id, MatchReason::Phone)));
        }
    }

    if let Some(domain) = record_website_domain(record) {
        let mut stmt =
            conn.prepare("SELECT id, match_name FROM businesses WHERE website_domain=?1 LIMIT 10")?;
        let candidates = stmt
            .query_map([&domain], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let incoming = normalize_name_for_match(&record.name);
        for (id, candidate_name) in candidates {
            if similarity(&incoming, &candidate_name) >= 0.55
                || incoming.contains(&candidate_name)
                || candidate_name.contains(&incoming)
            {
                return Ok(Some((id, MatchReason::Website)));
            }
        }
    }

    let name_norm = normalize_name_for_match(&record.name);
    if name_norm.is_empty() {
        return Ok(None);
    }

    #[derive(Debug)]
    struct Candidate {
        id: String,
        latitude: Option<f64>,
        longitude: Option<f64>,
        address_line: Option<String>,
        street: Option<String>,
        house_number: Option<String>,
        barrio_norm: Option<String>,
        municipio_norm: Option<String>,
    }

    let mut stmt = conn.prepare(
        "SELECT id, latitude, longitude, address_line, street, house_number,
                barrio_norm, municipio_norm
         FROM businesses WHERE match_name=?1 LIMIT 50",
    )?;
    let candidates = stmt
        .query_map([&name_norm], |row| {
            Ok(Candidate {
                id: row.get(0)?,
                latitude: row.get(1)?,
                longitude: row.get(2)?,
                address_line: row.get(3)?,
                street: row.get(4)?,
                house_number: row.get(5)?,
                barrio_norm: row.get(6)?,
                municipio_norm: row.get(7)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;

    let incoming_address = normalize_text(record.address.line.as_deref().unwrap_or(""));
    let incoming_street = normalize_text(record.address.street.as_deref().unwrap_or(""));
    let incoming_number = normalize_text(record.address.house_number.as_deref().unwrap_or(""));
    let incoming_barrio = normalize_text(record.address.barrio.as_deref().unwrap_or(""));
    let incoming_muni = normalize_text(record.address.municipio.as_deref().unwrap_or(""));

    for candidate in candidates {
        let candidate_address = normalize_text(candidate.address_line.as_deref().unwrap_or(""));
        let candidate_street = normalize_text(candidate.street.as_deref().unwrap_or(""));
        let candidate_number = normalize_text(candidate.house_number.as_deref().unwrap_or(""));

        if incoming_address.len() >= 5 && incoming_address == candidate_address {
            return Ok(Some((candidate.id, MatchReason::NameAndAddress)));
        }

        if !incoming_street.is_empty()
            && incoming_street == candidate_street
            && incoming_number == candidate_number
        {
            return Ok(Some((candidate.id, MatchReason::NameAndAddress)));
        }

        if let (Some(lat), Some(lon), Some(clat), Some(clon)) = (
            record.latitude,
            record.longitude,
            candidate.latitude,
            candidate.longitude,
        ) {
            if haversine_m(lat, lon, clat, clon) <= 300.0 {
                return Ok(Some((candidate.id, MatchReason::NameAndProximity)));
            }
            continue;
        }

        let same_barrio = !incoming_barrio.is_empty()
            && candidate.barrio_norm.as_deref() == Some(incoming_barrio.as_str());
        let same_muni = !incoming_muni.is_empty()
            && candidate.municipio_norm.as_deref() == Some(incoming_muni.as_str());

        if same_barrio && (same_muni || candidate.municipio_norm.is_none()) {
            return Ok(Some((candidate.id, MatchReason::NameAndAddress)));
        }
    }

    Ok(None)
}

fn insert_new(conn: &Connection, record: &BusinessRecord) -> Result<String> {
    let id = Ulid::new().to_string();
    let now = now();
    let name_norm = normalize_text(&record.name);
    let match_name = normalize_name_for_match(&record.name);
    let website = record_website(record);
    let website_domain = website.as_deref().and_then(domain_of);
    let phone_primary = primary_contact(record, ContactKind::Phone)
        .or_else(|| primary_contact(record, ContactKind::Mobile))
        .or_else(|| primary_contact(record, ContactKind::Whatsapp));
    let email_primary = primary_contact(record, ContactKind::Email);
    let address = &record.address;
    let country_code = address
        .country_code
        .clone()
        .filter(|v| !v.trim().is_empty())
        .unwrap_or_else(|| "NI".to_string());
    let geo = match (record.latitude, record.longitude) {
        (Some(lat), Some(lon)) => Some(geohash(lat, lon, 9)),
        _ => None,
    };

    conn.execute(
        "INSERT INTO businesses(
            id, name, name_norm, match_name, legal_name, description,
            primary_category, business_type, status, phone_primary, email_primary,
            website, website_domain, address_line, street, house_number, barrio,
            barrio_norm, comarca, comarca_norm, municipio, municipio_norm,
            departamento, departamento_norm, country_code, postal_code, latitude,
            longitude, geohash, opening_hours, price_range, rating, review_count,
            source_count, confidence, first_seen_at, last_seen_at, updated_at, extra_json
         ) VALUES (
            ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16,
            ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24, ?25, ?26, ?27, ?28, ?29, ?30,
            ?31, ?32, ?33, 1, ?34, ?35, ?35, ?35, '{}'
         )",
        params![
            id,
            record.name.trim(),
            name_norm,
            match_name,
            record.legal_name,
            record.description,
            record.primary_category,
            record.business_type,
            record.status.as_deref().map(|s| s.to_lowercase()),
            phone_primary,
            email_primary,
            website,
            website_domain,
            address.line,
            address.street,
            address.house_number,
            address.barrio,
            normalize_text(address.barrio.as_deref().unwrap_or("")),
            address.comarca,
            normalize_text(address.comarca.as_deref().unwrap_or("")),
            address.municipio,
            normalize_text(address.municipio.as_deref().unwrap_or("")),
            address.departamento,
            normalize_text(address.departamento.as_deref().unwrap_or("")),
            country_code,
            address.postal_code,
            record.latitude,
            record.longitude,
            geo,
            record.opening_hours,
            record.price_range,
            record.rating,
            record.review_count,
            record.confidence.clamp(0.0, 1.0),
            now,
        ],
    )
    .with_context(|| format!("No se pudo insertar el negocio «{}»", record.name))?;

    insert_related(conn, &id, record, &record.name)?;
    insert_source(conn, &id, record)?;
    update_source_count(conn, &id)?;
    refresh_fts(conn, &id)?;
    Ok(id)
}

fn merge_into(conn: &Connection, id: &str, record: &BusinessRecord) -> Result<()> {
    let existing = load_business(conn, id)?
        .with_context(|| format!("Negocio canónico {id} desapareció durante la fusión"))?;
    let now = now();

    let name = existing.name.clone();
    let name_norm = existing.name_norm.clone();
    let match_name = normalize_name_for_match(&name);
    let website = existing.website.clone().or_else(|| record_website(record));
    let website_domain = website.as_deref().and_then(domain_of);
    let phone_primary = existing
        .phone_primary
        .clone()
        .or_else(|| primary_contact(record, ContactKind::Phone))
        .or_else(|| primary_contact(record, ContactKind::Mobile))
        .or_else(|| primary_contact(record, ContactKind::Whatsapp));
    let email_primary = existing
        .email_primary
        .clone()
        .or_else(|| primary_contact(record, ContactKind::Email));

    let description = prefer_longer(existing.description.clone(), record.description.clone());
    let opening_hours = prefer_longer(existing.opening_hours.clone(), record.opening_hours.clone());
    let (rating, review_count) = prefer_rating(&existing, record.rating, record.review_count);

    let incoming_status = record.status.as_deref().map(|s| s.to_lowercase());
    let status = match (existing.status.as_deref(), incoming_status) {
        (Some(_current), Some(new)) if new == "closed" || new == "closed_temporarily" => Some(new),
        (Some(current), _) => Some(current.to_string()),
        (None, new) => new,
    };

    let latitude = existing.latitude.or(record.latitude);
    let longitude = existing.longitude.or(record.longitude);
    let geo = match (latitude, longitude) {
        (Some(lat), Some(lon)) => Some(geohash(lat, lon, 9)),
        _ => None,
    };

    let address = merge_address(&existing.address, &record.address);
    let country_code = address.country_code.clone().unwrap_or_else(|| {
        existing
            .address
            .country_code
            .clone()
            .unwrap_or_else(|| "NI".into())
    });

    conn.execute(
        "UPDATE businesses SET
            name=?1, name_norm=?2, match_name=?3,
            legal_name=COALESCE(legal_name, ?4),
            description=?5,
            primary_category=COALESCE(primary_category, ?6),
            business_type=COALESCE(business_type, ?7),
            status=?8,
            phone_primary=?9, email_primary=?10, website=?11, website_domain=?12,
            address_line=?13, street=?14, house_number=?15,
            barrio=?16, barrio_norm=?17, comarca=?18, comarca_norm=?19,
            municipio=?20, municipio_norm=?21, departamento=?22, departamento_norm=?23,
            country_code=?24, postal_code=?25, latitude=?26, longitude=?27,
            geohash=?28, opening_hours=?29, price_range=COALESCE(price_range, ?30),
            rating=?31, review_count=?32, confidence=?33,
            last_seen_at=?34, updated_at=?34
         WHERE id=?35",
        params![
            name,
            name_norm,
            match_name,
            record.legal_name,
            description,
            record.primary_category,
            record.business_type,
            status,
            phone_primary,
            email_primary,
            website,
            website_domain,
            address.line,
            address.street,
            address.house_number,
            address.barrio,
            normalize_text(address.barrio.as_deref().unwrap_or("")),
            address.comarca,
            normalize_text(address.comarca.as_deref().unwrap_or("")),
            address.municipio,
            normalize_text(address.municipio.as_deref().unwrap_or("")),
            address.departamento,
            normalize_text(address.departamento.as_deref().unwrap_or("")),
            country_code,
            address.postal_code,
            latitude,
            longitude,
            geo,
            opening_hours,
            record.price_range,
            rating,
            review_count,
            existing.confidence.max(record.confidence).clamp(0.0, 1.0),
            now,
            id,
        ],
    )?;

    insert_related(conn, id, record, &existing.name)?;
    insert_source(conn, id, record)?;
    update_source_count(conn, id)?;
    update_confidence(conn, id)?;
    refresh_fts(conn, id)?;
    Ok(())
}

fn prefer_longer(existing: Option<String>, incoming: Option<String>) -> Option<String> {
    match (existing, incoming) {
        (Some(a), Some(b)) => {
            if b.trim().len() > a.trim().len() + 10 {
                Some(b)
            } else {
                Some(a)
            }
        }
        (Some(a), None) => Some(a),
        (None, Some(b)) => Some(b),
        (None, None) => None,
    }
}

fn prefer_rating(
    existing: &Business,
    rating: Option<f64>,
    review_count: Option<i64>,
) -> (Option<f64>, Option<i64>) {
    match (existing.review_count, review_count) {
        (Some(current), Some(incoming)) if incoming > current => (rating, review_count),
        (None, Some(_)) => (rating, review_count),
        _ => (existing.rating, existing.review_count),
    }
}

fn merge_address(existing: &Address, incoming: &Address) -> Address {
    Address {
        line: existing.line.clone().or_else(|| incoming.line.clone()),
        street: existing.street.clone().or_else(|| incoming.street.clone()),
        house_number: existing
            .house_number
            .clone()
            .or_else(|| incoming.house_number.clone()),
        barrio: existing.barrio.clone().or_else(|| incoming.barrio.clone()),
        comarca: existing
            .comarca
            .clone()
            .or_else(|| incoming.comarca.clone()),
        municipio: existing
            .municipio
            .clone()
            .or_else(|| incoming.municipio.clone()),
        departamento: existing
            .departamento
            .clone()
            .or_else(|| incoming.departamento.clone()),
        postal_code: existing
            .postal_code
            .clone()
            .or_else(|| incoming.postal_code.clone()),
        country_code: existing
            .country_code
            .clone()
            .or_else(|| incoming.country_code.clone()),
    }
}

fn insert_related(
    conn: &Connection,
    id: &str,
    record: &BusinessRecord,
    canonical_name: &str,
) -> Result<()> {
    for contact in &record.contacts {
        insert_contact(conn, id, contact, &record.source)?;
    }

    let mut aliases: Vec<String> = record.aliases.clone();
    if normalize_text(canonical_name) != normalize_text(&record.name) {
        aliases.push(record.name.clone());
    }
    for alias in aliases {
        let alias = alias.trim();
        if alias.is_empty() || normalize_text(alias) == normalize_text(canonical_name) {
            continue;
        }
        conn.execute(
            "INSERT INTO business_aliases(business_id, alias, alias_norm, source)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(business_id, alias_norm) DO UPDATE SET
                source = COALESCE(business_aliases.source, excluded.source)",
            params![id, alias, normalize_text(alias), record.source],
        )?;
    }

    let mut categories: Vec<String> = record.categories.clone();
    if let Some(category) = &record.primary_category {
        categories.push(category.clone());
    }
    if let Some(category) = &record.business_type {
        categories.push(category.clone());
    }
    for category in categories {
        let category = category.trim();
        if category.is_empty() {
            continue;
        }
        conn.execute(
            "INSERT INTO business_categories(business_id, category, category_norm, source)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(business_id, category_norm) DO UPDATE SET
                source = COALESCE(business_categories.source, excluded.source)",
            params![id, category, normalize_text(category), record.source],
        )?;
    }

    for (key, value) in &record.tags {
        if key.trim().is_empty() || value.trim().is_empty() {
            continue;
        }
        conn.execute(
            "INSERT INTO business_tags(business_id, key, value, source)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(business_id, key, source) DO UPDATE SET value = excluded.value",
            params![id, key, value, record.source],
        )?;
    }

    Ok(())
}

fn insert_contact(conn: &Connection, id: &str, contact: &Contact, source: &str) -> Result<()> {
    let raw = contact.value.trim();
    if raw.is_empty() {
        return Ok(());
    }
    let kind = contact.kind;

    let (value, normalized, match_key) = match kind {
        ContactKind::Phone | ContactKind::Mobile | ContactKind::Whatsapp => {
            let Some(normalized) = normalize_phone(raw) else {
                return Ok(());
            };
            let Some(key) = phone_key(raw) else {
                return Ok(());
            };
            (normalized.clone(), Some(normalized), Some(key))
        }
        ContactKind::Email => {
            let Some(normalized) = normalize_email(raw) else {
                return Ok(());
            };
            (
                normalized.clone(),
                Some(normalized.clone()),
                Some(normalized),
            )
        }
        ContactKind::Website => {
            let Some(normalized) = normalize_url(raw) else {
                return Ok(());
            };
            let key = domain_of(&normalized).or_else(|| Some(normalized.clone()));
            (normalized.clone(), Some(normalized), key)
        }
        _ if kind.is_url_like() => {
            let Some(normalized) = normalize_social(kind.as_str(), raw) else {
                return Ok(());
            };
            let key = Some(normalized.clone());
            (normalized.clone(), Some(normalized), key)
        }
        _ => (raw.to_string(), None, None),
    };

    conn.execute(
        "INSERT INTO contacts(
            business_id, kind, value, normalized, match_key, label, is_primary, source, created_at
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
         ON CONFLICT(business_id, kind, value) DO UPDATE SET
            normalized=excluded.normalized,
            match_key=excluded.match_key,
            label=COALESCE(excluded.label, contacts.label),
            is_primary=MAX(contacts.is_primary, excluded.is_primary),
            source=COALESCE(contacts.source, excluded.source)",
        params![
            id,
            kind.as_str(),
            value,
            normalized,
            match_key,
            contact.label,
            i64::from(contact.is_primary),
            contact.source.as_deref().unwrap_or(source),
            now(),
        ],
    )?;
    Ok(())
}

fn insert_source(conn: &Connection, id: &str, record: &BusinessRecord) -> Result<()> {
    let external_id = external_id_for(record);
    let raw = serde_json::to_string(&record.raw)?;
    let now = now();
    conn.execute(
        "INSERT INTO business_sources(
            business_id, source, external_id, source_url, raw_json, confidence,
            fetched_at, first_seen_at, last_seen_at
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7, ?7)
         ON CONFLICT(business_id, source, external_id) DO UPDATE SET
            source_url=COALESCE(excluded.source_url, business_sources.source_url),
            raw_json=excluded.raw_json,
            confidence=MAX(business_sources.confidence, excluded.confidence),
            fetched_at=excluded.fetched_at,
            last_seen_at=excluded.last_seen_at",
        params![
            id,
            record.source,
            external_id,
            record.source_url,
            raw,
            record.confidence.clamp(0.0, 1.0),
            now,
        ],
    )?;
    Ok(())
}

fn update_source_count(conn: &Connection, id: &str) -> Result<()> {
    conn.execute(
        "UPDATE businesses SET source_count = (
            SELECT COUNT(DISTINCT source) FROM business_sources WHERE business_id=?1
         ) WHERE id=?1",
        [id],
    )?;
    Ok(())
}

fn update_confidence(conn: &Connection, id: &str) -> Result<()> {
    conn.execute(
        "UPDATE businesses SET confidence = MIN(0.99,
            MAX(confidence, (SELECT MAX(confidence) FROM business_sources WHERE business_id=?1))
            + 0.02 * MAX(0, source_count - 1)
         ) WHERE id=?1",
        [id],
    )?;
    Ok(())
}

pub fn refresh_fts(conn: &Connection, id: &str) -> Result<()> {
    if !schema::fts_available(conn) {
        return Ok(());
    }
    let Some(business) = load_business(conn, id)? else {
        return Ok(());
    };
    let aliases = group_concat(
        conn,
        "SELECT alias FROM business_aliases WHERE business_id=?1",
        id,
    )?;
    let categories = group_concat(
        conn,
        "SELECT category FROM business_categories WHERE business_id=?1",
        id,
    )?;
    let tags = group_concat(
        conn,
        "SELECT key || ' ' || value FROM business_tags WHERE business_id=?1",
        id,
    )?;
    let address = [
        business.address.line.as_deref(),
        business.address.street.as_deref(),
        business.address.house_number.as_deref(),
        business.address.barrio.as_deref(),
        business.address.comarca.as_deref(),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>()
    .join(" ");

    conn.execute("DELETE FROM businesses_fts WHERE business_id=?1", [id])?;
    conn.execute(
        "INSERT INTO businesses_fts(
            business_id, name, aliases, categories, address, description,
            municipio, departamento, tags
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![
            id,
            business.name,
            aliases,
            categories,
            address,
            business.description.unwrap_or_default(),
            business.address.municipio.unwrap_or_default(),
            business.address.departamento.unwrap_or_default(),
            tags,
        ],
    )?;
    Ok(())
}

fn group_concat(conn: &Connection, sql: &str, id: &str) -> Result<String> {
    let mut stmt = conn.prepare(sql)?;
    let values = stmt
        .query_map([id], |row| row.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(values.join(" "))
}

fn external_id_for(record: &BusinessRecord) -> String {
    record
        .external_id
        .clone()
        .filter(|v| !v.trim().is_empty())
        .unwrap_or_else(|| {
            let phone = record
                .phone_values()
                .next()
                .and_then(phone_key)
                .unwrap_or_default();
            let address = record.address.line.clone().unwrap_or_default();
            format!(
                "{}:{}",
                record.source,
                stable_hash(&format!(
                    "{}|{}|{}|{}|{}",
                    normalize_text(&record.name),
                    record
                        .latitude
                        .map(|v| format!("{v:.5}"))
                        .unwrap_or_default(),
                    record
                        .longitude
                        .map(|v| format!("{v:.5}"))
                        .unwrap_or_default(),
                    phone,
                    address
                ))
            )
        })
}

fn record_website(record: &BusinessRecord) -> Option<String> {
    record
        .contacts
        .iter()
        .filter(|c| c.kind == ContactKind::Website)
        .find_map(|c| normalize_url(&c.value))
        .or_else(|| {
            record
                .tags
                .get("website")
                .or_else(|| record.tags.get("contact:website"))
                .and_then(|v| normalize_url(v))
        })
}

fn record_website_domain(record: &BusinessRecord) -> Option<String> {
    record_website(record).as_deref().and_then(domain_of)
}

fn primary_contact(record: &BusinessRecord, kind: ContactKind) -> Option<String> {
    let raw = record
        .contacts
        .iter()
        .filter(|c| c.kind == kind)
        .find(|c| c.is_primary)
        .or_else(|| record.contacts.iter().find(|c| c.kind == kind))
        .map(|c| c.value.trim().to_string())
        .filter(|v| !v.is_empty())?;

    match kind {
        ContactKind::Phone | ContactKind::Mobile | ContactKind::Whatsapp => {
            Some(normalize_phone(&raw).unwrap_or(raw))
        }
        ContactKind::Email => Some(normalize_email(&raw).unwrap_or(raw)),
        ContactKind::Website => Some(normalize_url(&raw).unwrap_or(raw)),
        _ => Some(raw),
    }
}

pub fn load_business(conn: &Connection, id: &str) -> Result<Option<Business>> {
    let sql = format!("SELECT {BUSINESS_COLUMNS} FROM businesses b WHERE b.id=?1");
    let mut stmt = conn.prepare(&sql)?;
    let business = stmt
        .query_row([id], map_business)
        .optional()
        .context("Error leyendo negocio")?;
    Ok(business)
}

fn map_business(row: &rusqlite::Row<'_>) -> rusqlite::Result<Business> {
    let extra_raw: String = row.get(32)?;
    let extra = serde_json::from_str(&extra_raw).unwrap_or(JsonValue::Null);
    Ok(Business {
        id: row.get(0)?,
        name: row.get(1)?,
        name_norm: row.get(2)?,
        legal_name: row.get(3)?,
        description: row.get(4)?,
        primary_category: row.get(5)?,
        business_type: row.get(6)?,
        status: row.get(7)?,
        phone_primary: row.get(8)?,
        email_primary: row.get(9)?,
        website: row.get(10)?,
        address: Address {
            line: row.get(11)?,
            street: row.get(12)?,
            house_number: row.get(13)?,
            barrio: row.get(14)?,
            comarca: row.get(15)?,
            municipio: row.get(16)?,
            departamento: row.get(17)?,
            country_code: row.get(18)?,
            postal_code: row.get(19)?,
        },
        latitude: row.get(20)?,
        longitude: row.get(21)?,
        geohash: row.get(22)?,
        opening_hours: row.get(23)?,
        price_range: row.get(24)?,
        rating: row.get(25)?,
        review_count: row.get(26)?,
        source_count: row.get(27)?,
        confidence: row.get(28)?,
        first_seen_at: row.get(29)?,
        last_seen_at: row.get(30)?,
        updated_at: row.get(31)?,
        extra,
        distance_m: None,
    })
}

pub fn search(db: &Database, query: &SearchQuery) -> Result<Vec<Business>> {
    let conn = &db.conn;
    let fts = db.fts_enabled();
    let mut sql = format!("SELECT {BUSINESS_COLUMNS} FROM businesses b");
    let mut values: Vec<Value> = Vec::new();
    let mut where_parts: Vec<String> = Vec::new();

    let text = query
        .text
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty());

    if let Some(text) = text {
        if fts {
            if let Some(fts_q) = fts_query(text, true) {
                sql.push_str(" JOIN businesses_fts ON businesses_fts.business_id = b.id");
                where_parts.push("businesses_fts MATCH ?".to_string());
                values.push(Value::Text(fts_q));
            } else {
                // Solo había stopwords o signos: no filtres.
            }
        } else {
            let pattern = format!("%{}%", normalize_text(text));
            where_parts.push(
                "(b.name_norm LIKE ? OR b.description LIKE ? OR b.business_type LIKE ?
                  OR b.municipio LIKE ? OR b.departamento LIKE ?
                  OR EXISTS (SELECT 1 FROM business_aliases a
                             WHERE a.business_id=b.id AND a.alias_norm LIKE ?)
                  OR EXISTS (SELECT 1 FROM business_categories c
                             WHERE c.business_id=b.id AND c.category_norm LIKE ?))"
                    .to_string(),
            );
            for _ in 0..7 {
                values.push(Value::Text(pattern.clone()));
            }
        }
    }

    if let Some(dept) = query
        .departamento
        .as_deref()
        .map(normalize_text)
        .filter(|v| !v.is_empty())
    {
        where_parts.push("b.departamento_norm = ?".to_string());
        values.push(Value::Text(dept));
    }
    if let Some(muni) = query
        .municipio
        .as_deref()
        .map(normalize_text)
        .filter(|v| !v.is_empty())
    {
        where_parts.push("b.municipio_norm = ?".to_string());
        values.push(Value::Text(muni));
    }
    if let Some(barrio) = query
        .barrio
        .as_deref()
        .map(normalize_text)
        .filter(|v| !v.is_empty())
    {
        where_parts.push("(b.barrio_norm = ? OR b.comarca_norm = ?)".to_string());
        values.push(Value::Text(barrio.clone()));
        values.push(Value::Text(barrio));
    }
    if let Some(category) = query
        .category
        .as_deref()
        .map(normalize_text)
        .filter(|v| !v.is_empty())
    {
        let pattern = format!("%{category}%");
        where_parts.push(
            "(b.primary_category LIKE ? OR b.business_type LIKE ?
              OR EXISTS (SELECT 1 FROM business_categories c
                         WHERE c.business_id=b.id AND c.category_norm LIKE ?))"
                .to_string(),
        );
        values.push(Value::Text(pattern.clone()));
        values.push(Value::Text(pattern.clone()));
        values.push(Value::Text(pattern));
    }
    if let Some(source) = query.source.as_deref().filter(|v| !v.is_empty()) {
        where_parts.push(
            "EXISTS (SELECT 1 FROM business_sources s
                     WHERE s.business_id=b.id AND s.source=?)"
                .to_string(),
        );
        values.push(Value::Text(source.to_string()));
    }
    if let Some(status) = query.status.as_deref().filter(|v| !v.is_empty()) {
        where_parts.push("b.status = ?".to_string());
        values.push(Value::Text(status.to_lowercase()));
    }
    if query.has_phone {
        where_parts.push(
            "EXISTS (SELECT 1 FROM contacts c WHERE c.business_id=b.id
                     AND c.kind IN ('phone','mobile','whatsapp'))"
                .to_string(),
        );
    }
    if query.has_email {
        where_parts.push(
            "EXISTS (SELECT 1 FROM contacts c WHERE c.business_id=b.id AND c.kind='email')"
                .to_string(),
        );
    }
    if query.has_website {
        where_parts.push(
            "(b.website IS NOT NULL OR EXISTS (
                SELECT 1 FROM contacts c WHERE c.business_id=b.id AND c.kind='website'))"
                .to_string(),
        );
    }
    if let Some(min) = query.min_confidence {
        where_parts.push("b.confidence >= ?".to_string());
        values.push(Value::Real(min));
    }
    if let Some(since) = query.updated_since.as_deref().filter(|v| !v.is_empty()) {
        where_parts.push("b.updated_at >= ?".to_string());
        values.push(Value::Text(since.to_string()));
    }

    let geo = match (query.lat, query.lon) {
        (Some(lat), Some(lon)) => {
            let radius = query.radius_m.unwrap_or(1_000.0);
            let bbox = crate::normalize::Bbox::around(lat, lon, radius);
            where_parts
                .push("b.latitude BETWEEN ? AND ? AND b.longitude BETWEEN ? AND ?".to_string());
            values.push(Value::Real(bbox.south));
            values.push(Value::Real(bbox.north));
            values.push(Value::Real(bbox.west));
            values.push(Value::Real(bbox.east));
            Some((lat, lon, radius))
        }
        _ => None,
    };

    if !where_parts.is_empty() {
        sql.push_str(" WHERE ");
        sql.push_str(&where_parts.join(" AND "));
    }

    let order = match query.sort {
        SortBy::Name => "b.name COLLATE NOCASE ASC".to_string(),
        SortBy::Confidence => "b.confidence DESC, b.name COLLATE NOCASE ASC".to_string(),
        SortBy::Updated => "b.updated_at DESC, b.name COLLATE NOCASE ASC".to_string(),
        SortBy::Distance => "b.confidence DESC, b.name COLLATE NOCASE ASC".to_string(),
        SortBy::Relevance => {
            if text.is_some() && fts {
                "bm25(businesses_fts) ASC, b.confidence DESC".to_string()
            } else {
                "b.confidence DESC, b.name COLLATE NOCASE ASC".to_string()
            }
        }
    };
    sql.push_str(" ORDER BY ");
    sql.push_str(&order);

    let wanted_limit = if query.limit == 0 { 5_000 } else { query.limit };
    let sql_limit = if geo.is_some() {
        (wanted_limit.saturating_mul(20)).clamp(500, 10_000)
    } else {
        wanted_limit
    };
    let sql_offset = if geo.is_some() { 0 } else { query.offset };
    sql.push_str(" LIMIT ? OFFSET ?");
    values.push(Value::Integer(sql_limit as i64));
    values.push(Value::Integer(sql_offset as i64));

    let mut stmt = conn.prepare(&sql)?;
    let mut rows = stmt
        .query_map(params_from_iter(values.iter()), map_business)?
        .collect::<rusqlite::Result<Vec<_>>>()?;

    if let Some((lat, lon, radius)) = geo {
        rows.retain_mut(|business| {
            if let (Some(blat), Some(blon)) = (business.latitude, business.longitude) {
                let distance = haversine_m(lat, lon, blat, blon);
                if distance <= radius {
                    business.distance_m = Some(distance);
                    return true;
                }
            }
            false
        });
        if query.sort == SortBy::Distance {
            rows.sort_by(|a, b| {
                a.distance_m
                    .unwrap_or(f64::MAX)
                    .total_cmp(&b.distance_m.unwrap_or(f64::MAX))
            });
        }
        rows = rows
            .into_iter()
            .skip(query.offset)
            .take(if query.limit == 0 {
                usize::MAX
            } else {
                query.limit
            })
            .collect();
    }

    Ok(rows)
}

pub fn search_full(db: &Database, query: &SearchQuery) -> Result<Vec<BusinessFull>> {
    let businesses = search(db, query)?;
    let mut out = Vec::with_capacity(businesses.len());
    for business in businesses {
        out.push(full_from_business(&db.conn, business, query.include_raw)?);
    }
    Ok(out)
}

pub fn get_full(db: &Database, id_or_prefix: &str, include_raw: bool) -> Result<BusinessFull> {
    let id = resolve_id(&db.conn, id_or_prefix)?;
    let business =
        load_business(&db.conn, &id)?.with_context(|| format!("No existe el negocio {id}"))?;
    full_from_business(&db.conn, business, include_raw)
}

fn resolve_id(conn: &Connection, id_or_prefix: &str) -> Result<String> {
    let value = id_or_prefix.trim();
    if value.is_empty() {
        bail!("Indica un ID de negocio");
    }
    if let Some(id) = conn
        .query_row("SELECT id FROM businesses WHERE id=?1", [value], |row| {
            row.get::<_, String>(0)
        })
        .optional()?
    {
        return Ok(id);
    }

    let escaped = value
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_");
    let prefix = format!("{escaped}%");
    let prefix_ids = query_ids(conn, &prefix)?;
    if prefix_ids.len() == 1 {
        return Ok(prefix_ids.into_iter().next().unwrap());
    }

    // Los IDs cortos que muestra la CLI son el sufijo aleatorio del ULID.
    let suffix = format!("%{escaped}");
    let suffix_ids = query_ids(conn, &suffix)?;
    if suffix_ids.len() == 1 {
        return Ok(suffix_ids.into_iter().next().unwrap());
    }

    let mut all = prefix_ids;
    for id in suffix_ids {
        if !all.contains(&id) {
            all.push(id);
        }
    }
    match all.len() {
        0 => bail!("No se encontró un negocio con ID «{value}»"),
        1 => Ok(all.into_iter().next().unwrap()),
        _ => bail!(
            "El ID «{value}» coincide con varios negocios: {}",
            all.iter().take(10).cloned().collect::<Vec<_>>().join(", ")
        ),
    }
}

fn query_ids(conn: &Connection, pattern: &str) -> Result<Vec<String>> {
    let mut stmt =
        conn.prepare("SELECT id FROM businesses WHERE id LIKE ?1 ESCAPE '\\' LIMIT 10")?;
    let ids = stmt
        .query_map([pattern], |row| row.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(ids)
}

fn full_from_business(
    conn: &Connection,
    business: Business,
    include_raw: bool,
) -> Result<BusinessFull> {
    let contacts = load_contacts(conn, &business.id)?;
    let aliases = load_strings(
        conn,
        "SELECT alias FROM business_aliases WHERE business_id=?1 ORDER BY alias",
        &business.id,
    )?;
    let categories = load_strings(
        conn,
        "SELECT category FROM business_categories WHERE business_id=?1 ORDER BY category",
        &business.id,
    )?;
    let mut tags = BTreeMap::new();
    {
        let mut stmt = conn.prepare(
            "SELECT key, value FROM business_tags WHERE business_id=?1 ORDER BY key, value",
        )?;
        let rows = stmt
            .query_map([&business.id], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        for (key, value) in rows {
            tags.insert(key, value);
        }
    }
    let sources = load_sources(conn, &business.id, include_raw)?;
    Ok(BusinessFull {
        business,
        contacts,
        aliases,
        categories,
        tags,
        sources,
    })
}

fn load_contacts(conn: &Connection, id: &str) -> Result<Vec<Contact>> {
    let mut stmt = conn.prepare(
        "SELECT kind, value, normalized, label, is_primary, source
         FROM contacts WHERE business_id=?1
         ORDER BY is_primary DESC,
           CASE kind
             WHEN 'phone' THEN 0 WHEN 'mobile' THEN 1 WHEN 'whatsapp' THEN 2
             WHEN 'email' THEN 3 WHEN 'website' THEN 4 ELSE 5
           END, value",
    )?;
    let contacts = stmt
        .query_map([id], |row| {
            Ok(Contact {
                kind: ContactKind::from_str_lossy(&row.get::<_, String>(0)?),
                value: row.get(1)?,
                normalized: row.get(2)?,
                label: row.get(3)?,
                is_primary: row.get::<_, i64>(4)? == 1,
                source: row.get(5)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(contacts)
}

fn load_strings(conn: &Connection, sql: &str, id: &str) -> Result<Vec<String>> {
    let mut stmt = conn.prepare(sql)?;
    let values = stmt
        .query_map([id], |row| row.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(values)
}

fn load_sources(conn: &Connection, id: &str, include_raw: bool) -> Result<Vec<SourceRecord>> {
    let mut stmt = conn.prepare(
        "SELECT source, external_id, source_url, confidence, first_seen_at, last_seen_at, raw_json
         FROM business_sources WHERE business_id=?1
         ORDER BY confidence DESC, source",
    )?;
    let rows = stmt
        .query_map([id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, f64>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, String>(5)?,
                row.get::<_, String>(6)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;

    Ok(rows
        .into_iter()
        .map(
            |(source, external_id, url, confidence, first_seen_at, last_seen_at, raw)| {
                SourceRecord {
                    source,
                    external_id,
                    url,
                    confidence,
                    first_seen_at,
                    last_seen_at,
                    raw: if include_raw {
                        serde_json::from_str(&raw).ok()
                    } else {
                        None
                    },
                }
            },
        )
        .collect())
}

pub fn stats(conn: &Connection) -> Result<Stats> {
    let total_businesses = count_where(conn, "SELECT COUNT(*) FROM businesses")?;
    let with_phone = count_where(
        conn,
        "SELECT COUNT(*) FROM businesses b WHERE EXISTS (
            SELECT 1 FROM contacts c WHERE c.business_id=b.id
            AND c.kind IN ('phone','mobile','whatsapp'))",
    )?;
    let with_email = count_where(
        conn,
        "SELECT COUNT(*) FROM businesses b WHERE EXISTS (
            SELECT 1 FROM contacts c WHERE c.business_id=b.id AND c.kind='email')",
    )?;
    let with_website = count_where(
        conn,
        "SELECT COUNT(*) FROM businesses b WHERE b.website IS NOT NULL OR EXISTS (
            SELECT 1 FROM contacts c WHERE c.business_id=b.id AND c.kind='website')",
    )?;
    let with_coordinates = count_where(
        conn,
        "SELECT COUNT(*) FROM businesses WHERE latitude IS NOT NULL AND longitude IS NOT NULL",
    )?;
    let with_address = count_where(
        conn,
        "SELECT COUNT(*) FROM businesses
         WHERE address_line IS NOT NULL OR street IS NOT NULL OR barrio IS NOT NULL
            OR municipio IS NOT NULL OR departamento IS NOT NULL",
    )?;

    let by_departamento = count_rows(
        conn,
        "SELECT COALESCE(NULLIF(departamento,''),'(sin departamento)'), COUNT(*)
         FROM businesses GROUP BY departamento ORDER BY COUNT(*) DESC",
    )?;
    let by_category = count_rows(
        conn,
        "SELECT COALESCE(NULLIF(primary_category,''),'(sin categoría)'), COUNT(*)
         FROM businesses GROUP BY primary_category ORDER BY COUNT(*) DESC LIMIT 25",
    )?;
    let by_source = count_rows(
        conn,
        "SELECT source, COUNT(DISTINCT business_id) FROM business_sources
         GROUP BY source ORDER BY COUNT(DISTINCT business_id) DESC",
    )?;
    let by_status = count_rows(
        conn,
        "SELECT COALESCE(NULLIF(status,''),'(desconocido)'), COUNT(*)
         FROM businesses GROUP BY status ORDER BY COUNT(*) DESC",
    )?;

    let mut stmt = conn.prepare(
        "SELECT id, started_at, finished_at, location, sources, query,
                fetched, inserted, merged, errors
         FROM scans ORDER BY started_at DESC LIMIT 10",
    )?;
    let last_scans = stmt
        .query_map([], |row| {
            Ok(ScanLogRow {
                id: row.get(0)?,
                started_at: row.get(1)?,
                finished_at: row.get(2)?,
                location: row.get(3)?,
                sources: row.get(4)?,
                query: row.get(5)?,
                fetched: row.get(6)?,
                inserted: row.get(7)?,
                merged: row.get(8)?,
                errors: row.get(9)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;

    Ok(Stats {
        total_businesses,
        with_phone,
        with_email,
        with_website,
        with_coordinates,
        with_address,
        by_departamento,
        by_category,
        by_source,
        by_status,
        last_scans,
    })
}

fn count_where(conn: &Connection, sql: &str) -> Result<i64> {
    Ok(conn.query_row(sql, [], |row| row.get(0))?)
}

fn count_rows(conn: &Connection, sql: &str) -> Result<Vec<CountRow>> {
    let mut stmt = conn.prepare(sql)?;
    let rows = stmt
        .query_map([], |row| {
            Ok(CountRow {
                label: row.get(0)?,
                count: row.get(1)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}
