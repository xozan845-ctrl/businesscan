# ◈ BusinessCan

**Directorio internacional de negocios, desde la terminal.**

BusinessCan busca negocios en múltiples fuentes, normaliza y deduplica los
datos, enriquece las fichas y las guarda en **un único archivo SQLite
portable**. Sirve para explorar el directorio desde la terminal, exportarlo a
JSONL/CSV, abrir un panel de revisión o alimentar un agente de IA que ofrezca
servicios a esos negocios.

El primer foco es **Centroamérica** (Belice, Guatemala, Honduras, El Salvador,
Nicaragua, Costa Rica y Panamá), pero la arquitectura es internacional: incluye
250 países/territorios, nombres en español e inglés, bbox y códigos telefónicos.
Funciona en **Termux (Android), Linux, macOS y Windows** con un binario Rust.

```text
  ◈ BUSINESSCAN

  ◎ Antigua Guatemala, Sacatepéquez, Guatemala
  Filtro: área administrativa OSM (relación 374549)

  ✓ osm 128 encontrados · 121 nuevos · 7 fusionados
  ✓ 128 encontrados · 121 nuevos · 7 fusionados · 0 omitidos · 42.1s
```

## Tabla de contenido

- [Por qué existe](#por-qué-existe)
- [Qué incluye](#qué-incluye)
- [Instalación](#instalación)
- [Inicio rápido](#inicio-rápido)
- [Ubicaciones internacionales](#ubicaciones-internacionales)
- [Idiomas](#idiomas)
- [Referencia de comandos](#referencia-de-comandos)
- [Fuentes](#fuentes)
- [Modelo de datos](#modelo-de-datos)
- [Portabilidad de la base](#portabilidad-de-la-base)
- [Exportación para IA](#exportación-para-ia)
- [API local y panel web](#api-local-y-panel-web)
- [Configuración y variables de entorno](#configuración-y-variables-de-entorno)
- [Ejecución de pruebas](#ejecución-de-pruebas)
- [Desarrollo y arquitectura](#desarrollo-y-arquitectura)
- [CI y releases](#ci-y-releases)
- [Solución de problemas](#solución-de-problemas)
- [Legal, ética y atribuciones](#legal-ética-y-atribuciones)
- [Limitaciones y hoja de ruta](#limitaciones-y-hoja-de-ruta)
- [Licencia](#licencia)

## Por qué existe

Los datos de negocios están dispersos: OpenStreetMap tiene POIs, Google y
Foursquare tienen fichas comerciales, las cámaras y los portales públicos
publican CSV, y cada negocio tiene su propio Facebook o WhatsApp. BusinessCan no
intenta inventar datos: **integra fuentes, conserva la procedencia y deja una
base consultable** para que otra herramienta (por ejemplo, un agente de IA)
decida a quién ofrecerle qué servicio.

## Qué incluye

- **Fuentes conectables**: OpenStreetMap/Overpass (sin clave), Google Places
  (opcional, de pago), Foursquare Places (opcional), Overture Maps (PMTiles
  automático o GeoJSON local), enriquecimiento web e importación CSV/JSON/JSONL.
- **Búsqueda geográfica internacional**: continente, país, departamento/provincia,
  ciudad/municipio, barrio/comarca, radio de coordenadas o bounding box.
- **Deduplicación**: por ID externo, teléfono, dominio web y nombre + cercanía o
  dirección. Fusiona datos faltantes sin borrar la información de cada fuente.
- **Procedencia y confianza**: cada negocio guarda de qué fuentes vino, cuándo se
  vio y con qué confianza.
- **Calidad de ficha**: `quality_score` mide completitud y corroboración;
  `confidence` mide la confianza de la fuente.
- **Enriquecimiento web**: completa teléfonos, correos, direcciones y redes desde
  el sitio del negocio con JSON-LD, `mailto:`, `tel:`, `robots.txt` y límites de
  velocidad.
- **Revisión de duplicados**: comando `duplicates`, ficha completa con `show` y
  fusión manual con `merge`.
- **SQLite portable**: un solo `.db`, FTS5 para búsqueda de texto y respaldo
  consistente con `VACUUM INTO`.
- **Salidas para IA**: JSONL anidado, JSON, CSV, copia SQLite y una **API HTTP
  local** con OpenAPI.
- **Panel web**: búsqueda, fichas, estadísticas y revisión/fusión de duplicados.
- **Termux primero**: pensado para compilarse y ejecutarse en Android sin
  servicios externos.

## Instalación

### Termux (Android)

```bash
pkg update
pkg install rust clang pkg-config
git clone https://github.com/xozan845-ctrl/businesscan.git
cd businesscan
cargo build --release
install -m755 target/release/businesscan "$PREFIX/bin/businesscan"
businesscan --version
```

También puedes usar `scripts/build-termux.sh`:

```bash
./scripts/build-termux.sh
```

El binario no necesita Python, Node ni un servidor de base de datos.

### Linux, macOS y Windows

Con Rust estable:

```bash
cargo install --path .
# o
cargo build --release
```

O descarga un binario de **Releases**:

| Plataforma | Archivo |
|---|---|
| Android / Termux ARM64 | `businesscan-android-arm64` |
| Linux x86_64 (musl, estático) | `businesscan-linux-x86_64` |
| Linux ARM64 (musl, estático) | `businesscan-linux-aarch64` |
| Windows x86_64 | `businesscan-windows-x86_64.exe` |
| macOS Apple Silicon | `businesscan-macos-arm64` |
| macOS Intel | `businesscan-macos-x86_64` |

En Termux se recomienda compilar nativamente (`cargo build --release`); el
binario Android de Releases es una alternativa para dispositivos compatibles.
El build local de Termux se copia a `dist/businesscan-termux-arm64`.

### Desde el código

Requisitos:

- Rust 1.80+.
- Un compilador C y `pkg-config` (SQLite se compila embebido).
- Acceso a red para las fuentes externas (opcional si usas importaciones).

```bash
cargo test
cargo build --release
./target/release/businesscan init
```

### Verificar la instalación

```bash
businesscan --version
businesscan sources
businesscan db path
```

## Inicio rápido

```bash
# 1. Crear la base (también se crea sola al usarla)
businesscan init

# 2. Buscar y guardar negocios en León, Nicaragua
businesscan scan --country NI --department León --limit 200

# 3. Consultar lo guardado
businesscan search "ferretería" --country NI --has-phone
businesscan search --country NI --city Managua --category restaurante --limit 10

# 4. Ver una ficha completa (el ID corto es único en la lista)
businesscan show 7ZQ2TVRR

# 5. Exportar para una IA
businesscan export --format jsonl -o negocios.jsonl

# 6. Panel y API local
businesscan serve --port 8787
# http://127.0.0.1:8787/panel
```

La base por defecto está en:

- **Linux/Termux**: `~/.local/share/businesscan/businesscan.db`
- **macOS**: `~/Library/Application Support/com.businesscan.businesscan/businesscan.db`
- **Windows**: `%APPDATA%\businesscan\businesscan\data\businesscan.db`

Cambia la ruta con `--db /ruta/negocios.db` o `BUSINESSCAN_DB`.

## Ubicaciones internacionales

### Flags

| Flag | Ejemplo | Uso |
|---|---|---|
| `--continent` | `central-america`, `europe`, `world` | Grupo de países. |
| `--country` / `-C` | `NI`, `GT`, `Panama`, `Spain` | País por ISO2/ISO3 o nombre es/en. |
| `--department` / `-d` | `--department León` | Departamento, provincia o estado. |
| `--city` / `-m` | `--city "Antigua Guatemala"` | Ciudad o municipio. |
| `--neighborhood` | `--neighborhood Bolonia` | Barrio, colonia o vecindario. |
| `--comarca` | `--comarca "Las Jagüitas"` | Comarca o aldea. |
| `--place` / `-p` | `--place "Antigua Guatemala"` | Lugar libre resuelto por Nominatim. |
| `--lat`, `--lon`, `--radius` | `--lat 12.13 --lon -86.25 --radius 800` | Centro y radio en metros. |
| `--bbox` | `--bbox 12.0,-86.3,12.2,-86.1` | Caja geográfica. |
| `--osm-area-id` | `--osm-area-id 3600123` | Área Overpass avanzada. |

Aliases compatibles: `--departamento` y `--municipio` para `--department` y
`--city`; `--barrio` para `--neighborhood`.

### Continentes y grupos

```text
central-america   Belice, Guatemala, Honduras, El Salvador, Nicaragua,
                  Costa Rica y Panamá
north-america     Norteamérica (incluye Centroamérica y el Caribe)
south-america     Sudamérica
americas          Todo el continente americano
europe            Europa
asia              Asia
africa            África
oceania           Oceanía
world             Todos los países del gacetero
```

### Ejemplos

```bash
# País completo
businesscan scan --country NI --limit 1000

# Departamento y ciudad
businesscan scan --country GT --department Sacatepéquez --city "Antigua Guatemala"
businesscan scan --country CR --department "San José" --city "San José"

# Barrio o comarca
businesscan scan --country NI --neighborhood "Martha Quezada" --city Managua
businesscan scan --country NI --comarca "Las Jagüitas" --city Managua

# Toda Centroamérica
businesscan scan --continent central-america --limit 200

# Texto libre global
businesscan scan --place "Antigua Guatemala"

# Coordenadas
businesscan scan --country NI --lat 12.1364 --lon -86.2514 --radius 800

# Categoría y varias fuentes
businesscan scan --country NI --department León --category ferreteria,restaurante \
  --source osm,google --limit 100

# Búsqueda local internacional
businesscan search --country PA --city "Panamá" --has-phone
businesscan search --continent central-america --min-quality 0.6 --limit 50
```

### Cómo se resuelve una ubicación

1. **Continente**: el gacetero devuelve sus países. Para grupos pequeños
   (Centroamérica) se usa el área administrativa precisa de Nominatim; para
   grupos grandes se usa el bbox de cada país para evitar cientos de llamadas.
2. **País**: acepta ISO2/ISO3 o nombre. Si no hay más detalle, se resuelve el
   área del país; si hay ciudad/departamento, se geocodifica el conjunto.
3. **Departamento/ciudad/barrio**: se consulta Nominatim con `countrycodes` del
   país cuando se conoce. La respuesta se cachea en la tabla `places`.
4. **Coordenadas**: se usa un filtro `around` y se completa la administración con
   reverse geocoding una sola vez.
5. **Sin conexión**: se puede usar `--lat/--lon`, `--bbox`, `--osm-area-id` o un
   país con bbox conocido. `BUSINESSCAN_OFFLINE=1` evita llamadas de red.

La caché se invalida con `--refresh-place`. Nominatim respeta una pausa de ~1.1 s
entre llamadas, como pide su política de uso.

## Idiomas

```bash
businesscan scan --country PA --lang en --limit 100
businesscan search --country GT --lang en
BUSINESSCAN_LANG=en businesscan stats
```

`--lang es|en` afecta:

- consultas a Google/Nominatim (`languageCode`/`Accept-Language`),
- etiquetas de categorías de OSM,
- nombres de países en la salida de ubicación.

Los datos de origen conservan el idioma en que fueron publicados. La interfaz de
la CLI está en español por ahora; el módulo `i18n` está separado para añadir más
catálogos de mensajes sin tocar la lógica.

## Referencia de comandos

### Opciones globales

```text
--db <RUTA>       Ruta de la base SQLite
--json            Salida JSON para máquinas
--no-color        Desactiva colores ANSI
--lang es|en      Idioma de consultas y etiquetas
-q, --quiet       Sin adornos ni progreso
-v, --verbose...  -v info, -vv debug (stderr)
```

### `init`

Crea la base, aplica migraciones y muestra ruta, esquema y FTS5.

```bash
businesscan init
businesscan --json init
```

### `scan`

Consulta fuentes externas y guarda/deduplica resultados.

```bash
businesscan scan [UBICACIÓN] [OPCIONES]
```

Opciones principales:

| Opción | Descripción |
|---|---|
| `--country`, `--continent`, `--department`, `--city`, `--neighborhood`, `--comarca`, `--place` | Ubicación. |
| `--lat`, `--lon`, `--radius`, `--bbox`, `--osm-area-id` | Filtros geográficos. |
| `-c, --category` | Categoría o tipo; acepta coma para varias. |
| `-Q, --query` | Texto libre para acotar la consulta. |
| `-s, --source` | `osm`, `google`, `foursquare`; coma para varias. |
| `-l, --limit` | Máximo de resultados **por fuente y por ubicación**. |
| `--max-pages` | Páginas máximas en Google/Foursquare (por defecto 3). |
| `--include-unnamed` | Incluye POIs de OSM sin nombre (normalmente ruido). |
| `--overture-file` | GeoJSON/JSONL local de Overture; implica esa fuente si no hay `--source`. |
| `--dry-run` | Analiza y muestra sin guardar. |
| `--refresh-place` | Ignora la caché de geocodificación. |

Ejemplos:

```bash
# Un barrio
businesscan scan --country NI --city Managua --neighborhood Bolonia --limit 100

# Un país
businesscan scan --country GT --limit 500

# Un continente
businesscan scan --continent central-america --limit 100

# Google/Foursquare controlados
businesscan scan --country NI --city Managua --category restaurante \
  --source google,foursquare --limit 50 --max-pages 2

# Overture local
businesscan scan --country NI --source overture \
  --overture-file overture-nicaragua.geojson
```

Notas:

- Con `--continent`, el límite se aplica por país y por fuente.
- `--source` vacío usa todas las fuentes configuradas: OSM siempre; Google y
  Foursquare solo si tienen clave.
- Los resultados se escriben en lotes de 200 dentro de transacciones.
- Los errores de una fuente no descartan los resultados de las demás.

### `search`

Busca en la base local. Alias: `buscar`, `list`, `ls`.

```bash
businesscan search [TEXTO] [OPCIONES]
```

| Opción | Descripción |
|---|---|
| `TEXTO...` | Texto libre en nombre, categoría, dirección, alias y etiquetas. |
| `--country`, `--continent`, `--department`, `--city`, `--barrio` | Filtros. |
| `-c, --category` | Categoría/tipo. |
| `-s, --source` | Fuente de procedencia. |
| `--status` | `active`, `closed`, `closed_temporarily`… |
| `--has-phone`, `--has-email`, `--has-website` | Cobertura de contacto. |
| `--min-confidence`, `--min-quality` | Umbrales 0..1. |
| `--lat`, `--lon`, `--radius` | Radio geográfico. |
| `--sort` | `relevance`, `name`, `confidence`, `quality`, `updated`, `distance`. |
| `-n, --limit` | Resultados (0 = hasta 5000). |
| `--offset` | Paginación. |
| `--wide` | Más columnas en la tabla. |

```bash
businesscan search "farmacia" --country NI --city Managua --has-phone
businesscan search --country CR --category taller --lat 9.93 --lon -84.08 --radius 2000
businesscan search --continent central-america --min-quality 0.5 --sort quality --limit 50
businesscan search --country GT --json
```

El texto usa FTS5 y tolera acentos y prefijos: `leon` encuentra `León`, `ferre`
encuentra `Ferretería`.

### `show`

Muestra la ficha completa: contactos, dirección, coordenadas, horarios, rating,
categorías, alias, etiquetas, procedencia y fechas.

```bash
businesscan show 7ZQ2TVRR
businesscan show 7ZQ2TVRR --raw
businesscan show 7ZQ2TVRR --json
```

Acepta ID completo o un prefijo único; los IDs cortos que muestra la CLI son los
últimos 8 caracteres del ULID.

### `export`

Exporta a JSONL (recomendado para IA), JSON, CSV o copia SQLite.

```bash
businesscan export --format jsonl -o negocios.jsonl
businesscan export --country NI --department León --has-phone --format jsonl -o leon.jsonl
businesscan export --country GT --format csv -o guatemala.csv
businesscan export --format json --pretty -o todos.json
businesscan export --format sqlite -o respaldo.db
businesscan export --format jsonl --include-raw -o con_fuentes.jsonl
```

- `-o -` escribe a stdout.
- `--include-raw` incluye el JSON original de cada fuente.
- `--format sqlite` copia la base completa; no acepta filtros.
- `--limit 0` exporta todo el resultado filtrado.

### `import`

Importa CSV, JSON o JSONL con encabezados en español o inglés.

```bash
businesscan import negocios.csv --source mific --country NI
businesscan import directorio.json --source camara --country GT --department Sacatepéquez
businesscan import datos.csv --delimiter ';' --dry-run
```

Reconoce, entre otros: `nombre`, `name`, `telefono`, `phone`, `celular`,
`whatsapp`, `correo`, `email`, `sitio_web`, `website`, `direccion`, `barrio`,
`municipio`, `city`, `departamento`, `country`, `categoria`, `latitud`,
`longitud`, `horario`, `facebook`, `instagram`. Las columnas desconocidas se
conservan como etiquetas y la fila original en `business_sources.raw_json`.

Formatos y codificación:

- `.csv` autodetecta `,`, `;` o tab; `--delimiter` lo fuerza.
- `.json` acepta arreglo, objeto único o `{ "data": [...] }`.
- `.jsonl` una fila por línea.
- UTF-8; si falla, intenta Windows-1252.
- Si no hay nombre, la fila se omite.

### `enrich`

Completa fichas desde el sitio web del negocio.

```bash
businesscan enrich --country NI --limit 25
businesscan enrich --country NI --city Managua --missing-only --dry-run
businesscan enrich --country CR --refresh --limit 10
```

| Opción | Descripción |
|---|---|
| `--limit` | Máximo de sitios a revisar. |
| `--max-pages` | Portada + contacto/nosotros (1–5, por defecto 2). |
| `--missing-only` | Solo si falta teléfono o correo. |
| `--refresh` | Revisita sitios ya enriquecidos. |
| `--since-days` | Evita repetir en N días (por defecto 30). |
| `--delay-ms` | Pausa por host; por defecto 2000. |
| `--dry-run` | Muestra hallazgos sin guardar. |

Extrae JSON-LD, metaetiquetas, `mailto:`, `tel:`, WhatsApp y redes. Respeta
`robots.txt`, bloquea IPs privadas (SSRF), limita cada página a 2 MB y nunca
sigue enlaces fuera del sitio.

### `overture`

```bash
businesscan overture latest
businesscan overture fetch --country GT --city "Antigua Guatemala" --limit 300
businesscan overture fetch --country NI --city Managua --limit 300
businesscan overture fetch --lat 12.1364 --lon -86.2514 --radius 800 --limit 100
```

- `overture latest` lee el catálogo STAC y muestra release y URL PMTiles.
- `overture fetch` descarga solo los tiles del bbox, decodifica MVT y los
  importa. No requiere DuckDB.
- `--max-tiles` protege contra descargas gigantes (por defecto 120).
- `--zoom` fuerza un nivel; el archivo público solo tiene datos en zoom 14.
- `--release` fija una release; por defecto usa `latest`.
- `--pmtiles-url` permite un espejo o archivo alternativo.
- Para país completo o sin red usa `scan --overture-file` con un extracto.

### `duplicates` y `merge`

```bash
businesscan duplicates --min-score 0.75 --max-distance 5000
businesscan duplicates --json
businesscan show <id_a>
businesscan show <id_b>
businesscan merge <id_que_conservas> <id_absorbido> --yes
```

`duplicates` propone pares por nombre, teléfono, web, dirección, barrio y
distancia. `merge` mueve contactos, alias, categorías, etiquetas y observaciones
al negocio que conservas, recalcula calidad y elimina el absorbido.

### `serve`

API HTTP local y panel web.

```bash
businesscan serve --port 8787
BUSINESSCAN_API_TOKEN=secreto businesscan serve --host 0.0.0.0 --port 8787
```

Si escuchas fuera de loopback, el token es obligatorio. El panel está en
`http://127.0.0.1:8787/panel` y OpenAPI en `/openapi.json`.

### `places`

```bash
businesscan places resolve "Antigua Guatemala" --country GT --refresh
businesscan places resolve "Managua" --country NI --department Managua
businesscan places list --limit 50
businesscan places clear --yes
```

### `db`

```bash
businesscan db info
businesscan db path
businesscan db checkpoint
businesscan db vacuum
businesscan db optimize
businesscan db backup ./respaldo.db
businesscan db backup ./respaldo.db --force
businesscan db reset --yes
```

### `sources` y `stats`

```bash
businesscan sources
businesscan --json sources
businesscan stats
businesscan --json stats
```

`stats` muestra cobertura de teléfono/correo/web/coordenadas/dirección, calidad
media, conteos por país, departamento, categoría, fuente y estado, y las últimas
consultas.

## Fuentes

| Fuente | Clave | Costo | Qué aporta |
|---|---|---|---|
| `osm` | No | Gratis | POIs, contactos, horarios, dirección, redes, categorías. |
| `google` | `GOOGLE_PLACES_API_KEY` | De pago/cuota | Rating, reseñas, horarios, atributos, tipos, estado. |
| `foursquare` | `FOURSQUARE_API_KEY` | Plan gratuito limitado | Categorías, contacto, redes, horarios, popularidad. |
| `overture` | No | Gratis | Places por PMTiles automático o GeoJSON local. |
| `web` | No | Gratis | Enriquecimiento desde el sitio oficial. |
| `import` | No | Gratis | CSV/JSON/JSONL de portales, cámaras o planillas. |

Por defecto se usan todas las fuentes configuradas: OSM siempre; Google y
Foursquare si tienen clave. Overture se activa con `--source overture` o con
`--overture-file`. El enriquecimiento web se ejecuta explícitamente con `enrich`.

### OpenStreetMap / Overpass

- Consulta `shop`, `amenity`, `office`, `craft`, `tourism`, `healthcare`,
  `leisure`, `industrial` y `company` con nombre.
- Mapea `contact:*`, `phone`, `mobile`, `email`, `website`, `addr:*`,
  `opening_hours`, `cuisine`, `payment:*`, `wheelchair`, redes sociales y más.
- Estado `closed` con `disused:*`, `abandoned:*`, `was:*` o `shop=vacant`.
- Cachea geocodificación y limita las llamadas a Nominatim/Overpass.

### Google Places

- Usa `places:searchText` con `X-Goog-FieldMask: *`.
- Idiomas/región según `--lang` y `--country`.
- Controla el costo con `--limit` y `--max-pages`.
- Respeta los términos de Google para almacenamiento y exhibición.

### Foursquare

- Usa `places/search` con campos de contacto, redes, horarios, rating y estado.
- Rating 0–10 convertido a 0–5; el original queda en `foursquare.rating_10`.
- Controla páginas y límite.

### Overture

- **Automático**: `overture fetch` o `scan --source overture` lee el PMTiles
  público por HTTP Range, navega directorios y descarga solo los tiles del área.
- **Local**: `scan --overture-file archivo.geojson|jsonl`.
- El PMTiles público está poblado en zoom 14. Para un país completo usa un
  extracto local o sube `--max-tiles` asumiendo el volumen (varios GB).
- La descarga automática no redistribuye el dataset; revisa la licencia de
  Overture y sus fuentes.

### Enriquecimiento web

- JSON-LD `LocalBusiness`/`Restaurant`/`Store`/`Organization`.
- Metaetiquetas Open Graph y `business:contact_data:*`.
- `mailto:`, `tel:`, WhatsApp y redes.
- `robots.txt` por origen, `Crawl-delay`, pausa por host, 2 MB por página.
- Bloqueo de localhost/IPs privadas para evitar SSRF.
- Confianza web 0.55–0.78 según JSON-LD y contactos.

## Modelo de datos

Un negocio canónico (`businesses`) tiene:

- **Identidad**: `id` ULID, nombre, `match_name`, razón social, alias.
- **Clasificación**: categoría principal, tipo (`shop=hardware`), categorías y
  etiquetas libres.
- **País/ubicación**: `country_code`, dirección, calle, número, barrio, comarca,
  municipio/ciudad, departamento/provincia, código postal, lat/lon, geohash.
- **Contactos**: teléfono, celular, WhatsApp, correo, web y redes.
- **Operación**: horario, precio, rating, reseñas, estado.
- **Calidad**: `confidence`, `quality_score`, `source_count`.
- **Fechas**: `first_seen_at`, `last_seen_at`, `updated_at`.
- **Procedencia**: `business_sources` con fuente, ID externo, URL, JSON original,
  confianza y fechas.

Tablas:

| Tabla | Contenido |
|---|---|
| `businesses` | Ficha canónica. |
| `contacts` | Contactos normalizados y `match_key`. |
| `business_aliases` | Nombres alternativos. |
| `business_categories` | Categorías. |
| `business_tags` | Etiquetas por fuente. |
| `business_sources` | Observaciones y JSON original. |
| `places` | Caché de Nominatim. |
| `scans` | Historial de consultas. |
| `businesses_fts` | Índice FTS5. |
| `meta` | Versión de esquema y estado. |

### Deduplicación

Se fusiona cuando coincide, en orden:

1. Misma fuente + `external_id`.
2. Teléfono normalizado (E.164; en Nicaragua se compara por los últimos 8
   dígitos por compatibilidad).
3. Mismo dominio web y nombre parecido.
4. Mismo nombre normalizado + cercanía (≤ 300 m), misma dirección o mismo
   barrio/municipio.

La fusión rellena huecos: conserva el nombre canónico, agrega alias, contactos,
categorías y etiquetas, y no pierde la observación de cada fuente. Las cadenas y
sucursales con el mismo nombre no se fusionan solo por nombre.

### Calidad

`quality_score` es 0..1 y suma:

- confianza de la fuente,
- teléfono, correo, web,
- dirección y coordenadas,
- horario, descripción y categoría,
- rating/reseñas y redes sociales,
- corroboración por múltiples fuentes.

`confidence` mide la confianza de la fuente; `quality_score` mide qué tan
completa es la ficha. Se recalculan al insertar, fusionar o migrar.

## Portabilidad de la base

SQLite es un formato estable y legible desde Python, Go, Node, Rust, Java,
DuckDB, etc. La base usa WAL para escrituras seguras.

```bash
businesscan db checkpoint          # consolida el WAL
businesscan db backup salida.db    # copia consistente con VACUUM INTO
businesscan export --format sqlite -o respaldo.db
```

También puedes copiar el `.db` con la aplicación cerrada. `db backup` es la
forma recomendada si hay escrituras concurrentes.

## Exportación para IA

```bash
businesscan export --format jsonl --include-raw -o negocios.jsonl
```

Cada línea de JSONL contiene:

```json
{
  "id": "01M...",
  "name": "Café Las Flores",
  "country_code": "NI",
  "primary_category": "Cafetería",
  "contacts": [
    {"kind": "phone", "value": "+50588881111", "is_primary": true},
    {"kind": "instagram", "value": "https://www.instagram.com/lasflores"}
  ],
  "address": {"municipio": "Managua", "departamento": "Managua"},
  "quality_score": 0.81,
  "sources": [{"source": "osm", "external_id": "node/123", "url": "..."}]
}
```

Para SQL directo:

```sql
SELECT name, primary_category, phone_primary, municipio, departamento
FROM businesses
WHERE country_code = 'NI'
  AND departamento_norm = 'leon'
  AND phone_primary IS NOT NULL
ORDER BY quality_score DESC;
```

## API local y panel web

```bash
businesscan serve --port 8787
# o con token si escuchas fuera de loopback
BUSINESSCAN_API_TOKEN=secreto businesscan serve --host 0.0.0.0
```

| Método | Ruta | Uso |
|---|---|---|
| GET | `/v1/health` | Estado del servicio. |
| GET | `/v1/businesses` | Lista con filtros. |
| GET | `/v1/businesses/{id}` | Ficha completa. |
| POST | `/v1/search` | Búsqueda por JSON. |
| GET | `/v1/export` | NDJSON para streaming. |
| GET | `/v1/stats` | Cobertura y calidad. |
| GET | `/v1/sources` | Fuentes configuradas. |
| GET | `/v1/duplicates` | Pares sospechosos. |
| POST | `/v1/merge` | Fusión manual (`confirm:true`). |
| GET | `/panel` | Panel web de revisión. |
| GET | `/openapi.json` | Especificación OpenAPI. |

Ejemplos:

```bash
curl "http://127.0.0.1:8787/v1/businesses?country=NI&q=farmacia&has_phone=true&limit=5"
curl "http://127.0.0.1:8787/v1/businesses?continent=central-america&limit=10"
curl -X POST http://127.0.0.1:8787/v1/search \
  -H 'Content-Type: application/json' \
  -d '{"country":"NI","departamento":"León","min_quality":0.6,"limit":20}'
```

Si configuras token, envía `Authorization: Bearer <token>`. El panel pide el
token y lo guarda en `localStorage` del navegador.

Panel:

- búsqueda con filtros de país, continente, departamento, ciudad y categoría,
- ficha con contactos, procedencia y JSON,
- duplicados con score/señales y fusión,
- resumen de cobertura/calidad,
- exportación JSONL autenticada.

## Configuración y variables de entorno

| Variable | Uso |
|---|---|
| `BUSINESSCAN_DB` | Ruta de la base SQLite. |
| `BUSINESSCAN_LANG` | Idioma (`es`/`en`). |
| `BUSINESSCAN_USER_AGENT` | User-Agent con contacto para Nominatim/Overpass. |
| `BUSINESSCAN_OVERPASS_URL` | Espejos de Overpass separados por coma. |
| `BUSINESSCAN_NOMINATIM_URL` | Instancia de Nominatim. |
| `BUSINESSCAN_OFFLINE` | `1` evita llamadas de red. |
| `BUSINESSCAN_OVERTURE_FILE` | GeoJSON/JSONL local de Overture. |
| `BUSINESSCAN_ENRICH_DELAY_MS` | Pausa por host al enriquecer. |
| `BUSINESSCAN_API_TOKEN` | Token Bearer de `serve`. |
| `GOOGLE_PLACES_API_KEY` | Activa Google Places. |
| `FOURSQUARE_API_KEY` | Activa Foursquare. |
| `RUST_LOG` / `-v` / `-vv` | Nivel de logs en stderr. |

Copia [`.env.example`](.env.example) como referencia. La CLI **no** carga `.env`
automáticamente; exporta las variables en tu shell. Las variables `NICASCAN_*`
se aceptan como compatibilidad.

## Ejecución de pruebas

### Filosofía

- Las pruebas unitarias **no hacen red**: usan fixtures inline, SQLite en
  memoria o funciones puras.
- Las pruebas de integración de `tests/database.rs` usan bases `:memory:` y
  aislan cada caso.
- Las pruebas reales contra Overpass/Nominatim/Google/Foursquare/Overture son
  **manuales** y opcionales; están documentadas abajo.
- Ninguna prueba toca la base por defecto del usuario.

### Comandos principales

```bash
# Toda la suite (unitarias + integración), igual que CI
cargo test --all-targets

# Solo unitarias de la librería
cargo test --lib

# Solo integración de base de datos
cargo test --test database

# Ver nombres y salida de cada prueba
cargo test --all-targets -- --nocapture

# Una prueba concreta
cargo test --lib gazetteer::tests::central_america_has_seven_countries -- --exact

# Pruebas con logging
RUST_LOG=debug cargo test --all-targets

# Compilación optimizada (más lenta de compilar)
cargo test --release --all-targets
```

Salida esperada (los números pueden crecer al añadir pruebas):

```text
running 42 tests
...
test result: ok. 42 passed; 0 failed; 0 ignored

running 8 tests
...
test result: ok. 8 passed; 0 failed; 0 ignored
```

### Qué cubre cada grupo

| Grupo | Qué verifica |
|---|---|
| `normalize::tests` | Acentos, nombres, teléfonos E.164 por país, URLs, redes, geohash, Haversine, consultas FTS. |
| `gazetteer::tests` | 250 países, aliases es/en, Centroamérica con 7 países, códigos telefónicos. |
| `location::tests` | Construcción de consultas, claves de caché, bbox, país con bbox. |
| `providers::overpass` | Query QL por área/radio/categoría y mapeo de etiquetas/contactos. |
| `providers::google` | Mapeo de Place, rating, dirección, contacto y restricción circular. |
| `providers::foursquare` | Mapeo de categorías, rating 0–10, redes y dirección. |
| `providers::overture` | GeoJSON, direcciones, categorías, contactos y filtro por radio. |
| `providers::overture_pmtiles` | IDs Hilbert, directorios PMTiles, coordenadas de tile y propiedades MVT. |
| `providers::import` | CSV `;`/`,`, JSON, JSONL, columnas desconocidas y contactos nativos. |
| `providers::categories` | Etiquetas es/en y traducción de categorías a filtros OSM. |
| `enrich::tests` | `robots.txt`, wildcards, JSON-LD, contactos y registro sin inventar nombre. |
| `export::tests` | JSONL y CSV. |
| `tests/database.rs` | FTS, filtros, calidad, dedupe por teléfono/nombre, país, fusión manual. |

### Pruebas manuales de red

Usa siempre una base temporal para no contaminar la real:

```bash
export BUSINESSCAN_DB="$PWD/businesscan-test.db"
rm -f "$BUSINESSCAN_DB"*
businesscan init
```

#### 1. Geocodificación internacional

```bash
businesscan places resolve "Antigua Guatemala" --country GT --refresh
businesscan places resolve "Managua" --country NI --refresh
businesscan places resolve "Panamá" --country PA --refresh
```

Comprueba que muestren país, departamento, ciudad y filtro (`Area`, `Bbox` o
`Around`).

#### 2. OpenStreetMap

```bash
# Barrio/ciudad pequeño; tarda según la cola pública de Overpass
businesscan scan --country NI --city Managua --neighborhood Bolonia --limit 10 --source osm

# Otro país
businesscan scan --country GT --city "Antigua Guatemala" --limit 10 --source osm
```

Comprueba el resumen y luego:

```bash
businesscan search --country NI --limit 5
businesscan search --country GT --limit 5
```

#### 3. Overture

```bash
businesscan overture latest
businesscan overture fetch --country GT --city "Antigua Guatemala" --limit 5 --dry-run
businesscan overture fetch --country GT --city "Antigua Guatemala" --limit 5
businesscan search --country GT --limit 5
```

Debe mostrar `zoom 14`, tiles, features, negocios y contactos con teléfono del
país (por ejemplo `+502...`).

#### 4. Importación y exportación

```bash
cat > demo.csv <<'EOF'
nombre,telefono,correo,categoria,pais,departamento,municipio,barrio
Café Demo,2222-3333,demo@example.com,Cafetería,GT,Sacatepéquez,Antigua Guatemala,Centro
EOF

businesscan import demo.csv --source demo --country GT
businesscan search --country GT --source demo --has-phone
businesscan export --country GT --source demo --format jsonl -o demo.jsonl
head -1 demo.jsonl | python -m json.tool
```

Comprueba que el CSV tenga encabezados correctos, que el país sea `GT` y que el
JSONL incluya contactos y procedencia.

#### 5. Enriquecimiento web

```bash
businesscan enrich --country GT --limit 1 --max-pages 1 --dry-run
businesscan enrich --country GT --limit 1 --max-pages 1
```

Usa un negocio con sitio web. Verifica en `show` que aparezca la observación
`web`, contactos nuevos y que no se haya visitado un sitio fuera de `robots.txt`.

#### 6. Duplicados y fusión

```bash
businesscan duplicates --min-score 0.4 --limit 5
businesscan show <id_a>
businesscan show <id_b>
businesscan merge <id_a> <id_b> --yes
businesscan search <nombre>
```

Hazlo solo con una copia de prueba: la fusión es destructiva.

#### 7. API y panel

```bash
BUSINESSCAN_API_TOKEN=testtoken businesscan serve --port 8787 &

curl -s http://127.0.0.1:8787/v1/health
curl -s -H "Authorization: Bearer testtoken" \
  "http://127.0.0.1:8787/v1/businesses?country=NI&limit=2"
curl -s -H "Authorization: Bearer testtoken" \
  "http://127.0.0.1:8787/v1/stats"
curl -s -H "Authorization: Bearer testtoken" \
  "http://127.0.0.1:8787/v1/duplicates?min_score=0.4"

# Panel web
python -m webbrowser http://127.0.0.1:8787/panel 2>/dev/null || true
kill %1
```

Prueba el panel: guardar token, buscar, abrir ficha, ver duplicados y fusionar
en una base temporal.

#### 8. Prueba negativa

```bash
# Sin ubicación debe fallar con ayuda
businesscan scan --limit 1 --source osm

# País inexistente debe sugerir o fallar
businesscan scan --country XX --limit 1

# Filtros de export SQLite no aplican y deben avisar
businesscan export --country NI --format sqlite -o /tmp/x.db
```

### Checklist de regresión antes de publicar

- [ ] `cargo fmt --check`
- [ ] `cargo clippy --all-targets -- -D warnings`
- [ ] `cargo test --all-targets`
- [ ] `cargo build --release`
- [ ] `businesscan --version`
- [ ] `businesscan init` en una ruta temporal
- [ ] `businesscan places resolve` de un país de Centroamérica
- [ ] `businesscan scan --country NI --city Managua --limit 5 --source osm`
- [ ] `businesscan search --country NI --limit 5`
- [ ] `businesscan export --format jsonl` y validar JSON
- [ ] `businesscan overture fetch --country GT --city "Antigua Guatemala" --limit 5 --dry-run`
- [ ] `businesscan serve` + `/v1/health`, `/v1/businesses`, `/v1/stats`
- [ ] Abrir `/panel` y probar búsqueda
- [ ] `businesscan sources`
- [ ] `businesscan stats`

### CI

GitHub Actions ejecuta en cada push/PR:

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test --all-targets
cargo doc --no-deps
```

Además compila el objetivo Android ARM64 con `cargo-ndk`. El workflow de release
genera binarios para Linux musl x86_64/aarch64, Windows, macOS y Android, con
`SHA256SUMS`.

### Solución de problemas en pruebas

- **Overpass 502/504**: es un servicio público; espera y reintenta. Usa un área
  más pequeña o configura otro espejo con `BUSINESSCAN_OVERPASS_URL`.
- **Nominatim 429**: respeta la pausa de ~1.1 s; no ejecutes muchas pruebas de
  geocodificación en paralelo.
- **Google/Foursquare 401/403**: falta la clave o la cuota; las pruebas
  automáticas no las necesitan.
- **Overture “necesita N tiles”**: el PMTiles público solo tiene zoom 14; usa
  una ciudad/barrio, sube `--max-tiles` o usa un GeoJSON local.
- **SQLite bloqueada**: cierra otras instancias o usa `db checkpoint`.
- **La primera compilación tarda**: SQLite, TLS y HTML/MVT se compilan desde
  cero; las siguientes son incrementales.

## Desarrollo y arquitectura

```text
src/
  cli/          comandos y presentación
  providers/    osm, google, foursquare, overture, pmtiles, import
  db/           esquema, repositorio, deduplicación, caché
  location.rs   geocodificación y filtros geográficos
  gazetteer.rs  250 países, continentes y códigos telefónicos
  normalize.rs  nombres, teléfonos E.164, URLs, geohash, distancias
  i18n.rs       español/inglés
  enrich.rs     enriquecimiento web y robots
  export.rs     JSONL/JSON/CSV/SQLite
  server.rs     API HTTP y panel
  web/panel.html
```

Flujo:

```text
CLI → LocationResolver (gacetero + Nominatim + caché)
    → Providers (OSM/Google/Foursquare/Overture/import)
    → BusinessRecord
    → dedupe + merge
    → SQLite + FTS5
    → search/show/export/API/panel
```

Ver [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) y
[docs/SOURCES.md](docs/SOURCES.md) para el detalle de tablas, mapeo de fuentes,
políticas y cómo añadir un proveedor.

Comandos de desarrollo:

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test --all-targets
cargo doc --no-deps
make dist
```

## CI y releases

- `.github/workflows/ci.yml`: formato, Clippy, pruebas, documentación y build
  Android ARM64.
- `.github/workflows/release.yml`: binarios Linux musl, Windows, macOS y Android
  al publicar un tag `v*`.
- `scripts/build-all.sh`: compila los targets instalados localmente.
- `make dist`: copia el binario del host a `dist/`.

## Solución de problemas

- **Overpass lento/502**: espera, reduce el área o cambia de espejo.
- **No aparecen negocios**: prueba `--dry-run`, revisa `sources`, ejecuta con
  `-vv` y verifica que la geocodificación haya encontrado el área.
- **País/ciudad no reconocidos**: usa ISO2 (`NI`, `GT`, `PA`) o un nombre más
  específico; `--place` ayuda cuando hay ambigüedad.
- **Google/Foursquare no disponibles**: `businesscan sources` indica la variable
  faltante.
- **Overture descarga demasiado**: usa ciudad/barrio o un extracto local.
- **Base antigua**: el esquema se migra al abrir; si viene de la versión
  anterior, copia el `.db` a la nueva ruta o usa `--db`.
- **Quiero ver los logs**: `-v`, `-vv` o `RUST_LOG=debug`.

## Legal, ética y atribuciones

- **OpenStreetMap** es © colaboradores de OpenStreetMap, licencia ODbL. Si
  publicas datos derivados, atribuye y comparte bajo ODbL según corresponda.
- **Nominatim** y **Overpass** tienen políticas de uso: User-Agent identificable,
  límites de tasa y uso razonable. BusinessCan cachea y limita las llamadas.
- **Google Places** y **Foursquare** tienen términos, cuotas y restricciones de
  almacenamiento/visualización. Revisa sus licencias antes de redistribuir.
- **Overture Maps** se distribuye bajo CDLA-Permissive 2.0 y las licencias de sus
  fuentes. No redistribuyas el dataset sin cumplir sus términos.
- **Datos personales**: cumple la ley del país que consultes (en Nicaragua, la
  Ley 787). BusinessCan está pensado para información comercial pública. No lo
  uses para perfilar personas.
- **Scraping**: respeta `robots.txt`, los términos de servicio y la ley aplicable.
  El enriquecimiento web está limitado a los sitios que el negocio publica.

## Limitaciones y hoja de ruta

- La cobertura de OSM es desigual; los negocios informales pueden no aparecer.
- Overture PMTiles público está poblado en zoom 14: ideal para barrio/ciudad,
  no para un país completo sin extracto local.
- Google/Foursquare requieren clave y pueden costar.
- La interfaz de terminal está en español; las consultas y etiquetas soportan
  es/en.
- Roadmap:
  - [x] Núcleo SQLite + FTS5 + deduplicación.
  - [x] OSM/Overpass + Nominatim con caché.
  - [x] Google Places y Foursquare opcionales.
  - [x] Importación CSV/JSON/JSONL y exportación para IA.
  - [x] Termux/Linux/macOS/Windows.
  - [x] Enriquecimiento web respetuoso.
  - [x] Overture automático por PMTiles y GeoJSON local.
  - [x] API local, OpenAPI y panel web.
  - [x] Calidad, revisión y fusión manual de duplicados.
  - [x] Cobertura internacional es/en y Centroamérica.
  - [ ] Catálogos de mensajes de CLI en más idiomas.
  - [ ] Extracción Overture de país completo sin DuckDB.

## Licencia

MIT. Consulta [LICENSE](LICENSE) y [THIRD_PARTY.md](THIRD_PARTY.md).
