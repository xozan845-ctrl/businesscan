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

## Gacetero de Nicaragua

`assets/nicaragua.json` contiene los 17 departamentos/regiones y sus 153
municipios. Fue adaptado de:

- <https://github.com/nodenica/node-nicaragua-municipalities> (MIT)

Se usa para validar y canonicalizar nombres escritos por el usuario. No contiene
datos personales ni coordenadas.

## Dependencias de Rust

El proyecto usa crates de código abierto, en su mayoría bajo licencias MIT o
Apache-2.0. Entre ellos: `clap`, `serde`, `serde_json`, `tokio`, `reqwest`,
`rusqlite`/`libsqlite3-sys` (SQLite dominio público), `chrono`, `csv`,
`comfy-table`, `owo-colors`, `indicatif`, `url`, `unicode-normalization`,
`strsim`, `ulid`, `directories`, `env_logger`, `log`, `anyhow`, `thiserror` y
`encoding_rs`.

Puedes generar el inventario exacto con:

```bash
cargo install cargo-license
cargo license
```
