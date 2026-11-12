# ◈ NicaScan

**Directorio extensible de negocios de Nicaragua, desde la terminal.**

NicaScan busca negocios en varias fuentes, normaliza y deduplica los datos, y
los guarda en **un único archivo SQLite portable**. Sirve para explorar el
directorio desde la terminal, exportarlo a JSONL/CSV o alimentar un agente de
IA que ofrezca servicios a esos negocios.

Funciona en **Termux (Android), Linux, macOS y Windows** con un binario Rust.

```
  ◈ NICASCAN

  ◎ León, Departamento de León, Nicaragua
  Filtro: área administrativa OSM (relación 18466957)

  ✓ osm 128 encontrados · 121 nuevos · 7 fusionados
  ✓ 128 encontrados · 121 nuevos · 7 fusionados · 0 omitidos · 42.1s
```

## Por qué existe

Los datos de negocios de Nicaragua están dispersos: OpenStreetMap tiene POIs,
Google y Foursquare tienen fichas comerciales, las cámaras y el MIFIC publican
CSV, y cada negocio tiene su propio Facebook o WhatsApp. NicaScan no intenta
inventar datos: **integra fuentes, conserva la procedencia y deja una base
consultable** para que otra herramienta (por ejemplo, un agente de IA) decida a
quién ofrecerle qué servicio.

## Qué incluye

- **Múltiples fuentes conectables**: OpenStreetMap/Overpass (sin clave), Google
  Places (opcional, de pago), Foursquare Places (opcional), Overture Maps
  (descarga automática por PMTiles o GeoJSON local) e importación CSV/JSON/JSONL.
- **Búsqueda por ubicación**: país, departamento, municipio, barrio, comarca,
  radio alrededor de coordenadas o bounding box. La geocodificación usa
  Nominatim y se cachea.
- **Datos extensos**: nombre, razón social, descripción, categoría/tipo,
  teléfonos, celulares, WhatsApp, correos, sitio web, Facebook, Instagram,
  TikTok, X/Twitter, Telegram, LinkedIn, YouTube, dirección completa, barrio,
  comarca, municipio, departamento, coordenadas, geohash, horarios, precio,
  rating, reseñas, etiquetas específicas de cada fuente y el JSON original.
- **Deduplicación**: por ID externo, teléfono, dominio web y nombre + cercanía o
  dirección. Fusiona datos faltantes sin borrar la información de cada fuente.
- **Procedencia y confianza**: cada negocio guarda de qué fuentes vino, cuándo se
  vio y con qué confianza.
- **SQLite portable**: un solo `.db`; búsqueda full-text con FTS5 y respaldo
  consistente con `VACUUM INTO`.
- **Enriquecimiento web**: completa teléfonos, correos, direcciones y redes
  desde el sitio del negocio, con JSON-LD, `mailto:`, `tel:`, `robots.txt` y
  límites de velocidad.
- **Calidad y revisión**: puntuación de ficha completa, detección de duplicados
  no fusionados y `merge` manual.
- **Salidas para IA**: JSONL anidado, JSON, CSV plano, copia SQLite y una **API
  HTTP local** con OpenAPI.
- **Termux primero**: pensado para compilarse y ejecutarse en Android sin
  servicios externos.

## Instalación

### Termux (Android)

```bash
pkg update
pkg install rust clang pkg-config
git clone https://github.com/xozan845-ctrl/nicascan.git
cd nicascan
cargo build --release
install -m755 target/release/nicascan "$PREFIX/bin/nicascan"
nicascan --version
```

También puedes usar `scripts/build-termux.sh`. El binario pesa poco y no
necesita Python, Node ni un servidor de base de datos.

### Linux, macOS y Windows

Si tienes Rust estable:

```bash
cargo install --path .
```

O descarga un binario de la sección **Releases**. Los artefactos esperados son:

| Plataforma | Archivo |
|---|---|
| Android / Termux ARM64 | `nicascan-android-arm64` |
| Linux x86_64 (musl, estático) | `nicascan-linux-x86_64` |
| Linux ARM64 (musl, estático) | `nicascan-linux-aarch64` |
| Windows x86_64 | `nicascan-windows-x86_64.exe` |
| macOS Apple Silicon | `nicascan-macos-arm64` |
| macOS Intel | `nicascan-macos-x86_64` |

En Termux se recomienda compilar nativamente (`cargo build --release`); el
binario Android de Releases es una alternativa para dispositivos compatibles.

### Desde el código

Requisitos: Rust 1.80+, un compilador C (para SQLite embebido) y `pkg-config`.
SQLite se compila dentro del binario; no hace falta instalarlo por separado.

```bash
cargo test
cargo build --release
./target/release/nicascan init
```

## Inicio rápido

```bash
# 1. Crear la base (también se crea sola al usarla)
nicascan init

# 2. Buscar negocios en León y guardarlos
nicascan scan --departamento León --limit 200

# 3. Consultar lo guardado
nicascan search "ferretería" --has-phone
nicascan search --municipio Managua --category restaurante --limit 10

# 4. Ver una ficha completa (el ID corto es único en la lista)
nicascan show 7ZQ2TVRR

# 5. Exportar para una IA
nicascan export --format jsonl -o negocios.jsonl
```

La base por defecto está en:

- **Linux/Termux**: `~/.local/share/nicascan/nicascan.db`
- **macOS**: `~/Library/Application Support/ni.nicascan.nicascan/nicascan.db`
- **Windows**: `%APPDATA%\nicascan\nicascan\data\nicascan.db`

Cambia la ruta con `--db /ruta/negocios.db` o `NICASCAN_DB`.

## Comandos

| Comando | Para qué sirve |
|---|---|
| `init` | Crea la base y muestra ruta, esquema y FTS5. |
| `scan` | Consulta fuentes externas y guarda/deduplica resultados. |
| `search` (`buscar`, `list`, `ls`) | Busca en la base local con filtros. |
| `show` | Muestra todos los datos de un negocio por ID o prefijo único. |
| `export` (`exportar`) | Exporta a JSONL, JSON, CSV o copia SQLite. |
| `import` (`importar`) | Importa CSV/JSON/JSONL con encabezados en español o inglés. |
| `enrich` | Completa fichas desde el sitio web del negocio. |
| `duplicates` | Encuentra pares sospechosos que no se fusionaron. |
| `merge` | Fusiona dos negocios manualmente (con `--yes`). |
| `overture` | Descarga Places de Overture por PMTiles (`fetch`) o muestra la release (`latest`). |
| `serve` | API HTTP local + OpenAPI + panel de revisión. |
| `sources` | Lista fuentes, claves requeridas y disponibilidad. |
| `stats` | Cobertura, calidad, distribución y últimas consultas. |
| `places` | Geocodifica, lista o limpia la caché de ubicaciones. |
| `db` | Info, backup, checkpoint, vacuum, optimize y reset. |

### `scan`

```bash
# Departamento completo
nicascan scan --departamento Chinandega

# Municipio (con o sin departamento)
nicascan scan --departamento Managua --municipio Managua

# Barrio o comarca
nicascan scan --barrio "Martha Quezada" --municipio Managua --departamento Managua
nicascan scan --comarca "Las Jagüitas" --municipio Managua

# Texto libre (Nominatim decide el polígono/radio)
nicascan scan --place "Barrio Bolonia, Managua"

# Coordenadas y radio
nicascan scan --lat 12.1364 --lon -86.2514 --radius 800

# Todo Nicaragua (consulta grande; usa --limit)
nicascan scan --all --limit 1000

# Categoría y varias fuentes
nicascan scan --departamento León --category ferreteria,restaurante \
  --source osm,google --limit 100

# Overture Maps automático por tiles (barrio, ciudad o área)
nicascan overture fetch --departamento Managua --municipio Managua --limit 500
nicascan overture fetch --barrio "Martha Quezada" --municipio Managua --limit 200
nicascan overture fetch --lat 12.1364 --lon -86.2514 --radius 800 --limit 100

# Overture desde un GeoJSON ya extraído (país completo o sin red)
nicascan scan --all --source overture --overture-file overture-nicaragua.geojson
```

Opciones importantes:

- `--limit N`: máximo de resultados **por fuente**; también limita la salida de
  Overpass y ayuda a controlar el costo de Google/Foursquare.
- `--max-pages N`: páginas en fuentes paginadas (por defecto 3).
- `--dry-run`: muestra lo encontrado sin guardar.
- `--include-unnamed`: incluye POIs de OSM sin nombre (normalmente ruido).
- `--refresh-place`: ignora la caché de geocodificación.
- `--json`: salida para máquinas (global).

### `search`

```bash
nicascan search "farmacia" --departamento Managua --has-phone
nicascan search --category taller --lat 12.13 --lon -86.25 --radius 2000
nicascan search --barrio Bolonia --sort name --limit 50
nicascan search --source google --min-confidence 0.8 --json
```

El texto libre usa FTS5 y tolera acentos y prefijos: `leon` encuentra `León`,
`ferre` encuentra `Ferretería`.

### `export`

```bash
# JSONL: una línea por negocio, con contactos, etiquetas y procedencia
nicascan export --format jsonl -o negocios.jsonl

# Solo los que tienen teléfono o web, en un departamento
nicascan export --departamento León --has-phone --format jsonl -o leon.jsonl

# CSV plano (contactos separados por " | ")
nicascan export --format csv -o negocios.csv

# Copia SQLite completa (consistente incluso con WAL)
nicascan db backup ./respaldo.db
# o
nicascan export --format sqlite -o respaldo.db

# Incluye el JSON original de cada fuente
nicascan export --format jsonl --include-raw -o con_fuentes.jsonl
```

Formato JSONL recomendado para IA: conserva objetos anidados (`contacts`,
`address`, `sources`, `categories`, `tags`) y es fácil de transmitir por
streaming.

### `import`

```bash
nicascan import negocios.csv --source mific
nicascan import directorio.json --source camara --departamento León
nicascan import datos.csv --delimiter ';' --dry-run
```

Reconoce encabezados como `nombre`, `name`, `teléfono`, `celular`, `correo`,
`sitio_web`, `direccion`, `barrio`, `municipio`, `departamento`, `categoria`,
`latitud`, `longitud`, `horario`, `facebook`, `instagram`, etc. Las columnas
desconocidas se conservan como etiquetas y la fila original queda en la
procedencia. También importa JSON anidado con `contacts` y `address`.

## Fuentes

| Fuente | Clave | Costo | Qué aporta |
|---|---|---|---|
| `osm` | No | Gratis | Cobertura amplia, contactos, horarios, dirección, redes sociales. |
| `google` | `GOOGLE_PLACES_API_KEY` | De pago / cuota | Rating, reseñas, horarios, atributos, tipos, estado. |
| `foursquare` | `FOURSQUARE_API_KEY` | Plan gratuito limitado | Categorías, contacto, redes sociales, horarios, popularidad. |
| `overture` | No | Gratis | Places de Overture por PMTiles (descarga solo los tiles del área) o GeoJSON local. |
| `web` | No | Gratis | Enriquecimiento desde el sitio oficial del negocio. |
| `import` | No | Gratis | Datasets de MIFIC, cámaras, INE, planillas propias. |

Por defecto se usan todas las fuentes **configuradas** (OSM y Google/Foursquare
si tienen clave). Overture se descarga automáticamente con `scan --source overture`
o `overture fetch`, o se lee local con `--overture-file`. Puedes forzar
`--source osm` para evitar llamadas de pago.

### Overture automático

```bash
nicascan overture latest
nicascan overture fetch --departamento León --limit 300
nicascan overture fetch --lat 12.1364 --lon -86.2514 --radius 1000 --limit 100
```

El comando descubre la release más reciente, lee el PMTiles por HTTP Range y
solo descarga los tiles que cubren el área. No requiere DuckDB ni bajar 12 GB.
El PMTiles público tiene datos en zoom 14; para áreas muy grandes (país
completo) usa `scan --overture-file` con un extracto DuckDB, o sube
`--max-tiles` si estás dispuesto a descargar cientos de MB.

Más detalle de mapeo y políticas en [docs/SOURCES.md](docs/SOURCES.md).

## Modelo de datos

Un negocio canónico tiene:

- **Identidad**: `id` ULID, nombre, `match_name`, razón social, alias.
- **Clasificación**: categoría principal, tipo (`shop=hardware`), categorías y
  etiquetas libres (`payment:*`, `wheelchair`, `cuisine`, atributos de Google…).
- **Contactos**: teléfono, celular, WhatsApp, correo, sitio web y redes.
- **Ubicación**: dirección, calle, número, barrio, comarca, municipio,
  departamento, país, código postal, lat/lon, geohash.
- **Operación**: horario, rango de precio, rating, reseñas, estado.
- **Procedencia**: una fila por observación (`source`, `external_id`, URL, JSON
  original, confianza, primera/última vez visto).
- **Calidad**: `confidence` (confianza de la fuente), `quality_score` (ficha
  completa: contactos, web, dirección, coordenadas, horarios, redes, fuentes),
  `source_count` y fechas de creación/actualización.

### Deduplicación

Se fusiona cuando coincide, en orden:

1. La misma fuente + `external_id`.
2. Un teléfono normalizado (últimos 8 dígitos en Nicaragua).
3. El mismo dominio web y un nombre parecido.
4. El mismo nombre normalizado + cercanía (≤ 300 m) o misma dirección/barrio.

La fusión **rellena huecos**: conserva el nombre canónico, agrega alias,
contactos, categorías y etiquetas, y no pierde la observación de cada fuente.
Los cajeros, sucursales y cadenas con el mismo nombre pero direcciones distintas
no se fusionan si no comparten teléfono/web/cercanía.

Para revisar lo que quedó pendiente:

```bash
nicascan duplicates --min-score 0.75
nicascan show <id>
nicascan merge <id_que_conservas> <id_absorbido> --yes
```

### Enriquecimiento web

```bash
nicascan enrich --source osm --limit 25
nicascan enrich --departamento León --missing-only --dry-run
nicascan enrich --refresh --limit 10
```

Lee la portada y hasta `--max-pages` páginas de contacto/nosotros. Extrae
JSON-LD `LocalBusiness`, metaetiquetas, enlaces `mailto:`/`tel:`, WhatsApp y
redes. Respeta `robots.txt`, espera 2 s por host por defecto y bloquea IPs
privadas (SSRF).

## Variables de entorno

| Variable | Uso |
|---|---|
| `NICASCAN_DB` | Ruta de la base SQLite. |
| `NICASCAN_USER_AGENT` | User-Agent con contacto para Nominatim/Overpass. |
| `NICASCAN_OVERPASS_URL` | Espejos de Overpass separados por coma. |
| `NICASCAN_NOMINATIM_URL` | Instancia de Nominatim. |
| `GOOGLE_PLACES_API_KEY` | Activa Google Places. |
| `FOURSQUARE_API_KEY` | Activa Foursquare. |
| `NICASCAN_LANGUAGE` | Idioma de consultas (por defecto `es`). |
| `NICASCAN_OFFLINE` | `1` evita llamadas de red. |
| `NICASCAN_OVERTURE_FILE` | GeoJSON/JSONL de Overture. |
| `NICASCAN_ENRICH_DELAY_MS` | Pausa por host al enriquecer (por defecto 2000). |
| `NICASCAN_API_TOKEN` | Token Bearer para `nicascan serve`. |
| `RUST_LOG` / `-v` / `-vv` | Nivel de logs en stderr. |

Copia [`.env.example`](.env.example) como referencia; la CLI **no** carga `.env`
automáticamente, exporta las variables en tu shell.

## Portabilidad de la base

SQLite es un formato estable y legible desde Python, Go, Node, Rust, Java,
DuckDB, etc. La base usa WAL para escrituras seguras. Para copiarla:

```bash
nicascan db checkpoint        # consolida el WAL
nicascan db backup salida.db  # copia consistente (VACUUM INTO)
```

También puedes copiar el `.db` con la aplicación cerrada. `db backup` es la
forma recomendada si hay escrituras concurrentes.

## API local para IA

```bash
nicascan serve --port 8787
# o con token si vas a escuchar fuera de loopback
NICASCAN_API_TOKEN=secreto nicascan serve --host 0.0.0.0
```

Endpoints:

| Método | Ruta | Uso |
|---|---|---|
| GET | `/v1/health` | Estado del servicio. |
| GET | `/v1/businesses` | Lista con los mismos filtros de `search`. |
| GET | `/v1/businesses/{id}` | Ficha completa (acepta prefijo corto). |
| POST | `/v1/search` | Búsqueda por JSON. |
| GET | `/v1/export` | JSONL para streaming. |
| GET | `/v1/stats` | Cobertura y calidad. |
| GET | `/v1/sources` | Fuentes configuradas. |
| GET | `/v1/duplicates` | Pares sospechosos para revisión. |
| POST | `/v1/merge` | Fusión manual (`confirm:true`). |
| GET | `/panel` | Panel web de búsqueda y revisión. |
| GET | `/openapi.json` | Especificación OpenAPI mínima. |

Ejemplo:

```bash
curl "http://127.0.0.1:8787/v1/businesses?q=farmacia&has_phone=true&limit=5"
curl -X POST http://127.0.0.1:8787/v1/search \
  -H 'Content-Type: application/json' \
  -d '{"departamento":"León","min_quality":0.6,"limit":20}'
```

Si defines `NICASCAN_API_TOKEN`, envía `Authorization: Bearer <token>`.
El panel está en <http://127.0.0.1:8787/panel>; guarda el token en el navegador
si lo configuraste.

## Para alimentar una IA

1. Exporta JSONL:
   ```bash
   nicascan export --format jsonl --include-raw -o negocios.jsonl
   ```
2. Cada línea contiene un negocio con contactos, dirección, etiquetas y
   procedencia. Tu agente puede filtrar por departamento, categoría, teléfono,
   coordenadas y confianza.
3. Si prefieres SQL, abre la base con cualquier cliente SQLite:
   ```sql
   SELECT name, primary_category, phone_primary, municipio, departamento
   FROM businesses
   WHERE departamento_norm = 'leon'
     AND phone_primary IS NOT NULL
   ORDER BY confidence DESC;
   ```
4. La tabla `business_sources` conserva el JSON original y la URL de cada
   fuente para auditoría o enriquecimiento posterior.

## Legal, ética y límites

- **OpenStreetMap** es © colaboradores de OpenStreetMap, licencia ODbL. Si
  publicas datos derivados, atribuye y comparte bajo ODbL según corresponda.
- **Nominatim** y **Overpass** tienen políticas de uso: envía un User-Agent
  identificable, no abuses del servicio y respeta sus límites. NicaScan cachea
  geocodificaciones y limita a ~1 req/s.
- **Google Places** y **Foursquare** tienen términos, cuotas y restricciones de
  almacenamiento/visualización. Revisa sus licencias antes de redistribuir.
- **Datos personales**: la Ley 787 de Nicaragua protege datos personales.
  NicaScan está pensado para información comercial pública (nombre del negocio,
  teléfono comercial, dirección). No lo uses para perfilar personas.
- **Scraping**: el proyecto no scrapea sitios que lo prohíben. Para agregar una
  web nueva, respeta `robots.txt`, los términos de servicio y la ley aplicable.
- La cobertura de OSM es desigual: hay negocios informales que no aparecen en
  ninguna fuente. NicaScan no inventa datos; indica la procedencia y confianza.

## Arquitectura y desarrollo

```
src/
  cli/          comandos y presentación
  providers/    osm, google, foursquare, import
  db/           esquema, repositorio, deduplicación, caché
  location.rs   geocodificación y filtros geográficos
  normalize.rs  nombres, teléfonos, URLs, geohash, distancias
  export.rs     JSONL/JSON/CSV/SQLite
```

Ver [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) para el flujo, el esquema SQL y
cómo añadir una fuente nueva.

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test --all-targets
make dist
```

## Estado y hoja de ruta

- [x] Núcleo SQLite + FTS5 + deduplicación.
- [x] OSM/Overpass + Nominatim con caché.
- [x] Google Places y Foursquare opcionales.
- [x] Importación CSV/JSON/JSONL y exportación para IA.
- [x] Termux/Linux/macOS/Windows.
- [x] Enriquecimiento respetuoso desde sitios web (JSON-LD, `mailto:`, `tel:`).
- [x] Overture Maps: descarga automática por PMTiles y lectura de GeoJSON local.
- [x] API local para agentes (`nicascan serve`).
- [x] Calidad de ficha, revisión de duplicados y fusión manual.
- [x] Panel web de búsqueda, calidad y revisión de duplicados.

### Limitación conocida de Overture

El PMTiles público de Overture Places solo está poblado en zoom 14. Para un
barrio, ciudad o área mediana funciona muy bien; para **todo Nicaragua** el
número de tiles crece a decenas de miles y el volumen ronda varios GB. En ese
caso usa `scan --overture-file` con un extracto preparado con DuckDB (ver
`docs/SOURCES.md`), o sube `--max-tiles` asumiendo la descarga. No es una
limitación de NicaScan sino del dataset público.

## Licencia

MIT. Consulta [LICENSE](LICENSE) y [THIRD_PARTY.md](THIRD_PARTY.md).
