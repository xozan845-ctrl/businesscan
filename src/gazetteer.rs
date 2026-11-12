//! Gacetero integrado de Nicaragua: 17 departamentos/regiones y 153 municipios.
//!
//! Sirve para validar y canonicalizar nombres escritos por el usuario. Los
//! barrios, comarcas y colonias no se incluyen porque cambian con frecuencia y
//! se resuelven en línea con OpenStreetMap/Nominatim.

use std::sync::OnceLock;

use serde::Deserialize;

use crate::normalize::{normalize_text, similarity};

const NICARAGUA_JSON: &str = include_str!("../assets/nicaragua.json");

#[derive(Debug, Clone, Deserialize)]
struct RawDepartment {
    department: String,
    municipalities: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct Department {
    pub name: String,
    pub norm: String,
    pub aliases: Vec<String>,
    pub municipalities: Vec<Municipality>,
}

#[derive(Debug, Clone)]
pub struct Municipality {
    pub name: String,
    pub norm: String,
    pub department: String,
}

#[derive(Debug, Clone)]
pub struct Gazetteer {
    departments: Vec<Department>,
}

impl Gazetteer {
    pub fn global() -> &'static Self {
        static GAZETTEER: OnceLock<Gazetteer> = OnceLock::new();
        GAZETTEER.get_or_init(|| Self::load().expect("gacetero de Nicaragua inválido"))
    }

    pub fn load() -> anyhow::Result<Self> {
        let raw: Vec<RawDepartment> = serde_json::from_str(NICARAGUA_JSON)?;
        let departments = raw
            .into_iter()
            .map(|dept| {
                let norm = normalize_text(&dept.department);
                let aliases = department_aliases(&norm)
                    .into_iter()
                    .map(normalize_text)
                    .collect();
                let municipalities = dept
                    .municipalities
                    .into_iter()
                    .map(|name| Municipality {
                        norm: normalize_text(&name),
                        name,
                        department: dept.department.clone(),
                    })
                    .collect();
                Department {
                    name: dept.department,
                    norm,
                    aliases,
                    municipalities,
                }
            })
            .collect();
        Ok(Self { departments })
    }

    pub fn departments(&self) -> &[Department] {
        &self.departments
    }

    pub fn department_names(&self) -> Vec<&str> {
        self.departments
            .iter()
            .map(|d| d.name.as_str())
            .collect::<Vec<_>>()
    }

    pub fn all_municipalities(&self) -> Vec<&Municipality> {
        self.departments
            .iter()
            .flat_map(|d| d.municipalities.iter())
            .collect()
    }

    /// Devuelve el nombre oficial del departamento o región.
    pub fn canonical_department(&self, input: &str) -> Option<&str> {
        let norm = normalize_text(strip_admin_prefix(input));
        if norm.is_empty() {
            return None;
        }
        for dept in &self.departments {
            if dept.norm == norm || dept.aliases.iter().any(|a| a == &norm) {
                return Some(&dept.name);
            }
        }
        None
    }

    /// Devuelve el departamento y municipio canónicos.
    ///
    /// Si se especifica departamento, solo busca dentro de él.
    pub fn canonical_municipality(
        &self,
        input: &str,
        department: Option<&str>,
    ) -> Option<(&str, &str)> {
        let norm = normalize_text(strip_admin_prefix(input));
        if norm.is_empty() {
            return None;
        }
        let dept_filter = department.and_then(|d| self.canonical_department(d));

        for dept in &self.departments {
            if let Some(filter) = dept_filter {
                if dept.name != filter {
                    continue;
                }
            }
            for municipality in &dept.municipalities {
                if municipality.norm == norm {
                    return Some((&dept.name, &municipality.name));
                }
            }
        }
        None
    }

    pub fn municipalities_of(&self, department: &str) -> Vec<&str> {
        let Some(canonical) = self.canonical_department(department) else {
            return Vec::new();
        };
        self.departments
            .iter()
            .find(|d| d.name == canonical)
            .map(|d| d.municipalities.iter().map(|m| m.name.as_str()).collect())
            .unwrap_or_default()
    }

    /// Sugerencias por parecido, para mensajes de error útiles.
    pub fn suggest_department(&self, input: &str) -> Option<&str> {
        let norm = normalize_text(input);
        self.departments
            .iter()
            .map(|d| (d, similarity(&norm, &d.norm)))
            .filter(|(_, score)| *score >= 0.72)
            .max_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(d, _)| d.name.as_str())
    }

    pub fn suggest_municipality(&self, input: &str) -> Option<(&str, &str)> {
        let norm = normalize_text(input);
        let mut best: Option<(&Municipality, f64)> = None;
        for dept in &self.departments {
            for municipality in &dept.municipalities {
                let score = similarity(&norm, &municipality.norm);
                if score >= 0.72
                    && best
                        .as_ref()
                        .map(|(_, current)| score > *current)
                        .unwrap_or(true)
                {
                    best = Some((municipality, score));
                }
            }
        }
        best.map(|(m, _)| (m.department.as_str(), m.name.as_str()))
    }
}

fn strip_admin_prefix(value: &str) -> &str {
    let mut value = value.trim();
    for prefix in [
        "Departamento de ",
        "departamento de ",
        "Departamento del ",
        "departamento del ",
        "Región Autónoma de la ",
        "Región Autónoma del ",
        "Region Autonoma de la ",
        "Region Autonoma del ",
        "Municipio de ",
        "municipio de ",
        "Municipio del ",
        "municipio del ",
    ] {
        if let Some(rest) = value.strip_prefix(prefix) {
            value = rest.trim();
        }
    }
    value
}

fn department_aliases(norm: &str) -> Vec<&'static str> {
    match norm {
        "atlantico norte" => vec![
            "RAAN",
            "Region Autonoma de la Costa Caribe Norte",
            "Costa Caribe Norte",
            "Caribe Norte",
        ],
        "atlantico sur" => vec![
            "RAAS",
            "Region Autonoma de la Costa Caribe Sur",
            "Costa Caribe Sur",
            "Caribe Sur",
        ],
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loads_all_departments_and_municipalities() {
        let gaz = Gazetteer::load().unwrap();
        assert_eq!(gaz.departments().len(), 17);
        assert_eq!(gaz.all_municipalities().len(), 153);
    }

    #[test]
    fn canonicalizes_accents_and_aliases() {
        let gaz = Gazetteer::global();
        assert_eq!(gaz.canonical_department("leon"), Some("León"));
        assert_eq!(
            gaz.canonical_department("Departamento de León"),
            Some("León")
        );
        assert_eq!(gaz.canonical_department("RAAN"), Some("Atlántico Norte"));
        assert_eq!(
            gaz.canonical_municipality("san jose de los remates", Some("Boaco")),
            Some(("Boaco", "San José de Los Remates"))
        );
    }

    #[test]
    fn suggests_close_names() {
        let gaz = Gazetteer::global();
        assert_eq!(gaz.suggest_department("Managu"), Some("Managua"));
        assert!(gaz.suggest_municipality("Jinotepe").is_some());
    }
}
