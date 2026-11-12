//! `nicascan show`: ficha completa de un negocio.

use anyhow::Result;
use clap::Args;

use crate::cli::GlobalArgs;
use crate::config::Config;
use crate::db::business::get_full;
use crate::model::BusinessFull;
use crate::output::{self, Theme};

#[derive(Debug, Args)]
pub struct ShowArgs {
    /// ID completo o prefijo único del negocio
    pub id: String,

    /// Incluye el JSON original de cada fuente
    #[arg(long)]
    pub raw: bool,
}

pub fn run(args: ShowArgs, config: &Config, global: &GlobalArgs, theme: &Theme) -> Result<i32> {
    let db = crate::cli::open_db(config)?;
    let full = get_full(&db, &args.id, args.raw)?;

    if global.json {
        println!("{}", serde_json::to_string_pretty(&full)?);
        return Ok(0);
    }

    print_human(&full, theme);
    Ok(0)
}

fn print_human(full: &BusinessFull, theme: &Theme) {
    let b = &full.business;
    println!();
    println!("  {}", theme.heading(&b.name));
    println!(
        "  {}",
        theme.muted(format!(
            "{} · {}",
            b.primary_category
                .as_deref()
                .or(b.business_type.as_deref())
                .unwrap_or("sin categoría"),
            b.location_label()
        ))
    );
    println!();
    output::kv_line(theme, "ID", &b.id);
    if let Some(value) = b.status.as_deref() {
        output::kv_line(theme, "Estado", value);
    }
    output::kv_line(
        theme,
        "Confianza",
        &format!(
            "{:.0}% · {} fuente{}",
            b.confidence * 100.0,
            b.source_count,
            if b.source_count == 1 { "" } else { "s" }
        ),
    );
    output::kv_line(
        theme,
        "Calidad",
        &format!("{:.0}% de ficha completa", b.quality_score * 100.0),
    );
    if let Some(value) = b.business_type.as_deref() {
        output::kv_line(theme, "Tipo", value);
    }
    if let Some(value) = b.legal_name.as_deref() {
        output::kv_line(theme, "Razón social", value);
    }

    if let Some(description) = b.description.as_deref() {
        output::section(theme, "Descripción");
        println!("  {description}");
    }

    if !full.contacts.is_empty() {
        output::section(theme, "Contactos");
        for contact in &full.contacts {
            let star = if contact.is_primary { "*" } else { " " };
            let label = contact
                .label
                .as_deref()
                .unwrap_or_else(|| contact.kind.label_es());
            println!(
                "  {} {} {}",
                theme.cyan(star),
                theme.muted(format!("{label:<16}")),
                contact.value
            );
        }
    }

    output::section(theme, "Ubicación");
    if let Some(line) = b.address.line.as_deref() {
        output::kv_line(theme, "Dirección", line);
    }
    if let Some(street) = b.address.street.as_deref() {
        let number = b.address.house_number.as_deref().unwrap_or("");
        let calle = format!("{street} {number}");
        output::kv_line(theme, "Calle", calle.trim());
    }
    if let Some(value) = b.address.barrio.as_deref() {
        output::kv_line(theme, "Barrio", value);
    }
    if let Some(value) = b.address.comarca.as_deref() {
        output::kv_line(theme, "Comarca", value);
    }
    if let Some(value) = b.address.municipio.as_deref() {
        output::kv_line(theme, "Municipio", value);
    }
    if let Some(value) = b.address.departamento.as_deref() {
        output::kv_line(theme, "Departamento", value);
    }
    if let Some(value) = b.address.postal_code.as_deref() {
        output::kv_line(theme, "Código postal", value);
    }
    if let (Some(lat), Some(lon)) = (b.latitude, b.longitude) {
        output::kv_line(theme, "Coordenadas", &format!("{lat:.6}, {lon:.6}"));
        if let Some(geohash) = b.geohash.as_deref() {
            output::kv_line(theme, "Geohash", geohash);
        }
        println!(
            "  {} {}",
            theme.muted("Mapa         "),
            theme.cyan(format!(
                "https://www.openstreetmap.org/?mlat={lat}&mlon={lon}#map=18/{lat}/{lon}"
            ))
        );
    }
    if let Some(distance) = b.distance_m {
        output::kv_line(
            theme,
            "Distancia",
            &crate::normalize::format_distance(distance),
        );
    }

    if b.opening_hours.is_some() || b.price_range.is_some() || b.rating.is_some() {
        output::section(theme, "Operación");
        if let Some(value) = b.opening_hours.as_deref() {
            output::kv_line(theme, "Horario", value);
        }
        if let Some(value) = b.price_range.as_deref() {
            output::kv_line(theme, "Precio", value);
        }
        if let Some(value) = b.rating {
            let reviews = b
                .review_count
                .map(|count| format!(" · {count} reseñas"))
                .unwrap_or_default();
            output::kv_line(theme, "Rating", &format!("{value:.1}{reviews}"));
        }
    }

    if !full.categories.is_empty() {
        output::section(theme, "Categorías");
        for category in &full.categories {
            output::bullet(theme, category);
        }
    }
    if !full.aliases.is_empty() {
        output::section(theme, "También conocido como");
        for alias in &full.aliases {
            output::bullet(theme, alias);
        }
    }

    if !full.tags.is_empty() {
        output::section(theme, "Etiquetas");
        for (key, value) in &full.tags {
            println!(
                "  {} {} {}",
                theme.muted("·"),
                theme.muted(format!("{key}:")),
                value
            );
        }
    }

    output::section(theme, "Procedencia");
    for source in &full.sources {
        println!(
            "  {} {} {}",
            theme.cyan("·"),
            theme.bold(&source.source),
            theme.muted(format!("confianza {:.0}%", source.confidence * 100.0))
        );
        println!("    {} {}", theme.muted("id:"), source.external_id);
        if let Some(url) = source.url.as_deref() {
            println!("    {url}");
        }
        println!(
            "    {} visto por última vez {}",
            theme.muted("→"),
            source.last_seen_at
        );
    }

    output::section(theme, "Fechas");
    output::kv_line(theme, "Primera vez", &b.first_seen_at);
    output::kv_line(theme, "Última vez", &b.last_seen_at);
    output::kv_line(theme, "Actualizado", &b.updated_at);
    println!();
}
