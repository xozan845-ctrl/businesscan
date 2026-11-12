//! `businesscan overture`: descarga automática de Places desde PMTiles.

use std::time::Instant;

use anyhow::{bail, Result};
use clap::{Args, Subcommand};
use serde::Serialize;

use crate::cli::{GlobalArgs, LocationArgs};
use crate::config::Config;
use crate::db::business::import_batch;
use crate::http::HttpClient;
use crate::location::LocationResolver;
use crate::model::UpsertAction;
use crate::output::{self, Theme};
use crate::providers::overture_pmtiles::{
    exact_bbox, fetch_records, latest_release, pmtiles_url, FetchOptions,
};

#[derive(Debug, Args)]
pub struct OvertureArgs {
    #[command(subcommand)]
    pub command: OvertureCommand,
}

#[derive(Debug, Subcommand)]
#[allow(clippy::large_enum_variant)]
pub enum OvertureCommand {
    /// Descarga Places del área y las importa a la base
    Fetch(FetchArgs),
    /// Muestra la release más reciente y su URL PMTiles
    Latest,
}

#[derive(Debug, Args)]
pub struct FetchArgs {
    #[command(flatten)]
    pub location: LocationArgs,

    /// Release de Overture, por ejemplo 2026-10-21.0 (por defecto latest)
    #[arg(long)]
    pub release: Option<String>,

    /// URL PMTiles alternativa (avanzado)
    #[arg(long)]
    pub pmtiles_url: Option<String>,

    /// Zoom de tiles (por defecto automático según el área)
    #[arg(long)]
    pub zoom: Option<u8>,

    /// Máximo de tiles a leer
    #[arg(long, default_value_t = 120)]
    pub max_tiles: usize,

    /// Máximo de negocios a importar
    #[arg(short = 'n', long)]
    pub limit: Option<usize>,

    /// Descarga y analiza sin guardar
    #[arg(long)]
    pub dry_run: bool,
}

#[derive(Debug, Serialize)]
struct OvertureSummary {
    release: String,
    pmtiles_url: String,
    location: String,
    locations: usize,
    zoom: u8,
    tiles: usize,
    fetched_tiles: usize,
    features: usize,
    records: usize,
    inserted: usize,
    merged: usize,
    skipped: usize,
    duration_ms: u64,
    dry_run: bool,
}

pub async fn run(
    args: OvertureArgs,
    config: &Config,
    global: &GlobalArgs,
    theme: &Theme,
) -> Result<i32> {
    match args.command {
        OvertureCommand::Latest => {
            let http = HttpClient::new(config)?;
            let release = latest_release(&http).await?;
            let url = pmtiles_url(&release);
            if global.json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::json!({
                        "release": release,
                        "pmtiles_url": url,
                    }))?
                );
            } else {
                println!("release: {release}");
                println!("pmtiles: {url}");
            }
            Ok(0)
        }
        OvertureCommand::Fetch(args) => fetch_command(args, config, global, theme).await,
    }
}

async fn fetch_command(
    args: FetchArgs,
    config: &Config,
    global: &GlobalArgs,
    theme: &Theme,
) -> Result<i32> {
    if args.max_tiles == 0 {
        bail!("--max-tiles debe ser mayor que cero");
    }
    let input = args.location.to_input();
    input.validate()?;
    let mut db = crate::cli::open_db(config)?;
    let http = HttpClient::new(config)?;
    let resolver = LocationResolver::new(config, http.clone());

    let locations = if input.continent.is_some() {
        resolver.resolve_many(&input).await?
    } else {
        let cache_key = input.cache_key();
        match if args.location.refresh_place {
            None
        } else {
            db.cached_place(&cache_key)?
        } {
            Some(location) => vec![location],
            None => {
                let location = resolver.resolve(&input).await.map_err(|error| {
                    anyhow::anyhow!("No se pudo resolver la ubicación: {error:#}")
                })?;
                db.save_place(&cache_key, &location)?;
                vec![location]
            }
        }
    };

    let (release, url) = match args.pmtiles_url.clone() {
        Some(url) => {
            let release = args.release.clone().unwrap_or_else(|| "custom".to_string());
            (release, url)
        }
        None => {
            let release = match args.release.as_deref() {
                Some("latest") | None => latest_release(&http).await?,
                Some(release) => release.to_string(),
            };
            let url = pmtiles_url(&release);
            (release, url)
        }
    };

    if !global.json && !global.quiet {
        output::banner(theme);
        for location in &locations {
            println!("  {} {}", theme.muted("Ubicación:"), location.display_name);
        }
        println!("  {} {}", theme.muted("Release:"), release);
        println!("  {} {}", theme.muted("PMTiles:"), url);
        println!();
    }

    let started = Instant::now();
    let mut records = Vec::new();
    let mut total_tiles = 0usize;
    let mut total_fetched_tiles = 0usize;
    let mut total_features = 0usize;
    let mut zoom = 0u8;

    for location in &locations {
        let bbox = exact_bbox(location).ok_or_else(|| {
            anyhow::anyhow!(
                "No se pudo determinar un bbox para «{}». Usa --bbox, --lat/--lon o un municipio/departamento",
                location.display_name
            )
        })?;
        let (mut location_records, stats) = fetch_records(
            &http,
            &url,
            &FetchOptions {
                location: location.clone(),
                bbox,
                zoom: args.zoom,
                max_tiles: args.max_tiles,
                limit: args.limit,
            },
        )
        .await?;
        if !global.json && !global.quiet {
            println!(
                "  {} {} · zoom {} · {} tiles · {} features · {} negocios · {:.1}s",
                theme.success("✓"),
                location.display_name,
                stats.zoom,
                stats.tiles,
                stats.features,
                location_records.len(),
                stats.duration_ms as f64 / 1000.0
            );
            if stats.fetched_tiles == 0 {
                eprintln!(
                    "  {} No hay tiles PMTiles en zoom {}. Usa --zoom 14 (el archivo solo tiene datos en ese nivel).",
                    theme.warning("!"),
                    stats.zoom
                );
            }
        }
        if zoom == 0 {
            zoom = stats.zoom;
        }
        total_tiles += stats.tiles;
        total_fetched_tiles += stats.fetched_tiles;
        total_features += stats.features;
        records.append(&mut location_records);
    }

    let summary_location = if locations.len() == 1 {
        locations[0].display_name.clone()
    } else {
        format!("{} países/áreas", locations.len())
    };
    let mut summary = OvertureSummary {
        release,
        pmtiles_url: url,
        location: summary_location,
        locations: locations.len(),
        zoom,
        tiles: total_tiles,
        fetched_tiles: total_fetched_tiles,
        features: total_features,
        records: records.len(),
        inserted: 0,
        merged: 0,
        skipped: 0,
        duration_ms: started.elapsed().as_millis() as u64,
        dry_run: args.dry_run,
    };

    if args.dry_run {
        if !global.json {
            for record in records.iter().take(10) {
                println!(
                    "  {} {} {}",
                    theme.cyan("·"),
                    theme.bold(&record.name),
                    theme.muted(format!(
                        "{} · {}",
                        record
                            .primary_category
                            .as_deref()
                            .unwrap_or("sin categoría"),
                        record
                            .address
                            .municipio
                            .as_deref()
                            .unwrap_or("sin municipio")
                    ))
                );
            }
            println!("  {}", theme.warning("dry-run: no se guardó nada"));
        }
    } else if !records.is_empty() {
        for chunk in records.chunks(200) {
            let results = import_batch(&mut db, chunk)?;
            for result in results {
                match result.action {
                    UpsertAction::Inserted => summary.inserted += 1,
                    UpsertAction::Merged => summary.merged += 1,
                    UpsertAction::Skipped => summary.skipped += 1,
                }
            }
        }
        if !global.json && !global.quiet {
            println!(
                "  {} {} nuevos · {} fusionados · {} omitidos",
                theme.success("✓"),
                summary.inserted,
                summary.merged,
                summary.skipped
            );
            println!();
        }
    }

    if global.json {
        println!("{}", serde_json::to_string_pretty(&summary)?);
    }
    Ok(0)
}
