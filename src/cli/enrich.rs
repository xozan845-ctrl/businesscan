//! `businesscan enrich`: completa fichas desde el sitio web del negocio.

use std::collections::HashSet;
use std::time::{Duration, Instant};

use anyhow::{bail, Result};
use chrono::{Duration as ChronoDuration, Utc};
use clap::Args;
use serde::Serialize;

use crate::cli::{FilterArgs, GlobalArgs};
use crate::config::Config;
use crate::db::business::{add_observation, search, web_last_seen};
use crate::enrich::WebEnricher;
use crate::http::HttpClient;
use crate::normalize::domain_of;
use crate::output::{self, Theme};

#[derive(Debug, Args)]
pub struct EnrichArgs {
    #[command(flatten)]
    pub filters: FilterArgs,

    /// Máximo de sitios a revisar
    #[arg(short = 'n', long, default_value_t = 50)]
    pub limit: usize,

    /// Páginas por sitio: portada + contacto/nosotros (1–5)
    #[arg(long, default_value_t = 2)]
    pub max_pages: usize,

    /// Solo negocios a los que les falta teléfono o correo
    #[arg(long)]
    pub missing_only: bool,

    /// Vuelve a visitar sitios ya enriquecidos recientemente
    #[arg(long)]
    pub refresh: bool,

    /// No repetir sitios enriquecidos en los últimos N días
    #[arg(long, default_value_t = 30)]
    pub since_days: i64,

    /// Pausa por host en milisegundos (por defecto 2000)
    #[arg(long)]
    pub delay_ms: Option<u64>,

    /// Analiza y muestra lo encontrado sin guardar
    #[arg(long)]
    pub dry_run: bool,
}

#[derive(Debug, Serialize)]
struct EnrichItem {
    id: String,
    name: String,
    url: String,
    status: String,
    pages: usize,
    contacts: usize,
    had_jsonld: bool,
    error: Option<String>,
}

#[derive(Debug, Serialize, Default)]
struct EnrichSummary {
    candidates: usize,
    checked: usize,
    updated: usize,
    skipped: usize,
    errors: usize,
    contacts_detected: usize,
    duration_ms: u64,
    dry_run: bool,
    results: Vec<EnrichItem>,
}

pub async fn run(
    args: EnrichArgs,
    config: &Config,
    global: &GlobalArgs,
    theme: &Theme,
) -> Result<i32> {
    if args.max_pages == 0 || args.max_pages > 5 {
        bail!("--max-pages debe estar entre 1 y 5");
    }
    if args.limit == 0 {
        bail!("--limit debe ser mayor que cero");
    }
    if config.offline {
        bail!("Modo offline: el enriquecimiento web necesita red");
    }

    let mut db = crate::cli::open_db(config)?;
    let mut query = args.filters.to_query(args.limit, 0)?;
    query.has_website = true;
    query.limit = args.limit;
    let candidates = search(&db, &query)?;

    if !global.json && !global.quiet {
        output::banner(theme);
        println!(
            "  {} {}",
            theme.muted("Sitios web candidatos:"),
            candidates.len()
        );
    }

    let mut summary = EnrichSummary {
        candidates: candidates.len(),
        dry_run: args.dry_run,
        ..Default::default()
    };
    let started = Instant::now();
    let delay = Duration::from_millis(args.delay_ms.unwrap_or(config.enrich_delay_ms).min(30_000));
    let http = HttpClient::new(config)?;
    let mut enricher = WebEnricher::new(http, delay);
    let cutoff = Utc::now() - ChronoDuration::days(args.since_days.max(0));
    let cutoff = cutoff.to_rfc3339();
    let mut seen_domains: HashSet<String> = HashSet::new();

    for business in candidates {
        let Some(website) = business.website.clone() else {
            summary.skipped += 1;
            continue;
        };
        let Some(domain) = domain_of(&website) else {
            summary.skipped += 1;
            continue;
        };
        if !seen_domains.insert(domain) {
            summary.skipped += 1;
            continue;
        }
        if args.missing_only && business.phone_primary.is_some() && business.email_primary.is_some()
        {
            summary.skipped += 1;
            continue;
        }
        if !args.refresh {
            if let Some(last) = web_last_seen(&db.conn, &business.id)? {
                if last >= cutoff {
                    summary.skipped += 1;
                    continue;
                }
            }
        }

        summary.checked += 1;
        let result = enricher.enrich(&business, &website, args.max_pages).await;
        match result {
            Ok((record, facts)) => {
                summary.contacts_detected += facts.contacts;
                let status = if args.dry_run {
                    "encontrado"
                } else {
                    add_observation(&mut db, &business.id, &record)?;
                    summary.updated += 1;
                    "actualizado"
                };
                let item = EnrichItem {
                    id: business.id.clone(),
                    name: business.name.clone(),
                    url: facts.url.clone(),
                    status: status.to_string(),
                    pages: facts.pages,
                    contacts: facts.contacts,
                    had_jsonld: facts.had_jsonld,
                    error: None,
                };
                if !global.json && !global.quiet {
                    println!(
                        "  {} {} {}",
                        theme.success("✓"),
                        theme.bold(&business.name),
                        theme.muted(format!(
                            "{} · {} pág. · {} contactos{}",
                            facts.url,
                            facts.pages,
                            facts.contacts,
                            if facts.had_jsonld { " · JSON-LD" } else { "" }
                        ))
                    );
                }
                summary.results.push(item);
            }
            Err(error) => {
                summary.errors += 1;
                let message = format!("{error:#}");
                if !global.json {
                    eprintln!(
                        "  {} {}: {}",
                        theme.error("✕"),
                        theme.bold(&business.name),
                        theme.red(&message)
                    );
                }
                summary.results.push(EnrichItem {
                    id: business.id,
                    name: business.name,
                    url: website,
                    status: "error".to_string(),
                    pages: 0,
                    contacts: 0,
                    had_jsonld: false,
                    error: Some(message),
                });
            }
        }
    }

    summary.duration_ms = started.elapsed().as_millis() as u64;

    if global.json {
        println!("{}", serde_json::to_string_pretty(&summary)?);
    } else if !global.quiet {
        println!();
        println!(
            "  {} {} revisados · {} actualizados · {} omitidos · {} contactos · {} errores · {:.1}s",
            theme.success("✓"),
            summary.checked,
            summary.updated,
            summary.skipped,
            summary.contacts_detected,
            summary.errors,
            summary.duration_ms as f64 / 1000.0
        );
        if args.dry_run {
            println!("  {}", theme.warning("dry-run: no se guardó ningún cambio"));
        }
        println!();
    }

    Ok(if summary.errors > 0 && summary.updated == 0 {
        2
    } else {
        0
    })
}
