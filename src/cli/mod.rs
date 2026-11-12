//! Interfaz de línea de comandos.

mod db_cmd;
mod enrich;
mod export_cmd;
mod import_cmd;
mod places;
mod review;
mod scan;
mod search_cmd;
mod serve;
mod show;
mod sources;
mod stats;

use std::path::PathBuf;

use anyhow::{bail, Context, Result};
use clap::{ArgAction, Args, Parser, Subcommand, ValueEnum};

use crate::config::Config;
use crate::db::Database;
use crate::gazetteer::Gazetteer;
use crate::location::LocationInput;
use crate::model::{SearchQuery, SortBy};
use crate::normalize::normalize_text;
use crate::output::Theme;

#[derive(Debug, Parser)]
#[command(
    name = "nicascan",
    version,
    about = "Directorio extensible de negocios de Nicaragua",
    long_about = "NicaScan busca negocios en múltiples fuentes (OpenStreetMap, Google Places, \
Foursquare, Overture Maps e importaciones CSV/JSON), los deduplica, enriquece desde sus sitios web \
y los guarda en una base SQLite portable. Incluye API HTTP local para agentes de IA.\n\n\
Funciona en Termux (Android), Linux, macOS y Windows.",
    propagate_version = true,
    arg_required_else_help = true
)]
pub struct Cli {
    #[command(flatten)]
    pub global: GlobalArgs,
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Clone, Args)]
pub struct GlobalArgs {
    /// Ruta de la base SQLite (por defecto ~/.local/share/nicascan/nicascan.db)
    #[arg(long, global = true, env = "NICASCAN_DB", value_name = "RUTA")]
    pub db: Option<PathBuf>,

    /// Desactiva colores ANSI
    #[arg(long, global = true)]
    pub no_color: bool,

    /// Salida JSON para máquinas
    #[arg(long, global = true)]
    pub json: bool,

    /// Más detalle en stderr (-v info, -vv debug)
    #[arg(short, long, global = true, action = ArgAction::Count)]
    pub verbose: u8,

    /// Sin mensajes de progreso ni adornos
    #[arg(short, long, global = true)]
    pub quiet: bool,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Crea la base y muestra su ubicación
    Init,
    /// Busca negocios en fuentes externas y los guarda
    Scan(scan::ScanArgs),
    /// Busca en los negocios ya guardados
    #[command(visible_aliases = ["buscar", "list", "ls"])]
    Search(search_cmd::SearchArgs),
    /// Muestra todos los datos de un negocio
    Show(show::ShowArgs),
    /// Exporta a JSON, JSONL, CSV o SQLite
    #[command(visible_alias = "exportar")]
    Export(export_cmd::ExportArgs),
    /// Importa un CSV/JSON/JSONL externo
    #[command(visible_alias = "importar")]
    Import(import_cmd::ImportArgs),
    /// Completa fichas desde el sitio web de cada negocio
    Enrich(enrich::EnrichArgs),
    /// Lista las fuentes y su estado
    Sources,
    /// Estadísticas y cobertura de la base
    Stats,
    /// Gestiona ubicaciones geocodificadas
    Places(places::PlacesArgs),
    /// Levanta una API HTTP local para agentes de IA
    Serve(serve::ServeArgs),
    /// Revisa posibles duplicados que no se fusionaron
    Duplicates(review::DuplicatesArgs),
    /// Fusiona dos negocios manualmente
    Merge(review::MergeArgs),
    /// Utilidades de la base de datos
    Db(db_cmd::DbArgs),
}

#[derive(Debug, Clone, Copy, Default, ValueEnum)]
pub enum SortArg {
    #[default]
    Relevance,
    Name,
    Confidence,
    Quality,
    Updated,
    Distance,
}

impl From<SortArg> for SortBy {
    fn from(value: SortArg) -> Self {
        match value {
            SortArg::Relevance => SortBy::Relevance,
            SortArg::Name => SortBy::Name,
            SortArg::Confidence => SortBy::Confidence,
            SortArg::Quality => SortBy::Quality,
            SortArg::Updated => SortBy::Updated,
            SortArg::Distance => SortBy::Distance,
        }
    }
}

/// Argumentos de ubicación compartidos por scan, import y places.
#[derive(Debug, Clone, Args, Default)]
pub struct LocationArgs {
    /// Lugar libre, por ejemplo "Barrio Martha Quezada, Managua"
    #[arg(short = 'p', long)]
    pub place: Option<String>,

    /// Departamento o región autónoma
    #[arg(short = 'd', long)]
    pub departamento: Option<String>,

    /// Municipio
    #[arg(short = 'm', long)]
    pub municipio: Option<String>,

    /// Barrio o colonia
    #[arg(long)]
    pub barrio: Option<String>,

    /// Comarca
    #[arg(long)]
    pub comarca: Option<String>,

    /// Latitud del centro
    #[arg(long, allow_hyphen_values = true)]
    pub lat: Option<f64>,

    /// Longitud del centro
    #[arg(long, allow_hyphen_values = true)]
    pub lon: Option<f64>,

    /// Radio en metros cuando se usan --lat/--lon
    #[arg(long, default_value_t = 1_000.0)]
    pub radius: f64,

    /// Caja geográfica: minLat,minLon,maxLat,maxLon
    #[arg(long)]
    pub bbox: Option<String>,

    /// Todo Nicaragua (puede ser una consulta grande)
    #[arg(long = "all")]
    pub all_nicaragua: bool,

    /// ID de área de Overpass (avanzado)
    #[arg(long)]
    pub osm_area_id: Option<i64>,

    /// Ignora la caché de geocodificación
    #[arg(long)]
    pub refresh_place: bool,
}

impl LocationArgs {
    pub fn to_input(&self) -> LocationInput {
        LocationInput {
            place: self.place.clone(),
            departamento: self.departamento.clone(),
            municipio: self.municipio.clone(),
            barrio: self.barrio.clone(),
            comarca: self.comarca.clone(),
            lat: self.lat,
            lon: self.lon,
            radius_m: if self.lat.is_some() {
                Some(self.radius)
            } else {
                None
            },
            bbox: self.bbox.clone(),
            all_nicaragua: self.all_nicaragua,
            osm_area_id: self.osm_area_id,
        }
    }
}

/// Filtros de búsqueda compartidos por search y export.
#[derive(Debug, Clone, Args, Default)]
pub struct FilterArgs {
    /// Texto libre (nombre, categoría, dirección, etiquetas)
    #[arg(value_name = "TEXTO")]
    pub text: Vec<String>,

    /// Departamento (nombre oficial o alias)
    #[arg(short = 'd', long)]
    pub departamento: Option<String>,

    /// Municipio
    #[arg(short = 'm', long)]
    pub municipio: Option<String>,

    /// Barrio o comarca
    #[arg(long)]
    pub barrio: Option<String>,

    /// Categoría o tipo de negocio
    #[arg(short = 'c', long)]
    pub category: Option<String>,

    /// Fuente: osm, google, foursquare, import…
    #[arg(short = 's', long)]
    pub source: Option<String>,

    /// Estado: active, closed, closed_temporarily…
    #[arg(long)]
    pub status: Option<String>,

    /// Solo negocios con teléfono
    #[arg(long)]
    pub has_phone: bool,

    /// Solo negocios con correo
    #[arg(long)]
    pub has_email: bool,

    /// Solo negocios con sitio web
    #[arg(long)]
    pub has_website: bool,

    /// Confianza mínima, 0..1
    #[arg(long)]
    pub min_confidence: Option<f64>,

    /// Calidad mínima de la ficha, 0..1
    #[arg(long)]
    pub min_quality: Option<f64>,

    /// Centro geográfico (latitud)
    #[arg(long, allow_hyphen_values = true)]
    pub lat: Option<f64>,

    /// Centro geográfico (longitud)
    #[arg(long, allow_hyphen_values = true)]
    pub lon: Option<f64>,

    /// Radio en metros para el filtro geográfico
    #[arg(long, default_value_t = 1_000.0)]
    pub radius: f64,

    /// Orden de los resultados
    #[arg(long, value_enum, default_value_t = SortArg::Relevance)]
    pub sort: SortArg,
}

impl FilterArgs {
    pub fn is_filtered(&self) -> bool {
        !self.text.is_empty()
            || self.departamento.is_some()
            || self.municipio.is_some()
            || self.barrio.is_some()
            || self.category.is_some()
            || self.source.is_some()
            || self.status.is_some()
            || self.has_phone
            || self.has_email
            || self.has_website
            || self.min_confidence.is_some()
            || self.min_quality.is_some()
            || self.lat.is_some()
            || self.lon.is_some()
    }

    pub fn to_query(&self, limit: usize, offset: usize) -> Result<SearchQuery> {
        if self.lat.is_some() != self.lon.is_some() {
            bail!("--lat y --lon deben usarse juntos");
        }
        if let Some(value) = self.min_confidence {
            if !(0.0..=1.0).contains(&value) {
                bail!("--min-confidence debe estar entre 0 y 1");
            }
        }
        if let Some(value) = self.min_quality {
            if !(0.0..=1.0).contains(&value) {
                bail!("--min-quality debe estar entre 0 y 1");
            }
        }
        let gaz = Gazetteer::global();
        let departamento = self.departamento.as_deref().map(|value| {
            gaz.canonical_department(value)
                .map(ToString::to_string)
                .unwrap_or_else(|| value.to_string())
        });
        let municipio = self.municipio.as_deref().map(|value| {
            gaz.canonical_municipality(value, departamento.as_deref())
                .map(|(_, municipality)| municipality.to_string())
                .unwrap_or_else(|| value.to_string())
        });
        Ok(SearchQuery {
            text: {
                let text = self.text.join(" ").trim().to_string();
                if text.is_empty() {
                    None
                } else {
                    Some(text)
                }
            },
            departamento,
            municipio,
            barrio: self.barrio.clone(),
            category: self.category.clone(),
            source: self.source.clone(),
            status: self.status.clone(),
            has_phone: self.has_phone,
            has_email: self.has_email,
            has_website: self.has_website,
            min_confidence: self.min_confidence,
            min_quality: self.min_quality,
            updated_since: None,
            lat: self.lat,
            lon: self.lon,
            radius_m: if self.lat.is_some() {
                Some(self.radius)
            } else {
                None
            },
            sort: self.sort.into(),
            limit,
            offset,
            include_raw: false,
        })
    }
}

pub fn init_logging(verbose: u8, quiet: bool) {
    let default = if quiet {
        "warn"
    } else {
        match verbose {
            0 => "warn",
            1 => "info",
            _ => "debug",
        }
    };
    let _ = env_logger::Builder::from_env(env_logger::Env::default().default_filter_or(default))
        .format_timestamp(None)
        .target(env_logger::Target::Stderr)
        .try_init();
}

pub async fn run(cli: Cli) -> Result<i32> {
    let config = Config::load(cli.global.db.clone())?;
    let theme = Theme::new(cli.global.no_color);

    match cli.command {
        Command::Init => init_command(&config, &cli.global, &theme),
        Command::Scan(args) => scan::run(args, &config, &cli.global, &theme).await,
        Command::Search(args) => search_cmd::run(args, &config, &cli.global, &theme),
        Command::Show(args) => show::run(args, &config, &cli.global, &theme),
        Command::Export(args) => export_cmd::run(args, &config, &cli.global, &theme),
        Command::Import(args) => import_cmd::run(args, &config, &cli.global, &theme),
        Command::Enrich(args) => enrich::run(args, &config, &cli.global, &theme).await,
        Command::Sources => sources::run(&config, &cli.global, &theme),
        Command::Stats => stats::run(&config, &cli.global, &theme),
        Command::Places(args) => places::run(args, &config, &cli.global, &theme).await,
        Command::Serve(args) => serve::run(args, &config, &cli.global, &theme).await,
        Command::Duplicates(args) => review::duplicates(args, &config, &cli.global, &theme),
        Command::Merge(args) => review::merge(args, &config, &cli.global, &theme),
        Command::Db(args) => db_cmd::run(args, &config, &cli.global, &theme),
    }
}

fn init_command(config: &Config, global: &GlobalArgs, theme: &Theme) -> Result<i32> {
    let db = Database::open(&config.db_path)?;
    db.checkpoint()?;
    let info = db.info()?;
    if global.json {
        println!("{}", serde_json::to_string_pretty(&info)?);
    } else {
        crate::output::banner(theme);
        println!("  {}", theme.heading("Base de datos lista"));
        crate::output::kv_line(theme, "Archivo", &info.path);
        crate::output::kv_line(theme, "Esquema", &format!("v{}", info.schema_version));
        crate::output::kv_line(theme, "FTS5", if info.fts5 { "sí" } else { "no" });
        crate::output::kv_line(theme, "Negocios", &format!("{}", info.businesses));
        println!();
        println!(
            "  {}",
            theme.muted("Siguiente: nicascan scan --place \"León\" --source osm")
        );
        println!();
    }
    Ok(0)
}

pub fn open_db(config: &Config) -> Result<Database> {
    Database::open(&config.db_path)
        .with_context(|| format!("No se pudo abrir la base {}", config.db_path.display()))
}

pub fn canonical_admin(value: Option<&str>) -> Option<String> {
    let value = value?;
    Some(
        Gazetteer::global()
            .canonical_department(value)
            .map(ToString::to_string)
            .unwrap_or_else(|| value.trim().to_string()),
    )
}

pub fn normalize_place_filter(value: &str) -> String {
    normalize_text(value)
}
