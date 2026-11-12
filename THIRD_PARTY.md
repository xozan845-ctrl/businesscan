# Terceros y atribuciones

## Datos y servicios

- **OpenStreetMap** — datos © colaboradores de OpenStreetMap, disponibles bajo
  Open Database License (ODbL 1.0). <https://www.openstreetmap.org/copyright>
- **Nominatim** — geocodificación sobre OpenStreetMap; uso sujeto a su política.
- **Overpass API** — consultas sobre OpenStreetMap; uso sujeto a sus políticas.
- **Google Places API** — datos y términos propiedad de Google. Requiere clave y
  facturación; revisa las condiciones de almacenamiento y exhibición.
- **Foursquare Places API** — datos y términos de Foursquare. Requiere clave;
  revisa el plan y las restricciones de uso.
- **Overture Maps** — datos de lugares bajo CDLA-Permissive 2.0 y las licencias
  de sus fuentes. Ver <https://overturemaps.org/>. BusinessCan lee un extracto
  local producido por el usuario; no redistribuye el dataset.

## Gacetero internacional

`assets/countries.json` contiene 250 países/territorios con nombres en español e
inglés, bbox y código telefónico. Se generó a partir de:

- <https://github.com/mledoze/countries> (ODbL)
- Natural Earth 1:110m admin 0 countries (dominio público)

No contiene datos personales. Los departamentos, ciudades y barrios se resuelven
en línea con Nominatim; no se distribuye un gacetero administrativo de cada país.

## Dependencias de Rust

El proyecto usa crates de código abierto, en su mayoría bajo licencias MIT o
Apache-2.0. Entre ellos: `clap`, `serde`, `serde_json`, `tokio`, `reqwest`,
`rusqlite`/`libsqlite3-sys` (SQLite dominio público), `chrono`, `csv`,
`comfy-table`, `owo-colors`, `indicatif`, `url`, `unicode-normalization`,
`strsim`, `ulid`, `directories`, `env_logger`, `log`, `anyhow`, `thiserror`,
`encoding_rs`, `scraper` (HTML), `axum` y `tower-http` (API local),
`mapbox_vector_tile` (MVT), `flate2`/`zstd` (compresión PMTiles).

Puedes generar el inventario exacto con:

```bash
cargo install cargo-license
cargo license
```
