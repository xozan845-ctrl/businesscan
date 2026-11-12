//! `businesscan import`: datos de terceros en CSV, JSON o JSONL.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};
use clap::Args;
use indicatif::{ProgressBar, ProgressStyle};
use serde::Serialize;

use crate::cli::GlobalArgs;
use crate::config::Config;
use crate::db::business::import_batch;
use crate::gazetteer::Gazetteer;
use crate::location::{GeoFilter, ResolvedLocation};
use crate::model::UpsertAction;
use crate::output::{self, Theme};
use crate::providers::import::{import_file, ImportFormat, ImportOptions};

#[derive(Debug, Args)]
pub struct ImportArgs {
    /// Archivo CSV, JSON o JSONL
    pub archivo: PathBuf,

    /// Formato (por defecto se detecta por la extensión)
    #[arg(short = 'f', long)]
    pub format: Option<String>,

    /// Etiqueta de la fuente que se guardará en la procedencia
    #[arg(short = 's', long, default_value = "import")]
    pub source: String,

    /// Delimitador CSV (por defecto se detecta: coma, punto y coma o tab)
    #[arg(long, value_parser = parse_delimiter)]
    pub delimiter: Option<u8>,

    /// País por defecto si el archivo no lo trae (ISO o nombre)
    #[arg(short = 'C', long)]
    pub country: Option<String>,

    /// Departamento por defecto si el archivo no lo trae
    #[arg(short = 'd', long)]
    pub departamento: Option<String>,

    /// Ciudad/municipio por defecto si el archivo no lo trae
    #[arg(short = 'm', long)]
    pub municipio: Option<String>,

    /// Barrio por defecto si el archivo no lo trae
    #[arg(long)]
    pub barrio: Option<String>,

    /// Comarca por defecto si el archivo no lo trae
    #[arg(long)]
    pub comarca: Option<String>,

    /// Latitud por defecto para registros sin coordenadas
    #[arg(long, allow_hyphen_values = true)]
    pub lat: Option<f64>,

    /// Longitud por defecto para registros sin coordenadas
    #[arg(long, allow_hyphen_values = true)]
    pub lon: Option<f64>,

    /// Máximo de registros a importar
    #[arg(short = 'n', long)]
    pub limit: Option<usize>,

    /// Analiza el archivo sin guardar
    #[arg(long)]
    pub dry_run: bool,
}

#[derive(Debug, Serialize)]
struct ImportSummary {
    file: String,
    format: String,
    source: String,
    found: usize,
    inserted: usize,
    merged: usize,
    skipped: usize,
    duration_ms: u64,
    dry_run: bool,
}

pub fn run(args: ImportArgs, config: &Config, global: &GlobalArgs, theme: &Theme) -> Result<i32> {
    if args.lat.is_some() != args.lon.is_some() {
        bail!("--lat y --lon deben usarse juntos");
    }
    if args.source.trim().is_empty() {
        bail!("--source no puede estar vacío");
    }

    let format = match args.format.as_deref() {
        Some(value) => ImportFormat::parse(value)?,
        None => ImportFormat::detect(&args.archivo),
    };
    let hints = build_hints(
        args.country.as_deref(),
        args.departamento.as_deref(),
        args.municipio.as_deref(),
        args.barrio.as_deref(),
        args.comarca.as_deref(),
        args.lat,
        args.lon,
    )?;

    if !global.json && !global.quiet {
        output::banner(theme);
        println!(
            "  {} {}",
            theme.muted("Importando:"),
            args.archivo.display()
        );
    }

    let started = Instant::now();
    let options = ImportOptions {
        source: &args.source,
        delimiter: args.delimiter,
        location: &hints,
    };
    let mut records = import_file(&args.archivo, format, &options)
        .with_context(|| format!("No se pudo importar {}", args.archivo.display()))?;
    if let Some(limit) = args.limit {
        records.truncate(limit);
    }
    for record in &mut records {
        if record.latitude.is_none() {
            record.latitude = args.lat;
        }
        if record.longitude.is_none() {
            record.longitude = args.lon;
        }
    }

    let mut summary = ImportSummary {
        file: args.archivo.display().to_string(),
        format: format.as_str().to_string(),
        source: args.source.clone(),
        found: records.len(),
        inserted: 0,
        merged: 0,
        skipped: 0,
        duration_ms: 0,
        dry_run: args.dry_run,
    };

    if args.dry_run {
        if !global.json {
            println!();
            println!(
                "  {} {} registros listos para importar (dry-run)",
                theme.warning("dry-run:"),
                records.len()
            );
            for record in records.iter().take(10) {
                println!(
                    "  {} {} {}",
                    theme.cyan("·"),
                    theme.bold(&record.name),
                    theme.muted(
                        record
                            .primary_category
                            .as_deref()
                            .unwrap_or("sin categoría")
                    )
                );
            }
            println!();
        }
    } else if !records.is_empty() {
        let mut db = crate::cli::open_db(config)?;
        let pb = progress_bar(!global.json && !global.quiet, records.len() as u64);
        for chunk in records.chunks(200) {
            let results = import_batch(&mut db, chunk)?;
            for result in results {
                match result.action {
                    UpsertAction::Inserted => summary.inserted += 1,
                    UpsertAction::Merged => summary.merged += 1,
                    UpsertAction::Skipped => summary.skipped += 1,
                }
            }
            if let Some(pb) = &pb {
                pb.inc(chunk.len() as u64);
            }
        }
        if let Some(pb) = pb {
            pb.finish_and_clear();
        }
    }

    summary.duration_ms = started.elapsed().as_millis() as u64;

    if global.json {
        println!("{}", serde_json::to_string_pretty(&summary)?);
    } else if !global.quiet && !args.dry_run {
        println!(
            "  {} {} encontrados · {} nuevos · {} fusionados · {} omitidos · {:.1}s",
            theme.success("✓"),
            summary.found,
            summary.inserted,
            summary.merged,
            summary.skipped,
            summary.duration_ms as f64 / 1000.0
        );
        if summary.inserted + summary.merged > 0 {
            println!(
                "  {}",
                theme.muted(format!(
                    "Consulta con `businesscan search --source {}` o exporta con `businesscan export`.",
                    args.source
                ))
            );
        }
        println!();
    }
    Ok(0)
}

fn build_hints(
    country: Option<&str>,
    departamento: Option<&str>,
    municipio: Option<&str>,
    barrio: Option<&str>,
    comarca: Option<&str>,
    lat: Option<f64>,
    lon: Option<f64>,
) -> Result<ResolvedLocation> {
    let gaz = Gazetteer::global();
    let country = match country {
        Some(value) => Some(gaz.country(value).ok_or_else(|| {
            let suggestion = gaz
                .suggest_country(value)
                .map(|country| format!(" ¿Quisiste decir «{}»?", country.name_es))
                .unwrap_or_default();
            anyhow::anyhow!("País «{value}» no reconocido.{suggestion}")
        })?),
        None => None,
    };

    let mut parts = Vec::new();
    for value in [
        barrio,
        comarca,
        municipio,
        departamento,
        country.map(|country| country.name_es.as_str()),
    ]
    .into_iter()
    .flatten()
    {
        if !parts
            .iter()
            .any(|part: &String| part.eq_ignore_ascii_case(value))
        {
            parts.push(value.to_string());
        }
    }
    let display_name = if parts.is_empty() {
        "Sin ubicación (solo datos del archivo)".to_string()
    } else {
        parts.join(", ")
    };

    let filter = if let (Some(lat), Some(lon)) = (lat, lon) {
        GeoFilter::Around {
            lat,
            lon,
            radius_m: 1_000.0,
        }
    } else if let Some(country) = country {
        let bbox = country.bbox_or_point();
        GeoFilter::Bbox {
            south: bbox[1],
            west: bbox[0],
            north: bbox[3],
            east: bbox[2],
        }
    } else {
        GeoFilter::Bbox {
            south: -90.0,
            west: -180.0,
            north: 90.0,
            east: 180.0,
        }
    };

    Ok(ResolvedLocation {
        query: display_name.clone(),
        display_name,
        country_code: country.map(|country| country.iso2.clone()),
        country_name: country.map(|country| country.name_es.clone()),
        continent: country.and_then(|country| country.continent.clone()),
        departamento: departamento.map(ToString::to_string),
        municipio: municipio.map(ToString::to_string),
        barrio: barrio.map(ToString::to_string),
        comarca: comarca.map(ToString::to_string),
        filter,
        center: lat
            .zip(lon)
            .map(|(lat, lon)| crate::location::GeoPoint { lat, lon }),
        bbox: None,
        radius_m: None,
        osm_type: None,
        osm_id: None,
        source: "import".to_string(),
    })
}

fn parse_delimiter(value: &str) -> std::result::Result<u8, String> {
    match value {
        "\\t" | "tab" | "t" => Ok(b'\t'),
        ";" | "," | "|" => Ok(value.as_bytes()[0]),
        other if other.len() == 1 => Ok(other.as_bytes()[0]),
        _ => Err("usa un solo carácter: , ; | o tab".to_string()),
    }
}

fn progress_bar(enabled: bool, len: u64) -> Option<ProgressBar> {
    if !enabled || !std::io::IsTerminal::is_terminal(&std::io::stderr()) {
        return None;
    }
    let pb = ProgressBar::new(len);
    pb.set_style(
        ProgressStyle::with_template("{spinner:.cyan} importando [{bar:28.cyan/blue}] {pos}/{len}")
            .unwrap()
            .progress_chars("=>-"),
    );
    pb.enable_steady_tick(Duration::from_millis(100));
    Some(pb)
}
