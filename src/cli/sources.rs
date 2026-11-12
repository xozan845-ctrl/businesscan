//! `nicascan sources`: estado de las fuentes integradas.

use anyhow::Result;
use serde::Serialize;

use crate::cli::GlobalArgs;
use crate::config::Config;
use crate::output::{self, Theme};
use crate::providers::all_providers;

#[derive(Serialize)]
struct SourceInfo {
    name: String,
    description: String,
    requires_key: bool,
    key_env: Option<String>,
    available: bool,
    status: String,
}

pub fn run(config: &Config, global: &GlobalArgs, theme: &Theme) -> Result<i32> {
    let sources: Vec<SourceInfo> = all_providers()
        .into_iter()
        .map(|provider| {
            let available = provider.is_available(config);
            let status = if !provider.requires_key() {
                "lista".to_string()
            } else if available {
                "clave configurada".to_string()
            } else {
                format!("falta {}", provider.key_env().unwrap_or("la clave de API"))
            };
            SourceInfo {
                name: provider.name().to_string(),
                description: provider.description().to_string(),
                requires_key: provider.requires_key(),
                key_env: provider.key_env().map(ToString::to_string),
                available,
                status,
            }
        })
        .collect();

    if global.json {
        println!("{}", serde_json::to_string_pretty(&sources)?);
        return Ok(0);
    }

    output::banner(theme);
    println!("  {}", theme.heading("Fuentes"));
    let mut table = comfy_table::Table::new();
    table
        .load_preset(comfy_table::presets::UTF8_FULL)
        .set_content_arrangement(comfy_table::ContentArrangement::Dynamic)
        .set_header(vec!["Fuente", "Estado", "Clave", "Descripción"]);
    for source in &sources {
        table.add_row(vec![
            source.name.clone(),
            source.status.clone(),
            source.key_env.clone().unwrap_or_else(|| "—".to_string()),
            source.description.clone(),
        ]);
    }
    println!("{table}");
    println!();
    println!(
        "  {}",
        theme.muted(
            "Variables: GOOGLE_PLACES_API_KEY, FOURSQUARE_API_KEY. \
             OSM no requiere clave y es la fuente por defecto."
        )
    );
    println!(
        "  {}",
        theme.muted(
            "Respeta los términos de cada proveedor y las políticas de uso de \
             OpenStreetMap/Nominatim/Overpass."
        )
    );
    println!();
    Ok(0)
}
