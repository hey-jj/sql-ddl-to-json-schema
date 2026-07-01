//! JSON Schema draft-07 formatter. Builds one schema document per table from
//! the compact table model.

use serde_json::{Map, Value};

use crate::parser::JsonSchemaOptions;

// Integer ranges. The bigint range uses the JavaScript safe-integer bounds to
// match the source, not the true 64-bit range.
const TINYINT: (i64, i64, i64, i64) = (-128, 127, 0, 255);
const SMALLINT: (i64, i64, i64, i64) = (-32768, 32767, 0, 65535);
const MEDIUMINT: (i64, i64, i64, i64) = (-8_388_608, 8_388_607, 0, 16_777_215);
const INT: (i64, i64, i64, i64) = (-2_147_483_648, 2_147_483_647, 0, 4_294_967_295);
const MAX_SAFE: i64 = 9_007_199_254_740_991;
const BIGINT: (i64, i64, i64, i64) = (-MAX_SAFE, MAX_SAFE, 0, MAX_SAFE);

/// Format compact tables into JSON Schema documents.
pub fn format(tables: &[Value], options: JsonSchemaOptions) -> Vec<Value> {
    tables.iter().map(|t| table_to_schema(t, options)).collect()
}

/// Build one schema document from a compact table.
fn table_to_schema(table: &Value, options: JsonSchemaOptions) -> Value {
    let name = table.get("name").and_then(Value::as_str).unwrap_or("");
    let comment = table
        .get("options")
        .and_then(|o| o.get("comment"))
        .and_then(Value::as_str);

    // Names that are primary key columns.
    let pk_columns: Vec<String> = table
        .get("primaryKey")
        .and_then(|pk| pk.get("columns"))
        .and_then(Value::as_array)
        .map(|cols| {
            cols.iter()
                .filter_map(|c| c.get("column").and_then(Value::as_str).map(String::from))
                .collect()
        })
        .unwrap_or_default();

    let empty = Vec::new();
    let columns = table
        .get("columns")
        .and_then(Value::as_array)
        .unwrap_or(&empty);

    let mut required: Vec<Value> = Vec::new();
    let mut definitions = Map::new();
    let mut properties = Map::new();

    for col in columns {
        let col_name = col.get("name").and_then(Value::as_str).unwrap_or("");
        let is_pk = pk_columns.iter().any(|n| n == col_name);
        let schema = column_to_schema(col, is_pk);

        // properties[name] = { $ref: ... }.
        let mut refobj = Map::new();
        refobj.insert(
            "$ref".into(),
            Value::String(format!("#/definitions/{}", col_name)),
        );
        properties.insert(col_name.to_string(), Value::Object(refobj));
        definitions.insert(col_name.to_string(), schema);

        // required when the column is not nullable.
        let nullable = col
            .get("options")
            .and_then(|o| o.get("nullable"))
            .and_then(Value::as_bool);
        if nullable == Some(false) {
            required.push(Value::String(col_name.to_string()));
        }
    }

    let mut doc = Map::new();
    doc.insert(
        "$schema".into(),
        Value::String("http://json-schema.org/draft-07/schema".into()),
    );
    doc.insert(
        "$comment".into(),
        Value::String(format!("JSON Schema for {} table", name)),
    );
    doc.insert("$id".into(), Value::String(name.to_string()));
    doc.insert("title".into(), Value::String(name.to_string()));
    if let Some(c) = comment {
        doc.insert("description".into(), Value::String(c.to_string()));
    }
    doc.insert("type".into(), Value::String("object".into()));
    doc.insert("required".into(), Value::Array(required));

    if options.use_ref {
        doc.insert("definitions".into(), Value::Object(definitions));
        doc.insert("properties".into(), Value::Object(properties));
    } else {
        // Flatten: properties hold the schemas, no definitions.
        doc.insert("properties".into(), Value::Object(definitions));
    }

    Value::Object(doc)
}

/// Build a column schema from a compact column.
fn column_to_schema(col: &Value, is_primary_key: bool) -> Value {
    let type_field = col.get("type").cloned().unwrap_or(Value::Null);
    let datatype = type_field.get("datatype").and_then(Value::as_str).unwrap_or("");
    let options = col.get("options");

    let unsigned = options
        .and_then(|o| o.get("unsigned"))
        .and_then(Value::as_bool)
        == Some(true);

    // Column-level default: keep only null or non-empty string defaults.
    let default = options.and_then(|o| o.get("default")).and_then(|d| match d {
        Value::Null => Some(Value::Null),
        Value::String(s) if !s.is_empty() => Some(Value::String(s.clone())),
        _ => None,
    });

    // Column-level comment.
    let comment = options
        .and_then(|o| o.get("comment"))
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty());

    // Build the datatype schema.
    let mut type_schema = datatype_schema(&type_field, datatype, unsigned);

    let mut json = Map::new();

    if is_primary_key {
        json.insert("$comment".into(), Value::String("primary key".into()));
        if let Some(Value::String(t)) = type_schema.get("type") {
            if t == "integer" || t == "number" {
                type_schema.insert("minimum".into(), Value::from(1));
            }
        }
    }

    if let Some(c) = comment {
        json.insert("description".into(), Value::String(c.to_string()));
    }

    // Copy each field of the type schema, skipping non-finite numbers. All the
    // numbers this formatter builds are finite, so this always copies.
    for (key, value) in type_schema.iter() {
        let keep = match value {
            Value::Number(n) => n.as_f64().map(|f| f.is_finite()).unwrap_or(false),
            _ => true,
        };
        if keep {
            json.insert(key.clone(), value.clone());
        }
    }

    if let Some(d) = default {
        json.insert("default".into(), d);
    }

    if datatype == "uuid" {
        json.remove("default");
    }

    Value::Object(json)
}

/// Build the base type schema for a datatype.
fn datatype_schema(type_field: &Value, datatype: &str, unsigned: bool) -> Map<String, Value> {
    let mut m = Map::new();
    m.insert("type".into(), Value::String(filter_type(datatype).into()));

    match datatype {
        "tinyint" => set_int_range(&mut m, TINYINT, unsigned),
        "smallint" => set_int_range(&mut m, SMALLINT, unsigned),
        "mediumint" => set_int_range(&mut m, MEDIUMINT, unsigned),
        "int" => set_int_range(&mut m, INT, unsigned),
        "bigint" => set_int_range(&mut m, BIGINT, unsigned),
        "decimal" | "float" => {
            let digits = type_field.get("digits").and_then(Value::as_i64).unwrap_or(0);
            let decimals = type_field.get("decimals").and_then(Value::as_i64).unwrap_or(0);
            let maximum = decimal_maximum(digits, decimals);
            m.insert("maximum".into(), number(maximum));
            if unsigned {
                m.insert("minimum".into(), Value::from(0));
            } else {
                m.insert("minimum".into(), number(-maximum));
            }
        }
        "date" => {
            m.insert("format".into(), Value::String("date".into()));
        }
        "time" => {
            m.insert("format".into(), Value::String("time".into()));
        }
        "datetime" => {
            m.insert("format".into(), Value::String("date-time".into()));
        }
        "year" => {
            let digits = type_field.get("digits").and_then(Value::as_i64).unwrap_or(0);
            m.insert("pattern".into(), Value::String(format!("\\d{{1,{}}}", digits)));
        }
        "char" | "binary" | "varchar" | "nvarchar" | "varbinary" | "text" => {
            if let Some(len) = type_field.get("length") {
                m.insert("maxLength".into(), len.clone());
            }
        }
        "enum" => {
            if let Some(vals) = type_field.get("values") {
                m.insert("enum".into(), vals.clone());
            }
        }
        "set" => {
            let vals: Vec<String> = type_field
                .get("values")
                .and_then(Value::as_array)
                .map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect())
                .unwrap_or_default();
            let opts = vals.join("|");
            m.insert(
                "pattern".into(),
                Value::String(format!("^({0})(,({0}))*$", opts)),
            );
        }
        "uuid" => {
            m.insert(
                "pattern".into(),
                Value::String("^[a-f\\d]{8}-([a-f\\d]{4}-){3}[a-f\\d]{12}$".into()),
            );
        }
        _ => {}
    }

    m
}

/// Map a datatype name to a JSON Schema type name.
fn filter_type(datatype: &str) -> &'static str {
    match datatype {
        "tinyint" | "smallint" | "mediumint" | "int" | "bigint" => "integer",
        "decimal" | "float" | "double" => "number",
        "boolean" => "boolean",
        _ => "string",
    }
}

/// Set the minimum and maximum for an integer type.
fn set_int_range(m: &mut Map<String, Value>, range: (i64, i64, i64, i64), unsigned: bool) {
    let (smin, smax, umin, umax) = range;
    if unsigned {
        m.insert("minimum".into(), Value::from(umin));
        m.insert("maximum".into(), Value::from(umax));
    } else {
        m.insert("minimum".into(), Value::from(smin));
        m.insert("maximum".into(), Value::from(smax));
    }
}

/// Compute the decimal or float maximum via the source's string method.
///
/// It builds `"9" * (digits - decimals) + "." + "9" * decimals` then parses it
/// as a number.
fn decimal_maximum(digits: i64, decimals: i64) -> f64 {
    let whole = (digits - decimals).max(0) as usize;
    let frac = decimals.max(0) as usize;
    let s = format!("{}.{}", "9".repeat(whole), "9".repeat(frac));
    s.parse::<f64>().unwrap_or(0.0)
}

/// Build a JSON number from an f64, rendering whole values as integers.
fn number(f: f64) -> Value {
    if f.is_finite() && f.fract() == 0.0 && f >= i64::MIN as f64 && f <= i64::MAX as f64 {
        Value::from(f as i64)
    } else {
        serde_json::Number::from_f64(f)
            .map(Value::Number)
            .unwrap_or(Value::Null)
    }
}
