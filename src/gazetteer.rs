//! Gacetero internacional: países, continentes y subregiones.
//!
//! Incluye 250 países/territorios con nombres en español e inglés, bbox
//! aproximado (Natural Earth) y código telefónico. Nicaragua sigue siendo un
//! caso de primera clase, pero ya no es el centro del modelo.

use std::collections::HashMap;
use std::sync::OnceLock;

use serde::Deserialize;

use crate::i18n::Language;
use crate::normalize::{normalize_text, similarity};

const COUNTRIES_JSON: &str = include_str!("../assets/countries.json");

#[derive(Debug, Clone, Deserialize)]
pub struct Country {
    pub iso2: String,
    pub iso3: Option<String>,
    pub name_es: String,
    pub name_en: String,
    #[serde(default)]
    pub aliases: Vec<String>,
    pub region: Option<String>,
    pub subregion: Option<String>,
    pub continent: Option<String>,
    pub lat: f64,
    pub lon: f64,
    /// [oeste, sur, este, norte]
    pub bbox: [f64; 4],
    pub area: f64,
    pub calling_code: String,
}

impl Country {
    pub fn name(&self, language: Language) -> &str {
        match language {
            Language::Es => &self.name_es,
            Language::En => &self.name_en,
        }
    }

    pub fn bbox_or_point(&self) -> [f64; 4] {
        if self.bbox.iter().all(|value| value.is_finite()) {
            self.bbox
        } else {
            [
                self.lon - 0.5,
                self.lat - 0.5,
                self.lon + 0.5,
                self.lat + 0.5,
            ]
        }
    }
}

#[derive(Debug, Clone)]
pub struct Gazetteer {
    countries: Vec<Country>,
    index: HashMap<String, usize>,
}

impl Gazetteer {
    pub fn global() -> &'static Self {
        static GAZETTEER: OnceLock<Gazetteer> = OnceLock::new();
        GAZETTEER.get_or_init(|| Self::load().expect("gacetero internacional inválido"))
    }

    pub fn load() -> anyhow::Result<Self> {
        let countries: Vec<Country> = serde_json::from_str(COUNTRIES_JSON)?;
        let mut index = HashMap::new();
        for (position, country) in countries.iter().enumerate() {
            for name in std::iter::once(country.name_es.as_str())
                .chain(std::iter::once(country.name_en.as_str()))
                .chain(country.aliases.iter().map(String::as_str))
            {
                let key = normalize_text(name);
                if !key.is_empty() {
                    index.entry(key).or_insert(position);
                }
            }
            index
                .entry(normalize_text(&country.iso2))
                .or_insert(position);
            if let Some(iso3) = country.iso3.as_deref() {
                index.entry(normalize_text(iso3)).or_insert(position);
            }
        }
        Ok(Self { countries, index })
    }

    pub fn countries(&self) -> &[Country] {
        &self.countries
    }

    pub fn country(&self, input: &str) -> Option<&Country> {
        let key = normalize_text(input);
        if key.is_empty() {
            return None;
        }
        self.index
            .get(&key)
            .map(|position| &self.countries[*position])
    }

    /// Convierte "Nicaragua", "ni" o "NI" a código ISO de dos letras.
    pub fn normalize_country_code(&self, value: &str) -> Option<String> {
        let trimmed = value.trim();
        if trimmed.len() == 2 {
            let code = trimmed.to_uppercase();
            if self.country_by_iso2(&code).is_some() {
                return Some(code);
            }
        }
        self.country(trimmed).map(|country| country.iso2.clone())
    }

    pub fn country_by_iso2(&self, iso2: &str) -> Option<&Country> {
        self.index
            .get(&normalize_text(iso2))
            .map(|position| &self.countries[*position])
    }

    pub fn suggest_country(&self, input: &str) -> Option<&Country> {
        let key = normalize_text(input);
        self.countries
            .iter()
            .map(|country| {
                let score = [&country.name_es, &country.name_en]
                    .into_iter()
                    .map(|name| similarity(&key, &normalize_text(name)))
                    .fold(0.0, f64::max);
                (country, score)
            })
            .filter(|(_, score)| *score >= 0.72)
            .max_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(country, _)| country)
    }

    /// Países de un continente, subregión o grupo especial.
    ///
    /// Acepta `central-america`, `centroamerica`, `north-america`, `europe`,
    /// `americas`, `world`, etc. Devuelve `None` si el grupo no existe.
    pub fn countries_in_group(&self, input: &str) -> Option<Vec<&Country>> {
        let key = normalize_text(input);
        if key.is_empty() {
            return None;
        }
        if matches!(
            key.as_str(),
            "world" | "mundo" | "global" | "todo el mundo" | "all"
        ) {
            return Some(self.countries.iter().collect());
        }

        let group = if matches!(
            key.as_str(),
            "central america"
                | "central-america"
                | "centroamerica"
                | "centro america"
                | "centro-america"
                | "america central"
                | "america-central"
        ) {
            Group::Subregion("central america")
        } else if matches!(
            key.as_str(),
            "north america" | "norteamerica" | "north-america"
        ) {
            Group::Continent("north america")
        } else if matches!(
            key.as_str(),
            "south america" | "sudamerica" | "south-america" | "america del sur"
        ) {
            Group::Continent("south america")
        } else if matches!(key.as_str(), "americas" | "america" | "las americas") {
            Group::Region("americas")
        } else if matches!(key.as_str(), "europe" | "europa") {
            Group::Continent("europe")
        } else if matches!(key.as_str(), "asia") {
            Group::Continent("asia")
        } else if matches!(key.as_str(), "africa") {
            Group::Continent("africa")
        } else if matches!(key.as_str(), "oceania" | "oceanía") {
            Group::Continent("oceania")
        } else if matches!(key.as_str(), "middle east" | "oriente medio") {
            Group::Subregion("western asia")
        } else if matches!(key.as_str(), "caribbean" | "caribe") {
            Group::Subregion("caribbean")
        } else {
            // También acepta el nombre de un continente/subregión tal cual.
            let countries: Vec<&Country> = self
                .countries
                .iter()
                .filter(|country| {
                    [country.continent.as_deref(), country.subregion.as_deref()]
                        .into_iter()
                        .flatten()
                        .any(|value| normalize_text(value) == key)
                        || normalize_text(country.region.as_deref().unwrap_or("")) == key
                })
                .collect();
            return if countries.is_empty() {
                None
            } else {
                Some(countries)
            };
        };

        let countries: Vec<&Country> = self
            .countries
            .iter()
            .filter(|country| match group {
                Group::Subregion(value) => {
                    normalize_text(country.subregion.as_deref().unwrap_or(""))
                        == normalize_text(value)
                }
                Group::Continent(value) => {
                    normalize_text(country.continent.as_deref().unwrap_or(""))
                        == normalize_text(value)
                }
                Group::Region(value) => {
                    normalize_text(country.region.as_deref().unwrap_or("")) == normalize_text(value)
                }
            })
            .collect();
        if countries.is_empty() {
            None
        } else {
            Some(countries)
        }
    }

    pub fn group_names(&self) -> Vec<&str> {
        vec![
            "central-america",
            "north-america",
            "south-america",
            "americas",
            "europe",
            "asia",
            "africa",
            "oceania",
            "world",
        ]
    }
}

enum Group {
    Subregion(&'static str),
    Continent(&'static str),
    Region(&'static str),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loads_world_countries() {
        let gaz = Gazetteer::load().unwrap();
        assert!(gaz.countries().len() >= 170);
        assert_eq!(gaz.country("Nicaragua").unwrap().iso2, "NI");
        assert_eq!(gaz.country("NI").unwrap().iso2, "NI");
        assert_eq!(gaz.country("Belice").unwrap().iso2, "BZ");
    }

    #[test]
    fn central_america_has_seven_countries() {
        let gaz = Gazetteer::global();
        let countries = gaz.countries_in_group("centroamerica").unwrap();
        assert_eq!(countries.len(), 7);
        assert!(countries.iter().any(|c| c.iso2 == "NI"));
        assert!(countries.iter().any(|c| c.iso2 == "PA"));
        assert!(gaz.countries_in_group("world").unwrap().len() > 200);
    }

    #[test]
    fn calling_codes_are_central_american() {
        let gaz = Gazetteer::global();
        assert_eq!(gaz.country("NI").unwrap().calling_code, "+505");
        assert_eq!(gaz.country("GT").unwrap().calling_code, "+502");
        assert_eq!(gaz.country("CR").unwrap().calling_code, "+506");
    }
}
