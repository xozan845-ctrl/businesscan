//! Clasificación de categorías y traducción a filtros de Overpass.

use std::collections::BTreeMap;

use crate::i18n::Language;
use crate::normalize::normalize_text;

pub const AMENITY_REGEX: &str = "restaurant|cafe|fast_food|bar|pub|food_court|ice_cream|bank|atm|bureau_de_change|pharmacy|clinic|hospital|dentist|doctors|veterinary|fuel|car_wash|car_rental|car_sharing|taxi|driving_school|marketplace|money_transfer|nightclub|casino|cinema|theatre|arts_centre|conference_centre|coworking_space|childcare|kindergarten|school|college|university|library|post_office|social_facility|studio";
pub const TOURISM_REGEX: &str = "hotel|hostel|guest_house|motel|apartment|resort|chalet|camp_site|caravan_site|attraction|museum|gallery|zoo|theme_park|information";
pub const LEISURE_REGEX: &str = "fitness_centre|sports_centre|swimming_pool|bowling_alley|dance|escape_game|amusement_arcade|water_park|marina|golf_course";

pub const BUSINESS_KEYS: &[&str] = &[
    "shop",
    "amenity",
    "office",
    "craft",
    "tourism",
    "healthcare",
    "leisure",
    "industrial",
    "company",
];

pub fn pick_category(tags: &BTreeMap<String, String>) -> Option<(String, String)> {
    for key in BUSINESS_KEYS {
        if let Some(value) = tags.get(*key) {
            if !value.trim().is_empty() {
                return Some(((*key).to_string(), value.clone()));
            }
        }
    }
    None
}

pub fn category_label(key: &str, value: &str, language: Language) -> String {
    let value = value.split(';').next().unwrap_or(value).trim();
    if language == Language::En {
        return english_label(key, value);
    }
    let label = match (key, value) {
        (_, "supermarket") => "Supermercado",
        (_, "convenience") => "Tienda de conveniencia",
        (_, "greengrocer") => "Verdulería",
        (_, "butcher") => "Carnicería",
        (_, "bakery") => "Panadería",
        (_, "hardware") => "Ferretería",
        (_, "doityourself") => "Materiales de construcción",
        (_, "paint") => "Pinturas",
        (_, "clothes") => "Ropa",
        (_, "shoes") => "Zapatería",
        (_, "jewelry") => "Joyería",
        (_, "optician") => "Óptica",
        (_, "furniture") => "Mueblería",
        (_, "electronics") => "Electrónica",
        (_, "mobile_phone") => "Celulares",
        (_, "computer") => "Computadoras",
        (_, "car_parts") => "Repuestos",
        (_, "car_repair") => "Taller mecánico",
        (_, "tyres") => "Llantas",
        (_, "beauty") => "Salón de belleza",
        (_, "hairdresser") => "Barbería / peluquería",
        (_, "massage") => "Masajes",
        (_, "laundry") => "Lavandería",
        (_, "books") => "Librería",
        (_, "stationery") => "Papelería",
        (_, "florist") => "Florería",
        (_, "gift") => "Regalos",
        (_, "department_store") => "Tienda por departamento",
        (_, "mall") => "Centro comercial",
        (_, "wholesale") => "Mayorista",
        (_, "alcohol") => "Licorería",
        (_, "tobacco") => "Tabacalera",
        (_, "pet") => "Mascotas",
        (_, "sports") => "Artículos deportivos",
        (_, "toys") => "Juguetes",
        (_, "photo") => "Fotografía",
        (_, "travel_agency") => "Agencia de viajes",
        (_, "insurance") => "Seguros",
        (_, "motorcycle") => "Motocicletas",
        (_, "bicycle") => "Bicicletas",
        (_, "agrarian") => "Agroservicios",
        (_, "restaurant") => "Restaurante",
        (_, "cafe") => "Cafetería",
        (_, "fast_food") => "Comida rápida",
        (_, "bar") => "Bar",
        (_, "pub") => "Pub",
        (_, "food_court") => "Patio de comidas",
        (_, "ice_cream") => "Heladería",
        (_, "bank") => "Banco",
        (_, "atm") => "Cajero automático",
        (_, "bureau_de_change") => "Casa de cambio",
        (_, "pharmacy") => "Farmacia",
        (_, "clinic") => "Clínica",
        (_, "hospital") => "Hospital",
        (_, "dentist") => "Dentista",
        (_, "doctors") => "Consultorio médico",
        (_, "veterinary") => "Veterinaria",
        (_, "fuel") => "Gasolinera",
        (_, "car_wash") => "Lavado de autos",
        (_, "car_rental") => "Alquiler de autos",
        (_, "driving_school") => "Escuela de manejo",
        (_, "marketplace") => "Mercado",
        (_, "money_transfer") => "Remesas",
        (_, "nightclub") => "Discoteca",
        (_, "casino") => "Casino",
        (_, "cinema") => "Cine",
        (_, "theatre") => "Teatro",
        (_, "coworking_space") => "Coworking",
        (_, "childcare") => "Guardería",
        (_, "kindergarten") => "Preescolar",
        (_, "school") => "Colegio",
        (_, "college") => "Instituto",
        (_, "university") => "Universidad",
        (_, "library") => "Biblioteca",
        (_, "post_office") => "Correos",
        (_, "hotel") => "Hotel",
        (_, "hostel") => "Hostal",
        (_, "guest_house") => "Hospedaje",
        (_, "motel") => "Motel",
        (_, "apartment") => "Apartamento turístico",
        (_, "resort") => "Resort",
        (_, "camp_site") => "Campamento",
        (_, "attraction") => "Atracción turística",
        (_, "museum") => "Museo",
        (_, "gallery") => "Galería",
        (_, "zoo") => "Zoológico",
        (_, "theme_park") => "Parque temático",
        (_, "information") => "Información turística",
        (_, "fitness_centre") => "Gimnasio",
        (_, "sports_centre") => "Centro deportivo",
        (_, "swimming_pool") => "Piscina",
        (_, "bowling_alley") => "Boliche",
        (_, "dance") => "Escuela de baile",
        (_, "escape_game") => "Escape room",
        (_, "amusement_arcade") => "Sala de juegos",
        (_, "water_park") => "Parque acuático",
        (_, "marina") => "Marina",
        (_, "golf_course") => "Campo de golf",
        (_, "lawyer") => "Abogado",
        (_, "accountant") => "Contador",
        (_, "estate_agent") => "Inmobiliaria",
        (_, "financial") => "Financiera",
        (_, "employment_agency") => "Agencia de empleo",
        (_, "it") => "Servicios informáticos",
        (_, "architect") => "Arquitecto",
        (_, "engineer") => "Ingeniero",
        (_, "company") => "Empresa",
        (_, "industrial") => "Industria",
        _ => "",
    };
    if !label.is_empty() {
        return label.to_string();
    }
    let pretty = value.replace('_', " ");
    let mut chars = pretty.chars();
    match chars.next() {
        Some(first) => format!("{}{}", first.to_uppercase(), chars.as_str()),
        None => key.to_string(),
    }
}

fn english_label(key: &str, value: &str) -> String {
    let label = match (key, value) {
        (_, "supermarket") => "Supermarket",
        (_, "convenience") => "Convenience store",
        (_, "greengrocer") => "Greengrocer",
        (_, "butcher") => "Butcher",
        (_, "bakery") => "Bakery",
        (_, "hardware") => "Hardware store",
        (_, "doityourself") => "DIY store",
        (_, "clothes") => "Clothing store",
        (_, "shoes") => "Shoe store",
        (_, "jewelry") => "Jewelry store",
        (_, "optician") => "Optician",
        (_, "furniture") => "Furniture store",
        (_, "electronics") => "Electronics store",
        (_, "mobile_phone") => "Mobile phone store",
        (_, "computer") => "Computer store",
        (_, "car_parts") => "Car parts",
        (_, "car_repair") => "Car repair",
        (_, "beauty") => "Beauty salon",
        (_, "hairdresser") => "Hairdresser",
        (_, "laundry") => "Laundry",
        (_, "books") => "Bookstore",
        (_, "stationery") => "Stationery store",
        (_, "florist") => "Florist",
        (_, "gift") => "Gift shop",
        (_, "restaurant") => "Restaurant",
        (_, "cafe") => "Café",
        (_, "fast_food") => "Fast food",
        (_, "bar") => "Bar",
        (_, "pub") => "Pub",
        (_, "bank") => "Bank",
        (_, "atm") => "ATM",
        (_, "pharmacy") => "Pharmacy",
        (_, "clinic") => "Clinic",
        (_, "hospital") => "Hospital",
        (_, "dentist") => "Dentist",
        (_, "doctors") => "Doctor's office",
        (_, "veterinary") => "Veterinarian",
        (_, "fuel") => "Gas station",
        (_, "marketplace") => "Market",
        (_, "nightclub") => "Nightclub",
        (_, "cinema") => "Cinema",
        (_, "theatre") => "Theater",
        (_, "school") => "School",
        (_, "university") => "University",
        (_, "library") => "Library",
        (_, "post_office") => "Post office",
        (_, "hotel") => "Hotel",
        (_, "hostel") => "Hostel",
        (_, "guest_house") => "Guest house",
        (_, "museum") => "Museum",
        (_, "fitness_centre") => "Gym",
        (_, "sports_centre") => "Sports center",
        (_, "swimming_pool") => "Swimming pool",
        (_, "lawyer") => "Lawyer",
        (_, "accountant") => "Accountant",
        (_, "estate_agent") => "Real estate agency",
        (_, "travel_agency") => "Travel agency",
        (_, "insurance") => "Insurance",
        (_, "company") => "Company",
        (_, "industrial") => "Industry",
        _ => "",
    };
    if !label.is_empty() {
        return label.to_string();
    }
    let pretty = value.replace('_', " ");
    let mut chars = pretty.chars();
    match chars.next() {
        Some(first) => format!("{}{}", first.to_uppercase(), chars.as_str()),
        None => key.to_string(),
    }
}

/// Traduce una categoría escrita por el usuario a `(clave, valor opcional)`.
pub fn osm_constraint(category: &str) -> Option<(String, Option<String>)> {
    let raw = category.trim();
    if raw.is_empty() {
        return None;
    }
    if let Some((key, value)) = raw.split_once('=') {
        return Some((key.trim().to_lowercase(), Some(value.trim().to_string())));
    }
    let norm = normalize_text(raw);
    let key_only = [
        "shop",
        "tienda",
        "tiendas",
        "comercio",
        "comercios",
        "amenity",
        "servicio",
        "servicios",
        "office",
        "oficina",
        "oficinas",
        "craft",
        "artesania",
        "tourism",
        "turismo",
        "healthcare",
        "salud",
        "leisure",
        "recreacion",
        "industrial",
        "industria",
    ];
    if key_only.contains(&norm.as_str()) {
        let key = match norm.as_str() {
            "tienda" | "tiendas" | "comercio" | "comercios" => "shop",
            "servicio" | "servicios" | "amenity" => "amenity",
            "oficina" | "oficinas" => "office",
            "artesania" => "craft",
            "turismo" => "tourism",
            "salud" => "healthcare",
            "recreacion" => "leisure",
            "industria" => "industrial",
            other => other,
        };
        return Some((key.to_string(), None));
    }

    let mapping: &[(&[&str], &str, &str)] = &[
        (
            &["restaurante", "restaurantes", "comida", "comedor"],
            "amenity",
            "restaurant",
        ),
        (
            &["cafe", "cafeteria", "cafeterias", "café"],
            "amenity",
            "cafe",
        ),
        (
            &["comida rapida", "fast food", "pizza", "pupuseria"],
            "amenity",
            "fast_food",
        ),
        (&["bar", "bares", "cerveza"], "amenity", "bar"),
        (&["heladeria", "helados"], "amenity", "ice_cream"),
        (&["banco", "bancos"], "amenity", "bank"),
        (&["cajero", "cajeros", "atm"], "amenity", "atm"),
        (&["farmacia", "farmacias"], "amenity", "pharmacy"),
        (&["clinica", "clinicas", "consultorio"], "amenity", "clinic"),
        (&["hospital", "hospitales"], "amenity", "hospital"),
        (
            &["dentista", "dentistas", "odontologia"],
            "amenity",
            "dentist",
        ),
        (&["medico", "medicos", "doctor"], "amenity", "doctors"),
        (&["veterinaria", "veterinarias"], "amenity", "veterinary"),
        (
            &["gasolinera", "gasolineras", "combustible"],
            "amenity",
            "fuel",
        ),
        (&["lavado de autos", "car wash"], "amenity", "car_wash"),
        (
            &["alquiler de autos", "renta de autos"],
            "amenity",
            "car_rental",
        ),
        (&["mercado", "mercados"], "amenity", "marketplace"),
        (&["remesas", "money transfer"], "amenity", "money_transfer"),
        (
            &["discoteca", "discotecas", "antro"],
            "amenity",
            "nightclub",
        ),
        (&["casino", "casinos"], "amenity", "casino"),
        (&["cine", "cines", "cinema"], "amenity", "cinema"),
        (&["teatro", "teatros"], "amenity", "theatre"),
        (&["coworking"], "amenity", "coworking_space"),
        (&["guarderia", "guarderias"], "amenity", "childcare"),
        (&["preescolar", "kinder"], "amenity", "kindergarten"),
        (
            &["colegio", "colegios", "escuela", "escuelas"],
            "amenity",
            "school",
        ),
        (&["universidad", "universidades"], "amenity", "university"),
        (&["biblioteca", "bibliotecas"], "amenity", "library"),
        (&["correo", "correos"], "amenity", "post_office"),
        (&["supermercado", "supermercados"], "shop", "supermarket"),
        (
            &["pulperia", "pulperias", "abarroteria", "miscelanea"],
            "shop",
            "convenience",
        ),
        (&["ferreteria", "ferreterias"], "shop", "hardware"),
        (&["materiales", "construccion"], "shop", "doityourself"),
        (&["panaderia", "panaderias"], "shop", "bakery"),
        (&["carniceria", "carnicerias"], "shop", "butcher"),
        (&["ropa", "boutique", "tienda de ropa"], "shop", "clothes"),
        (&["zapateria", "zapatos"], "shop", "shoes"),
        (&["joyeria", "joyas"], "shop", "jewelry"),
        (&["optica", "opticas", "lentes"], "shop", "optician"),
        (&["muebleria", "muebles"], "shop", "furniture"),
        (&["electronica", "electronicos"], "shop", "electronics"),
        (
            &["celular", "celulares", "telefonia"],
            "shop",
            "mobile_phone",
        ),
        (
            &["computadora", "computadoras", "informatica"],
            "shop",
            "computer",
        ),
        (&["repuestos", "autopartes"], "shop", "car_parts"),
        (
            &["taller", "talleres", "taller mecanico", "mecanico"],
            "shop",
            "car_repair",
        ),
        (&["llantas", "neumaticos"], "shop", "tyres"),
        (&["salon de belleza", "salon", "belleza"], "shop", "beauty"),
        (
            &["barberia", "barberias", "peluqueria", "peluquerias"],
            "shop",
            "hairdresser",
        ),
        (&["lavanderia", "lavanderias"], "shop", "laundry"),
        (&["libreria", "librerias"], "shop", "books"),
        (&["papeleria", "papelerias"], "shop", "stationery"),
        (&["floreria", "floristeria", "flores"], "shop", "florist"),
        (&["regalos", "regaleria"], "shop", "gift"),
        (&["licoreria", "licores"], "shop", "alcohol"),
        (&["mascotas", "pet shop"], "shop", "pet"),
        (&["deportes", "articulos deportivos"], "shop", "sports"),
        (&["juguetes", "jugueteria"], "shop", "toys"),
        (&["fotografia", "foto"], "shop", "photo"),
        (&["agencia de viajes", "viajes"], "office", "travel_agency"),
        (&["seguros", "seguro"], "office", "insurance"),
        (&["abogado", "abogados", "notaria"], "office", "lawyer"),
        (
            &["contador", "contadores", "contabilidad"],
            "office",
            "accountant",
        ),
        (&["inmobiliaria", "bienes raices"], "office", "estate_agent"),
        (&["hotel", "hoteles"], "tourism", "hotel"),
        (
            &["hostal", "hostales", "hospedaje"],
            "tourism",
            "guest_house",
        ),
        (&["motel", "moteles"], "tourism", "motel"),
        (
            &["turismo", "atraccion", "atracciones"],
            "tourism",
            "attraction",
        ),
        (&["museo", "museos"], "tourism", "museum"),
        (&["galeria", "galerias"], "tourism", "gallery"),
        (
            &["gimnasio", "gimnasios", "fitness"],
            "leisure",
            "fitness_centre",
        ),
        (&["piscina", "piscinas"], "leisure", "swimming_pool"),
        (&["boliche", "bolos"], "leisure", "bowling_alley"),
        (&["escuela de baile", "baile"], "leisure", "dance"),
        (&["escape room"], "leisure", "escape_game"),
        (&["parque acuatico"], "leisure", "water_park"),
        (&["marina"], "leisure", "marina"),
        (&["campo de golf", "golf"], "leisure", "golf_course"),
        (
            &["industria", "fabrica", "manufactura"],
            "industrial",
            "factory",
        ),
    ];

    for (aliases, key, value) in mapping {
        if aliases.contains(&norm.as_str()) {
            return Some(((*key).to_string(), Some((*value).to_string())));
        }
    }
    None
}

/// Si la categoría no se pudo traducir, se filtra del lado del cliente.
pub fn matches_category_text(
    category: Option<&str>,
    business_type: Option<&str>,
    categories: &[String],
    name: &str,
    term: &str,
) -> bool {
    let term = normalize_text(term);
    if term.is_empty() {
        return true;
    }
    let haystack = [
        category.unwrap_or(""),
        business_type.unwrap_or(""),
        name,
        &categories.join(" "),
    ]
    .join(" ");
    let haystack = normalize_text(&haystack);
    term.split_whitespace()
        .all(|token| haystack.contains(token))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_spanish_categories() {
        assert_eq!(
            osm_constraint("ferretería"),
            Some(("shop".into(), Some("hardware".into())))
        );
        assert_eq!(
            osm_constraint("restaurantes"),
            Some(("amenity".into(), Some("restaurant".into())))
        );
        assert_eq!(
            osm_constraint("shop=supermarket"),
            Some(("shop".into(), Some("supermarket".into())))
        );
        assert!(osm_constraint("algo raro").is_none());
    }

    #[test]
    fn labels_are_human_readable() {
        assert_eq!(
            category_label("shop", "hardware", Language::Es),
            "Ferretería"
        );
        assert_eq!(
            category_label("amenity", "restaurant", Language::Es),
            "Restaurante"
        );
        assert_eq!(
            category_label("shop", "custom_thing", Language::Es),
            "Custom thing"
        );
    }
}
