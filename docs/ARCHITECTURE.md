# Arquitectura de NicaScan

## Flujo general

```
                     ┌────────────────────┐
  nicascan scan ───▶ │ LocationResolver   │
                     │ Nominatim + caché  │
                     └─────────┬──────────┘
                               │ ResolvedLocation
                               ▼
        ┌───────────────────────────────────────────┐
        │ Providers                                 │
        │  osm (Overpass)                           │
        │  google (Places API)                      │
        │  foursquare (Places API)                  │
        │  import (CSV/JSON/JSONL)                  │
        └──────────────────┬────────────────────────┘
                           │ Vec<BusinessRecord>
                           ▼
        ┌───────────────────────────────────────────┐
        │ Dedupe + merge (db::business)             │
        │  external_id → teléfono → web → nombre    │
        └──────────────────┬────────────────────────┘
                           ▼
        ┌───────────────────────────────────────────┐
        │ SQLite portable                           │
        │  businesses, contacts, aliases, sources…  │
        │  FTS5 para búsqueda de texto              │
        └──────────────────┬────────────────────────┘
                           ▼
             search / show / stats / export
```

Todo el procesamiento es secuencial en la escritura. Las fuentes externas se
consultan una por una para respetar límites de tasa y para que un fallo de una
fuente no pierda los datos ya importados de otra. Las escrituras se hacen en
lotes de 200 registros dentro de una transacción.

## Módulos

| Ruta | Responsabilidad |
|---|---|
| `src/main.rs` | Arranque Tokio, parseo de CLI, logging y códigos de salida. |
| `src/cli/` | Subcomandos, argumentos, tablas y mensajes. |
| `src/config.rs` | Rutas, claves, endpoints, timeouts y User-Agent. |
| `src/http.rs` | Cliente reqwest, reintentos con backoff, rate limiter. |
| `src/location.rs` | `LocationInput` → `ResolvedLocation`; Nominatim search/reverse y filtros geográficos. |
| `src/gazetteer.rs` | 17 departamentos/regiones y 153 municipios para validar nombres. |
| `src/normalize.rs` | Sin acentos, teléfonos E.164, URLs, redes, geohash, Haversine, FTS. |
| `src/providers/` | Adaptadores de fuentes y normalización a `BusinessRecord`. |
| `src/db/business.rs` | Inserción, deduplicación, fusión, FTS, búsqueda y estadísticas. |
| `src/db/places.rs` | Caché de geocodificación. |
| `src/db/schema.rs` | Esquema SQL y migraciones. |
| `src/export.rs` | Streaming JSON/JSONL/CSV y copia SQLite. |

## Modelo canónico

`BusinessRecord` es una **observación normalizada** de una fuente. No tiene ID
canónico ni contactos fusionados: describe exactamente lo que dijo una fuente y
conserva `raw` (JSON original). Los proveedores no escriben en SQLite.

`Business` es el **registro canónico** guardado en SQLite. `BusinessFull` añade
contactos, alias, categorías, etiquetas y fuentes para `show`/`export`.

## Esquema SQLite (v1)

| Tabla | Contenido |
|---|---|
| `businesses` | Una fila por negocio canónico; campos normalizados para filtros. |
| `contacts` | Teléfonos, correos, webs y redes; `match_key` para deduplicar. |
| `business_aliases` | Nombres alternativos. |
| `business_categories` | Categorías normalizadas. |
| `business_tags` | Etiquetas libres por fuente. |
| `business_sources` | Procedencia: fuente, ID externo, URL, JSON, confianza, fechas. |
| `places` | Caché de Nominatim con el `ResolvedLocation` serializado. |
| `scans` | Historial de consultas y resultados. |
| `businesses_fts` | FTS5 (nombre, alias, categorías, dirección, descripción, admin, tags). |
| `meta` | Versión de app, disponibilidad de FTS5, etc. |

La versión del esquema vive en `PRAGMA user_version`. Al abrir una base más
nueva que el binario se aborta con un mensaje claro. Las migraciones futuras
deben añadirse a `db/schema.rs` incrementando `SCHEMA_VERSION`.

### Normalización para filtros

Se guardan columnas `*_norm` (`departamento_norm`, `municipio_norm`,
`barrio_norm`, `comarca_norm`) sin acentos ni mayúsculas. `match_name` elimina
sufijos societarios (`S.A.`, `S. de R.L.`, `LTDA`, etc.) solo para comparar.

### FTS5

```sql
CREATE VIRTUAL TABLE businesses_fts USING fts5(
  business_id UNINDEXED, name, aliases, categories, address,
  description, municipio, departamento, tags,
  tokenize = 'unicode61 remove_diacritics 2'
);
```

Se refresca a mano después de cada inserción/fusión (`refresh_fts`) para evitar
triggers complejos. Si el SQLite compilado no tiene FTS5, la app lo detecta y
usa `LIKE` como respaldo.

## Deduplicación y fusión

`find_match` consulta candidatos en este orden:

1. `business_sources(source, external_id)`.
2. `contacts.match_key` para teléfonos (últimos 8 dígitos en NI).
3. `businesses.website_domain` + similitud de `match_name` ≥ 0.55.
4. `businesses.match_name` exacto +:
   - distancia Haversine ≤ 300 m, o
   - misma dirección normalizada, o
   - mismo barrio/municipio cuando no hay coordenadas.

No se fusionan dos sucursales de una cadena solo por nombre: se exige teléfono,
web, dirección o cercanía. Es una decisión conservadora para no contaminar el
directorio.

La fusión (`merge_into`):

- conserva el nombre canónico y agrega el nuevo como alias si difiere;
- rellena campos vacíos (descripción, web, email, dirección, coordenadas…);
- prefiere descripciones/horarios más largos;
- toma rating/reseñas de la fuente con más reseñas;
- agrega contactos, categorías y etiquetas sin duplicar;
- registra una nueva observación en `business_sources`;
- recalcula `source_count` y `confidence` (máximo + bonus por fuente).

## Geocodificación

1. `--lat/--lon` + `--radius`: filtro `around` de Overpass. Si faltan datos
   administrativos, se hace **reverse** una vez y se cachea.
2. `--bbox`: filtro de caja.
3. `--osm-area-id`: área avanzada de Overpass.
4. `--all`: relación OSM de Nicaragua (287666), área 3600287666.
5. Departamento/municipio/barrio/comarca/`--place`: Nominatim search, elegido
   según el nivel pedido. Una relación administrativa se convierte en
   `area(3600000000 + relation_id)`; un nodo de barrio se convierte en `around`.

Las respuestas de Nominatim se guardan en `places` con una clave normalizada.
`--refresh-place` fuerza una consulta nueva. El rate limiter espera ~1.1 s entre
llamadas, como pide la política de Nominatim.

## Proveedores

`ProviderRequest` contiene la ubicación resuelta, texto/categoría, límite,
páginas y si se incluyen POIs sin nombre. Cada proveedor devuelve
`Vec<BusinessRecord>`; no conoce SQLite.

Para añadir una fuente:

1. Crea `src/providers/mi_fuente.rs` e implementa `Provider`.
2. Normaliza a `BusinessRecord` (contactos con `ContactKind`, dirección,
   coordenadas, etiquetas y `raw`).
3. Regístrala en `provider_by_name` y `all_providers` de
   `src/providers/mod.rs`.
4. Añade la clave (si aplica) a `Config` y a `sources`/README.
5. Añade pruebas de mapeo con un JSON de ejemplo.

## Concurrencia y cancelación

Tokio se usa para HTTP y para el resolutor. Las escrituras son secuenciales.
Cada lote de 200 registros es atómico: un Ctrl-C puede dejar los lotes ya
confirmados, pero no una transacción a medias. Las observaciones crudas permiten
reprocesar o auditar después.

## Portabilidad

- `rusqlite` con SQLite embebido (`bundled`), sin dependencia del sistema.
- `reqwest` con `rustls` (sin OpenSSL).
- Sin rutas Unix fijas; `directories` elige el directorio de datos por SO.
- WAL + `synchronous=NORMAL`; `db backup` usa `VACUUM INTO`.
- CI compila y prueba en Linux; Releases añade Windows, macOS, Linux musl y
  Android ARM64.

## Pruebas

- Unitarias: normalización, gafete, geohash, construcción de consultas
  Overpass, mapeo de Google/Foursquare/import.
- Integración (`tests/database.rs`): FTS, filtros, deduplicación por teléfono y
  por nombre+cercanía, procedencia múltiple.
- Exportación JSONL/CSV en memoria.
- `cargo clippy --all-targets -- -D warnings` en CI.
