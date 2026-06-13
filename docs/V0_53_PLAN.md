# PDL v0.53 Plan

Status: Shipped
Target version: 0.53.0
Owner: PDL maintainers
Related spec: [`PDL_SPEC.md`](PDL_SPEC.md)
Predecessor plan: [`V0_52_PLAN.md`](V0_52_PLAN.md)

## Purpose

PDL v0.53 adds geospatial table I/O so PDL can prepare the same local spatial
datasets that Algraf can render.

The immediate gap is county-style choropleth preparation. Algraf can read a
GeoJSON county file and color by existing feature properties, but PDL cannot
currently join external county attributes onto those features and write the
enriched map back out. That forces users into an extra one-off geospatial
script between PDL and Algraf.

The target workflow is:

```pdl
let metrics =
  load "county_metrics.csv"
  | select GEOID, unemployment_rate

load "us_counties.geojson"
  | join metrics on GEOID kind left
  | save "us_counties_with_metrics.geojson"
```

Then Algraf can consume the prepared output as ordinary GeoJSON:

```algraf
Chart(data: GeoJson("us_counties_with_metrics.geojson")) {
    Scale(fill: unemployment_rate, gradient: ["#f7fbff", "#08306b"])
    Space(geom, projection: "albers_usa") {
        Geo(fill: unemployment_rate)
    }
}
```

## Release Thesis

PDL should own persistent data preparation, including geospatial attribute
joins. Algraf should remain a renderer and chart-local geospatial consumer.

This release deliberately keeps PDL and Algraf separately packaged and
separately buildable. PDL may copy or adapt Algraf's existing geospatial
behavior, but v0.53 must not introduce a shared `datafarm-geo` crate, Git
dependency, or path dependency that couples PDL's CI, Homebrew formula, or
release archive to Algraf or another repository.

PDL v0.53 is not a full GIS release. It adds enough geometry awareness to load
feature tables, preserve geometry through tabular preparation, and write
normalized GeoJSON. Spatial analysis operations remain deferred.

## Must

- Add geospatial format names.

  Status: Implemented.

  PDL must recognize these format names:

  - `geojson`
  - `shapefile`
  - `topojson`

  Path inference must recognize:

  - `.geojson` as `geojson`
  - `.shp` as `shapefile`
  - `.topojson` as `topojson`

  `geojson` must support load and save. `shapefile` and `topojson` are load-only
  in v0.53.

  Acceptance criteria:

  - `load "counties.geojson"` and `load "counties.data" format "geojson"` load
    GeoJSON.
  - `load "counties.shp"` and `load "counties.data" format "shapefile"` load a
    shapefile bundle.
  - `load "regions.topojson"` and `load "regions.data" format "topojson"` load
    supported TopoJSON.
  - `save "out.geojson"` and `save "out.data" format "geojson"` write GeoJSON.
  - Saving as `shapefile` or `topojson` produces a stable unsupported-format
    diagnostic.

- Add a geometry value and column class.

  Status: Implemented.

  PDL's table model must gain an opaque geometry value class. Geometry is not a
  string, number, boolean, or null. It exists so geospatial file loaders can
  carry feature geometry through ordinary PDL table operations.

  Geospatial loads produce:

  - one row per GeoJSON feature, shapefile record, or TopoJSON feature;
  - scalar properties or DBF attributes as ordinary PDL scalar columns;
  - a geometry column named `geom`.

  Geometry values are opaque in v0.53:

  - they cannot be used as join keys;
  - they cannot be used in arithmetic or comparisons;
  - they cannot be passed to scalar, aggregate, or window functions;
  - they cannot be used as parameter or control values.

  Acceptance criteria:

  - Schema reporting and editor services can identify `geom` as a geometry
    column.
  - Geometry columns can be referenced in column-position syntax for `select`,
    `drop`, and `rename`.
  - Invalid expression use of geometry produces targeted diagnostics rather than
    stringifying or silently dropping geometry.

- Load GeoJSON feature tables.

  Status: Implemented.

  GeoJSON loading must accept a `FeatureCollection` or a lone `Feature`.
  Properties become scalar columns using PDL's existing deterministic scalar
  inference. Each feature's geometry becomes the `geom` value for that row.

  A bare GeoJSON geometry is not a table and must be rejected.

  Acceptance criteria:

  - Feature order is preserved.
  - Property column order is deterministic, using first appearance order across
    features.
  - Missing properties produce null cells.
  - Null feature geometry is preserved as null geometry.
  - Unsupported or malformed geometry reports a stable geospatial load
    diagnostic.

- Load shapefile feature tables.

  Status: Implemented.

  Shapefile loading must read `.shp` geometry with its sidecar attribute files.
  Path-backed loads resolve sidecars next to the named `.shp` path. The required
  sidecar behavior should match the `shapefile` crate's needs while producing
  PDL diagnostics rather than raw IO errors.

  Acceptance criteria:

  - A shapefile fixture loads into DBF attribute columns plus `geom`.
  - Polygon and multipolygon records are represented as geometry values.
  - Missing or unreadable required sidecars report stable diagnostics.
  - Shapefile input works for path-backed CLI loads and host-provided bytes when
    the host supplies a complete sidecar bundle.

- Load single-object TopoJSON feature tables.

  Status: Implemented.

  TopoJSON loading must accept a `Topology` with exactly one object. The selected
  object is decoded into one row per feature-like member, with properties as
  scalar columns and geometry as `geom`.

  v0.53 must not add source options or named arguments to `load`. A topology
  with multiple objects is ambiguous and must be rejected with a diagnostic that
  says explicit object selection is not available in this release.

  Acceptance criteria:

  - A single-object TopoJSON fixture loads.
  - Quantized topology transforms are applied.
  - Missing, malformed, or unsupported topology structures produce stable
    diagnostics.
  - Multi-object TopoJSON is rejected.

- Preserve geometry through existing tabular stages.

  Status: Implemented.

  Geometry-bearing tables must behave like ordinary tables except where an
  operation explicitly evaluates values as scalars. Existing row-preserving and
  schema-transforming stages should preserve geometry columns naturally.

  Required stages:

  - `filter`
  - `select`
  - `drop`
  - `rename`
  - `mutate`
  - `sort`
  - `limit`
  - `distinct`
  - `join`
  - `union`
  - non-terminal and terminal `save`

  Join output must follow the existing PDL join contract. Geometry columns are
  ordinary output columns for collision purposes. Right-side non-key columns are
  appended and may receive the existing `_right` suffix policy.

  Acceptance criteria:

  - A GeoJSON table can be left-joined to a CSV metric table on `GEOID`.
  - Differently named key joins such as `on (GEOID, county_fips)` work when both
    keys are scalar-compatible.
  - Geometry columns cannot be used as join keys.
  - Dropping `geom` yields an ordinary scalar table that can be saved to existing
    scalar formats.
  - Selecting or renaming `geom` changes the geometry column used by GeoJSON
    output.

- Write normalized GeoJSON.

  Status: Implemented.

  GeoJSON output writes a `FeatureCollection`. Exactly one geometry column is
  required. Non-geometry columns become feature `properties` in table column
  order.

  Output must be deterministic and should not preserve source formatting,
  whitespace, original feature IDs, or foreign members unless later releases
  explicitly add metadata preservation.

  Acceptance criteria:

  - Row order becomes feature order.
  - Geometry cells become feature `geometry`.
  - Null geometry cells write `"geometry": null`.
  - Scalar nulls write JSON null property values.
  - Boolean, number, and string cells write JSON values of the corresponding
    class.
  - Tables with zero geometry columns or more than one geometry column are
    rejected for GeoJSON save.

- Reject geometry-bearing scalar output.

  Status: Implemented.

  v0.53 must not define ad hoc string encodings for geometry in CSV, JSON Lines,
  Parquet, Arrow IPC file, or Arrow IPC stream output. A table with any geometry
  column must be saved as GeoJSON or transformed to remove geometry first.

  Acceptance criteria:

  - `save "out.csv"` on a table containing `geom` reports a stable diagnostic.
  - `drop geom | save "out.csv"` succeeds.
  - `save stdout format "geojson"` succeeds only for text-capable stdout hosts.
  - Binary/native output paths do not silently coerce geometry.

- Keep native execution conservative.

  Status: Implemented.

  Geometry pipelines must run on the row runtime in v0.53. The Polars-backed
  native engine does not need geometry support in this release.

  Acceptance criteria:

  - `--engine auto` falls back to the row runtime for geospatial loads or
    geometry-carrying bindings.
  - Forced native mode reports a clear unsupported-geometry diagnostic.
  - Existing native coverage for non-geometry pipelines is unchanged.

- Update documentation and editor metadata.

  Status: Implemented.

  `PDL_SPEC.md`, examples, editor grammar assets, completion metadata, and
  diagnostics documentation must be updated to describe geospatial formats and
  geometry behavior.

  Acceptance criteria:

  - The spec includes format names, geometry value semantics, GeoJSON output
    rules, TopoJSON object-selection limits, and native-engine fallback rules.
  - Examples include the county metric join workflow.
  - Editor services offer the new format names where format names are completed.
  - Plan/spec wording explicitly states that no shared PDL/Algraf geospatial
    crate is introduced in v0.53.

## Should

- Use Algraf's geospatial behavior as the compatibility reference.

  Status: Implemented.

  PDL should copy, adapt, or port the relevant Algraf geospatial loader behavior
  into the PDL repository instead of adding a shared dependency. This keeps PDL's
  release artifact self-contained while aligning user-visible behavior.

  Acceptance criteria:

  - GeoJSON and shapefile fixtures that represent the same data produce
    compatible schemas and rows.
  - Unsupported geometry and malformed input diagnostics are PDL diagnostics,
    not leaked dependency errors.

- Preserve leading-zero geospatial keys as author responsibility.

  Status: Implemented.

  County FIPS and `GEOID` values often require leading zeroes. PDL v0.53 should
  document that key columns must be loaded as strings when leading zeroes matter.
  It should not add CSV schema annotations or padded string functions as part of
  this release.

  Acceptance criteria:

  - The county join example uses text `GEOID` values.
  - Documentation warns that numeric CSV cells such as `01001` may be parsed as
    numbers unless quoted or otherwise represented as text.

- Add focused geospatial fixtures.

  Status: Implemented.

  Tests should use small fixtures committed to the PDL repo rather than relying
  on large public datasets.

  Acceptance criteria:

  - GeoJSON, shapefile sidecars, and TopoJSON fixtures are small enough for the
    normal test suite.
  - Fixture schemas include at least one string key, one numeric metric, one
    missing property, and one null geometry or missing geometry case.

## Could

- Add lightweight geometry metadata in plan/schema output.

  Status: Proposed.

  CLI JSON, schema JSON, and editor hovers could report geometry columns with a
  type label such as `geometry` so hosts can render better previews.

- Add a `has_geometry` or `geometry_column` field to preparation reports.

  Status: Proposed.

  This could help Studio or future hosts decide whether a prepared output is
  chartable as GeoJSON without parsing the full output.

## Won't

- Do not add shapefile output in v0.53.
- Do not add TopoJSON output in v0.53.
- Do not add TopoJSON object-selection syntax in v0.53.
- Do not add CRS transforms, projections, clipping, dissolve, buffering, or true
  spatial joins in v0.53.
- Do not introduce a shared `datafarm-geo` crate, Git dependency, path
  dependency, or workspace parent dependency in v0.53.
- Do not make Algraf depend on PDL or PDL depend on Algraf.

## Test Matrix

Required tests:

- GeoJSON `FeatureCollection` load.
- Single GeoJSON `Feature` load.
- Bare GeoJSON geometry rejected.
- GeoJSON with missing properties and null geometry.
- Shapefile fixture load with DBF attributes.
- Missing shapefile sidecar diagnostic.
- Single-object TopoJSON load.
- Multi-object TopoJSON rejected.
- GeoJSON county-style left join to CSV metrics.
- Differently named scalar join keys on a geometry-bearing left table.
- Geometry column rejected as a join key.
- `select`, `drop`, and `rename` behavior for `geom`.
- GeoJSON save from a joined table.
- GeoJSON save with zero geometry columns rejected.
- GeoJSON save with multiple geometry columns rejected.
- Geometry-bearing CSV, JSON Lines, Parquet, Arrow IPC file, and Arrow IPC stream
  saves rejected.
- Geometry-bearing pipeline falls back to row runtime under `--engine auto`.
- Geometry-bearing pipeline is rejected under forced native mode.
- Existing non-geospatial examples and parity fixtures remain unchanged.

## Release Version Notes

The Rust/CLI release version is `0.53.0`. The workspace `Cargo.toml`,
`Cargo.lock`, `docs/PDL_SPEC.md`, and the VS Code extension manifest
(`editors/vscode/package.json` and its lockfile) are stamped at `0.53.0`.

Browser package publication is independent from the Rust/CLI release. As of
this release, npm publishes `pdl-wasm` and `pdl-editor` only through `0.52.0`,
so the `pdl-wasm`/`pdl-editor` package manifests (`packages/wasm`,
`editors/monaco`) and the demo consumer pins remain on the latest verified
published version `0.52.0` rather than pointing at an unpublished `0.53.0`.

## Implementation Notes

- Geometry is carried as an opaque `Value::Geometry` (a boxed
  `geo_types::Geometry<f64>`) in the row table model. The GeoJSON, shapefile,
  and TopoJSON loaders live in `crates/pdl-data/src/geo.rs`, adapted from
  Algraf's loaders but self-contained: no shared crate, Git dependency, or path
  dependency couples PDL to Algraf. The new third-party dependencies are
  `geo-types`, `geojson` (with the `geo-types` feature), and `shapefile` (with
  the `geo-types` feature).
- Geometry-column detection for GeoJSON output and scalar-output rejection scans
  cells for `Value::Geometry`; an all-null geometry column reads as scalar.
- New diagnostic codes: `E1233` (geometry join key), `E1234` (geometry in a
  scalar expression), `E1711` (geometry-bearing scalar save), `E1712` (GeoJSON
  save geometry-column count), `E1819` (geospatial load failure), `E1820`
  (shapefile sidecar), `E1821` (multi-object TopoJSON), `E1822` (bare GeoJSON
  geometry). Forced-native geometry pipelines reuse `E1211` with a new
  `geometry` reason.
