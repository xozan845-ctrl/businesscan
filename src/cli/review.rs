//! Revisión de duplicados y fusión manual.

use anyhow::{bail, Result};
use clap::Args;

use crate::cli::GlobalArgs;
use crate::config::Config;
use crate::db::business::{find_duplicates, merge_businesses};
use crate::normalize::short_id;
use crate::output::{self, Theme};

#[derive(Debug, Args)]
pub struct DuplicatesArgs {
    /// Umbral mínimo de similitud (0..1)
    #[arg(long, default_value_t = 0.75)]
    pub min_score: f64,

    /// Distancia máxima entre candidatos con coordenadas (metros)
    #[arg(long, default_value_t = 5_000.0)]
    pub max_distance: f64,

    /// Máximo de pares a mostrar
    #[arg(short = 'n', long, default_value_t = 30)]
    pub limit: usize,
}

#[derive(Debug, Args)]
pub struct MergeArgs {
    /// ID (o prefijo único) del negocio que se conserva
    pub keep: String,

    /// ID (o prefijo único) del negocio que se absorbe
    pub other: String,

    /// Confirma la fusión
    #[arg(long)]
    pub yes: bool,
}

pub fn duplicates(
    args: DuplicatesArgs,
    config: &Config,
    global: &GlobalArgs,
    theme: &Theme,
) -> Result<i32> {
    if !(0.0..=1.0).contains(&args.min_score) {
        bail!("--min-score debe estar entre 0 y 1");
    }
    if args.max_distance < 0.0 {
        bail!("--max-distance no puede ser negativo");
    }
    let db = crate::cli::open_db(config)?;
    let pairs = find_duplicates(&db.conn, args.min_score, args.max_distance, args.limit)?;

    if global.json {
        println!("{}", serde_json::to_string_pretty(&pairs)?);
        return Ok(0);
    }

    output::banner(theme);
    if pairs.is_empty() {
        println!(
            "  {} No se encontraron pares por encima de {:.0}%.",
            theme.success("✓"),
            args.min_score * 100.0
        );
        println!();
        return Ok(0);
    }

    println!(
        "  {} {} posibles duplicados (revisa antes de fusionar)",
        theme.warning("!"),
        pairs.len()
    );
    let mut table = comfy_table::Table::new();
    table
        .load_preset(comfy_table::presets::UTF8_FULL)
        .set_content_arrangement(comfy_table::ContentArrangement::Dynamic)
        .set_header(vec!["Score", "Negocio A", "Negocio B", "Señales"]);
    for pair in &pairs {
        table.add_row(vec![
            format!("{:.0}%", pair.score * 100.0),
            format!(
                "{} ({})\n{}",
                pair.left.name,
                short_id(&pair.left.id),
                pair.left.phone_primary.as_deref().unwrap_or("sin teléfono")
            ),
            format!(
                "{} ({})\n{}",
                pair.right.name,
                short_id(&pair.right.id),
                pair.right
                    .phone_primary
                    .as_deref()
                    .unwrap_or("sin teléfono")
            ),
            pair.reasons.join(", "),
        ]);
    }
    println!("{table}");
    println!();
    println!(
        "  {}",
        theme.muted("Fusiona con: businesscan merge <id_que_conservas> <id_absorbido> --yes")
    );
    println!();
    Ok(0)
}

pub fn merge(args: MergeArgs, config: &Config, global: &GlobalArgs, theme: &Theme) -> Result<i32> {
    if !args.yes {
        bail!(
            "Esto fusiona dos negocios de forma irreversible. Revisa con `businesscan show` y repite con --yes"
        );
    }
    let mut db = crate::cli::open_db(config)?;
    let keep_id = merge_businesses(&mut db, &args.keep, &args.other)?;
    let full = crate::db::business::get_full(&db, &keep_id, false)?;
    if global.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "kept": full.business.id,
                "name": full.business.name,
                "sources": full.sources.len(),
                "contacts": full.contacts.len(),
            }))?
        );
    } else if !global.quiet {
        println!(
            "{} Fusión completada: {} ({})",
            theme.success("✓"),
            full.business.name,
            short_id(&full.business.id)
        );
        println!(
            "  {}",
            theme.muted(format!(
                "{} observaciones · {} contactos · calidad {:.0}%",
                full.sources.len(),
                full.contacts.len(),
                full.business.quality_score * 100.0
            ))
        );
        println!();
    }
    Ok(0)
}
