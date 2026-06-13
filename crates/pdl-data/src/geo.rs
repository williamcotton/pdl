//! Geospatial table I/O (PDL_SPEC §10.13).
//!
//! GeoJSON, shapefile, and TopoJSON sources all decode to the same row-oriented
//! [`Table`](crate::Table) shape as every other format: one row per feature in
//! file order, each scalar property or DBF attribute a [`Value`] column run
//! through PDL's deterministic scalar inference (so a numeric property infers
//! exactly as it would from CSV), and each feature's geometry a single
//! [`Value::Geometry`] column named [`GEOM_COLUMN`]. Geometry decodes to
//! `geo_types`, the common in-memory representation shared by all three
//! loaders and the GeoJSON writer.
//!
//! This module is adapted from Algraf's geospatial loaders. PDL keeps its own
//! self-contained copy rather than sharing a crate (PDL_SPEC §10.13): no
//! `datafarm-geo` crate, Git dependency, or path dependency couples PDL to
//! Algraf.

use std::path::Path;

use geo_types::Geometry;
use geojson::GeoJson;
use indexmap::IndexMap;
use pdl_core::{codes, Diagnostic, Span};
use serde_json::{Map as JsonMap, Number as JsonNumber, Value as JsonValue};

use crate::format_number;
use crate::{Row, Table, Value};

/// The column name assigned to every feature's geometry (PDL_SPEC §10.13).
pub const GEOM_COLUMN: &str = "geom";

fn geo_error(message: impl Into<String>) -> Diagnostic {
    Diagnostic::error(codes::E1819, message, Span::zero())
}

// ---------------------------------------------------------------------------
// Shared assembly
// ---------------------------------------------------------------------------

/// Accumulates property columns in first-appearance order across features and
/// the parallel geometry column, then assembles a [`Table`].
struct GeometryTableBuilder {
    prop_names: Vec<String>,
    prop_index: IndexMap<String, usize>,
    prop_cols: Vec<Vec<Value>>,
    geoms: Vec<Value>,
}

impl GeometryTableBuilder {
    fn new() -> Self {
        Self {
            prop_names: Vec::new(),
            prop_index: IndexMap::new(),
            prop_cols: Vec::new(),
            geoms: Vec::new(),
        }
    }

    /// Begin a new row; pad every known property column with a null cell so
    /// columns stay the same length as the geometry column.
    fn begin_row(&mut self) {
        for column in &mut self.prop_cols {
            column.push(Value::Null);
        }
    }

    /// Record a property cell for the current (last) row.
    fn set_property(&mut self, key: &str, value: Value) {
        let row = self.geoms.len();
        let index = *self.prop_index.entry(key.to_string()).or_insert_with(|| {
            self.prop_names.push(key.to_string());
            self.prop_cols.push(vec![Value::Null; row + 1]);
            self.prop_names.len() - 1
        });
        self.prop_cols[index][row] = value;
    }

    /// Record the geometry cell, closing the current row.
    fn push_geometry(&mut self, geometry: Value) {
        self.geoms.push(geometry);
    }

    fn finish(self) -> Table {
        let row_count = self.geoms.len();
        let mut columns = self.prop_names;
        columns.push(GEOM_COLUMN.to_string());

        let mut prop_cols = self.prop_cols;
        for column in &mut prop_cols {
            column.resize(row_count, Value::Null);
        }

        let rows = (0..row_count)
            .map(|row| {
                let mut values: Vec<Value> =
                    prop_cols.iter().map(|column| column[row].clone()).collect();
                values.push(self.geoms[row].clone());
                Row { values }
            })
            .collect();

        Table { columns, rows }
    }
}

/// Render a JSON scalar property to a PDL [`Value`] using CSV-equivalent scalar
/// inference (PDL_SPEC §10.13): a JSON string is parsed exactly as a CSV cell,
/// so `"5"` infers as a number and `"true"` as a boolean. Arrays and objects
/// have no scalar form and become their compact JSON text.
fn json_property_value(value: &JsonValue) -> Value {
    match value {
        JsonValue::Null => Value::Null,
        JsonValue::Bool(value) => Value::Bool(*value),
        JsonValue::Number(number) => number
            .as_f64()
            .map(Value::Number)
            .unwrap_or_else(|| Value::String(number.to_string())),
        JsonValue::String(text) => Value::parse_csv_cell(text),
        JsonValue::Array(_) | JsonValue::Object(_) => Value::parse_csv_cell(&value.to_string()),
    }
}

// ---------------------------------------------------------------------------
// GeoJSON load
// ---------------------------------------------------------------------------

/// Schema column names for a GeoJSON source: property names in first-appearance
/// order followed by [`GEOM_COLUMN`].
pub fn read_geojson_schema_from_bytes(
    path: &Path,
    bytes: &[u8],
) -> Result<Vec<String>, Diagnostic> {
    Ok(read_geojson_from_bytes(path, bytes)?.columns)
}

/// Load a GeoJSON `FeatureCollection` or a lone `Feature` (PDL_SPEC §10.13). A
/// bare geometry is not a table and is rejected.
pub fn read_geojson_from_bytes(path: &Path, bytes: &[u8]) -> Result<Table, Diagnostic> {
    let text = std::str::from_utf8(bytes).map_err(|error| {
        geo_error(format!(
            "GeoJSON input for `{}` is not UTF-8: {error}",
            path.display()
        ))
    })?;
    let geojson: GeoJson = text
        .parse()
        .map_err(|error: geojson::Error| geo_error(format!("GeoJSON parse failed: {error}")))?;

    let features = match geojson {
        GeoJson::FeatureCollection(collection) => collection.features,
        GeoJson::Feature(feature) => vec![feature],
        GeoJson::Geometry(_) => {
            return Err(Diagnostic::error(
                codes::E1822,
                "expected a GeoJSON FeatureCollection or Feature, found a bare geometry",
                Span::zero(),
            ));
        }
    };

    let mut builder = GeometryTableBuilder::new();
    for feature in &features {
        builder.begin_row();
        if let Some(properties) = &feature.properties {
            for (key, value) in properties {
                builder.set_property(key, json_property_value(value));
            }
        }
        let geometry = match &feature.geometry {
            Some(geometry) => {
                let geometry = Geometry::<f64>::try_from(geometry.clone()).map_err(
                    |error: geojson::Error| {
                        geo_error(format!("unsupported GeoJSON geometry: {error}"))
                    },
                )?;
                Value::geometry(geometry)
            }
            None => Value::Null,
        };
        builder.push_geometry(geometry);
    }

    Ok(builder.finish())
}

// ---------------------------------------------------------------------------
// Shapefile load
// ---------------------------------------------------------------------------

use shapefile::dbase::FieldValue;
use shapefile::{Shape, ShapeReader};
use std::io::Cursor;

/// Bytes for an ESRI shapefile bundle: the `.shp` geometry plus its required
/// `.dbf` attribute sidecar and an optional `.shx` index sidecar.
pub struct ShapefileBundle<'a> {
    pub shp: &'a [u8],
    pub dbf: &'a [u8],
    pub shx: Option<&'a [u8]>,
}

/// Schema column names for a shapefile bundle: DBF field names in file order
/// followed by [`GEOM_COLUMN`].
pub fn read_shapefile_schema_from_bundle(
    bundle: ShapefileBundle<'_>,
) -> Result<Vec<String>, Diagnostic> {
    Ok(read_shapefile_from_bundle(bundle)?.columns)
}

/// Load a shapefile bundle from already-resolved sidecar bytes (PDL_SPEC
/// §10.13). DBF attributes become scalar columns; each record's geometry
/// becomes the `geom` value for that row.
pub fn read_shapefile_from_bundle(bundle: ShapefileBundle<'_>) -> Result<Table, Diagnostic> {
    let shp = Cursor::new(bundle.shp);
    let shape_reader = match bundle.shx {
        Some(shx) => ShapeReader::with_shx(shp, Cursor::new(shx)),
        None => ShapeReader::new(shp),
    }
    .map_err(|error| {
        Diagnostic::error(
            codes::E1820,
            format!("shapefile `.shp` could not be read: {error}"),
            Span::zero(),
        )
    })?;
    let dbf_reader = shapefile::dbase::Reader::new(Cursor::new(bundle.dbf)).map_err(|error| {
        Diagnostic::error(
            codes::E1820,
            format!("shapefile `.dbf` sidecar could not be read: {error}"),
            Span::zero(),
        )
    })?;
    let mut reader = shapefile::Reader::new(shape_reader, dbf_reader);

    let mut builder = GeometryTableBuilder::new();
    for shape_record in reader.iter_shapes_and_records() {
        let (shape, record) =
            shape_record.map_err(|error| geo_error(format!("shapefile record failed: {error}")))?;
        builder.begin_row();
        for (name, value) in record {
            builder.set_property(&name, field_value_to_value(&value));
        }
        builder.push_geometry(shape_to_geometry(shape)?);
    }

    Ok(builder.finish())
}

/// Convert a shapefile shape to a geometry value. A null shape is preserved as
/// a null geometry cell; anything the converter rejects is a load error.
fn shape_to_geometry(shape: Shape) -> Result<Value, Diagnostic> {
    if matches!(shape, Shape::NullShape) {
        return Ok(Value::Null);
    }
    Geometry::<f64>::try_from(shape)
        .map(Value::geometry)
        .map_err(|error| geo_error(format!("unsupported shapefile geometry: {error}")))
}

/// Render a dBASE field value to a PDL scalar value via CSV-equivalent
/// inference. A null/absent value is a missing (null) cell; an integral number
/// prints without a trailing decimal so it infers as an integer, matching CSV.
fn field_value_to_value(value: &FieldValue) -> Value {
    let text = match value {
        FieldValue::Character(Some(text)) => text.clone(),
        FieldValue::Numeric(Some(number)) | FieldValue::Double(number) => format_number(*number),
        FieldValue::Float(Some(number)) => format_number(*number as f64),
        FieldValue::Currency(number) => format_number(*number),
        FieldValue::Integer(number) => number.to_string(),
        FieldValue::Logical(Some(flag)) => flag.to_string(),
        FieldValue::Date(Some(date)) => {
            format!("{:04}-{:02}-{:02}", date.year(), date.month(), date.day())
        }
        FieldValue::Memo(text) => text.clone(),
        _ => String::new(),
    };
    Value::parse_csv_cell(&text)
}

// ---------------------------------------------------------------------------
// TopoJSON load
// ---------------------------------------------------------------------------

/// The optional quantization transform applied to delta-encoded arc and point
/// coordinates (`q = position * scale + translate`).
struct Transform {
    scale: [f64; 2],
    translate: [f64; 2],
}

/// Schema column names for a single-object TopoJSON source.
pub fn read_topojson_schema_from_bytes(
    path: &Path,
    bytes: &[u8],
) -> Result<Vec<String>, Diagnostic> {
    Ok(read_topojson_from_bytes(path, bytes)?.columns)
}

/// Load a single-object TopoJSON `Topology` (PDL_SPEC §10.13). The topology
/// must define exactly one object; a multi-object topology is ambiguous and is
/// rejected because explicit object selection is not available in this release.
pub fn read_topojson_from_bytes(path: &Path, bytes: &[u8]) -> Result<Table, Diagnostic> {
    let text = std::str::from_utf8(bytes).map_err(|error| {
        geo_error(format!(
            "TopoJSON input for `{}` is not UTF-8: {error}",
            path.display()
        ))
    })?;
    let root: JsonValue = serde_json::from_str(text)
        .map_err(|error| geo_error(format!("TopoJSON parse failed: {error}")))?;

    if root.get("type").and_then(JsonValue::as_str) != Some("Topology") {
        return Err(geo_error(
            "expected a TopoJSON document with \"type\": \"Topology\"",
        ));
    }

    let transform = parse_transform(root.get("transform"))?;
    let raw_arcs = root
        .get("arcs")
        .and_then(JsonValue::as_array)
        .ok_or_else(|| geo_error("TopoJSON topology has no `arcs` array"))?;
    let arcs = decode_arcs(raw_arcs, transform.as_ref())?;

    let objects = root
        .get("objects")
        .and_then(JsonValue::as_object)
        .ok_or_else(|| geo_error("TopoJSON topology has no `objects` map"))?;

    let selected = {
        let mut entries = objects.iter();
        match (entries.next(), entries.next()) {
            (Some((_, value)), None) => value,
            (Some(_), Some(_)) => {
                let available: Vec<&str> = objects.keys().map(String::as_str).collect();
                return Err(Diagnostic::error(
                    codes::E1821,
                    format!(
                        "TopoJSON topology defines multiple objects ({}); explicit object \
                         selection is not available in this release",
                        available.join(", ")
                    ),
                    Span::zero(),
                ));
            }
            _ => return Err(geo_error("TopoJSON topology has no objects")),
        }
    };

    // A GeometryCollection object yields one row per member geometry (like a
    // GeoJSON FeatureCollection); any other object is a single feature.
    let members: Vec<&JsonValue> = match selected.get("type").and_then(JsonValue::as_str) {
        Some("GeometryCollection") => selected
            .get("geometries")
            .and_then(JsonValue::as_array)
            .map(|geometries| geometries.iter().collect())
            .unwrap_or_default(),
        _ => vec![selected],
    };

    let mut builder = GeometryTableBuilder::new();
    for member in members {
        builder.begin_row();
        if let Some(properties) = member.get("properties").and_then(JsonValue::as_object) {
            for (key, value) in properties {
                builder.set_property(key, json_property_value(value));
            }
        }
        builder.push_geometry(decode_topo_geometry(member, &arcs, transform.as_ref())?);
    }

    Ok(builder.finish())
}

fn parse_transform(value: Option<&JsonValue>) -> Result<Option<Transform>, Diagnostic> {
    let Some(transform) = value else {
        return Ok(None);
    };
    let pair = |key: &str| -> Result<[f64; 2], Diagnostic> {
        let array = transform
            .get(key)
            .and_then(JsonValue::as_array)
            .filter(|array| array.len() == 2)
            .ok_or_else(|| geo_error(format!("TopoJSON transform `{key}` must be [x, y]")))?;
        Ok([num(&array[0]), num(&array[1])])
    };
    Ok(Some(Transform {
        scale: pair("scale")?,
        translate: pair("translate")?,
    }))
}

fn decode_arcs(
    raw: &[JsonValue],
    transform: Option<&Transform>,
) -> Result<Vec<Vec<geo_types::Coord<f64>>>, Diagnostic> {
    raw.iter()
        .map(|arc| {
            let positions = arc
                .as_array()
                .ok_or_else(|| geo_error("TopoJSON arc must be an array"))?;
            let mut x = 0.0;
            let mut y = 0.0;
            let mut out = Vec::with_capacity(positions.len());
            for position in positions {
                let point = position
                    .as_array()
                    .filter(|array| array.len() >= 2)
                    .ok_or_else(|| geo_error("TopoJSON arc position must be [x, y]"))?;
                match transform {
                    Some(transform) => {
                        x += num(&point[0]);
                        y += num(&point[1]);
                        out.push(geo_types::Coord {
                            x: x * transform.scale[0] + transform.translate[0],
                            y: y * transform.scale[1] + transform.translate[1],
                        });
                    }
                    None => out.push(geo_types::Coord {
                        x: num(&point[0]),
                        y: num(&point[1]),
                    }),
                }
            }
            Ok(out)
        })
        .collect()
}

fn stitch(
    arc_list: &[JsonValue],
    arcs: &[Vec<geo_types::Coord<f64>>],
) -> Result<Vec<geo_types::Coord<f64>>, Diagnostic> {
    let mut coords: Vec<geo_types::Coord<f64>> = Vec::new();
    for (position, index) in arc_list.iter().enumerate() {
        let raw = index
            .as_i64()
            .ok_or_else(|| geo_error("TopoJSON arc index must be an integer"))?;
        let (resolved, reversed) = if raw < 0 {
            ((-raw - 1) as usize, true)
        } else {
            (raw as usize, false)
        };
        let arc = arcs
            .get(resolved)
            .ok_or_else(|| geo_error(format!("TopoJSON arc index {raw} out of range")))?;
        let mut segment: Vec<geo_types::Coord<f64>> = arc.clone();
        if reversed {
            segment.reverse();
        }
        if position > 0 && !segment.is_empty() {
            segment.remove(0);
        }
        coords.extend(segment);
    }
    Ok(coords)
}

fn decode_topo_geometry(
    value: &JsonValue,
    arcs: &[Vec<geo_types::Coord<f64>>],
    transform: Option<&Transform>,
) -> Result<Value, Diagnostic> {
    use geo_types::{LineString, MultiLineString, MultiPoint, MultiPolygon, Point};
    let Some(kind) = value.get("type").and_then(JsonValue::as_str) else {
        return Ok(Value::Null);
    };
    let geometry = match kind {
        "Point" => Geometry::Point(Point(point_coord(value.get("coordinates"), transform)?)),
        "MultiPoint" => {
            let positions = coord_array(value.get("coordinates"))?;
            let points = positions
                .iter()
                .map(|position| Ok(Point(transform_point(position, transform)?)))
                .collect::<Result<Vec<_>, Diagnostic>>()?;
            Geometry::MultiPoint(MultiPoint(points))
        }
        "LineString" => Geometry::LineString(LineString(stitch(arc_indices(value)?, arcs)?)),
        "MultiLineString" => {
            let lines = arc_indices(value)?
                .iter()
                .map(|line| {
                    let line = line
                        .as_array()
                        .ok_or_else(|| geo_error("TopoJSON line arcs must be an array"))?;
                    Ok(LineString(stitch(line, arcs)?))
                })
                .collect::<Result<Vec<_>, Diagnostic>>()?;
            Geometry::MultiLineString(MultiLineString(lines))
        }
        "Polygon" => Geometry::Polygon(decode_polygon(arc_indices(value)?, arcs)?),
        "MultiPolygon" => {
            let polygons = value
                .get("arcs")
                .and_then(JsonValue::as_array)
                .ok_or_else(|| geo_error("TopoJSON MultiPolygon has no `arcs`"))?
                .iter()
                .map(|rings| {
                    let rings = rings
                        .as_array()
                        .ok_or_else(|| geo_error("TopoJSON polygon rings must be arrays"))?;
                    decode_polygon(rings, arcs)
                })
                .collect::<Result<Vec<_>, Diagnostic>>()?;
            Geometry::MultiPolygon(MultiPolygon(polygons))
        }
        other => {
            return Err(geo_error(format!(
                "unsupported TopoJSON geometry type `{other}`"
            )))
        }
    };
    Ok(Value::geometry(geometry))
}

fn decode_polygon(
    rings: &[JsonValue],
    arcs: &[Vec<geo_types::Coord<f64>>],
) -> Result<geo_types::Polygon<f64>, Diagnostic> {
    use geo_types::{LineString, Polygon};
    let mut decoded: Vec<LineString<f64>> = rings
        .iter()
        .map(|ring| {
            let ring = ring
                .as_array()
                .ok_or_else(|| geo_error("TopoJSON polygon ring must be an array"))?;
            Ok(LineString(stitch(ring, arcs)?))
        })
        .collect::<Result<Vec<_>, Diagnostic>>()?;
    if decoded.is_empty() {
        return Ok(Polygon::new(LineString(Vec::new()), Vec::new()));
    }
    let exterior = decoded.remove(0);
    Ok(Polygon::new(exterior, decoded))
}

fn arc_indices(value: &JsonValue) -> Result<&Vec<JsonValue>, Diagnostic> {
    value
        .get("arcs")
        .and_then(JsonValue::as_array)
        .ok_or_else(|| geo_error("TopoJSON geometry has no `arcs`"))
}

fn point_coord(
    value: Option<&JsonValue>,
    transform: Option<&Transform>,
) -> Result<geo_types::Coord<f64>, Diagnostic> {
    let position = value
        .and_then(JsonValue::as_array)
        .filter(|array| array.len() >= 2)
        .ok_or_else(|| geo_error("TopoJSON point coordinates must be [x, y]"))?;
    transform_point(position, transform)
}

fn coord_array(value: Option<&JsonValue>) -> Result<Vec<Vec<JsonValue>>, Diagnostic> {
    value
        .and_then(JsonValue::as_array)
        .map(|positions| {
            positions
                .iter()
                .filter_map(|position| position.as_array().cloned())
                .collect()
        })
        .ok_or_else(|| geo_error("TopoJSON coordinates must be an array"))
}

fn transform_point(
    position: &[JsonValue],
    transform: Option<&Transform>,
) -> Result<geo_types::Coord<f64>, Diagnostic> {
    if position.len() < 2 {
        return Err(geo_error("TopoJSON point must have x and y"));
    }
    let (x, y) = (num(&position[0]), num(&position[1]));
    Ok(match transform {
        Some(transform) => geo_types::Coord {
            x: x * transform.scale[0] + transform.translate[0],
            y: y * transform.scale[1] + transform.translate[1],
        },
        None => geo_types::Coord { x, y },
    })
}

fn num(value: &JsonValue) -> f64 {
    value.as_f64().unwrap_or(0.0)
}

// ---------------------------------------------------------------------------
// GeoJSON write
// ---------------------------------------------------------------------------

/// Write a [`Table`] as a GeoJSON `FeatureCollection` (PDL_SPEC §10.13).
/// Exactly one geometry column is required; non-geometry columns become feature
/// `properties` in table column order. Output is deterministic and does not
/// preserve source formatting, feature IDs, or foreign members.
pub fn write_geojson_to_vec(table: &Table) -> Result<Vec<u8>, Diagnostic> {
    let geometry_columns = table.geometry_columns();
    let geometry_index = match geometry_columns.len() {
        1 => table
            .column_index(&geometry_columns[0])
            .expect("geometry column index"),
        0 => {
            return Err(Diagnostic::error(
                codes::E1712,
                "GeoJSON output requires exactly one geometry column, found none",
                Span::zero(),
            ));
        }
        count => {
            return Err(Diagnostic::error(
                codes::E1712,
                format!(
                    "GeoJSON output requires exactly one geometry column, found {count} ({})",
                    geometry_columns.join(", ")
                ),
                Span::zero(),
            ));
        }
    };

    let property_indices: Vec<usize> = (0..table.columns.len())
        .filter(|index| *index != geometry_index)
        .collect();

    let mut features = Vec::with_capacity(table.rows.len());
    for row in &table.rows {
        let mut properties = JsonMap::new();
        for &index in &property_indices {
            let value = row.values.get(index).unwrap_or(&Value::Null);
            properties.insert(table.columns[index].clone(), scalar_to_json(value)?);
        }

        let geometry = match row.values.get(geometry_index) {
            Some(Value::Geometry(geometry)) => Some(geojson::Geometry::new(geojson::Value::from(
                geometry.as_ref(),
            ))),
            Some(Value::Null) | None => None,
            Some(_) => {
                return Err(Diagnostic::error(
                    codes::E1712,
                    "geometry column contains a non-geometry value",
                    Span::zero(),
                ));
            }
        };

        features.push(geojson::Feature {
            bbox: None,
            geometry,
            id: None,
            properties: Some(properties),
            foreign_members: None,
        });
    }

    let collection = geojson::FeatureCollection {
        bbox: None,
        features,
        foreign_members: None,
    };
    Ok(GeoJson::FeatureCollection(collection)
        .to_string()
        .into_bytes())
}

fn scalar_to_json(value: &Value) -> Result<JsonValue, Diagnostic> {
    match value {
        Value::Null => Ok(JsonValue::Null),
        Value::Bool(flag) => Ok(JsonValue::Bool(*flag)),
        Value::Number(number) => {
            if number.is_finite()
                && number.fract() == 0.0
                && *number >= i64::MIN as f64
                && *number <= i64::MAX as f64
            {
                return Ok(JsonValue::Number(JsonNumber::from(*number as i64)));
            }
            JsonNumber::from_f64(*number)
                .map(JsonValue::Number)
                .ok_or_else(|| {
                    geo_error(format!(
                        "GeoJSON output cannot encode non-finite number `{}`",
                        format_number(*number)
                    ))
                })
        }
        Value::String(text) => Ok(JsonValue::String(text.clone())),
        Value::Geometry(_) => Err(Diagnostic::error(
            codes::E1712,
            "GeoJSON output requires exactly one geometry column, found more than one",
            Span::zero(),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loads_feature_collection_with_geometry_and_properties() {
        let bytes = br#"{
          "type": "FeatureCollection",
          "features": [
            {"type":"Feature","properties":{"GEOID":1,"name":"A"},
             "geometry":{"type":"Point","coordinates":[1,2]}},
            {"type":"Feature","properties":{"GEOID":2},
             "geometry":null}
          ]
        }"#;
        let table = read_geojson_from_bytes(Path::new("memory.geojson"), bytes).expect("load");
        assert_eq!(table.columns, vec!["GEOID", "name", GEOM_COLUMN]);
        assert_eq!(table.rows.len(), 2);
        assert!(matches!(table.rows[0].values[2], Value::Geometry(_)));
        // Missing property is null; null geometry is preserved as null.
        assert_eq!(table.rows[1].values[1], Value::Null);
        assert_eq!(table.rows[1].values[2], Value::Null);
    }

    #[test]
    fn bare_geometry_is_rejected() {
        let bytes = br#"{"type":"Point","coordinates":[0,0]}"#;
        let error = read_geojson_from_bytes(Path::new("memory.geojson"), bytes).unwrap_err();
        assert_eq!(error.code, "E1822");
    }

    #[test]
    fn geojson_round_trips_through_writer() {
        let bytes = br#"{
          "type":"FeatureCollection",
          "features":[
            {"type":"Feature","properties":{"name":"A"},
             "geometry":{"type":"Point","coordinates":[1,2]}}
          ]
        }"#;
        let table = read_geojson_from_bytes(Path::new("memory.geojson"), bytes).expect("load");
        let out = write_geojson_to_vec(&table).expect("write");
        let reloaded = read_geojson_from_bytes(Path::new("out.geojson"), &out).expect("reload");
        assert_eq!(reloaded.columns, vec!["name", GEOM_COLUMN]);
        assert!(matches!(reloaded.rows[0].values[1], Value::Geometry(_)));
    }

    #[test]
    fn write_rejects_table_without_geometry() {
        let table = Table::new(
            vec!["name".to_string()],
            vec![Row {
                values: vec![Value::String("A".to_string())],
            }],
        );
        let error = write_geojson_to_vec(&table).unwrap_err();
        assert_eq!(error.code, "E1712");
    }

    #[test]
    fn single_object_topojson_loads_with_transform() {
        let bytes = br#"{
          "type":"Topology",
          "transform":{"scale":[2.0,2.0],"translate":[100.0,200.0]},
          "objects":{"pts":{"type":"Point","coordinates":[5,10]}},
          "arcs":[]
        }"#;
        let table = read_topojson_from_bytes(Path::new("memory.topojson"), bytes).expect("load");
        assert_eq!(table.rows.len(), 1);
        match &table.rows[0].values[0] {
            Value::Geometry(geometry) => match geometry.as_ref() {
                Geometry::Point(point) => {
                    assert_eq!(
                        (point.x(), point.y()),
                        (5.0 * 2.0 + 100.0, 10.0 * 2.0 + 200.0)
                    );
                }
                other => panic!("expected point, got {other:?}"),
            },
            other => panic!("expected geometry, got {other:?}"),
        }
    }

    #[test]
    fn multi_object_topojson_is_rejected() {
        let bytes = br#"{"type":"Topology","arcs":[],
          "objects":{"a":{"type":"Point","coordinates":[0,0]},
                     "b":{"type":"Point","coordinates":[1,1]}}}"#;
        let error = read_topojson_from_bytes(Path::new("memory.topojson"), bytes).unwrap_err();
        assert_eq!(error.code, "E1821");
    }
}
