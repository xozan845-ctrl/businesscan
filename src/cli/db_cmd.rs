//! `nicascan db`: mantenimiento de la base SQLite.

use std::fs;
use std::path::PathBuf;

use anyhow::{bail, Context, Result};
use clap::{Args, Subcommand};

use crate::cli::GlobalArgs;
use crate::config::Config;
use crate::db::Database;
use crate::output::{self, Theme};

#[derive(Debug, Args)]
pub struct DbArgs {
    #[command(subcommand)]
    pub command: DbCommand,
}

#[derive(Debug, Subcommand)]
pub enum DbCommand {
    /// Información del archivo, esquema y conteos
    Info,
    /// Imprime la ruta de la base
    Path,
    /// Consolida el WAL en el archivo principal
    Checkpoint,
    /// Compacta la base
    Vacuum,
    /// Actualiza estadísticas internas del planificador
    Optimize,
    /// Copia consistente a otro archivo
    Backup {
        /// Archivo destino
        destino: PathBuf,
        /// Sobrescribe si existe
        #[arg(long)]
        force: bool,
    },
    /// Borra la base y crea una vacía (destructivo)
    Reset {
        /// Confirma la eliminación
        #[arg(long)]
        yes: bool,
    },
}

pub fn run(args: DbArgs, config: &Config, global: &GlobalArgs, theme: &Theme) -> Result<i32> {
    match args.command {
        DbCommand::Info => {
            let db = Database::open(&config.db_path)?;
            let info = db.info()?;
            if global.json {
                println!("{}", serde_json::to_string_pretty(&info)?);
            } else {
                output::banner(theme);
                output::kv_line(theme, "Archivo", &info.path);
                output::kv_line(theme, "Tamaño", &human_bytes(info.size_bytes));
                output::kv_line(theme, "Esquema", &format!("v{}", info.schema_version));
                output::kv_line(theme, "Journal", &info.journal_mode);
                output::kv_line(theme, "FTS5", if info.fts5 { "sí" } else { "no" });
                output::kv_line(theme, "Negocios", &info.businesses.to_string());
                output::kv_line(theme, "Contactos", &info.contacts.to_string());
                output::kv_line(theme, "Observaciones", &info.sources.to_string());
                println!();
            }
        }
        DbCommand::Path => {
            if global.json {
                println!("{}", serde_json::to_string(&config.db_path)?);
            } else {
                println!("{}", config.db_path.display());
            }
        }
        DbCommand::Checkpoint => {
            let db = Database::open(&config.db_path)?;
            db.checkpoint()?;
            if !global.quiet {
                println!(
                    "{} WAL consolidado en la base principal.",
                    theme.success("✓")
                );
            }
        }
        DbCommand::Vacuum => {
            let db = Database::open(&config.db_path)?;
            db.vacuum()?;
            if !global.quiet {
                println!("{} Base compactada.", theme.success("✓"));
            }
        }
        DbCommand::Optimize => {
            let db = Database::open(&config.db_path)?;
            db.optimize()?;
            if !global.quiet {
                println!(
                    "{} Estadísticas del planificador actualizadas.",
                    theme.success("✓")
                );
            }
        }
        DbCommand::Backup { destino, force } => {
            let db = Database::open(&config.db_path)?;
            if destino.exists() && force {
                fs::remove_file(&destino)
                    .with_context(|| format!("No se pudo reemplazar {}", destino.display()))?;
            }
            db.backup_to(&destino)?;
            let size = fs::metadata(&destino).map(|m| m.len()).unwrap_or(0);
            if !global.quiet {
                println!(
                    "{} Respaldo creado en {} ({})",
                    theme.success("✓"),
                    destino.display(),
                    human_bytes(size)
                );
            }
        }
        DbCommand::Reset { yes } => {
            if !yes {
                bail!(
                    "Esto borra todos los datos. Ejecuta `nicascan db reset --yes` para confirmar."
                );
            }
            for suffix in ["", "-wal", "-shm"] {
                let path = PathBuf::from(format!("{}{suffix}", config.db_path.display()));
                if path.exists() {
                    fs::remove_file(&path)
                        .with_context(|| format!("No se pudo borrar {}", path.display()))?;
                }
            }
            let db = Database::open(&config.db_path)?;
            drop(db);
            if !global.quiet {
                println!(
                    "{} Base reiniciada en {}",
                    theme.success("✓"),
                    config.db_path.display()
                );
            }
        }
    }
    Ok(0)
}

fn human_bytes(bytes: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;
    const GB: f64 = MB * 1024.0;
    let value = bytes as f64;
    if value >= GB {
        format!("{:.2} GB", value / GB)
    } else if value >= MB {
        format!("{:.2} MB", value / MB)
    } else if value >= KB {
        format!("{:.1} KB", value / KB)
    } else {
        format!("{bytes} B")
    }
}
