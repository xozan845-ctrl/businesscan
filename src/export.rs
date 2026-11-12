//! Exportación para consumo humano o por una IA: JSON, JSONL, CSV y SQLite.

use std::collections::BTreeSet;
use std::fs::File;
use std::io::{self, BufWriter, Write};
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};

use crate::db::business::search_full;
use crate::db::Database;
use crate::model::{BusinessFull, ContactKind, SearchQuery};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportFormat {
    Json,
    Jsonl,
    Csv,
    Sqlite,
}

impl ExportFormat {
    pub fn parse(value: &str) -> Result<Self> {
        match value.to_lowercase().as_str() {
            "json" => Ok(Self::Json),
            "jsonl" | "ndjson" => Ok(Self::Jsonl),
            "csv" => Ok(Self::Csv),
            "sqlite" | "db" => Ok(Self::Sqlite),
            other => bail!("Formato desconocido: {other} (usa json, jsonl, csv o sqlite)"),
        }
    }
}

pub fn export(
    db: &Database,
    query: &SearchQuery,
    format: ExportFormat,
    output: &str,
    include_raw: bool,
    pretty: bool,
    force: bool,
) -> Result<usize> {
    if format == ExportFormat::Sqlite {
        let target = PathBuf::from(output);
        if output == "-" {
            bail!("SQLite no se puede escribir en stdout; indica un archivo .db");
        }
        if target.exists() && !force {
            bail!(
                "El archivo {} ya existe (usa --force para sobrescribirlo)",
                target.display()
            );
        }
        if target.exists() && force {
            std::fs::remove_file(&target)
                .with_context(|| format!("No se pudo reemplazar {}", target.display()))?;
        }
        db.backup_to(&target)?;
        return Ok(1);
    }

    let writer = open_writer(output)?;
    let mut writer = BufWriter::new(writer);
    let mut query = query.clone();
    query.include_raw = include_raw;
    let mut offset = 0usize;
    let mut written = 0usize;
    let requested = query.limit;

    if format == ExportFormat::Json {
        writeln!(writer, "[")?;
    }

    if format == ExportFormat::Csv {
        write_csv_header(&mut writer)?;
    }

    loop {
        let remaining = if requested == 0 {
            500
        } else {
            requested.saturating_sub(written).min(500)
        };
        if remaining == 0 {
            break;
        }
        query.limit = remaining;
        query.offset = offset;
        let page = search_full(db, &query)?;
        if page.is_empty() {
            break;
        }
        let page_len = page.len();
        for business in page.iter() {
            match format {
                ExportFormat::Json => {
                    let separator = if written == 0 { "" } else { "," };
                    if pretty {
                        let text = serde_json::to_string_pretty(business)?;
                        writeln!(writer, "{separator}{text}")?;
                    } else {
                        let text = serde_json::to_string(business)?;
                        write!(writer, "{separator}{text}")?;
                    }
                }
                ExportFormat::Jsonl => {
                    let text = serde_json::to_string(business)?;
                    writeln!(writer, "{text}")?;
                }
                ExportFormat::Csv => write_csv_row(&mut writer, business)?,
                ExportFormat::Sqlite => unreachable!(),
            }
            written += 1;
            if written % 1000 == 0 {
                log::debug!("Exportados {written} negocios…");
            }
        }
        offset += page_len;
        if page_len < remaining {
            break;
        }
    }

    if format == ExportFormat::Json {
        writeln!(writer, "\n]")?;
    }
    writer.flush()?;
    Ok(written)
}

fn open_writer(output: &str) -> Result<Box<dyn Write>> {
    if output == "-" {
        return Ok(Box::new(io::stdout()));
    }
    let path = Path::new(output);
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)?;
        }
    }
    let file = File::create(path)
        .with_context(|| format!("No se pudo crear el archivo {}", path.display()))?;
    Ok(Box::new(file))
}

fn write_csv_header(writer: &mut impl Write) -> Result<()> {
    let columns = [
        "id",
        "name",
        "legal_name",
        "primary_category",
        "business_type",
        "status",
        "description",
        "phone_primary",
        "email_primary",
        "website",
        "phones",
        "mobiles",
        "whatsapps",
        "emails",
        "websites",
        "facebook",
        "instagram",
        "tiktok",
        "twitter",
        "telegram",
        "linkedin",
        "youtube",
        "address_line",
        "street",
        "house_number",
        "barrio",
        "comarca",
        "municipio",
        "departamento",
        "country_code",
        "postal_code",
        "latitude",
        "longitude",
        "geohash",
        "opening_hours",
        "price_range",
        "rating",
        "review_count",
        "source_count",
        "confidence",
        "quality_score",
        "sources",
        "source_urls",
        "aliases",
        "categories",
        "tags",
        "first_seen_at",
        "last_seen_at",
        "updated_at",
    ];
    let mut csv = csv::WriterBuilder::new()
        .has_headers(false)
        .from_writer(writer);
    csv.write_record(columns)?;
    csv.flush()?;
    Ok(())
}

fn write_csv_row(writer: &mut impl Write, item: &BusinessFull) -> Result<()> {
    let b = &item.business;
    let phones = contact_values(item, &[ContactKind::Phone]);
    let mobiles = contact_values(item, &[ContactKind::Mobile]);
    let whatsapps = contact_values(item, &[ContactKind::Whatsapp]);
    let emails = contact_values(item, &[ContactKind::Email]);
    let websites = contact_values(item, &[ContactKind::Website]);
    let facebook = contact_values(item, &[ContactKind::Facebook]);
    let instagram = contact_values(item, &[ContactKind::Instagram]);
    let tiktok = contact_values(item, &[ContactKind::TikTok]);
    let twitter = contact_values(item, &[ContactKind::Twitter]);
    let telegram = contact_values(item, &[ContactKind::Telegram]);
    let linkedin = contact_values(item, &[ContactKind::LinkedIn]);
    let youtube = contact_values(item, &[ContactKind::YouTube]);

    let sources: BTreeSet<&str> = item.sources.iter().map(|s| s.source.as_str()).collect();
    let source_urls = item
        .sources
        .iter()
        .filter_map(|s| s.url.as_deref())
        .collect::<Vec<_>>()
        .join(" | ");
    let tags = item
        .tags
        .iter()
        .map(|(key, value)| format!("{key}={value}"))
        .collect::<Vec<_>>()
        .join(" | ");
    let aliases = item.aliases.join(" | ");
    let categories = item.categories.join(" | ");

    let record = [
        b.id.clone(),
        b.name.clone(),
        b.legal_name.clone().unwrap_or_default(),
        b.primary_category.clone().unwrap_or_default(),
        b.business_type.clone().unwrap_or_default(),
        b.status.clone().unwrap_or_default(),
        b.description.clone().unwrap_or_default(),
        b.phone_primary.clone().unwrap_or_default(),
        b.email_primary.clone().unwrap_or_default(),
        b.website.clone().unwrap_or_default(),
        phones,
        mobiles,
        whatsapps,
        emails,
        websites,
        facebook,
        instagram,
        tiktok,
        twitter,
        telegram,
        linkedin,
        youtube,
        b.address.line.clone().unwrap_or_default(),
        b.address.street.clone().unwrap_or_default(),
        b.address.house_number.clone().unwrap_or_default(),
        b.address.barrio.clone().unwrap_or_default(),
        b.address.comarca.clone().unwrap_or_default(),
        b.address.municipio.clone().unwrap_or_default(),
        b.address.departamento.clone().unwrap_or_default(),
        b.address.country_code.clone().unwrap_or_default(),
        b.address.postal_code.clone().unwrap_or_default(),
        b.latitude.map(|v| v.to_string()).unwrap_or_default(),
        b.longitude.map(|v| v.to_string()).unwrap_or_default(),
        b.geohash.clone().unwrap_or_default(),
        b.opening_hours.clone().unwrap_or_default(),
        b.price_range.clone().unwrap_or_default(),
        b.rating.map(|v| v.to_string()).unwrap_or_default(),
        b.review_count.map(|v| v.to_string()).unwrap_or_default(),
        b.source_count.to_string(),
        format!("{:.3}", b.confidence),
        format!("{:.3}", b.quality_score),
        sources.into_iter().collect::<Vec<_>>().join(","),
        source_urls,
        aliases,
        categories,
        tags,
        b.first_seen_at.clone(),
        b.last_seen_at.clone(),
        b.updated_at.clone(),
    ];

    let mut csv = csv::WriterBuilder::new()
        .has_headers(false)
        .from_writer(writer);
    csv.write_record(record)?;
    csv.flush()?;
    Ok(())
}

fn contact_values(item: &BusinessFull, kinds: &[ContactKind]) -> String {
    item.contacts
        .iter()
        .filter(|contact| kinds.contains(&contact.kind))
        .map(|contact| contact.value.clone())
        .collect::<Vec<_>>()
        .join(" | ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::business::import_batch;
    use crate::model::BusinessRecord;

    #[test]
    fn exports_jsonl_and_csv() {
        let mut db = Database::open_memory().unwrap();
        let mut record = BusinessRecord::new("import", "Café Central");
        record.primary_category = Some("Cafetería".into());
        record.address.municipio = Some("Managua".into());
        import_batch(&mut db, &[record]).unwrap();

        let dir = tempfile::tempdir().unwrap();
        let jsonl = dir.path().join("out.jsonl");
        let count = export(
            &db,
            &SearchQuery::default(),
            ExportFormat::Jsonl,
            jsonl.to_str().unwrap(),
            false,
            false,
            false,
        )
        .unwrap();
        assert_eq!(count, 1);
        let content = std::fs::read_to_string(&jsonl).unwrap();
        assert!(content.contains("Café Central"));

        let csv = dir.path().join("out.csv");
        export(
            &db,
            &SearchQuery::default(),
            ExportFormat::Csv,
            csv.to_str().unwrap(),
            false,
            false,
            false,
        )
        .unwrap();
        let content = std::fs::read_to_string(&csv).unwrap();
        assert!(content.contains("Café Central"));
        assert!(content.contains("municipio"));
    }
}
