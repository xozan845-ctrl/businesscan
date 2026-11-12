//! `nicascan scan`: consulta una o varias fuentes y guarda los negocios.

use std::io::IsTerminal;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use clap::Args;
use indicatif::{ProgressBar, ProgressStyle};

use crate::cli::{GlobalArgs, LocationArgs};
use crate::config::Config;
use crate::db::business::import_batch;
use crate::http::HttpClient;
use crate::location::{GeoFilter, LocationResolver, ResolvedLocation};
use crate::model::{BusinessRecord, ScanSourceSummary, ScanSummary, UpsertAction};
use crate::output::{self, Theme};
use crate::providers::{parse_sources, resolve_sources, ProviderContext, ProviderRequest};

#[derive(Debug, Args)]
pub struct ScanArgs {
    #[command(flatten)]
    pub location: LocationArgs,

    /// Categoría o tipo de negocio (coma para varias): ferreteria, restaurante…
    #[arg(short = 'c', long)]
    pub category: Option<String>,

    /// Texto libre para acotar la búsqueda en las fuentes
    #[arg(short = 'Q', long)]
    pub query: Option<String>,

    /// Fuentes separadas por coma (por defecto todas las configuradas)
    #[arg(short = 's', long, default_value = "")]
    pub source: String,

    /// Máximo de resultados por fuente
    #[arg(short = 'l', long)]
    pub limit: Option<usize>,

    /// Páginas máximas en fuentes paginadas (Google/Foursquare)
    #[arg(long, default_value_t = 3)]
    pub max_pages: usize,

    /// Incluye POIs sin nombre (OSM)
    #[arg(long)]
    pub include_unnamed: bool,

    /// Archivo GeoJSON/JSONL exportado de Overture Maps
    #[arg(long)]
    pub overture_file: Option<PathBuf>,

    /// Muestra lo que se encontró sin guardar
    #[arg(long)]
    pub dry_run: bool,
}

pub async fn run(
    args: ScanArgs,
    config: &Config,
    global: &GlobalArgs,
    theme: &Theme,
) -> Result<i32> {
    let human = !global.json && !global.quiet;
    if human {
        output::banner(theme);
        println!(
            "  {}",
            theme.muted("Consultando fuentes externas y deduplicando resultados…")
        );
    }

    let input = args.location.to_input();
    input.validate()?;
    let mut scan_config = config.clone();
    if let Some(path) = args.overture_file.clone() {
        scan_config.overture_file = Some(path);
    }
    let mut db = crate::cli::open_db(config)?;
    let http = HttpClient::new(&scan_config)?;
    let resolver = LocationResolver::new(&scan_config, http.clone());

    let cache_key = input.cache_key();
    let cached = if args.location.refresh_place {
        None
    } else {
        db.cached_place(&cache_key)?
    };
    let location = match cached {
        Some(location) => {
            if human {
                println!(
                    "  {} {}",
                    theme.muted("Ubicación:"),
                    theme.muted("desde caché")
                );
            }
            location
        }
        None => {
            let location = resolver
                .resolve(&input)
                .await
                .context("No se pudo resolver la ubicación")?;
            db.save_place(&cache_key, &location)?;
            location
        }
    };

    if human {
        println!();
        println!(
            "  {} {}",
            theme.cyan("◎"),
            theme.bold(&location.display_name)
        );
        println!(
            "  {} {}",
            theme.muted("Filtro:"),
            theme.muted(location_filter_label(&location))
        );
        println!();
    }

    let requested = if args.source.trim().is_empty() {
        if args.overture_file.is_some() {
            vec![crate::providers::SOURCE_OVERTURE.to_string()]
        } else {
            Vec::new()
        }
    } else {
        parse_sources(&args.source)
    };
    let providers = resolve_sources(&requested, &scan_config)?;
    let provider_names: Vec<String> = providers.iter().map(|p| p.name().to_string()).collect();

    if matches!(location.filter, GeoFilter::Country)
        && args.limit.is_none()
        && !global.quiet
        && provider_names.iter().any(|name| name == "osm")
    {
        eprintln!(
            "  {} Consulta de todo el país sin --limit: puede tardar y consumir mucha memoria en Overpass",
            theme.warning("!")
        );
    }

    if human
        && provider_names
            .iter()
            .any(|name| name == "google" || name == "foursquare")
        && args.limit.is_none()
    {
        eprintln!(
            "  {} Google/Foursquare pueden generar costos sin --limit. Considera usar -l 100",
            theme.warning("!")
        );
    }

    let ctx = ProviderContext {
        http,
        config: scan_config.clone(),
    };
    let request = ProviderRequest {
        location: location.clone(),
        query: args.query.clone(),
        category: args.category.clone(),
        limit: args.limit,
        include_unnamed: args.include_unnamed,
        page_size: 20,
        max_pages: args.max_pages,
    };

    let mut summary = ScanSummary {
        scan_id: db.begin_scan(
            &location.display_name,
            &provider_names,
            args.query.as_deref().or(args.category.as_deref()),
        )?,
        location: location.display_name.clone(),
        ..Default::default()
    };
    let scan_started = Instant::now();
    let mut samples: Vec<BusinessRecord> = Vec::new();

    for provider in &providers {
        let name = provider.name().to_string();
        let started = Instant::now();
        let spinner = spinner(human, &format!("{name}: consultando…"));
        let fetched = provider.fetch(&ctx, &request).await;
        if let Some(spinner) = spinner {
            spinner.finish_and_clear();
        }

        let mut source_summary = ScanSourceSummary {
            source: name.clone(),
            ..Default::default()
        };

        match fetched {
            Ok(records) => {
                source_summary.fetched = records.len();
                if args.dry_run {
                    for record in records.iter().take(5) {
                        if !samples
                            .iter()
                            .any(|r| r.name == record.name && r.source == record.source)
                        {
                            samples.push(record.clone());
                        }
                    }
                    if human {
                        println!(
                            "  {} {} {}",
                            theme.cyan("·"),
                            theme.bold(&name),
                            theme.muted(format!("{} resultados (dry-run)", records.len()))
                        );
                    }
                } else {
                    let pb =
                        progress_bar(human, records.len() as u64, &format!("{name}: guardando"));
                    let mut inserted = 0usize;
                    let mut merged = 0usize;
                    let mut skipped = 0usize;
                    for chunk in records.chunks(200) {
                        let results = import_batch(&mut db, chunk)?;
                        for result in results {
                            match result.action {
                                UpsertAction::Inserted => inserted += 1,
                                UpsertAction::Merged => merged += 1,
                                UpsertAction::Skipped => skipped += 1,
                            }
                        }
                        if let Some(pb) = &pb {
                            pb.inc(chunk.len() as u64);
                        }
                    }
                    if let Some(pb) = pb {
                        pb.finish_and_clear();
                    }
                    summary.total_inserted += inserted;
                    summary.total_merged += merged;
                    summary.total_skipped += skipped;
                    source_summary.inserted = inserted;
                    source_summary.merged = merged;
                    source_summary.skipped = skipped;
                    if human {
                        println!(
                            "  {} {} {}",
                            theme.success("✓"),
                            theme.bold(&name),
                            theme.muted(format!(
                                "{} encontrados · {} nuevos · {} fusionados",
                                records.len(),
                                inserted,
                                merged
                            ))
                        );
                    }
                }
                summary.total_fetched += records.len();
            }
            Err(error) => {
                let message = format!("{error:#}");
                log::warn!("{name}: {message}");
                if !global.json {
                    eprintln!(
                        "  {} {}: {}",
                        theme.error("✕"),
                        theme.bold(&name),
                        theme.red(&message)
                    );
                }
                source_summary.errors.push(message);
            }
        }

        source_summary.duration_ms = started.elapsed().as_millis() as u64;
        summary.sources.push(source_summary);
    }

    summary.duration_ms = scan_started.elapsed().as_millis() as u64;
    db.finish_scan(&summary)?;

    if global.json {
        println!("{}", serde_json::to_string_pretty(&summary)?);
        return Ok(exit_code(&summary));
    }

    if !global.quiet {
        print_summary(theme, &summary, args.dry_run);
        if args.dry_run && !samples.is_empty() {
            println!();
            println!("  {}", theme.heading("Muestra (dry-run)"));
            for record in samples {
                let category = record.primary_category.as_deref().unwrap_or("—");
                let location = record.address.municipio.as_deref().unwrap_or("—");
                let phone = record.phone_values().next().unwrap_or("—");
                println!(
                    "  {} {} {} {}",
                    theme.cyan("·"),
                    theme.bold(&record.name),
                    theme.muted(format!("{category} · {location} · {phone}")),
                    theme.muted(format!("[{}]", record.source))
                );
            }
        }
        println!();
        println!(
            "  {} {}",
            theme.muted("Siguiente:"),
            theme.cyan("nicascan search --departamento <depto> --has-phone")
        );
        println!();
    }

    Ok(exit_code(&summary))
}

fn exit_code(summary: &ScanSummary) -> i32 {
    if summary.sources.iter().all(|s| !s.errors.is_empty()) {
        2
    } else {
        0
    }
}

fn print_summary(theme: &Theme, summary: &ScanSummary, dry_run: bool) {
    println!();
    println!("  {}", theme.heading("Resumen"));
    let mut table = comfy_table::Table::new();
    table
        .load_preset(comfy_table::presets::UTF8_FULL)
        .set_content_arrangement(comfy_table::ContentArrangement::Dynamic)
        .set_header(vec![
            "Fuente",
            "Encontrados",
            "Nuevos",
            "Fusionados",
            "Errores",
            "Tiempo",
        ]);
    for source in &summary.sources {
        table.add_row(vec![
            source.source.clone(),
            source.fetched.to_string(),
            source.inserted.to_string(),
            source.merged.to_string(),
            source.errors.len().to_string(),
            format!("{:.1}s", source.duration_ms as f64 / 1000.0),
        ]);
    }
    println!("{table}");
    println!();
    if dry_run {
        println!(
            "  {} {} negocios encontrados (no se guardó nada)",
            theme.warning("dry-run:"),
            summary.total_fetched
        );
    } else {
        println!(
            "  {} {} encontrados · {} nuevos · {} fusionados · {} omitidos · {:.1}s",
            theme.success("✓"),
            summary.total_fetched,
            summary.total_inserted,
            summary.total_merged,
            summary.total_skipped,
            summary.duration_ms as f64 / 1000.0
        );
    }
    if !summary.sources.iter().all(|s| s.errors.is_empty()) {
        println!(
            "  {} Algunas fuentes fallaron; los resultados de las demás se conservaron.",
            theme.warning("!")
        );
    }
}

fn location_filter_label(location: &ResolvedLocation) -> String {
    match &location.filter {
        GeoFilter::Area { relation_id } => {
            format!("área administrativa OSM (relación {relation_id})")
        }
        GeoFilter::Country => "todo Nicaragua (área OSM)".to_string(),
        GeoFilter::OverpassArea { area_id } => format!("área Overpass {area_id}"),
        GeoFilter::Around { lat, lon, radius_m } => {
            format!("radio {radius_m:.0} m alrededor de {lat:.5}, {lon:.5}")
        }
        GeoFilter::Bbox {
            south,
            west,
            north,
            east,
        } => format!("bbox {south:.4},{west:.4} → {north:.4},{east:.4}"),
    }
}

fn progress_bar(enabled: bool, len: u64, message: &str) -> Option<ProgressBar> {
    if !enabled || !std::io::stderr().is_terminal() {
        return None;
    }
    let pb = ProgressBar::new(len);
    pb.set_style(
        ProgressStyle::with_template("{spinner:.cyan} {msg} [{bar:28.cyan/blue}] {pos}/{len}")
            .unwrap()
            .progress_chars("=>-"),
    );
    pb.set_message(message.to_string());
    pb.enable_steady_tick(Duration::from_millis(100));
    Some(pb)
}

fn spinner(enabled: bool, message: &str) -> Option<ProgressBar> {
    if !enabled || !std::io::stderr().is_terminal() {
        return None;
    }
    let pb = ProgressBar::new_spinner();
    pb.set_style(
        ProgressStyle::with_template("{spinner:.cyan} {msg}")
            .unwrap()
            .tick_strings(&["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"]),
    );
    pb.set_message(message.to_string());
    pb.enable_steady_tick(Duration::from_millis(90));
    Some(pb)
}
