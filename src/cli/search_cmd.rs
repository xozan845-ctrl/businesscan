//! `businesscan search`: consulta la base local.

use anyhow::Result;
use clap::Args;

use crate::cli::{FilterArgs, GlobalArgs};
use crate::config::Config;
use crate::db::business::search;
use crate::output::{self, Theme};

#[derive(Debug, Args)]
pub struct SearchArgs {
    #[command(flatten)]
    pub filters: FilterArgs,

    /// Número máximo de resultados (0 = todos, hasta 5000)
    #[arg(short = 'n', long, default_value_t = 25)]
    pub limit: usize,

    /// Desplazamiento para paginar
    #[arg(long, default_value_t = 0)]
    pub offset: usize,

    /// Muestra más columnas
    #[arg(long)]
    pub wide: bool,
}

pub fn run(args: SearchArgs, config: &Config, global: &GlobalArgs, theme: &Theme) -> Result<i32> {
    let db = crate::cli::open_db(config)?;
    let query = args.filters.to_query(args.limit, args.offset)?;
    let results = search(&db, &query)?;

    if global.json {
        println!("{}", serde_json::to_string_pretty(&results)?);
        return Ok(0);
    }

    if results.is_empty() {
        println!();
        println!(
            "  {} No se encontraron negocios con esos filtros.",
            theme.warning("·")
        );
        println!(
            "  {}",
            theme.muted(
                "Prueba con menos filtros, revisa `businesscan stats` o ejecuta `businesscan search --limit 10`."
            )
        );
        println!();
        return Ok(0);
    }

    if !global.quiet {
        println!();
        println!("  {}", theme.heading("Resultados"));
    }
    println!("{}", output::business_table(&results, theme, args.wide));
    if !global.quiet {
        println!();
        let start = args.offset + 1;
        let end = args.offset + results.len();
        println!(
            "  {} {}",
            theme.muted(format!("Mostrando {start}–{end}")),
            if results.len() == args.limit {
                theme.muted("· usa --offset para ver más o `businesscan export`")
            } else {
                String::new()
            }
        );
        println!();
    }
    Ok(0)
}
