//! Internacionalización ligera: español e inglés.
//!
//! El idioma afecta las consultas a proveedores, las etiquetas de categorías y
//! los nombres de países. La interfaz de terminal mantiene español por ahora,
//! pero la base está lista para añadir más idiomas.

use clap::ValueEnum;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, ValueEnum)]
pub enum Language {
    #[default]
    Es,
    En,
}

impl Language {
    pub fn code(self) -> &'static str {
        match self {
            Self::Es => "es",
            Self::En => "en",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_lowercase().as_str() {
            "es" | "spa" | "espanol" | "español" | "spanish" => Some(Self::Es),
            "en" | "eng" | "english" | "ingles" | "inglés" => Some(Self::En),
            _ => None,
        }
    }

    pub fn from_env() -> Self {
        for name in ["BUSINESSCAN_LANG", "BUSINESSCAN_LANGUAGE"] {
            if let Ok(value) = std::env::var(name) {
                if let Some(language) = Self::parse(&value) {
                    return language;
                }
            }
        }
        if let Ok(value) = std::env::var("NICASCAN_LANGUAGE") {
            if let Some(language) = Self::parse(&value) {
                return language;
            }
        }
        Self::Es
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Es => "español",
            Self::En => "English",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_language_aliases() {
        assert_eq!(Language::parse("en"), Some(Language::En));
        assert_eq!(Language::parse("español"), Some(Language::Es));
        assert_eq!(Language::parse("fr"), None);
    }
}
