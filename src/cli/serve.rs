//! `nicascan serve`: API HTTP local para agentes de IA.

use std::net::{IpAddr, SocketAddr};

use anyhow::{bail, Result};
use clap::Args;

use crate::cli::GlobalArgs;
use crate::config::Config;
use crate::output::{self, Theme};
use crate::server;

#[derive(Debug, Args)]
pub struct ServeArgs {
    /// Host de escucha (por defecto solo local)
    #[arg(long, default_value = "127.0.0.1")]
    pub host: String,

    /// Puerto HTTP
    #[arg(short = 'p', long, default_value_t = 8787)]
    pub port: u16,

    /// Token Bearer opcional (o NICASCAN_API_TOKEN)
    #[arg(long, env = "NICASCAN_API_TOKEN")]
    pub token: Option<String>,
}

pub async fn run(
    args: ServeArgs,
    config: &Config,
    global: &GlobalArgs,
    theme: &Theme,
) -> Result<i32> {
    let ip: IpAddr = args
        .host
        .parse()
        .map_err(|_| anyhow::anyhow!("--host debe ser una IP (por ejemplo 127.0.0.1)"))?;
    let address = SocketAddr::new(ip, args.port);
    let token = args.token.filter(|value| !value.trim().is_empty());
    if !ip.is_loopback() && token.is_none() {
        bail!(
            "Escuchar en {ip} sin --token expone la base a la red. Usa --token o vuelve a 127.0.0.1"
        );
    }

    let db = crate::cli::open_db(config)?;
    let url = format!("http://{address}");

    if global.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "status": "listening",
                "url": url,
                "openapi": format!("{url}/openapi.json"),
                "auth": token.is_some(),
                "version": crate::version::VERSION,
            }))?
        );
    } else if !global.quiet {
        output::banner(theme);
        println!("  {}", theme.heading("API local lista"));
        output::kv_line(theme, "URL", &url);
        output::kv_line(theme, "OpenAPI", &format!("{url}/openapi.json"));
        output::kv_line(
            theme,
            "Autenticación",
            if token.is_some() {
                "Bearer token"
            } else {
                "sin token (solo loopback)"
            },
        );
        println!();
        println!(
            "  {}",
            theme.muted("Ctrl-C para detener. Ejemplo: curl \"$URL/v1/businesses?q=farmacia&has_phone=true\"")
        );
        println!();
    }

    server::serve(db, config.clone(), address, token).await?;
    Ok(0)
}
