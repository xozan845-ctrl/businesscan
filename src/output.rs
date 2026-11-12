//! Salida por terminal: colores, tablas y mensajes consistentes.

use std::io::IsTerminal;

use comfy_table::{presets::UTF8_FULL, Cell, ContentArrangement, Table};
use owo_colors::OwoColorize;

use crate::model::Business;

#[derive(Debug, Clone, Copy)]
pub struct Theme {
    pub color: bool,
}

impl Theme {
    pub fn new(no_color: bool) -> Self {
        let color =
            !no_color && std::env::var_os("NO_COLOR").is_none() && std::io::stdout().is_terminal();
        Self { color }
    }

    pub fn cyan(&self, text: impl AsRef<str>) -> String {
        let text = text.as_ref();
        if self.color {
            text.truecolor(34, 211, 238).to_string()
        } else {
            text.to_string()
        }
    }

    pub fn green(&self, text: impl AsRef<str>) -> String {
        let text = text.as_ref();
        if self.color {
            text.truecolor(52, 211, 153).to_string()
        } else {
            text.to_string()
        }
    }

    pub fn amber(&self, text: impl AsRef<str>) -> String {
        let text = text.as_ref();
        if self.color {
            text.truecolor(251, 191, 36).to_string()
        } else {
            text.to_string()
        }
    }

    pub fn red(&self, text: impl AsRef<str>) -> String {
        let text = text.as_ref();
        if self.color {
            text.truecolor(251, 113, 133).to_string()
        } else {
            text.to_string()
        }
    }

    pub fn muted(&self, text: impl AsRef<str>) -> String {
        let text = text.as_ref();
        if self.color {
            text.truecolor(148, 163, 184).to_string()
        } else {
            text.to_string()
        }
    }

    pub fn bold(&self, text: impl AsRef<str>) -> String {
        let text = text.as_ref();
        if self.color {
            text.bold().to_string()
        } else {
            text.to_string()
        }
    }

    pub fn heading(&self, text: impl AsRef<str>) -> String {
        self.bold(self.cyan(text))
    }

    pub fn success(&self, text: impl AsRef<str>) -> String {
        self.bold(self.green(text))
    }

    pub fn warning(&self, text: impl AsRef<str>) -> String {
        self.bold(self.amber(text))
    }

    pub fn error(&self, text: impl AsRef<str>) -> String {
        self.bold(self.red(text))
    }
}

pub fn banner(theme: &Theme) {
    println!();
    println!("  {} {}", theme.cyan("◈"), theme.heading("NICASCAN"),);
    println!(
        "  {}",
        theme.muted("Negocios de Nicaragua, de todas las fuentes, en una sola base.")
    );
    println!();
}

pub fn business_table(businesses: &[Business], _theme: &Theme, wide: bool) -> String {
    let mut table = Table::new();
    table
        .load_preset(UTF8_FULL)
        .set_content_arrangement(ContentArrangement::Dynamic);

    if wide {
        table.set_header(vec![
            Cell::new("ID"),
            Cell::new("Nombre"),
            Cell::new("Categoría"),
            Cell::new("Ubicación"),
            Cell::new("Teléfono"),
            Cell::new("Web/Redes"),
            Cell::new("Fuentes"),
            Cell::new("Conf."),
        ]);
    } else {
        table.set_header(vec![
            Cell::new("ID"),
            Cell::new("Nombre"),
            Cell::new("Categoría"),
            Cell::new("Municipio"),
            Cell::new("Teléfono"),
            Cell::new("Conf."),
        ]);
    }

    for business in businesses {
        let id = crate::normalize::short_id(&business.id);
        let confidence = format!("{:.0}%", business.confidence * 100.0);
        let category = business
            .primary_category
            .as_deref()
            .or(business.business_type.as_deref())
            .unwrap_or("—");
        let location = if wide {
            business.location_label()
        } else {
            business
                .address
                .municipio
                .clone()
                .unwrap_or_else(|| "—".to_string())
        };
        let phone = business
            .phone_primary
            .clone()
            .unwrap_or_else(|| "—".to_string());
        if wide {
            let mut web = Vec::new();
            if business.website.is_some() {
                web.push("web");
            }
            if business.email_primary.is_some() {
                web.push("email");
            }
            table.add_row(vec![
                Cell::new(id),
                Cell::new(&business.name),
                Cell::new(category),
                Cell::new(location),
                Cell::new(phone),
                Cell::new(web.join(" · ")),
                Cell::new(business.source_count.to_string()),
                Cell::new(confidence),
            ]);
        } else {
            table.add_row(vec![
                Cell::new(id),
                Cell::new(&business.name),
                Cell::new(category),
                Cell::new(location),
                Cell::new(phone),
                Cell::new(confidence),
            ]);
        }
    }

    table.to_string()
}

pub fn kv_line(theme: &Theme, label: &str, value: &str) {
    println!("  {} {}", theme.muted(format!("{label:<14}")), value);
}

pub fn section(theme: &Theme, title: &str) {
    println!();
    println!("  {}", theme.heading(title));
}

pub fn bullet(theme: &Theme, text: &str) {
    println!("  {} {}", theme.cyan("·"), text);
}
