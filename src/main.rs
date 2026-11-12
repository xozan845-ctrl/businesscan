use clap::Parser;
use nicascan::cli::{init_logging, run, Cli};

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    let json = cli.global.json;
    let quiet = cli.global.quiet;
    init_logging(cli.global.verbose, quiet);

    match run(cli).await {
        Ok(code) => std::process::exit(code),
        Err(error) => {
            if json {
                eprintln!("{}", serde_json::json!({ "error": format!("{error:#}") }));
            } else {
                eprintln!("\n  ✕ {error:#}\n");
            }
            std::process::exit(1);
        }
    }
}
