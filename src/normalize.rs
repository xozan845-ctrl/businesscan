//! Normalización de nombres, teléfonos, URLs y utilidades geográficas.
//!
//! Todo lo que se compara o se busca pasa por aquí para que `León`, `LEON` y
//! `Leon` sean equivalentes, y para que un teléfono escrito de cinco formas
//! distintas se guarde siempre igual.

use std::net::IpAddr;

use unicode_normalization::UnicodeNormalization;
use url::Url;

/// Texto en minúsculas, sin acentos, sin puntuación y con espacios simples.
pub fn normalize_text(input: &str) -> String {
    let decomposed: String = input.nfd().collect();
    let mut out = String::with_capacity(decomposed.len());
    let mut last_was_space = true;

    for ch in decomposed.chars() {
        if unicode_normalization::char::is_combining_mark(ch) {
            continue;
        }
        let ch = ch.to_lowercase().next().unwrap_or(ch);
        if ch.is_alphanumeric() {
            out.push(ch);
            last_was_space = false;
        } else if !last_was_space {
            out.push(' ');
            last_was_space = true;
        }
    }

    out.trim().to_string()
}

/// Variante para deduplicar: además quita sufijos societarios comunes.
pub fn normalize_name_for_match(input: &str) -> String {
    let normalized = normalize_text(input);
    let mut tokens: Vec<&str> = normalized.split_whitespace().collect();

    fn is_legal_suffix(token: &str) -> bool {
        matches!(
            token,
            "sa" | "srl" | "ltda" | "limitada" | "eirl" | "inc" | "llc" | "corp"
        )
    }

    // Quita secuencias finales tipo "s a", "s de r l", "sociedad anonima".
    while let Some(last) = tokens.last().copied() {
        let lower = tokens.join(" ");
        let explicit = lower.ends_with("sociedad anonima")
            || lower.ends_with("sociedad de responsabilidad limitada")
            || lower.ends_with("s de r l")
            || lower.ends_with("s a");
        if explicit {
            let remove = if lower.ends_with("sociedad de responsabilidad limitada") {
                4
            } else if lower.ends_with("sociedad anonima") {
                2
            } else if lower.ends_with("s de r l") {
                4
            } else {
                2
            };
            tokens.truncate(tokens.len().saturating_sub(remove));
            continue;
        }
        if tokens.len() > 1 && is_legal_suffix(last) {
            tokens.pop();
            continue;
        }
        break;
    }

    if tokens.is_empty() {
        normalize_text(input)
    } else {
        tokens.join(" ")
    }
}

/// Devuelve `None` si el texto está vacío después de limpiar.
pub fn clean_opt(value: Option<String>) -> Option<String> {
    value.and_then(|v| {
        let trimmed = v.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    })
}

pub fn clean_str(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

/// Teléfono en formato E.164 cuando es posible. Para Nicaragua usa +505.
pub fn normalize_phone(raw: &str) -> Option<String> {
    let mut digits: String = raw.chars().filter(|c| c.is_ascii_digit()).collect();
    if digits.is_empty() {
        return None;
    }

    // Extensiones: "2222-3333 ext 12" -> conserva solo el número principal.
    if let Some(idx) = raw.to_lowercase().find("ext") {
        let before = &raw[..idx];
        digits = before.chars().filter(|c| c.is_ascii_digit()).collect();
    }

    if digits.len() > 15 {
        digits.truncate(15);
    }

    if digits.len() == 8 {
        return Some(format!("+505{digits}"));
    }
    if digits.len() == 11 && digits.starts_with("505") {
        return Some(format!("+{digits}"));
    }
    if digits.starts_with("00") && digits.len() > 10 {
        return Some(format!("+{}", &digits[2..]));
    }
    if digits.len() >= 8 {
        return Some(format!("+{digits}"));
    }
    None
}

/// Clave de comparación de teléfonos: para Nicaragua, los últimos 8 dígitos;
/// para números internacionales, el número completo.
pub fn phone_key(raw: &str) -> Option<String> {
    let normalized = normalize_phone(raw)?;
    let digits: String = normalized.chars().filter(|c| c.is_ascii_digit()).collect();
    if digits.starts_with("505") && digits.len() == 11 {
        Some(digits[3..].to_string())
    } else if digits.len() >= 8 {
        Some(digits)
    } else {
        None
    }
}

pub fn normalize_email(raw: &str) -> Option<String> {
    let value = raw.trim().trim_start_matches("mailto:").trim();
    let lower = value.to_lowercase();
    // Validación deliberadamente laxa: los directorios traen correos raros.
    if lower.contains('@') && lower.contains('.') && !lower.contains(' ') {
        Some(lower)
    } else {
        None
    }
}

/// URL absoluta y canónica. Acepta `dominio.com` sin esquema.
pub fn normalize_url(raw: &str) -> Option<String> {
    let value = raw.trim();
    if value.is_empty() {
        return None;
    }
    let with_scheme = if value.starts_with("http://") || value.starts_with("https://") {
        value.to_string()
    } else if value.starts_with("//") {
        format!("https:{value}")
    } else if value.contains('.') && !value.contains(' ') {
        format!("https://{value}")
    } else {
        return None;
    };

    let mut url = Url::parse(&with_scheme).ok()?;
    if url.scheme() != "http" && url.scheme() != "https" {
        return None;
    }
    if let Some(host) = url.host_str() {
        if host.parse::<IpAddr>().is_ok() || host == "localhost" {
            return None;
        }
    }
    url.set_fragment(None);
    let mut out = url.to_string();
    if out.ends_with('/') && url.path() == "/" {
        out.pop();
    }
    Some(out)
}

pub fn domain_of(raw: &str) -> Option<String> {
    let url = Url::parse(raw).ok()?;
    let host = url.host_str()?.to_lowercase();
    Some(host.strip_prefix("www.").unwrap_or(&host).to_string())
}

/// Convierte un enlace o usuario de red social en URL.
pub fn normalize_social(kind: &str, raw: &str) -> Option<String> {
    let value = raw.trim().trim_start_matches('@');
    if value.is_empty() {
        return None;
    }
    if value.starts_with("http://") || value.starts_with("https://") {
        return normalize_url(value);
    }

    let base = match kind {
        "facebook" => "https://www.facebook.com/",
        "instagram" => "https://www.instagram.com/",
        "tiktok" => "https://www.tiktok.com/@",
        "twitter" | "x" => "https://x.com/",
        "telegram" => "https://t.me/",
        "youtube" => "https://www.youtube.com/@",
        "linkedin" => "https://www.linkedin.com/company/",
        _ => return normalize_url(value),
    };

    let handle = value
        .trim_start_matches("facebook.com/")
        .trim_start_matches("www.facebook.com/")
        .trim_start_matches("instagram.com/")
        .trim_start_matches("www.instagram.com/")
        .trim_start_matches("tiktok.com/")
        .trim_start_matches("x.com/")
        .trim_start_matches("twitter.com/")
        .trim_start_matches("t.me/")
        .trim_start_matches("youtube.com/")
        .trim_start_matches("youtu.be/");
    if handle.is_empty() || handle.contains(' ') {
        return None;
    }
    Some(format!("{base}{handle}"))
}

/// Similitud de nombres en 0..1.
pub fn similarity(a: &str, b: &str) -> f64 {
    if a == b {
        return 1.0;
    }
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }
    strsim::normalized_levenshtein(a, b)
}

/// Distancia Haversine en metros.
pub fn haversine_m(lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> f64 {
    const EARTH_RADIUS_M: f64 = 6_371_000.0;
    let phi1 = lat1.to_radians();
    let phi2 = lat2.to_radians();
    let dphi = (lat2 - lat1).to_radians();
    let dlambda = (lon2 - lon1).to_radians();
    let a = (dphi / 2.0).sin().powi(2) + phi1.cos() * phi2.cos() * (dlambda / 2.0).sin().powi(2);
    2.0 * EARTH_RADIUS_M * a.sqrt().asin()
}

/// Caja geográfica con un pequeño margen para el filtro SQL aproximado.
#[derive(Debug, Clone, Copy)]
pub struct Bbox {
    pub south: f64,
    pub west: f64,
    pub north: f64,
    pub east: f64,
}

impl Bbox {
    pub fn around(lat: f64, lon: f64, radius_m: f64) -> Self {
        // Margen generoso: el filtro exacto se hace con Haversine.
        let lat_delta = (radius_m / 111_320.0) * 1.1;
        let cos = lat.to_radians().cos().abs().max(0.01);
        let lon_delta = (radius_m / (111_320.0 * cos)) * 1.1;
        Self {
            south: (lat - lat_delta).max(-90.0),
            west: (lon - lon_delta).max(-180.0),
            north: (lat + lat_delta).min(90.0),
            east: (lon + lon_delta).min(180.0),
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        let parts: Vec<f64> = value
            .split(',')
            .map(|p| p.trim().parse::<f64>())
            .collect::<Result<_, _>>()
            .ok()?;
        if parts.len() != 4 {
            return None;
        }
        let (a, b, c, d) = (parts[0], parts[1], parts[2], parts[3]);
        // Acepta tanto minLat,minLon,maxLat,maxLon como oeste,sur,este,norte.
        let (south, west, north, east) = if a.abs() <= 90.0 && c.abs() <= 90.0 {
            (a.min(c), b.min(d), a.max(c), b.max(d))
        } else {
            (b.min(d), a.min(c), b.max(d), a.max(c))
        };
        Some(Self {
            south,
            west,
            north,
            east,
        })
    }

    pub fn contains(&self, lat: f64, lon: f64) -> bool {
        lat >= self.south && lat <= self.north && lon >= self.west && lon <= self.east
    }
}

/// Geohash base32 (precisión 9 por defecto). Útil para agrupar por zona.
pub fn geohash(lat: f64, lon: f64, precision: usize) -> String {
    const BASE32: &[u8] = b"0123456789bcdefghjkmnpqrstuvwxyz";
    let mut lat_range = (-90.0, 90.0);
    let mut lon_range = (-180.0, 180.0);
    let mut out = String::with_capacity(precision);
    let mut bits = 0u8;
    let mut bit_count = 0;
    let mut even = true;

    while out.len() < precision {
        if even {
            let mid = (lon_range.0 + lon_range.1) / 2.0;
            if lon >= mid {
                bits = (bits << 1) | 1;
                lon_range.0 = mid;
            } else {
                bits <<= 1;
                lon_range.1 = mid;
            }
        } else {
            let mid = (lat_range.0 + lat_range.1) / 2.0;
            if lat >= mid {
                bits = (bits << 1) | 1;
                lat_range.0 = mid;
            } else {
                bits <<= 1;
                lat_range.1 = mid;
            }
        }
        even = !even;
        bit_count += 1;
        if bit_count == 5 {
            out.push(BASE32[bits as usize] as char);
            bits = 0;
            bit_count = 0;
        }
    }
    out
}

/// FNV-1a estable para generar identificadores de importación.
pub fn stable_hash(input: &str) -> String {
    let mut hash: u64 = 0xcbf29ce484222325;
    for byte in input.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{hash:016x}")
}

/// Tokens listos para FTS5, sin stopwords demasiado comunes.
pub fn search_tokens(text: &str) -> Vec<String> {
    normalize_text(text)
        .split_whitespace()
        .filter(|token| token.chars().count() >= 2)
        .map(ToString::to_string)
        .collect()
}

/// Construye una consulta FTS5 segura con prefijos.
pub fn fts_query(text: &str, all: bool) -> Option<String> {
    let tokens = search_tokens(text);
    if tokens.is_empty() {
        return None;
    }
    let joiner = if all { " AND " } else { " OR " };
    let parts: Vec<String> = tokens
        .into_iter()
        .map(|token| format!("\"{}\"*", token.replace('"', "\"\"")))
        .collect();
    Some(parts.join(joiner))
}

/// Identificador corto y legible: los últimos 8 caracteres del ULID.
/// El prefijo de un ULID es el timestamp y se repite en lotes grandes.
pub fn short_id(id: &str) -> &str {
    if id.len() <= 8 {
        id
    } else {
        &id[id.len() - 8..]
    }
}

pub fn truncate(value: &str, max_chars: usize) -> String {
    let count = value.chars().count();
    if count <= max_chars {
        return value.to_string();
    }
    if max_chars <= 1 {
        return "…".to_string();
    }
    let mut out: String = value.chars().take(max_chars - 1).collect();
    out.push('…');
    out
}

pub fn format_distance(meters: f64) -> String {
    if meters < 1_000.0 {
        format!("{meters:.0} m")
    } else {
        format!("{:.1} km", meters / 1_000.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accents_and_case_collapse() {
        assert_eq!(normalize_text("  León, NICARAGUA  "), "leon nicaragua");
        assert_eq!(normalize_text("Río San Juan"), "rio san juan");
        assert_eq!(normalize_text("A.B.C. #12"), "a b c 12");
    }

    #[test]
    fn legal_suffixes_are_removed_for_matching() {
        assert_eq!(
            normalize_name_for_match("Ferretería El Tornillo S.A."),
            "ferreteria el tornillo"
        );
        assert_eq!(
            normalize_name_for_match("Comercial López S. de R.L."),
            "comercial lopez"
        );
        assert_eq!(normalize_name_for_match("SA"), "sa");
    }

    #[test]
    fn phones_normalize_to_nicaragua() {
        assert_eq!(
            normalize_phone("(505) 8888-7777").as_deref(),
            Some("+50588887777")
        );
        assert_eq!(
            normalize_phone("8888 7777").as_deref(),
            Some("+50588887777")
        );
        assert_eq!(phone_key("+505 8888-7777").as_deref(), Some("88887777"));
        assert_eq!(normalize_phone("123"), None);
    }

    #[test]
    fn urls_and_socials() {
        assert_eq!(
            normalize_url("facebook.com/minegocio").as_deref(),
            Some("https://facebook.com/minegocio")
        );
        assert_eq!(
            normalize_social("instagram", "@mi_negocio").as_deref(),
            Some("https://www.instagram.com/mi_negocio")
        );
        assert_eq!(
            normalize_social("facebook", "https://facebook.com/x").as_deref(),
            Some("https://facebook.com/x")
        );
    }

    #[test]
    fn geohash_is_stable() {
        assert_eq!(geohash(12.1364, -86.2514, 9), "d44tj33es");
    }

    #[test]
    fn fts_quotes_and_prefixes() {
        assert_eq!(
            fts_query("Farmacia \"La Fe\"", true).as_deref(),
            Some("\"farmacia\"* AND \"la\"* AND \"fe\"*")
        );
    }

    #[test]
    fn distance_is_reasonable() {
        let d = haversine_m(12.1364, -86.2514, 12.4379, -86.8780);
        assert!(d > 60_000.0 && d < 100_000.0, "distancia: {d}");
    }
}
