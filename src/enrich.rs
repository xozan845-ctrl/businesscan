//! Enriquecimiento respetuoso desde el sitio web del negocio.
//!
//! Extrae JSON-LD (`LocalBusiness`), metaetiquetas, enlaces `mailto:`/`tel:`,
//! WhatsApp y redes sociales. Respeta `robots.txt`, limita la velocidad por
//! host, no sigue enlaces fuera del sitio y limita el tamaño de las páginas.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::net::IpAddr;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use chrono::Utc;
use regex::Regex;
use scraper::{Html, Selector};
use serde_json::{json, Value};
use tokio::time::Instant;
use url::Url;

use crate::http::HttpClient;
use crate::model::{Address, Business, BusinessRecord, Contact, ContactKind};
use crate::normalize::{
    domain_of, normalize_email, normalize_phone, normalize_social, normalize_text, normalize_url,
};

const MAX_BODY_BYTES: usize = 2 * 1024 * 1024;

#[derive(Debug, Clone, Default)]
pub struct EnrichFacts {
    pub url: String,
    pub pages: usize,
    pub contacts: usize,
    pub had_jsonld: bool,
    pub had_phone: bool,
    pub had_email: bool,
    pub had_address: bool,
}

pub struct WebEnricher {
    http: HttpClient,
    delay: Duration,
    robots: HashMap<String, RobotsRules>,
    last_request: HashMap<String, Instant>,
}

impl WebEnricher {
    pub fn new(http: HttpClient, delay: Duration) -> Self {
        Self {
            http,
            delay,
            robots: HashMap::new(),
            last_request: HashMap::new(),
        }
    }

    pub async fn enrich(
        &mut self,
        business: &Business,
        website: &str,
        max_pages: usize,
    ) -> Result<(BusinessRecord, EnrichFacts)> {
        let homepage =
            normalize_url(website).with_context(|| format!("Sitio web inválido: «{website}»"))?;
        let homepage_url = Url::parse(&homepage)?;
        ensure_public_url(&homepage_url).await?;

        let max_pages = max_pages.clamp(1, 5);
        let mut queue: Vec<(Url, bool)> = vec![(homepage_url.clone(), true)];
        let mut seen: HashSet<String> = HashSet::new();
        let mut pages: Vec<String> = Vec::new();
        let mut extracted = Extracted::default();
        let mut index = 0;

        while index < queue.len() && pages.len() < max_pages {
            let (page_url, is_home) = queue[index].clone();
            index += 1;
            if !seen.insert(page_url.to_string()) {
                continue;
            }

            let rules = self.robots_for(&page_url).await?;
            if !rules.allows(&path_and_query(&page_url)) {
                if is_home {
                    bail!("robots.txt no permite visitar {}", page_url);
                }
                log::debug!("robots.txt bloquea {}", page_url);
                continue;
            }

            let html = match self.fetch_page(&page_url, rules.crawl_delay).await {
                Ok(html) => html,
                Err(error) if is_home => return Err(error),
                Err(error) => {
                    log::debug!("No se pudo leer {}: {error:#}", page_url);
                    continue;
                }
            };
            pages.push(page_url.to_string());
            let parsed = parse_page(&html, &page_url);
            for link in parsed.contact_links {
                if queue.len() + pages.len() < max_pages + 1
                    && same_origin(&link, &homepage_url)
                    && !seen.contains(link.as_str())
                {
                    queue.push((link, false));
                }
            }
            extracted.merge(parsed.extracted);
        }

        if pages.is_empty() {
            bail!("No se pudo leer ninguna página de {}", homepage);
        }

        let had_jsonld = !extracted.jsonld.is_empty();
        let record = build_record(business, &homepage, &pages, &extracted);
        let facts = EnrichFacts {
            url: homepage,
            pages: pages.len(),
            contacts: record.contacts.len(),
            had_jsonld,
            had_phone: record
                .contacts
                .iter()
                .any(|contact| contact.kind.is_phone_like()),
            had_email: record
                .contacts
                .iter()
                .any(|contact| contact.kind == ContactKind::Email),
            had_address: !record.address.is_empty(),
        };
        Ok((record, facts))
    }

    async fn fetch_page(&mut self, url: &Url, crawl_delay: Option<Duration>) -> Result<String> {
        let origin = origin_key(url);
        self.wait_origin(&origin, crawl_delay).await;

        let response = self
            .http
            .raw()
            .get(url.clone())
            .timeout(Duration::from_secs(30))
            .send()
            .await
            .with_context(|| format!("Error HTTP al leer {url}"))?;

        let status = response.status();
        if !status.is_success() {
            bail!("HTTP {status} en {url}");
        }

        let final_url = response.url().clone();
        ensure_public_url(&final_url).await?;
        if origin_key(&final_url) != origin {
            let rules = self.robots_for(&final_url).await?;
            if !rules.allows(&path_and_query(&final_url)) {
                bail!("robots.txt bloquea la redirección a {final_url}");
            }
        }

        let content_type = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .unwrap_or("")
            .to_lowercase();
        if !content_type.is_empty()
            && !content_type.contains("html")
            && !content_type.contains("text/plain")
            && !content_type.contains("xml")
        {
            bail!("Contenido no HTML en {final_url}: {content_type}");
        }

        let bytes = read_limited(response, MAX_BODY_BYTES).await?;
        Ok(String::from_utf8_lossy(&bytes).to_string())
    }

    async fn robots_for(&mut self, url: &Url) -> Result<RobotsRules> {
        let origin = origin_key(url);
        if let Some(rules) = self.robots.get(&origin) {
            return Ok(rules.clone());
        }

        self.wait_origin(&origin, None).await;
        let robots_url = format!("{origin}/robots.txt");
        let response = self
            .http
            .raw()
            .get(&robots_url)
            .timeout(Duration::from_secs(15))
            .send()
            .await;

        let rules = match response {
            Ok(response) => {
                let status = response.status();
                if status.is_success() {
                    let bytes = read_limited(response, 512 * 1024).await.unwrap_or_default();
                    let body = String::from_utf8_lossy(&bytes);
                    parse_robots(&body)
                } else if status == reqwest::StatusCode::NOT_FOUND
                    || status == reqwest::StatusCode::GONE
                {
                    RobotsRules::allow_all()
                } else {
                    RobotsRules::disallow_all()
                }
            }
            Err(error) => {
                bail!("No se pudo leer {robots_url}: {error}");
            }
        };

        self.robots.insert(origin, rules.clone());
        Ok(rules)
    }

    async fn wait_origin(&mut self, origin: &str, crawl_delay: Option<Duration>) {
        let interval = self
            .delay
            .max(crawl_delay.unwrap_or_default())
            .min(Duration::from_secs(30));
        if let Some(last) = self.last_request.get(origin) {
            let elapsed = last.elapsed();
            if elapsed < interval {
                tokio::time::sleep(interval - elapsed).await;
            }
        }
        self.last_request.insert(origin.to_string(), Instant::now());
    }
}

async fn read_limited(mut response: reqwest::Response, limit: usize) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await? {
        let remaining = limit.saturating_sub(bytes.len());
        if remaining == 0 {
            break;
        }
        if chunk.len() > remaining {
            bytes.extend_from_slice(&chunk[..remaining]);
            break;
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

fn origin_key(url: &Url) -> String {
    url.origin().ascii_serialization()
}

fn path_and_query(url: &Url) -> String {
    match url.query() {
        Some(query) => format!("{}?{query}", url.path()),
        None => url.path().to_string(),
    }
}

fn same_origin(a: &Url, b: &Url) -> bool {
    origin_key(a) == origin_key(b)
}

async fn ensure_public_url(url: &Url) -> Result<()> {
    if url.scheme() != "http" && url.scheme() != "https" {
        bail!("Solo se permiten enlaces HTTP/HTTPS");
    }
    let host = url.host_str().context("URL sin host")?;
    if host.eq_ignore_ascii_case("localhost") || host.ends_with(".local") {
        bail!("No se permiten hosts locales");
    }
    if let Ok(ip) = host.parse::<IpAddr>() {
        if !is_public_ip(ip) {
            bail!("No se permiten direcciones IP privadas");
        }
        return Ok(());
    }
    let port = url.port_or_known_default().unwrap_or(80);
    let addrs = tokio::net::lookup_host((host, port))
        .await
        .with_context(|| format!("No se pudo resolver {host}"))?;
    let mut found = false;
    for addr in addrs {
        found = true;
        if !is_public_ip(addr.ip()) {
            bail!("{host} resuelve a una IP privada o reservada");
        }
    }
    if !found {
        bail!("{host} no tiene direcciones");
    }
    Ok(())
}

fn is_public_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            !(v4.is_private()
                || v4.is_loopback()
                || v4.is_link_local()
                || v4.is_unspecified()
                || v4.is_broadcast()
                || v4.is_multicast())
        }
        IpAddr::V6(v6) => {
            let first = v6.segments()[0];
            !(v6.is_loopback()
                || v6.is_unspecified()
                || v6.is_multicast()
                || (first & 0xfe00) == 0xfc00
                || (first & 0xffc0) == 0xfe80)
        }
    }
}

#[derive(Debug, Clone)]
struct RobotsRules {
    rules: Vec<(bool, String)>,
    crawl_delay: Option<Duration>,
    default_allow: bool,
}

impl RobotsRules {
    fn allow_all() -> Self {
        Self {
            rules: Vec::new(),
            crawl_delay: None,
            default_allow: true,
        }
    }

    fn disallow_all() -> Self {
        Self {
            rules: Vec::new(),
            crawl_delay: None,
            default_allow: false,
        }
    }

    fn allows(&self, path: &str) -> bool {
        if self.rules.is_empty() {
            return self.default_allow;
        }
        let mut best: Option<(usize, bool)> = None;
        for (allow, pattern) in &self.rules {
            if rule_matches(pattern, path) {
                let length = pattern.len();
                if best
                    .map(|(best_length, _)| length >= best_length)
                    .unwrap_or(true)
                {
                    best = Some((length, *allow));
                }
            }
        }
        best.map(|(_, allow)| allow).unwrap_or(self.default_allow)
    }
}

#[derive(Debug, Default)]
struct RobotsGroup {
    agents: Vec<String>,
    rules: Vec<(bool, String)>,
    crawl_delay: Option<Duration>,
}

fn parse_robots(body: &str) -> RobotsRules {
    let mut groups: Vec<RobotsGroup> = Vec::new();
    let mut current = RobotsGroup::default();
    let mut seen_rule = false;

    for line in body.lines() {
        let line = line.split('#').next().unwrap_or("").trim();
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let key = key.trim().to_lowercase();
        let value = value.trim();
        match key.as_str() {
            "user-agent" => {
                if seen_rule {
                    groups.push(std::mem::take(&mut current));
                    seen_rule = false;
                }
                current.agents.push(value.to_lowercase());
            }
            "allow" | "disallow" => {
                if value.is_empty() {
                    continue;
                }
                current.rules.push((key == "allow", value.to_string()));
                seen_rule = true;
            }
            "crawl-delay" => {
                if let Ok(seconds) = value.parse::<f64>() {
                    current.crawl_delay = Some(Duration::from_secs_f64(seconds.clamp(0.0, 30.0)));
                    seen_rule = true;
                }
            }
            _ => {}
        }
    }
    if !current.agents.is_empty() {
        groups.push(current);
    }

    let specific = groups.iter().find(|group| {
        group
            .agents
            .iter()
            .any(|agent| agent != "*" && "nicascan".contains(agent.as_str()))
    });
    let wildcard = groups
        .iter()
        .find(|group| group.agents.iter().any(|agent| agent == "*"));
    let Some(group) = specific.or(wildcard) else {
        return RobotsRules::allow_all();
    };

    RobotsRules {
        rules: group.rules.clone(),
        crawl_delay: group.crawl_delay,
        default_allow: true,
    }
}

fn rule_matches(pattern: &str, path: &str) -> bool {
    let (pattern, anchored) = match pattern.strip_suffix('$') {
        Some(value) => (value, true),
        None => (pattern, false),
    };
    if pattern.is_empty() {
        return true;
    }
    let mut regex = String::from("^");
    for ch in pattern.chars() {
        if ch == '*' {
            regex.push_str(".*");
        } else {
            regex.push_str(&regex::escape(&ch.to_string()));
        }
    }
    if anchored {
        regex.push('$');
    } else {
        regex.push_str(".*");
    }
    Regex::new(&regex)
        .map(|compiled| compiled.is_match(path))
        .unwrap_or(false)
}

#[derive(Debug, Default, Clone)]
struct Extracted {
    names: Vec<String>,
    legal_names: Vec<String>,
    descriptions: Vec<String>,
    phones: Vec<String>,
    whatsapps: Vec<String>,
    emails: Vec<String>,
    websites: Vec<String>,
    socials: Vec<(ContactKind, String)>,
    address: Address,
    latitude: Option<f64>,
    longitude: Option<f64>,
    opening_hours: Vec<String>,
    price_range: Option<String>,
    rating: Option<f64>,
    review_count: Option<i64>,
    schema_types: Vec<String>,
    meta: BTreeMap<String, String>,
    jsonld: Vec<Value>,
    contact_links: Vec<Url>,
}

impl Extracted {
    fn merge(&mut self, other: Extracted) {
        push_unique(&mut self.names, other.names);
        push_unique(&mut self.legal_names, other.legal_names);
        push_unique(&mut self.descriptions, other.descriptions);
        push_unique(&mut self.phones, other.phones);
        push_unique(&mut self.whatsapps, other.whatsapps);
        push_unique(&mut self.emails, other.emails);
        push_unique(&mut self.websites, other.websites);
        for (kind, value) in other.socials {
            if !self.socials.iter().any(|(k, v)| *k == kind && v == &value) {
                self.socials.push((kind, value));
            }
        }
        fill_address(&mut self.address, other.address);
        self.latitude = self.latitude.or(other.latitude);
        self.longitude = self.longitude.or(other.longitude);
        push_unique(&mut self.opening_hours, other.opening_hours);
        self.price_range = self.price_range.clone().or(other.price_range);
        self.rating = self.rating.or(other.rating);
        self.review_count = self.review_count.or(other.review_count);
        push_unique(&mut self.schema_types, other.schema_types);
        for (key, value) in other.meta {
            self.meta.entry(key).or_insert(value);
        }
        if self.jsonld.len() < 20 {
            for value in other.jsonld {
                if self.jsonld.len() >= 20 {
                    break;
                }
                self.jsonld.push(value);
            }
        }
        for link in other.contact_links {
            if !self
                .contact_links
                .iter()
                .any(|existing| existing.as_str() == link.as_str())
            {
                self.contact_links.push(link);
            }
        }
    }
}

struct ParsedPage {
    extracted: Extracted,
    contact_links: Vec<Url>,
}

fn parse_page(html: &str, page_url: &Url) -> ParsedPage {
    let document = Html::parse_document(html);
    let mut extracted = Extracted::default();
    let mut contact_links: Vec<Url> = Vec::new();

    let jsonld_selector = Selector::parse("script[type='application/ld+json']").unwrap();
    for element in document.select(&jsonld_selector) {
        let text = element.text().collect::<String>();
        let text = text.trim();
        if text.is_empty() {
            continue;
        }
        if let Ok(value) = serde_json::from_str::<Value>(text) {
            walk_jsonld(&value, &mut extracted, 0);
        }
    }

    let meta_selector = Selector::parse("meta").unwrap();
    for element in document.select(&meta_selector) {
        let Some(key) = element
            .value()
            .attr("property")
            .or_else(|| element.value().attr("name"))
            .map(|value| value.trim().to_lowercase())
        else {
            continue;
        };
        let Some(content) = element.value().attr("content") else {
            continue;
        };
        let content = content.trim();
        if content.is_empty() {
            continue;
        }
        extracted
            .meta
            .entry(key.clone())
            .or_insert_with(|| content.to_string());
        match key.as_str() {
            "og:site_name" | "application-name" => {
                push_unique(&mut extracted.names, vec![content.to_string()])
            }
            "og:title" => {
                if extracted.names.is_empty() {
                    extracted.names.push(content.to_string());
                }
            }
            "description" | "og:description" | "twitter:description" => {
                push_unique(&mut extracted.descriptions, vec![content.to_string()]);
            }
            "og:phone_number" | "business:contact_data:phone_number" | "contact:phone" => {
                push_unique(&mut extracted.phones, vec![content.to_string()]);
            }
            "og:email" | "business:contact_data:email" | "contact:email" => {
                push_unique(&mut extracted.emails, vec![content.to_string()]);
            }
            "business:contact_data:street_address" => {
                if extracted.address.line.is_none() {
                    extracted.address.line = Some(content.to_string());
                }
            }
            "business:contact_data:locality" => {
                if extracted.address.municipio.is_none() {
                    extracted.address.municipio = Some(content.to_string());
                }
            }
            "business:contact_data:region" => {
                if extracted.address.departamento.is_none() {
                    extracted.address.departamento = Some(content.to_string());
                }
            }
            "business:contact_data:postal_code" => {
                if extracted.address.postal_code.is_none() {
                    extracted.address.postal_code = Some(content.to_string());
                }
            }
            "business:contact_data:country_name" => {
                extracted
                    .address
                    .country_code
                    .get_or_insert_with(|| content.to_string());
            }
            _ => {}
        }
    }

    let anchor_selector = Selector::parse("a[href]").unwrap();
    for element in document.select(&anchor_selector) {
        let Some(href) = element.value().attr("href") else {
            continue;
        };
        let href = href.trim();
        let lower = href.to_lowercase();
        if let Some(rest) = lower.strip_prefix("mailto:") {
            if let Some(email) = normalize_email(rest.split('?').next().unwrap_or(rest)) {
                push_unique(&mut extracted.emails, vec![email]);
            }
            continue;
        }
        if let Some(rest) = lower.strip_prefix("tel:") {
            if normalize_phone(rest).is_some() {
                push_unique(&mut extracted.phones, vec![rest.to_string()]);
            }
            continue;
        }
        if lower.contains("wa.me/") || lower.contains("whatsapp.com/send") {
            if let Some(phone) = extract_whatsapp_phone(href) {
                push_unique(&mut extracted.whatsapps, vec![phone]);
            }
        }
        let Ok(absolute) = page_url.join(href) else {
            continue;
        };
        if absolute.scheme() != "http" && absolute.scheme() != "https" {
            continue;
        }
        if let Some(kind) = social_kind(&absolute) {
            if let Some(value) = normalize_social(kind.as_str(), absolute.as_str()) {
                if !extracted
                    .socials
                    .iter()
                    .any(|(k, v)| *k == kind && v == &value)
                {
                    extracted.socials.push((kind, value));
                }
            }
            continue;
        }
        if same_origin(&absolute, page_url)
            && is_contact_link(&absolute)
            && !contact_links
                .iter()
                .any(|existing| existing.as_str() == absolute.as_str())
        {
            contact_links.push(absolute);
        }
    }

    // Correos y teléfonos sueltos en el HTML, con filtros para evitar ruido.
    let email_regex = Regex::new(r"(?i)[a-z0-9._%+\-]+@[a-z0-9.\-]+\.[a-z]{2,}").unwrap();
    for capture in email_regex.find_iter(html) {
        let candidate = capture.as_str().trim_matches('.');
        if let Some(email) = normalize_email(candidate) {
            if !is_noise_email(&email) {
                push_unique(&mut extracted.emails, vec![email]);
            }
        }
    }
    if extracted.phones.is_empty() {
        let phone_regex = Regex::new(r"\+505[\s\-\.]?\(?\d{4}\)?[\s\-\.]?\d{4}").unwrap();
        for capture in phone_regex.find_iter(html) {
            push_unique(&mut extracted.phones, vec![capture.as_str().to_string()]);
        }
    }

    // Título como último recurso para el nombre.
    if extracted.names.is_empty() {
        let title_selector = Selector::parse("title").unwrap();
        if let Some(title) = document.select(&title_selector).next() {
            let title = title.text().collect::<String>();
            let title = title.trim();
            if !title.is_empty() {
                extracted.names.push(title.to_string());
            }
        }
    }

    ParsedPage {
        extracted,
        contact_links,
    }
}

fn walk_jsonld(value: &Value, extracted: &mut Extracted, depth: usize) {
    if depth > 8 {
        return;
    }
    match value {
        Value::Array(values) => {
            for value in values {
                walk_jsonld(value, extracted, depth + 1);
            }
        }
        Value::Object(object) => {
            let types = json_strings(object.get("@type"));
            let looks_like_business = types.iter().any(|kind| is_business_schema_type(kind))
                || object.get("telephone").is_some()
                || object.get("address").is_some();
            if looks_like_business {
                extract_jsonld_object(object, extracted);
            }
            if let Some(graph) = object.get("@graph") {
                walk_jsonld(graph, extracted, depth + 1);
            }
            for (key, value) in object {
                if key != "@graph" && (value.is_array() || value.is_object()) {
                    walk_jsonld(value, extracted, depth + 1);
                }
            }
        }
        _ => {}
    }
}

fn extract_jsonld_object(object: &serde_json::Map<String, Value>, extracted: &mut Extracted) {
    for key in ["name", "legalName"] {
        let values = json_strings(object.get(key));
        if key == "name" {
            push_unique(&mut extracted.names, values);
        } else {
            push_unique(&mut extracted.legal_names, values);
        }
    }
    push_unique(
        &mut extracted.descriptions,
        json_strings(object.get("description")),
    );
    push_unique(&mut extracted.phones, json_strings(object.get("telephone")));
    push_unique(&mut extracted.emails, json_strings(object.get("email")));
    push_unique(&mut extracted.websites, json_strings(object.get("url")));
    for value in json_strings(object.get("sameAs")) {
        let Ok(url) = Url::parse(&value) else {
            continue;
        };
        if let Some(kind) = social_kind(&url) {
            if let Some(normalized) = normalize_social(kind.as_str(), &value) {
                if !extracted
                    .socials
                    .iter()
                    .any(|(k, v)| *k == kind && v == &normalized)
                {
                    extracted.socials.push((kind, normalized));
                }
            }
        }
    }

    if let Some(address) = object.get("address") {
        if let Some(address) = address.as_object() {
            if let Some(value) = json_first_string(address.get("streetAddress")) {
                extracted.address.line.get_or_insert(value);
            }
            if let Some(value) = json_first_string(address.get("addressLocality")) {
                extracted.address.municipio.get_or_insert(value);
            }
            if let Some(value) = json_first_string(address.get("addressRegion")) {
                extracted.address.departamento.get_or_insert(value);
            }
            if let Some(value) = json_first_string(address.get("postalCode")) {
                extracted.address.postal_code.get_or_insert(value);
            }
            if let Some(value) = address.get("addressCountry") {
                let value = match value {
                    Value::String(value) => Some(value.clone()),
                    Value::Object(object) => json_first_string(object.get("name")),
                    _ => None,
                };
                if let Some(value) = value {
                    extracted.address.country_code.get_or_insert(value);
                }
            }
        } else if let Some(value) = address.as_str() {
            extracted.address.line.get_or_insert(value.to_string());
        }
    }

    if let Some(geo) = object.get("geo").and_then(Value::as_object) {
        let lat = json_number(geo.get("latitude"));
        let lon = json_number(geo.get("longitude"));
        if let (Some(lat), Some(lon)) = (lat, lon) {
            if in_nicaragua(lat, lon) {
                extracted.latitude.get_or_insert(lat);
                extracted.longitude.get_or_insert(lon);
            }
        }
    }

    if let Some(hours) = object.get("openingHours") {
        for value in json_strings(Some(hours)) {
            push_unique(&mut extracted.opening_hours, vec![value]);
        }
    }
    if let Some(specs) = object.get("openingHoursSpecification") {
        let values: Vec<&Value> = match specs {
            Value::Array(values) => values.iter().collect(),
            other => vec![other],
        };
        for spec in values {
            let Some(spec) = spec.as_object() else {
                continue;
            };
            let days = {
                let mut days = json_strings(spec.get("dayOfWeek"));
                if days.is_empty() {
                    days = json_strings(spec.get("dayOfWeek"));
                }
                days.into_iter()
                    .map(|day| short_day(&day))
                    .collect::<Vec<_>>()
                    .join(",")
            };
            let opens = json_first_string(spec.get("opens")).unwrap_or_default();
            let closes = json_first_string(spec.get("closes")).unwrap_or_default();
            if !days.is_empty() || !opens.is_empty() || !closes.is_empty() {
                push_unique(
                    &mut extracted.opening_hours,
                    vec![format!("{days} {opens}-{closes}").trim().to_string()],
                );
            }
        }
    }

    if let Some(price) = json_first_string(object.get("priceRange")) {
        extracted.price_range.get_or_insert(price);
    }
    if let Some(rating) = object.get("aggregateRating").and_then(Value::as_object) {
        if let Some(value) = json_number(rating.get("ratingValue")) {
            extracted.rating.get_or_insert(value);
        }
        if let Some(value) = json_number(rating.get("reviewCount")) {
            extracted.review_count.get_or_insert(value as i64);
        }
    }

    let types = json_strings(object.get("@type"));
    for kind in types {
        if !extracted.schema_types.contains(&kind) {
            extracted.schema_types.push(kind);
        }
    }
}

fn build_record(
    business: &Business,
    homepage: &str,
    pages: &[String],
    extracted: &Extracted,
) -> BusinessRecord {
    let mut contacts = Vec::new();
    let mut first_phone = true;
    for value in &extracted.phones {
        if let Some(normalized) = normalize_phone(value) {
            let mut contact = Contact::new(ContactKind::Phone, normalized).with_source("web");
            contact.label = Some("Teléfono (web)".to_string());
            if first_phone {
                contact = contact.primary();
                first_phone = false;
            }
            contacts.push(contact);
        }
    }
    let mut first_whatsapp = true;
    for value in &extracted.whatsapps {
        if let Some(normalized) = normalize_phone(value) {
            let mut contact = Contact::new(ContactKind::Whatsapp, normalized).with_source("web");
            if first_whatsapp {
                contact = contact.primary();
                first_whatsapp = false;
            }
            contacts.push(contact);
        }
    }
    let mut first_email = true;
    for value in &extracted.emails {
        if let Some(normalized) = normalize_email(value) {
            let mut contact = Contact::new(ContactKind::Email, normalized).with_source("web");
            if first_email {
                contact = contact.primary();
                first_email = false;
            }
            contacts.push(contact);
        }
    }
    let mut first_website = true;
    let mut websites = extracted.websites.clone();
    websites.push(homepage.to_string());
    for value in websites {
        if let Some(normalized) = normalize_url(&value) {
            if contacts
                .iter()
                .any(|contact| contact.kind == ContactKind::Website && contact.value == normalized)
            {
                continue;
            }
            let mut contact = Contact::new(ContactKind::Website, normalized).with_source("web");
            if first_website {
                contact = contact.primary();
                first_website = false;
            }
            contacts.push(contact);
        }
    }
    for (kind, value) in &extracted.socials {
        if let Some(normalized) = normalize_social(kind.as_str(), value) {
            contacts.push(Contact::new(*kind, normalized).with_source("web"));
        }
    }

    let mut address = extracted.address.clone();
    if let Some(country) = address.country_code.as_deref() {
        let country = normalize_text(country);
        if country == "nicaragua" || country == "ni" {
            address.country_code = Some("NI".to_string());
        }
    }

    let mut aliases = Vec::new();
    for name in &extracted.names {
        if normalize_text(name) != normalize_text(&business.name) {
            aliases.push(name.clone());
        }
    }

    let description = extracted
        .descriptions
        .iter()
        .max_by_key(|value| value.len())
        .map(|value| value.chars().take(2_000).collect::<String>());
    let opening_hours = if extracted.opening_hours.is_empty() {
        None
    } else {
        Some(extracted.opening_hours.join("; "))
    };
    let schema_type = extracted.schema_types.first().cloned();
    let primary_category = schema_type.as_deref().map(humanize_schema_type);

    let mut tags = BTreeMap::new();
    tags.insert("web.homepage".to_string(), homepage.to_string());
    tags.insert("web.pages".to_string(), pages.join(" | "));
    tags.insert(
        "web.has_jsonld".to_string(),
        (!extracted.jsonld.is_empty()).to_string(),
    );
    tags.insert("web.enriched_at".to_string(), Utc::now().to_rfc3339());
    if !extracted.schema_types.is_empty() {
        tags.insert(
            "web.schema_types".to_string(),
            extracted.schema_types.join(","),
        );
    }
    if let Some(title) = extracted.meta.get("og:title") {
        tags.insert("web.title".to_string(), title.clone());
    }

    let mut confidence: f64 = 0.55;
    if !extracted.jsonld.is_empty() {
        confidence += 0.08;
    }
    if !extracted.phones.is_empty() {
        confidence += 0.04;
    }
    if !extracted.emails.is_empty() {
        confidence += 0.04;
    }
    if !address.is_empty() {
        confidence += 0.03;
    }

    let raw = json!({
        "homepage": homepage,
        "pages": pages,
        "jsonld": extracted.jsonld,
        "meta": extracted.meta,
    });

    let mut record = BusinessRecord::new("web", business.name.clone());
    record.external_id = Some(homepage.to_string());
    record.source_url = Some(homepage.to_string());
    record.aliases = aliases;
    record.legal_name = extracted.legal_names.first().cloned();
    record.description = description;
    record.primary_category = primary_category;
    record.business_type = schema_type.map(|value| format!("web:schema={value}"));
    record.categories = extracted
        .schema_types
        .iter()
        .map(|value| humanize_schema_type(value))
        .collect();
    record.contacts = contacts;
    record.address = address;
    record.latitude = extracted.latitude;
    record.longitude = extracted.longitude;
    record.opening_hours = opening_hours;
    record.price_range = extracted.price_range.clone();
    record.rating = extracted.rating;
    record.review_count = extracted.review_count;
    record.tags = tags;
    record.raw = raw;
    record.confidence = confidence.min(0.78);
    record.fetched_at = Utc::now();
    record
}

fn push_unique<T: PartialEq + Clone>(target: &mut Vec<T>, values: Vec<T>) {
    for value in values {
        if !target.contains(&value) {
            target.push(value);
        }
    }
}

fn fill_address(target: &mut Address, incoming: Address) {
    target.line = target.line.take().or(incoming.line);
    target.street = target.street.take().or(incoming.street);
    target.house_number = target.house_number.take().or(incoming.house_number);
    target.barrio = target.barrio.take().or(incoming.barrio);
    target.comarca = target.comarca.take().or(incoming.comarca);
    target.municipio = target.municipio.take().or(incoming.municipio);
    target.departamento = target.departamento.take().or(incoming.departamento);
    target.postal_code = target.postal_code.take().or(incoming.postal_code);
    target.country_code = target.country_code.take().or(incoming.country_code);
}

fn json_strings(value: Option<&Value>) -> Vec<String> {
    match value {
        Some(Value::String(value)) => vec![value.trim().to_string()],
        Some(Value::Array(values)) => values
            .iter()
            .filter_map(|value| {
                value
                    .as_str()
                    .map(|value| value.trim().to_string())
                    .filter(|value| !value.is_empty())
            })
            .collect(),
        Some(Value::Number(value)) => vec![value.to_string()],
        _ => Vec::new(),
    }
}

fn json_first_string(value: Option<&Value>) -> Option<String> {
    json_strings(value).into_iter().next()
}

fn json_number(value: Option<&Value>) -> Option<f64> {
    match value {
        Some(Value::Number(value)) => value.as_f64(),
        Some(Value::String(value)) => value.replace(',', ".").parse().ok(),
        _ => None,
    }
}

fn is_business_schema_type(value: &str) -> bool {
    let value = value.to_lowercase();
    [
        "localbusiness",
        "store",
        "restaurant",
        "foodestablishment",
        "cafeorcoffeeshop",
        "barorpub",
        "bakery",
        "hotel",
        "lodgingbusiness",
        "professionalservice",
        "homeandconstructionbusiness",
        "healthandbeautybusiness",
        "medicalbusiness",
        "automotivebusiness",
        "financialservice",
        "realestateagent",
        "travelagency",
        "organization",
        "corporation",
        "ngo",
        "dentist",
        "pharmacy",
        "veterinarycare",
    ]
    .iter()
    .any(|kind| value.contains(kind))
}

fn humanize_schema_type(value: &str) -> String {
    let spaced = value
        .replace("LocalBusiness", "Negocio local")
        .replace("Organization", "Organización")
        .replace("Corporation", "Empresa")
        .replace("Restaurant", "Restaurante")
        .replace("Store", "Tienda")
        .replace("LodgingBusiness", "Hospedaje")
        .replace("CafeOrCoffeeShop", "Cafetería")
        .replace("BarOrPub", "Bar")
        .replace("Bakery", "Panadería")
        .replace("Pharmacy", "Farmacia")
        .replace("Dentist", "Dentista")
        .replace("VeterinaryCare", "Veterinaria")
        .replace("ProfessionalService", "Servicio profesional")
        .replace("FinancialService", "Servicio financiero");
    let spaced = spaced.replace('_', " ");
    let mut chars = spaced.chars();
    match chars.next() {
        Some(first) => format!("{}{}", first.to_uppercase(), chars.as_str()),
        None => spaced,
    }
}

fn short_day(value: &str) -> String {
    let value = value.to_lowercase();
    if value.starts_with("mon") {
        "Lun".to_string()
    } else if value.starts_with("tue") {
        "Mar".to_string()
    } else if value.starts_with("wed") {
        "Mié".to_string()
    } else if value.starts_with("thu") {
        "Jue".to_string()
    } else if value.starts_with("fri") {
        "Vie".to_string()
    } else if value.starts_with("sat") {
        "Sáb".to_string()
    } else if value.starts_with("sun") {
        "Dom".to_string()
    } else {
        value
    }
}

fn in_nicaragua(lat: f64, lon: f64) -> bool {
    (10.0..16.5).contains(&lat) && (-88.5..-82.0).contains(&lon)
}

fn is_contact_link(url: &Url) -> bool {
    let path = url.path().to_lowercase();
    let blocked = [
        "privacidad",
        "privacy",
        "terminos",
        "terms",
        "login",
        "carrito",
        "cart",
    ];
    if blocked.iter().any(|value| path.contains(value)) {
        return false;
    }
    let candidates = [
        "contact",
        "contacto",
        "nosotros",
        "about",
        "empresa",
        "quienes",
        "quienes-somos",
    ];
    candidates.iter().any(|value| path.contains(value))
        && !path.ends_with(".pdf")
        && !path.ends_with(".jpg")
        && !path.ends_with(".png")
}

fn social_kind(url: &Url) -> Option<ContactKind> {
    let domain = domain_of(url.as_str())?;
    if domain.ends_with("facebook.com") {
        Some(ContactKind::Facebook)
    } else if domain.ends_with("instagram.com") {
        Some(ContactKind::Instagram)
    } else if domain.ends_with("tiktok.com") {
        Some(ContactKind::TikTok)
    } else if domain.ends_with("twitter.com") || domain == "x.com" {
        Some(ContactKind::Twitter)
    } else if domain.ends_with("t.me") || domain.ends_with("telegram.me") {
        Some(ContactKind::Telegram)
    } else if domain.ends_with("linkedin.com") {
        Some(ContactKind::LinkedIn)
    } else if domain.ends_with("youtube.com") || domain.ends_with("youtu.be") {
        Some(ContactKind::YouTube)
    } else {
        None
    }
}

fn extract_whatsapp_phone(value: &str) -> Option<String> {
    let digits: String = value.chars().filter(|ch| ch.is_ascii_digit()).collect();
    if digits.len() < 8 {
        return None;
    }
    normalize_phone(&digits)
}

fn is_noise_email(email: &str) -> bool {
    let email = email.to_lowercase();
    let blocked = [
        "example.com",
        "domain.com",
        "sentry.io",
        "wixpress.com",
        "email.com",
        "tudominio.com",
        "sentry-next.wixpress.com",
    ];
    blocked.iter().any(|value| email.contains(value))
        || email.ends_with(".png")
        || email.ends_with(".jpg")
        || email.ends_with(".svg")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_robots_groups_and_wildcards() {
        let body = "
            User-agent: *
            Disallow: /privado
            Allow: /privado/publico

            User-agent: nicascan
            Disallow: /no
            Crawl-delay: 3
        ";
        let rules = parse_robots(body);
        assert!(!rules.allows("/no/loquesea"));
        assert!(rules.allows("/otra"));
        assert_eq!(rules.crawl_delay, Some(Duration::from_secs(3)));
    }

    #[test]
    fn robots_wildcards_match() {
        assert!(rule_matches("/buscar/*/detalle", "/buscar/x/detalle"));
        assert!(!rule_matches("/*.pdf$", "/archivo.pdf?x=1"));
        assert!(rule_matches("/*.pdf$", "/archivo.pdf"));
    }

    #[test]
    fn extracts_jsonld_business() {
        let html = r#"
        <html><head>
          <script type="application/ld+json">
          {
            "@context": "https://schema.org",
            "@type": "Restaurant",
            "name": "Café Las Flores",
            "telephone": "+505 8888-1111",
            "email": "hola@lasflores.ni",
            "url": "https://lasflores.ni",
            "priceRange": "$$",
            "address": {
              "@type": "PostalAddress",
              "streetAddress": "Calle Colón 123",
              "addressLocality": "Managua",
              "addressRegion": "Managua",
              "addressCountry": "NI"
            },
            "geo": {"latitude": 12.14, "longitude": -86.27},
            "sameAs": ["https://instagram.com/lasflores"],
            "openingHours": "Mo-Sa 08:00-18:00",
            "aggregateRating": {"ratingValue": 4.7, "reviewCount": 120}
          }
          </script>
        </head><body>
          <a href="mailto:ventas@lasflores.ni">Contacto</a>
          <a href="tel:+50522223333">Llamar</a>
          <a href="/contacto">Contacto</a>
        </body></html>
        "#;
        let url = Url::parse("https://lasflores.ni/").unwrap();
        let page = parse_page(html, &url);
        let ex = page.extracted;
        assert!(ex.names.iter().any(|name| name.contains("Las Flores")));
        assert!(ex.phones.iter().any(|phone| phone.contains("8888")));
        assert!(ex.emails.iter().any(|email| email == "hola@lasflores.ni"));
        assert_eq!(ex.address.municipio.as_deref(), Some("Managua"));
        assert_eq!(ex.rating, Some(4.7));
        assert_eq!(ex.review_count, Some(120));
        assert!(!page.contact_links.is_empty());
    }

    #[test]
    fn builds_web_record_without_inventing_name() {
        let business = Business {
            id: "x".into(),
            name: "Café Central".into(),
            name_norm: "cafe central".into(),
            legal_name: None,
            description: None,
            primary_category: None,
            business_type: None,
            status: None,
            phone_primary: None,
            email_primary: None,
            website: Some("https://cafecentral.ni".into()),
            address: Address::default(),
            latitude: None,
            longitude: None,
            geohash: None,
            opening_hours: None,
            price_range: None,
            rating: None,
            review_count: None,
            source_count: 1,
            confidence: 0.7,
            quality_score: 0.5,
            first_seen_at: "x".into(),
            last_seen_at: "x".into(),
            updated_at: "x".into(),
            extra: serde_json::Value::Null,
            distance_m: None,
        };
        let mut ex = Extracted::default();
        ex.phones.push("8888-1111".into());
        ex.names.push("Café Central Managua".into());
        let record = build_record(
            &business,
            "https://cafecentral.ni",
            &["https://cafecentral.ni".into()],
            &ex,
        );
        assert_eq!(record.name, "Café Central");
        assert!(record.aliases.iter().any(|a| a.contains("Managua")));
        assert!(record.contacts.iter().any(|c| c.kind == ContactKind::Phone));
    }
}
