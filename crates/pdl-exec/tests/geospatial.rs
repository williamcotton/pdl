// Geospatial table I/O integration tests (PDL_SPEC §10.13, V0.53 plan test
// matrix). These exercise GeoJSON/shapefile/TopoJSON loads, geometry
// preservation through tabular stages, GeoJSON output, scalar-output
// rejection, and native-engine fallback through the row runtime.

use std::collections::BTreeMap;

use pdl_core::Severity;
use pdl_data::DataBackend;
use pdl_driver::{prepare_source_with_io, InMemoryDriverIo};
use pdl_exec::{
    run_prepared_with_io_and_context_and_engine, ExecutionEngine, RunOptions, RunResult,
};

const COUNTIES_GEOJSON: &str = r#"{
  "type": "FeatureCollection",
  "features": [
    {"type":"Feature","properties":{"GEOID":"1001","name":"Autauga"},
     "geometry":{"type":"Polygon","coordinates":[[[0,0],[1,0],[1,1],[0,1],[0,0]]]}},
    {"type":"Feature","properties":{"GEOID":"1003","name":"Baldwin"},
     "geometry":{"type":"Polygon","coordinates":[[[1,0],[2,0],[2,1],[1,1],[1,0]]]}}
  ]
}"#;

const SINGLE_FEATURE_GEOJSON: &str = r#"{
  "type":"Feature","properties":{"GEOID":"1001","name":"Autauga"},
  "geometry":{"type":"Point","coordinates":[1,2]}
}"#;

// One missing property and one null geometry (Should: focused fixtures).
const SPARSE_GEOJSON: &str = r#"{
  "type":"FeatureCollection",
  "features":[
    {"type":"Feature","properties":{"GEOID":"1001","name":"Autauga","pop":54},
     "geometry":{"type":"Point","coordinates":[0,0]}},
    {"type":"Feature","properties":{"GEOID":"1003"},
     "geometry":null}
  ]
}"#;

const METRICS_CSV: &str = "GEOID,unemployment_rate\n1001,3.4\n1003,5.1\n";
const METRICS_RENAMED_CSV: &str = "county_fips,unemployment_rate\n1001,3.4\n1003,5.1\n";

fn run(source: &str, io: &InMemoryDriverIo, engine: ExecutionEngine) -> RunResult {
    let prepared = prepare_source_with_io("memory/main.pdl", source, io);
    run_prepared_with_io_and_context_and_engine(
        &prepared,
        RunOptions {
            stdout_format: None,
            dry_run: false,
            allow_binary_stdout: true,
        },
        io,
        BTreeMap::new(),
        engine,
    )
}

fn io_with_geojson(name: &str, geojson: &str) -> InMemoryDriverIo {
    InMemoryDriverIo::default()
        .with_file_bytes(format!("memory/{name}"), geojson.as_bytes().to_vec())
}

fn assert_ok(result: &RunResult) {
    assert!(
        !result
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.severity == Severity::Error),
        "unexpected diagnostics: {:?}",
        result.diagnostics
    );
}

fn error_codes(result: &RunResult) -> Vec<&str> {
    result
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.severity == Severity::Error)
        .map(|diagnostic| diagnostic.code)
        .collect()
}

fn stdout_text(result: &RunResult) -> String {
    String::from_utf8(result.stdout.clone().expect("stdout bytes")).expect("utf-8 stdout")
}

#[test]
fn loads_feature_collection_and_writes_geojson() {
    let io = io_with_geojson("counties.geojson", COUNTIES_GEOJSON);
    let result = run(
        r#"load "counties.geojson" | save stdout format "geojson""#,
        &io,
        ExecutionEngine::Auto,
    );
    assert_ok(&result);
    let out = stdout_text(&result);
    assert!(out.contains("\"FeatureCollection\""));
    assert!(out.contains("Autauga") && out.contains("Baldwin"));
    // Feature order is preserved.
    assert!(out.find("Autauga").unwrap() < out.find("Baldwin").unwrap());
    assert_eq!(result.backend, DataBackend::PortableRows);
}

#[test]
fn loads_single_feature() {
    let io = io_with_geojson("one.geojson", SINGLE_FEATURE_GEOJSON);
    let result = run(
        r#"load "one.geojson" | save stdout format "geojson""#,
        &io,
        ExecutionEngine::Auto,
    );
    assert_ok(&result);
    assert!(stdout_text(&result).contains("Autauga"));
}

#[test]
fn bare_geometry_is_rejected() {
    let io = io_with_geojson("bare.geojson", r#"{"type":"Point","coordinates":[0,0]}"#);
    let result = run(
        r#"load "bare.geojson" | save stdout format "geojson""#,
        &io,
        ExecutionEngine::Auto,
    );
    assert!(
        error_codes(&result).contains(&"E1822"),
        "{:?}",
        result.diagnostics
    );
}

#[test]
fn missing_properties_and_null_geometry_round_trip() {
    let io = io_with_geojson("sparse.geojson", SPARSE_GEOJSON);
    let result = run(
        r#"load "sparse.geojson" | save stdout format "geojson""#,
        &io,
        ExecutionEngine::Auto,
    );
    assert_ok(&result);
    let out = stdout_text(&result);
    // Null feature geometry writes `"geometry":null`.
    assert!(out.contains("\"geometry\":null"), "{out}");
    // A missing property writes a JSON null property value.
    assert!(out.contains("\"name\":null"), "{out}");
}

#[test]
fn loads_shapefile_bundle_with_dbf_attributes() {
    let io = InMemoryDriverIo::default()
        .with_file_bytes(
            "memory/tiny.shp",
            include_bytes!("fixtures/tiny.shp").to_vec(),
        )
        .with_file_bytes(
            "memory/tiny.dbf",
            include_bytes!("fixtures/tiny.dbf").to_vec(),
        )
        .with_file_bytes(
            "memory/tiny.shx",
            include_bytes!("fixtures/tiny.shx").to_vec(),
        );
    let result = run(
        r#"load "tiny.shp" | save stdout format "geojson""#,
        &io,
        ExecutionEngine::Auto,
    );
    assert_ok(&result);
    let out = stdout_text(&result);
    // DBF attribute columns become feature properties.
    assert!(
        out.contains("\"name\"") && out.contains("\"population\""),
        "{out}"
    );
    assert!(out.contains("Polygon"), "{out}");
}

#[test]
fn missing_shapefile_sidecar_reports_diagnostic() {
    // No `.dbf` sidecar registered.
    let io = InMemoryDriverIo::default().with_file_bytes(
        "memory/tiny.shp",
        include_bytes!("fixtures/tiny.shp").to_vec(),
    );
    let result = run(
        r#"load "tiny.shp" | save stdout format "geojson""#,
        &io,
        ExecutionEngine::Auto,
    );
    assert!(
        error_codes(&result).contains(&"E1820"),
        "{:?}",
        result.diagnostics
    );
}

#[test]
fn loads_single_object_topojson() {
    let topojson = r#"{
      "type":"Topology",
      "objects":{"regions":{"type":"GeometryCollection","geometries":[
        {"type":"Polygon","arcs":[[0]],"properties":{"name":"A","pop":10}}
      ]}},
      "arcs":[[[0,0],[1,0],[1,1],[0,1],[0,0]]]
    }"#;
    let io = io_with_geojson("regions.topojson", topojson);
    let result = run(
        r#"load "regions.topojson" | save stdout format "geojson""#,
        &io,
        ExecutionEngine::Auto,
    );
    assert_ok(&result);
    assert!(stdout_text(&result).contains("\"name\":\"A\""));
}

#[test]
fn multi_object_topojson_is_rejected() {
    let topojson = r#"{"type":"Topology","arcs":[],
      "objects":{"a":{"type":"Point","coordinates":[0,0]},
                 "b":{"type":"Point","coordinates":[1,1]}}}"#;
    let io = io_with_geojson("multi.topojson", topojson);
    let result = run(
        r#"load "multi.topojson" | save stdout format "geojson""#,
        &io,
        ExecutionEngine::Auto,
    );
    assert!(
        error_codes(&result).contains(&"E1821"),
        "{:?}",
        result.diagnostics
    );
}

#[test]
fn county_left_join_to_csv_metrics() {
    let io = io_with_geojson("counties.geojson", COUNTIES_GEOJSON)
        .with_file_bytes("memory/metrics.csv", METRICS_CSV.as_bytes().to_vec());
    let result = run(
        r#"let metrics = load "metrics.csv" | select GEOID, unemployment_rate
load "counties.geojson"
  | join metrics on GEOID kind left
  | save stdout format "geojson""#,
        &io,
        ExecutionEngine::Auto,
    );
    assert_ok(&result);
    let out = stdout_text(&result);
    assert!(out.contains("unemployment_rate"), "{out}");
    assert!(out.contains("3.4") && out.contains("5.1"), "{out}");
    assert_eq!(result.backend, DataBackend::PortableRows);
}

#[test]
fn differently_named_join_keys() {
    let io = io_with_geojson("counties.geojson", COUNTIES_GEOJSON).with_file_bytes(
        "memory/metrics.csv",
        METRICS_RENAMED_CSV.as_bytes().to_vec(),
    );
    let result = run(
        r#"let metrics = load "metrics.csv" | select county_fips, unemployment_rate
load "counties.geojson"
  | join metrics on (GEOID, county_fips) kind left
  | save stdout format "geojson""#,
        &io,
        ExecutionEngine::Auto,
    );
    assert_ok(&result);
    assert!(stdout_text(&result).contains("unemployment_rate"));
}

#[test]
fn geometry_cannot_be_a_join_key() {
    let io = io_with_geojson("counties.geojson", COUNTIES_GEOJSON)
        .with_file_bytes("memory/other.geojson", COUNTIES_GEOJSON.as_bytes().to_vec());
    let result = run(
        r#"let other = load "other.geojson"
load "counties.geojson"
  | join other on geom kind left
  | save stdout format "geojson""#,
        &io,
        ExecutionEngine::Auto,
    );
    assert!(
        error_codes(&result).contains(&"E1233"),
        "{:?}",
        result.diagnostics
    );
}

#[test]
fn select_drop_rename_geom() {
    let io = io_with_geojson("counties.geojson", COUNTIES_GEOJSON);

    // select keeps geom: GeoJSON output succeeds.
    let selected = run(
        r#"load "counties.geojson" | select GEOID, geom | save stdout format "geojson""#,
        &io,
        ExecutionEngine::Auto,
    );
    assert_ok(&selected);
    assert!(stdout_text(&selected).contains("Polygon"));

    // drop geom yields an ordinary scalar table saved to CSV.
    let dropped = run(
        r#"load "counties.geojson" | drop geom | save stdout format "csv""#,
        &io,
        ExecutionEngine::Auto,
    );
    assert_ok(&dropped);
    assert_eq!(
        stdout_text(&dropped),
        "GEOID,name\n1001,Autauga\n1003,Baldwin\n"
    );

    // rename geom changes the geometry column used by GeoJSON output.
    let renamed = run(
        r#"load "counties.geojson" | rename shape = geom | save stdout format "geojson""#,
        &io,
        ExecutionEngine::Auto,
    );
    assert_ok(&renamed);
    assert!(stdout_text(&renamed).contains("Polygon"));
}

#[test]
fn geojson_save_zero_geometry_columns_rejected() {
    let io = io_with_geojson("counties.geojson", COUNTIES_GEOJSON);
    let result = run(
        r#"load "counties.geojson" | drop geom | save stdout format "geojson""#,
        &io,
        ExecutionEngine::Auto,
    );
    assert!(
        error_codes(&result).contains(&"E1712"),
        "{:?}",
        result.diagnostics
    );
}

#[test]
fn geojson_save_multiple_geometry_columns_rejected() {
    let io = io_with_geojson("counties.geojson", COUNTIES_GEOJSON)
        .with_file_bytes("memory/other.geojson", COUNTIES_GEOJSON.as_bytes().to_vec());
    // Self-join on GEOID yields a second geometry column `geom_right`.
    let result = run(
        r#"let other = load "other.geojson"
load "counties.geojson"
  | join other on GEOID kind left
  | save stdout format "geojson""#,
        &io,
        ExecutionEngine::Auto,
    );
    assert!(
        error_codes(&result).contains(&"E1712"),
        "{:?}",
        result.diagnostics
    );
}

#[test]
fn geometry_bearing_scalar_saves_are_rejected() {
    let io = io_with_geojson("counties.geojson", COUNTIES_GEOJSON);
    for format in ["csv", "jsonl", "parquet", "arrow-file", "arrow-stream"] {
        let source = format!("load \"counties.geojson\" | save stdout format \"{format}\"");
        let result = run(&source, &io, ExecutionEngine::Auto);
        assert!(
            error_codes(&result).contains(&"E1711"),
            "format {format} should reject geometry: {:?}",
            result.diagnostics
        );
    }
}

#[test]
fn geometry_pipeline_falls_back_to_row_under_auto() {
    let io = io_with_geojson("counties.geojson", COUNTIES_GEOJSON);
    let result = run(
        r#"load "counties.geojson" | save stdout format "geojson""#,
        &io,
        ExecutionEngine::Auto,
    );
    assert_ok(&result);
    assert_eq!(result.backend, DataBackend::PortableRows);
}

#[test]
fn geometry_pipeline_rejected_under_forced_native() {
    let io = io_with_geojson("counties.geojson", COUNTIES_GEOJSON);
    let result = run(
        r#"load "counties.geojson" | save stdout format "geojson""#,
        &io,
        ExecutionEngine::Native,
    );
    let codes = error_codes(&result);
    assert!(codes.contains(&"E1211"), "{:?}", result.diagnostics);
}

#[test]
fn geometry_cannot_be_used_in_scalar_expression() {
    let io = io_with_geojson("counties.geojson", COUNTIES_GEOJSON);
    let result = run(
        r#"load "counties.geojson" | filter geom == "x" | save stdout format "geojson""#,
        &io,
        ExecutionEngine::Auto,
    );
    assert!(
        error_codes(&result).contains(&"E1234"),
        "{:?}",
        result.diagnostics
    );
}

#[test]
fn geojson_load_prepares_without_errors() {
    // Schema analysis of a geojson load succeeds, so downstream stages can
    // reference the `geom` column (exercised by the select/drop/rename test).
    let io = io_with_geojson("counties.geojson", COUNTIES_GEOJSON);
    let prepared = prepare_source_with_io("memory/main.pdl", r#"load "counties.geojson""#, &io);
    assert!(
        !prepared.has_errors(),
        "prepare errors: {:?}",
        prepared.diagnostics()
    );
}
