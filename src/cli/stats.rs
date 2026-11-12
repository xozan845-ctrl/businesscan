//! `nicascan stats`: cobertura y distribución de la base.

use anyhow::Result;
use comfy_table::{presets::UTF8_FULL, Cell, ContentArrangement, Table};

use crate::cli::GlobalArgs;
use crate::config::Config;
use crate::db::Database;
use crate::model::CountRow;
use crate::output::{self, Theme};

pub fn run(config: &Config, global: &GlobalArgs, theme: &Theme) -> Result<i32> {
    let db = Database::open(&config.db_path)?;
    let stats = db.stats()?;

    if global.json {
        println!("{}", serde_json::to_string_pretty(&stats)?);
        return Ok(0);
    }

    if stats.total_businesses == 0 {
        println!();
        println!(
            "  {} La base está vacía. Empieza con `nicascan scan --place \"León\"`.",
            theme.warning("·")
        );
        println!();
        return Ok(0);
    }

    output::banner(theme);
    println!("  {}", theme.heading("Cobertura"));
    let total = stats.total_businesses.max(1) as f64;
    for (label, value) in [
        ("Con teléfono", stats.with_phone),
        ("Con correo", stats.with_email),
        ("Con sitio web", stats.with_website),
        ("Con coordenadas", stats.with_coordinates),
        ("Con dirección", stats.with_address),
    ] {
        let pct = value as f64 / total * 100.0;
        println!(
            "  {} {} {:.0}%",
            theme.muted(format!("{label:<18}")),
            bar(pct, theme),
            pct
        );
    }
    println!();
    output::kv_line(theme, "Total negocios", &stats.total_businesses.to_string());
    println!();

    print_counts(theme, "Por departamento", &stats.by_departamento);
    print_counts(theme, "Por categoría", &stats.by_category);
    print_counts(theme, "Por fuente", &stats.by_source);
    print_counts(theme, "Por estado", &stats.by_status);

    if !stats.last_scans.is_empty() {
        output::section(theme, "Últimas consultas");
        let mut table = Table::new();
        table
            .load_preset(UTF8_FULL)
            .set_content_arrangement(ContentArrangement::Dynamic)
            .set_header(vec![
                "Fecha",
                "Ubicación",
                "Fuentes",
                "Encontrados",
                "Nuevos",
                "Fusionados",
            ]);
        for scan in &stats.last_scans {
            table.add_row(vec![
                Cell::new(truncate_date(&scan.started_at)),
                Cell::new(&scan.location),
                Cell::new(&scan.sources),
                Cell::new(scan.fetched.to_string()),
                Cell::new(scan.inserted.to_string()),
                Cell::new(scan.merged.to_string()),
            ]);
        }
        println!("{table}");
    }
    println!();
    Ok(0)
}

fn print_counts(theme: &Theme, title: &str, rows: &[CountRow]) {
    if rows.is_empty() {
        return;
    }
    output::section(theme, title);
    let max = rows.iter().map(|row| row.count).max().unwrap_or(1).max(1) as f64;
    for row in rows.iter().take(20) {
        let width = ((row.count as f64 / max) * 20.0).round() as usize;
        println!(
            "  {} {} {}",
            theme.muted(format!("{:<28}", truncate(&row.label, 28))),
            theme.cyan("▇".repeat(width.max(1))),
            row.count
        );
    }
}

fn bar(pct: f64, theme: &Theme) -> String {
    let filled = ((pct / 100.0) * 16.0).round() as usize;
    format!(
        "{}{}",
        theme.cyan("█".repeat(filled)),
        theme.muted("░".repeat(16 - filled))
    )
}

fn truncate(value: &str, max: usize) -> String {
    crate::normalize::truncate(value, max)
}

fn truncate_date(value: &str) -> String {
    value.split('T').next().unwrap_or(value).to_string()
}
