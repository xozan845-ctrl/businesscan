//! Importación de datasets CSV, JSON y JSONL (MIFIC, cámaras, INE, etc.).
//!
//! No asume un formato rígido: reconoce encabezados en español e inglés,
//! mantiene las columnas desconocidas como etiquetas y guarda la fila original.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use anyhow::{bail, Context, Result};
use chrono::Utc;
use serde_json::{json, Map, Value};

use crate::location::ResolvedLocation;
use crate::model::{Address, BusinessRecord, Contact, ContactKind};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImportFormat {
    Csv,
    Json,
    Jsonl,
}

impl ImportFormat {
    pub fn parse(value: &str) -> Result<Self> {
        match value.to_lowercase().as_str() {
            "csv" => Ok(Self::Csv),
            "json" => Ok(Self::Json),
            "jsonl" | "ndjson" => Ok(Self::Jsonl),
            other => bail!("Formato de importación desconocido: {other} (usa csv, json o jsonl)"),
        }
    }

    pub fn detect(path: &Path) -> Self {
        match path
            .extension()
            .and_then(|ext| ext.to_str())
            .unwrap_or("")
            .to_lowercase()
            .as_str()
        {
            "json" => Self::Json,
            "jsonl" | "ndjson" => Self::Jsonl,
            _ => Self::Csv,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Csv => "csv",
            Self::Json => "json",
            Self::Jsonl => "jsonl",
        }
    }
}

pub struct ImportOptions<'a> {
    pub source: &'a str,
    pub delimiter: Option<u8>,
    pub location: &'a ResolvedLocation,
}

pub fn import_file(
    path: impl AsRef<Path>,
    format: ImportFormat,
    options: &ImportOptions<'_>,
) -> Result<Vec<BusinessRecord>> {
    let path = path.as_ref();
    let bytes =
        fs::read(path).with_context(|| format!("No se pudo leer el archivo {}", path.display()))?;
    let text = decode_text(&bytes);
    match format {
        ImportFormat::Csv => import_csv(&text, options),
        ImportFormat::Json => import_json(&text, options),
        ImportFormat::Jsonl => import_jsonl(&text, options),
    }
}

fn decode_text(bytes: &[u8]) -> String {
    let bytes = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes);
    match std::str::from_utf8(bytes) {
        Ok(text) => text.to_string(),
        Err(_) => {
            let (decoded, _, _) = encoding_rs::WINDOWS_1252.decode(bytes);
            decoded.to_string()
        }
    }
}

fn import_csv(text: &str, options: &ImportOptions<'_>) -> Result<Vec<BusinessRecord>> {
    let delimiter = options.delimiter.unwrap_or_else(|| detect_delimiter(text));
    let mut reader = csv::ReaderBuilder::new()
        .delimiter(delimiter)
        .flexible(true)
        .trim(csv::Trim::All)
        .from_reader(text.as_bytes());
    let headers = reader
        .headers()
        .context("El CSV no tiene encabezados")?
        .clone();
    let normalized_headers: Vec<String> = headers.iter().map(normalize_header).collect();

    let mut records = Vec::new();
    for (index, row) in reader.records().enumerate() {
        let row = row.with_context(|| format!("Error leyendo fila CSV {}", index + 2))?;
        let mut map = BTreeMap::new();
        let mut raw = Map::new();
        for (column, value) in normalized_headers.iter().zip(row.iter()) {
            let value = value.trim();
            if value.is_empty() || column.is_empty() {
                continue;
            }
            map.insert(column.clone(), value.to_string());
            raw.insert(column.clone(), Value::String(value.to_string()));
        }
        if map.is_empty() {
            continue;
        }
        if let Some(record) = map_to_record(&map, Value::Object(raw), options) {
            records.push(record);
        }
    }
    Ok(records)
}

fn import_json(text: &str, options: &ImportOptions<'_>) -> Result<Vec<BusinessRecord>> {
    let value: Value = serde_json::from_str(text).context("JSON inválido")?;
    let items: Vec<Value> = match value {
        Value::Array(items) => items,
        Value::Object(object) => {
            let nested = ["businesses", "data", "results", "items", "negocios"]
                .iter()
                .find_map(|key| object.get(*key).and_then(Value::as_array).cloned());
            match nested {
                Some(items) => items,
                None => vec![Value::Object(object)],
            }
        }
        _ => bail!("El JSON debe ser un objeto o un arreglo de objetos"),
    };

    Ok(items
        .into_iter()
        .filter_map(|item| {
            let map = json_to_map(&item);
            map_to_record(&map, item, options)
        })
        .collect())
}

fn import_jsonl(text: &str, options: &ImportOptions<'_>) -> Result<Vec<BusinessRecord>> {
    let mut records = Vec::new();
    for (index, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let value: Value = serde_json::from_str(line)
            .with_context(|| format!("JSONL inválido en la línea {}", index + 1))?;
        let map = json_to_map(&value);
        if let Some(record) = map_to_record(&map, value, options) {
            records.push(record);
        }
    }
    Ok(records)
}

fn detect_delimiter(text: &str) -> u8 {
    let sample = text.lines().take(5).collect::<Vec<_>>().join("\n");
    let commas = sample.matches(',').count();
    let semicolons = sample.matches(';').count();
    let tabs = sample.matches('\t').count();
    if tabs > commas && tabs > semicolons {
        b'\t'
    } else if semicolons > commas {
        b';'
    } else {
        b','
    }
}

fn normalize_header(header: &str) -> String {
    header
        .trim()
        .to_lowercase()
        .replace(['á', 'à', 'ä'], "a")
        .replace(['é', 'è', 'ë'], "e")
        .replace(['í', 'ì', 'ï'], "i")
        .replace(['ó', 'ò', 'ö'], "o")
        .replace(['ú', 'ù', 'ü'], "u")
        .replace(['ñ'], "n")
        .replace(['.', '-', '/'], " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join("_")
}

fn json_to_map(value: &Value) -> BTreeMap<String, String> {
    let mut map = BTreeMap::new();
    let Some(object) = value.as_object() else {
        return map;
    };

    if let Some(business) = object.get("business").and_then(Value::as_object) {
        for (key, value) in business {
            insert_json_value(&mut map, &normalize_header(key), value);
        }
    }

    for (key, value) in object {
        let key_norm = normalize_header(key);
        if key_norm == "address" {
            if let Some(address) = value.as_object() {
                for (subkey, subvalue) in address {
                    insert_json_value(&mut map, &normalize_header(subkey), subvalue);
                }
            }
            continue;
        }
        if key_norm == "contacts" {
            if let Some(contacts) = value.as_array() {
                for contact in contacts {
                    let Some(kind) = contact.get("kind").and_then(Value::as_str) else {
                        continue;
                    };
                    let Some(value) = contact.get("value").and_then(Value::as_str) else {
                        continue;
                    };
                    let target = match kind {
                        "phone" | "mobile" | "whatsapp" => "telefono",
                        "email" => "correo",
                        "website" => "sitio_web",
                        "facebook" => "facebook",
                        "instagram" => "instagram",
                        "tiktok" => "tiktok",
                        "twitter" => "twitter",
                        "telegram" => "telegram",
                        "linkedin" => "linkedin",
                        "youtube" => "youtube",
                        _ => continue,
                    };
                    append_map(&mut map, target, value);
                }
            }
            continue;
        }
        if key_norm == "tags" {
            if let Some(tags) = value.as_object() {
                for (tag_key, tag_value) in tags {
                    insert_json_value(&mut map, &normalize_header(tag_key), tag_value);
                }
            }
            continue;
        }
        if matches!(
            key_norm.as_str(),
            "sources" | "raw" | "extra" | "distance_m"
        ) {
            continue;
        }
        insert_json_value(&mut map, &key_norm, value);
    }
    map
}

fn insert_json_value(map: &mut BTreeMap<String, String>, key: &str, value: &Value) {
    if key.is_empty() {
        return;
    }
    match value {
        Value::Null => {}
        Value::String(text) => {
            let text = text.trim();
            if !text.is_empty() {
                map.insert(key.to_string(), text.to_string());
            }
        }
        Value::Bool(value) => {
            map.insert(key.to_string(), value.to_string());
        }
        Value::Number(value) => {
            map.insert(key.to_string(), value.to_string());
        }
        Value::Array(items) => {
            let joined = items
                .iter()
                .filter_map(|item| match item {
                    Value::String(s) => Some(s.clone()),
                    Value::Number(n) => Some(n.to_string()),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join(";");
            if !joined.is_empty() {
                map.insert(key.to_string(), joined);
            }
        }
        Value::Object(_) => {
            // Los objetos anidados (p. ej. geocodes) se guardan serializados
            // como etiqueta para no perder información.
            if let Ok(text) = serde_json::to_string(value) {
                map.insert(key.to_string(), text);
            }
        }
    }
}

fn append_map(map: &mut BTreeMap<String, String>, key: &str, value: &str) {
    map.entry(key.to_string())
        .and_modify(|existing| {
            if !existing.split(';').any(|v| v.trim() == value) {
                existing.push(';');
                existing.push_str(value);
            }
        })
        .or_insert_with(|| value.to_string());
}

fn map_to_record(
    map: &BTreeMap<String, String>,
    raw: Value,
    options: &ImportOptions<'_>,
) -> Option<BusinessRecord> {
    let name = get_first(
        map,
        &[
            "nombre",
            "name",
            "negocio",
            "empresa",
            "nombre_comercial",
            "nombre_del_negocio",
            "razon_social_comercial",
            "title",
            "business_name",
            "comercio",
        ],
    )?;
    if name.trim().is_empty() {
        return None;
    }

    let mut contacts = Vec::new();
    let mut saw_phone = false;
    for key in map.keys() {
        if is_phone_key(key) {
            for value in split_multi(map.get(key).map(String::as_str).unwrap_or("")) {
                let kind = if key.contains("whatsapp") {
                    ContactKind::Whatsapp
                } else if key.contains("celular") || key.contains("movil") || key.contains("mobile")
                {
                    ContactKind::Mobile
                } else {
                    ContactKind::Phone
                };
                let mut contact = Contact::new(kind, value).with_source(options.source);
                if !saw_phone {
                    contact = contact.primary();
                    saw_phone = true;
                }
                contacts.push(contact);
            }
        }
    }
    let mut saw_email = false;
    for key in map.keys() {
        if is_email_key(key) {
            for value in split_multi(map.get(key).map(String::as_str).unwrap_or("")) {
                let mut contact =
                    Contact::new(ContactKind::Email, value).with_source(options.source);
                if !saw_email {
                    contact = contact.primary();
                    saw_email = true;
                }
                contacts.push(contact);
            }
        }
    }
    let mut saw_website = false;
    for key in map.keys() {
        if is_website_key(key) {
            for value in split_multi(map.get(key).map(String::as_str).unwrap_or("")) {
                let mut contact =
                    Contact::new(ContactKind::Website, value).with_source(options.source);
                if !saw_website {
                    contact = contact.primary();
                    saw_website = true;
                }
                contacts.push(contact);
            }
        }
    }
    for (aliases, kind) in [
        (
            &["facebook", "fb", "facebook_url"][..],
            ContactKind::Facebook,
        ),
        (
            &["instagram", "insta", "instagram_url"][..],
            ContactKind::Instagram,
        ),
        (&["tiktok", "tiktok_url"][..], ContactKind::TikTok),
        (&["twitter", "x", "twitter_url"][..], ContactKind::Twitter),
        (&["telegram", "telegram_url"][..], ContactKind::Telegram),
        (&["linkedin", "linkedin_url"][..], ContactKind::LinkedIn),
        (&["youtube", "youtube_url"][..], ContactKind::YouTube),
    ] {
        if let Some(value) = get_first(map, aliases) {
            for value in split_multi(&value) {
                contacts.push(Contact::new(kind, value).with_source(options.source));
            }
        }
    }

    let category = get_first(
        map,
        &[
            "categoria",
            "category",
            "rubro",
            "giro",
            "sector",
            "tipo",
            "tipo_negocio",
            "tipo_de_negocio",
            "actividad",
            "business_type",
            "industry",
        ],
    );
    let categories: Vec<String> = get_first(map, &["categorias", "categories"])
        .map(|value| split_multi(&value))
        .unwrap_or_default();

    let status = get_first(
        map,
        &[
            "status",
            "estado_negocio",
            "business_status",
            "activo",
            "estado_del_negocio",
        ],
    )
    .map(|value| match normalize_header(&value).as_str() {
        "activo" | "abierto" | "active" | "operativo" | "si" | "1" | "true" => "active".to_string(),
        "inactivo" | "cerrado" | "closed" | "no" | "0" | "false" => "closed".to_string(),
        other => other.to_string(),
    });

    let address = Address {
        line: get_first(
            map,
            &[
                "direccion",
                "address",
                "domicilio",
                "ubicacion",
                "direccion_completa",
                "formatted_address",
            ],
        ),
        street: get_first(map, &["calle", "street", "avenida", "road"]),
        house_number: get_first(map, &["numero", "numero_casa", "house_number", "num"]),
        barrio: get_first(
            map,
            &[
                "barrio",
                "neighborhood",
                "colonia",
                "barrio_colonia",
                "suburb",
            ],
        ),
        comarca: get_first(map, &["comarca", "hamlet", "valle"]),
        municipio: get_first(
            map,
            &[
                "municipio",
                "city",
                "ciudad",
                "alcaldia",
                "town",
                "localidad",
                "locality",
            ],
        ),
        departamento: get_first(
            map,
            &[
                "departamento",
                "depto",
                "department",
                "state",
                "region",
                "provincia",
            ],
        ),
        postal_code: get_first(map, &["codigo_postal", "postal_code", "zip", "zipcode"]),
        country_code: get_first(map, &["pais", "country", "country_code"])
            .map(|value| {
                crate::gazetteer::Gazetteer::global()
                    .normalize_country_code(&value)
                    .unwrap_or(value)
            })
            .or_else(|| options.location.country_code.clone()),
    };

    let latitude = get_first(map, &["latitud", "latitude", "lat"])
        .and_then(|v| v.replace(',', ".").parse().ok())
        .or_else(|| parse_coordinate(map, "lat"));
    let longitude = get_first(map, &["longitud", "longitude", "lon", "lng", "long"])
        .and_then(|v| v.replace(',', ".").parse().ok())
        .or_else(|| parse_coordinate(map, "lon"));

    let mut tags = BTreeMap::new();
    for (key, value) in map {
        if value.chars().count() <= 500 {
            tags.insert(key.clone(), value.clone());
        }
    }
    tags.insert("import.source".to_string(), options.source.to_string());

    let external_id = get_first(
        map,
        &[
            "id",
            "external_id",
            "codigo",
            "code",
            "fsq_id",
            "place_id",
            "registro",
            "numero_registro",
            "ruc",
        ],
    );

    let confidence = if contacts.is_empty() { 0.45 } else { 0.55 };

    Some(BusinessRecord {
        source: options.source.to_string(),
        external_id,
        source_url: get_first(map, &["url_fuente", "source_url", "enlace", "link"]),
        name,
        aliases: get_first(map, &["alias", "aliases", "nombres_alternativos"])
            .map(|value| split_multi(&value))
            .unwrap_or_default(),
        legal_name: get_first(
            map,
            &[
                "razon_social",
                "legal_name",
                "nombre_legal",
                "razon_social_legal",
            ],
        ),
        description: get_first(
            map,
            &[
                "descripcion",
                "description",
                "detalles",
                "about",
                "resena",
                "summary",
            ],
        ),
        primary_category: category.clone(),
        business_type: category.clone(),
        categories,
        status,
        contacts,
        address: fill_location(address, options.location),
        latitude,
        longitude,
        opening_hours: get_first(
            map,
            &[
                "horario",
                "horarios",
                "opening_hours",
                "hours",
                "horario_atencion",
            ],
        ),
        price_range: get_first(map, &["precio", "price_range", "rango_precio", "price"]),
        rating: get_first(map, &["rating", "calificacion", "estrellas", "puntuacion"])
            .and_then(|v| v.replace(',', ".").parse().ok()),
        review_count: get_first(
            map,
            &[
                "reviews",
                "resenas",
                "numero_resenas",
                "review_count",
                "cantidad_resenas",
            ],
        )
        .and_then(|v| v.replace([',', '.'], "").parse().ok()),
        tags,
        raw,
        confidence,
        fetched_at: Utc::now(),
    })
}

fn fill_location(mut address: Address, location: &ResolvedLocation) -> Address {
    if address.country_code.is_none() {
        address.country_code = location.country_code.clone();
    }
    if address.departamento.is_none() {
        address.departamento = location.departamento.clone();
    }
    if address.municipio.is_none() {
        address.municipio = location.municipio.clone();
    }
    if address.barrio.is_none() {
        address.barrio = location.barrio.clone();
    }
    if address.comarca.is_none() {
        address.comarca = location.comarca.clone();
    }
    address
}

fn get_first(map: &BTreeMap<String, String>, keys: &[&str]) -> Option<String> {
    keys.iter().find_map(|key| {
        map.get(*key)
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
    })
}

fn is_phone_key(key: &str) -> bool {
    let key = key.to_lowercase();
    (key.starts_with("telefono")
        || key.starts_with("phone")
        || key == "tel"
        || key == "celular"
        || key == "movil"
        || key == "mobile"
        || key == "whatsapp"
        || key == "contacto"
        || key == "numero_contacto"
        || key == "numeros_contacto")
        && !key.contains("tipo")
        && !key.contains("codigo")
}

fn is_email_key(key: &str) -> bool {
    let key = key.to_lowercase();
    key.contains("correo")
        || key == "email"
        || key == "e_mail"
        || key == "mail"
        || key == "correo_electronico"
}

fn is_website_key(key: &str) -> bool {
    let key = key.to_lowercase();
    (key.contains("sitio")
        || key.contains("website")
        || key == "web"
        || key == "url"
        || key.contains("pagina"))
        && !key.contains("facebook")
        && !key.contains("instagram")
}

fn split_multi(value: &str) -> Vec<String> {
    value
        .split([';', '|', '\n'])
        .flat_map(|part| {
            if part.contains(" / ") {
                part.split(" / ")
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
            } else {
                vec![part.to_string()]
            }
        })
        .map(|part| part.trim().to_string())
        .filter(|part| !part.is_empty())
        .collect()
}

fn parse_coordinate(map: &BTreeMap<String, String>, axis: &str) -> Option<f64> {
    let key: &[&str] = match axis {
        "lat" => &["latitud", "latitude", "lat"],
        _ => &["longitud", "longitude", "lon", "lng", "long"],
    };
    for candidate in key {
        if let Some(value) = map.get(*candidate) {
            if let Ok(number) = value.replace(',', ".").parse::<f64>() {
                return Some(number);
            }
        }
    }
    None
}

/// Objeto JSON de ejemplo para documentación y pruebas.
pub fn example_row() -> Value {
    json!({
        "nombre": "Ferretería El Tornillo",
        "telefono": "+505 2222-3333",
        "celular": "+505 8888-7777",
        "correo": "ventas@eltornillo.ni",
        "sitio_web": "eltornillo.ni",
        "categoria": "Ferretería",
        "direccion": "Calle Central 123",
        "barrio": "Barrio San Juan",
        "municipio": "León",
        "departamento": "León",
        "latitud": "12.435",
        "longitud": "-86.878"
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::location::{GeoFilter, GeoPoint, ResolvedLocation};

    fn location() -> ResolvedLocation {
        ResolvedLocation {
            query: "León".into(),
            display_name: "León, Nicaragua".into(),
            country_code: Some("NI".into()),
            country_name: Some("Nicaragua".into()),
            continent: Some("North America".into()),
            departamento: Some("León".into()),
            municipio: Some("León".into()),
            barrio: None,
            comarca: None,
            filter: GeoFilter::Area { relation_id: 1 },
            center: Some(GeoPoint {
                lat: 12.43,
                lon: -86.87,
            }),
            bbox: None,
            radius_m: None,
            osm_type: None,
            osm_id: None,
            source: "test".into(),
        }
    }

    fn options(loc: &ResolvedLocation) -> ImportOptions<'_> {
        ImportOptions {
            source: "import",
            delimiter: None,
            location: loc,
        }
    }

    #[test]
    fn imports_semicolon_csv_with_spanish_headers() {
        let csv = "Nombre;Teléfono;Correo;Rubro;Barrio;Municipio;Departamento\n\
                   Ferretería El Tornillo;2222-3333;ventas@eltornillo.ni;Ferretería;San Juan;León;León\n";
        let loc = location();
        let records = import_csv(csv, &options(&loc)).unwrap();
        assert_eq!(records.len(), 1);
        let record = &records[0];
        assert_eq!(record.name, "Ferretería El Tornillo");
        assert!(record.contacts.iter().any(|c| c.kind == ContactKind::Phone));
        assert!(record.contacts.iter().any(|c| c.kind == ContactKind::Email));
        assert_eq!(record.address.municipio.as_deref(), Some("León"));
        assert_eq!(record.primary_category.as_deref(), Some("Ferretería"));
    }

    #[test]
    fn imports_json_array_and_native_contacts() {
        let json = r#"[
          {
            "nombre": "Café Las Flores",
            "categoria": "Cafetería",
            "address": {"barrio": "Bolonia", "municipio": "Managua", "departamento": "Managua"},
            "contacts": [
              {"kind": "phone", "value": "+505 8888 1111"},
              {"kind": "instagram", "value": "lasflores"}
            ]
          }
        ]"#;
        let loc = location();
        let records = import_json(json, &options(&loc)).unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].name, "Café Las Flores");
        assert_eq!(records[0].address.barrio.as_deref(), Some("Bolonia"));
        assert!(records[0]
            .contacts
            .iter()
            .any(|c| c.kind == ContactKind::Instagram));
    }

    #[test]
    fn imports_jsonl() {
        let jsonl = "{\"name\":\"Uno\",\"phone\":\"8888-1111\"}\n{\"name\":\"Dos\",\"phone\":\"8888-2222\"}\n";
        let loc = location();
        let records = import_jsonl(jsonl, &options(&loc)).unwrap();
        assert_eq!(records.len(), 2);
        assert_eq!(records[1].name, "Dos");
    }

    #[test]
    fn detects_semicolon_delimiter() {
        assert_eq!(detect_delimiter("a;b;c\n1;2;3"), b';');
        assert_eq!(detect_delimiter("a,b,c\n1,2,3"), b',');
    }

    #[test]
    fn unknown_columns_become_tags() {
        let csv = "nombre,telefono,horario_especial\nTienda X,8888-1111,abierto feriados\n";
        let loc = location();
        let records = import_csv(csv, &options(&loc)).unwrap();
        assert_eq!(
            records[0].tags.get("horario_especial").map(String::as_str),
            Some("abierto feriados")
        );
    }
}
