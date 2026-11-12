//! `nicascan places`: caché de geocodificación.

use anyhow::{bail, Result};
use clap::{Args, Subcommand};
use serde::Serialize;

use crate::cli::GlobalArgs;
use crate::config::Config;
use crate::http::HttpClient;
use crate::location::{GeoFilter, LocationInput, LocationResolver};
use crate::output::{self, Theme};

#[derive(Debug, Args)]
pub struct PlacesArgs {
    #[command(subcommand)]
    pub command: PlacesCommand,
}

#[derive(Debug, Subcommand)]
pub enum PlacesCommand {
    /// Geocodifica un lugar y guarda el resultado en caché
    Resolve {
        /// Nombre del lugar, barrio, municipio o departamento
        #[arg(required = true, num_args = 1..)]
        query: Vec<String>,
        /// Departamento para desambiguar
        #[arg(short = 'd', long)]
        departamento: Option<String>,
        /// Municipio para desambiguar
        #[arg(short = 'm', long)]
        municipio: Option<String>,
        /// Ignora la caché existente
        #[arg(long)]
        refresh: bool,
    },
    /// Lista las ubicaciones en caché
    List {
        #[arg(short = 'n', long, default_value_t = 50)]
        limit: usize,
    },
    /// Vacía la caché de ubicaciones
    Clear {
        #[arg(long)]
        yes: bool,
    },
}

#[derive(Serialize)]
struct ResolveOutput {
    display_name: String,
    query: String,
    departamento: Option<String>,
    municipio: Option<String>,
    barrio: Option<String>,
    comarca: Option<String>,
    filter: GeoFilter,
    osm_type: Option<String>,
    osm_id: Option<i64>,
    source: String,
}

pub async fn run(
    args: PlacesArgs,
    config: &Config,
    global: &GlobalArgs,
    theme: &Theme,
) -> Result<i32> {
    let db = crate::cli::open_db(config)?;
    match args.command {
        PlacesCommand::Resolve {
            query,
            departamento,
            municipio,
            refresh,
        } => {
            let input = LocationInput {
                place: Some(query.join(" ")),
                departamento,
                municipio,
                ..Default::default()
            };
            input.validate()?;
            let key = input.cache_key();
            let cached = if refresh {
                None
            } else {
                db.cached_place(&key)?
            };
            let location = match cached {
                Some(location) => location,
                None => {
                    let http = HttpClient::new(config)?;
                    let resolver = LocationResolver::new(config, http);
                    let location = resolver.resolve(&input).await?;
                    db.save_place(&key, &location)?;
                    location
                }
            };
            if global.json {
                let output = ResolveOutput {
                    display_name: location.display_name.clone(),
                    query: location.query.clone(),
                    departamento: location.departamento.clone(),
                    municipio: location.municipio.clone(),
                    barrio: location.barrio.clone(),
                    comarca: location.comarca.clone(),
                    filter: location.filter.clone(),
                    osm_type: location.osm_type.clone(),
                    osm_id: location.osm_id,
                    source: location.source.clone(),
                };
                println!("{}", serde_json::to_string_pretty(&output)?);
            } else {
                println!();
                println!("  {}", theme.heading(&location.display_name));
                println!("  {} {}", theme.muted("Consulta:"), location.query);
                let filter = format!("{:?}", location.filter);
                println!("  {} {}", theme.muted("Filtro:"), filter);
                if let Some(value) = location.departamento.as_deref() {
                    output::kv_line(theme, "Departamento", value);
                }
                if let Some(value) = location.municipio.as_deref() {
                    output::kv_line(theme, "Municipio", value);
                }
                if let Some(value) = location.barrio.as_deref() {
                    output::kv_line(theme, "Barrio", value);
                }
                if let Some(value) = location.osm_id {
                    output::kv_line(
                        theme,
                        "OSM",
                        &format!("{} {}", location.osm_type.as_deref().unwrap_or(""), value),
                    );
                }
                println!();
            }
        }
        PlacesCommand::List { limit } => {
            let places = db.list_places(limit)?;
            if global.json {
                println!("{}", serde_json::to_string_pretty(&places)?);
            } else if places.is_empty() {
                println!("No hay ubicaciones en caché.");
            } else {
                let mut table = comfy_table::Table::new();
                table
                    .load_preset(comfy_table::presets::UTF8_FULL)
                    .set_content_arrangement(comfy_table::ContentArrangement::Dynamic)
                    .set_header(vec!["Consultado", "Lugar", "OSM", "Cacheado"]);
                for place in places {
                    table.add_row(vec![
                        place.query,
                        place.display_name,
                        place
                            .osm_id
                            .map(|id| format!("{} {id}", place.osm_type.unwrap_or_default()))
                            .unwrap_or_else(|| "—".to_string()),
                        place.fetched_at,
                    ]);
                }
                println!("{table}");
            }
        }
        PlacesCommand::Clear { yes } => {
            if !yes {
                bail!("Usa `nicascan places clear --yes` para confirmar.");
            }
            let count = db.clear_places()?;
            if !global.quiet {
                println!(
                    "{} {count} ubicaciones eliminadas de la caché.",
                    theme.success("✓")
                );
            }
        }
    }
    Ok(0)
}
