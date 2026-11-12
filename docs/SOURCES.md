# Fuentes, mapeo y políticas

BusinessCan normaliza cada fuente a un `BusinessRecord`. Aquí está qué se toma de
cada una y qué límites aplicar.

## OpenStreetMap (Overpass)

- **Clave**: no.
- **Endpoint por defecto**: `https://overpass-api.de/api/interpreter`, con
  respaldo `https://overpass.kumi.systems/api/interpreter`.
- **Consulta**: POIs con `name` dentro de un área/radio/bbox, para las claves
  `shop`, `amenity`, `office`, `craft`, `tourism`, `healthcare`, `leisure`,
  `industrial` y `company`. `amenity`, `tourism` y `leisure` usan listas de
  valores comerciales/de servicio para excluir elementos como bancas o parques.
- **Categoría**: primera clave presente en el orden anterior;
  `business_type = clave=valor`; etiqueta humana en español (p. ej.
  `shop=hardware` → `Ferretería`).

### Mapeo de etiquetas

| Campo BusinessCan | Etiquetas OSM |
|---|---|
| Nombre | `name`, `name:es`, `official_name`, `brand`, `operator` |
| Alias | `name:es`, `alt_name`, `official_name`, `short_name`, `loc_name`, `old_name`, `brand`, `operator` |
| Teléfono | `contact:phone`, `phone`, `contact:mobile`, `mobile` |
| WhatsApp | `contact:whatsapp`, `whatsapp` |
| Correo | `contact:email`, `email` |
| Web | `contact:website`, `website`, `url`, `contact:url` |
| Redes | `contact:facebook`/`facebook`, `contact:instagram`/`instagram`, `contact:tiktok`/`tiktok`, `contact:twitter`/`contact:x`/`twitter`, `contact:telegram`/`telegram`, `contact:youtube`/`youtube`, `contact:linkedin`/`linkedin` |
| Dirección | `addr:full`, `addr:street`, `addr:place`, `addr:housenumber`, `addr:neighbourhood`, `addr:suburb`, `addr:hamlet`, `addr:locality`, `addr:city`, `addr:town`, `addr:village`, `addr:municipality`, `addr:state`, `addr:postcode`, `addr:country` |
| Horario | `opening_hours` |
| Precio | `price_range`, `price` |
| Descripción | `description`, `note` |
| Categorías extra | `cuisine` y todas las claves de categoría |
| Etiquetas | Todas las etiquetas del elemento, sin recortar |

Los valores múltiples separados por `;` o `|` se dividen en contactos
individuales. Los teléfonos se normalizan a E.164 (`+505XXXXXXXX`) y se
comparan por los últimos 8 dígitos. El estado es `closed` si hay claves
`disused:*`, `abandoned:*`, `was:*` o `shop=vacant`; si no, `active`.

Los POIs sin nombre se omiten por defecto (`--include-unnamed` los incluye con
un nombre sintético). No se inventan teléfonos ni correos.

### Políticas y atribución

- Datos © colaboradores de OpenStreetMap, **ODbL 1.0**.
- Overpass es un servicio público: no lances consultas masivas en paralelo,
  respeta los timeouts y usa un User-Agent identificable.
- Si publicas una base derivada, cumple la atribución y comparte igual (ODbL)
  según corresponda. Consulta <https://www.openstreetmap.org/copyright>.

## Nominatim

- **Uso**: geocodificación directa e inversa, no es una fuente de negocios.
- **Caché**: tabla `places`; la clave incluye todos los filtros.
- **Límite**: ~1 req/s; User-Agent con contacto.
- **Política**: <https://operations.osmfoundation.org/policies/nominatim/>.
- En instalaciones grandes, configura tu propia instancia con
  `BUSINESSCAN_NOMINATIM_URL`.

## Google Places API (New)

- **Clave**: `GOOGLE_PLACES_API_KEY` (o `GOOGLE_MAPS_API_KEY`).
- **Endpoint**: `places.googleapis.com/v1/places:searchText`.
- **FieldMask**: `*` para traer toda la información disponible.
- **Datos**: nombre, dirección y componentes, coordenadas, teléfono nacional e
  internacional, web, URL de Maps, estado del negocio, rating y número de
  reseñas, nivel de precio, horarios, resumen editorial/generativo, tipos,
  atributos (`takeout`, `delivery`, `serves*`, `paymentOptions`,
  `parkingOptions`, `accessibilityOptions`, etc.).
- **Límite**: `--limit` y `--max-pages` (20 resultados por página). Cada
  categoría genera una consulta; controla el número de categorías.
- **Costo**: la API se factura por uso. Revisa precios y cuotas actuales antes
  de escanear un país completo.
- **Términos**: los datos de Google tienen restricciones de almacenamiento y
  visualización. No redistribuyas una base Google sin cumplir su contrato.
- **Rating**: se guarda en escala 0–5 tal como lo entrega Google.

## Foursquare Places

- **Clave**: `FOURSQUARE_API_KEY`.
- **Endpoint**: `api.foursquare.com/v3/places/search` con header
  `Authorization`.
- **Datos**: nombre, alias, categorías, distancia, geocódigos, dirección,
  localidad/región/barrio/país, teléfono, web, redes (`facebook_id`,
  `instagram`, `twitter`), horarios, rating (0–10 convertido a 0–5), precio
  (1–4 → `$`…`$$$$`), popularidad, descripción, tips, cadenas, atributos,
  estado (`closed_bucket`) y enlace.
- **Paginación**: cursor; `--max-pages`.
- **Términos**: respeta el plan, la atribución y las restricciones de la API.
- El rating original 0–10 se conserva también en
  `foursquare.rating_10` dentro de etiquetas.

## Overture Maps (PMTiles automático)

Overture publica Places como PMTiles en
`https://tiles.overturemaps.org/<release>/places.pmtiles`. BusinessCan:

1. Descubre la release `latest` en `https://stac.overturemaps.org/catalog.json`.
2. Abre el PMTiles con HTTP Range (header y directorios).
3. Calcula los tiles del bbox y descarga **solo esos tiles**.
4. Decodifica MVT y mapea `names`, `taxonomy`, `addresses`, `phones`, `emails`,
   `websites`, `socials`, `confidence`, `id` y estado.

```bash
businesscan overture latest
businesscan overture fetch --country NI --department León --limit 300
businesscan overture fetch --lat 12.1364 --lon -86.2514 --radius 1000 --limit 100
```

El PMTiles público está poblado en zoom 14. Para áreas muy grandes (país entero)
el número de tiles crece demasiado; usa un extracto local o sube `--max-tiles`
bajo tu responsabilidad. El comando muestra una estimación de tamaño antes de
fallar.

## Overture Maps (GeoJSON local, alternativa)

Overture también publica `theme=places/type=place` como GeoParquet en
`s3://overturemaps-us-west-2/release/...`. BusinessCan puede leer un **extracto
GeoJSON/JSONL** ya filtrado por tu zona.

Extracción típica con DuckDB (requiere extensión `spatial` y acceso a S3):

```sql
INSTALL spatial; LOAD spatial;
COPY (
  SELECT *
  FROM read_parquet('s3://overturemaps-us-west-2/release/*/theme=places/type=place/*.parquet')
  WHERE bbox.xmin BETWEEN -86.60 AND -86.10
    AND bbox.ymin BETWEEN 12.00 AND 12.35
) TO 'overture-managua.geojson' WITH (FORMAT GDAL, DRIVER 'GeoJSON');
```

Luego:

```bash
businesscan scan --country NI --source overture --overture-file overture-managua.geojson
```

Esta vía es la recomendada para país completo o para trabajar sin red después
de preparar el extracto.

Mapeo:

| Campo | Propiedad Overture |
|---|---|
| Nombre | `names.primary` o `names.common` |
| Alias | `names.common`, `brand.names.primary` |
| Categorías | `categories.primary`, `categories.alternate` |
| Confianza | `confidence` |
| Teléfonos | `phones` |
| Correos | `emails` |
| Webs | `websites` |
| Redes | `socials` |
| Dirección | `addresses[0]` (`freeform`, `locality`, `region`, `postcode`, `country`) |
| Estado | `operating_status` |
| Coordenadas | `geometry` (Point) |

El proveedor aplica el bbox/radio de la ubicación resuelta y filtra por
categoría/texto. Overture es opt-in (`--source overture`) para no mezclar
automáticamente extractos locales con un escaneo normal. Revisa la licencia
CDLA-Permissive 2.0 de Overture y las atribuciones de sus fuentes.

## Enriquecimiento web (`source=web`)

`businesscan enrich` visita el sitio del negocio y añade una observación con
`source='web'`:

- JSON-LD `LocalBusiness`/`Restaurant`/`Store`/`Organization`: nombre, teléfono,
  correo, URL, `sameAs`, `PostalAddress`, `geo`, horarios, precio, rating.
- Metaetiquetas Open Graph y `business:contact_data:*`.
- Enlaces `mailto:`, `tel:`, `wa.me`, WhatsApp y redes.
- Correos sueltos y teléfonos `+505` en el HTML, con listas de ruido.

Límites: `robots.txt` por origen (si no se puede leer, se omite el sitio),
`Crawl-delay`, pausa de 2 s por host, máximo 2 MB por página, solo HTTP/HTTPS y
bloqueo de IPs privadas. Configura la pausa con `BUSINESSCAN_ENRICH_DELAY_MS`.
La confianza de una observación web es 0.55–0.78 según JSON-LD y contactos.

## Importación CSV/JSON/JSONL

- **Formato**: se detecta por extensión o `--format`.
- **Codificación**: UTF-8; si falla, se intenta Windows-1252.
- **Delimitador CSV**: autodetecta `,`, `;` o tab; se puede forzar con
  `--delimiter`.
- **Nombre obligatorio**: si no hay columna de nombre, la fila se omite.
- **Procedencia**: `--source` (por defecto `import`). La fila completa queda en
  `business_sources.raw_json` y las columnas desconocidas en `business_tags`.

### Encabezados reconocidos

| Campo | Alias habituales |
|---|---|
| Nombre | `nombre`, `name`, `negocio`, `empresa`, `nombre_comercial`, `business_name`, `title`, `comercio` |
| ID externo | `id`, `external_id`, `codigo`, `ruc`, `registro`, `place_id`, `fsq_id` |
| Razón social | `razon_social`, `legal_name`, `nombre_legal` |
| Descripción | `descripcion`, `description`, `detalles`, `about`, `resena`, `summary` |
| Categoría | `categoria`, `category`, `rubro`, `giro`, `sector`, `tipo_negocio`, `actividad`, `industry` |
| Teléfonos | `telefono`, `telefono1`, `phone`, `tel`, `celular`, `movil`, `mobile`, `whatsapp`, `contacto`, `numero_contacto` |
| Correos | `correo`, `correo_electronico`, `email`, `e_mail`, `mail` |
| Web | `sitio_web`, `website`, `web`, `url`, `pagina_web`, `pagina` |
| Redes | `facebook`, `fb`, `instagram`, `insta`, `tiktok`, `twitter`, `x`, `telegram`, `linkedin`, `youtube` (+ sufijo `_url`) |
| Dirección | `direccion`, `address`, `domicilio`, `ubicacion`, `formatted_address` |
| Calle/número | `calle`, `street`, `avenida`, `road`; `numero`, `house_number` |
| Barrio | `barrio`, `neighborhood`, `colonia`, `suburb` |
| Comarca | `comarca`, `hamlet`, `valle` |
| Municipio | `municipio`, `city`, `ciudad`, `alcaldia`, `town`, `localidad`, `locality` |
| Departamento | `departamento`, `depto`, `department`, `state`, `region`, `provincia` |
| País | `pais`, `country`, `country_code` |
| Coordenadas | `latitud`/`latitude`/`lat`, `longitud`/`longitude`/`lon`/`lng`/`long` |
| Horario | `horario`, `horarios`, `opening_hours`, `hours` |
| Precio | `precio`, `price_range`, `rango_precio`, `price` |
| Rating/reseñas | `rating`, `calificacion`, `estrellas`; `reviews`, `resenas`, `review_count` |
| Estado | `status`, `estado_negocio`, `business_status`, `activo` |

`estado` no se usa como departamento para evitar ambigüedad con el estado del
negocio; usa `departamento` o `state` para la división política.

Valores múltiples: separa con `;`, `|` o saltos de línea. Los teléfonos de una
misma celda también aceptan ` / `.

## Fuentes futuras

- **Descarga automática de Overture** desde S3 por bbox (hoy se hace con DuckDB
  y se pasa el GeoJSON).
- **Portales oficiales** (MIFIC, INE, cámaras): se integran hoy vía `import`.
- **Directorios locales**: solo si sus términos permiten el acceso; se puede
  implementar un proveedor con rate limit y caché.

## Calidad de datos

- La confianza base varía por fuente: Google 0.9, Foursquare 0.8, OSM ~0.62–0.9
  según completitud, importación 0.45–0.55.
- `source_count` y `confidence` aumentan al confirmar un negocio desde otra
  fuente.
- Un teléfono o web compartido es evidencia fuerte; un nombre genérico en la
  misma ciudad **no** es suficiente para fusionar.
- Las observaciones crudas permiten revisar por qué se fusionó algo y volver a
  procesar cuando mejore la lógica.
