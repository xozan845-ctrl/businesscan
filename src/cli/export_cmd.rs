//! `businesscan export`: salida para análisis o para una IA.

use anyhow::{bail, Result};
use clap::Args;

use crate::cli::{FilterArgs, GlobalArgs};
use crate::config::Config;
use crate::export::{export, ExportFormat};
use crate::output::Theme;

#[derive(Debug, Args)]
pub struct ExportArgs {
    #[command(flatten)]
    pub filters: FilterArgs,

    /// Formato: jsonl (recomendado para IA), json, csv o sqlite
    #[arg(short = 'f', long, default_value = "jsonl")]
    pub format: String,

    /// Archivo de salida; "-" para stdout
    #[arg(short = 'o', long, default_value = "-")]
    pub output: String,

    /// Incluye el JSON original de cada fuente
    #[arg(long)]
    pub include_raw: bool,

    /// JSON con indentación (solo --format json)
    #[arg(long)]
    pub pretty: bool,

    /// Sobrescribe el destino SQLite
    #[arg(long)]
    pub force: bool,

    /// Máximo de negocios (0 = todos)
    #[arg(short = 'n', long, default_value_t = 0)]
    pub limit: usize,
}

pub fn run(args: ExportArgs, config: &Config, global: &GlobalArgs, _theme: &Theme) -> Result<i32> {
    let format = ExportFormat::parse(&args.format)?;
    if format == ExportFormat::Sqlite && args.filters.is_filtered() {
        bail!("--format sqlite copia la base completa; los filtros no aplican");
    }

    let db = crate::cli::open_db(config)?;
    let query = args.filters.to_query(args.limit, 0)?;
    let count = export(
        &db,
        &query,
        format,
        &args.output,
        args.include_raw,
        args.pretty,
        args.force,
    )?;

    if !global.quiet {
        if format == ExportFormat::Sqlite {
            eprintln!("✓ Base exportada a {} (SQLite portable)", args.output);
        } else if args.output == "-" {
            eprintln!("✓ {count} negocios exportados a stdout");
        } else {
            eprintln!("✓ {count} negocios exportados a {}", args.output);
        }
    }
    Ok(0)
}
