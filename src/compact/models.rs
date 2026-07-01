//! Compact table model. Mirrors the source model classes and their mutations.

use serde_json::{json, Map, Value};

use super::util::{get_str, is_defined};

/// A column datatype.
#[derive(Debug, Clone)]
pub struct Datatype {
    pub datatype: String,
    pub display_width: Option<Value>,
    pub digits: Option<Value>,
    pub decimals: Option<Value>,
    pub length: Option<Value>,
    pub fractional: Option<Value>,
    pub values: Option<Vec<String>>,
    pub binary_collation: Option<bool>,
}

impl Datatype {
    /// Build from an `O_DATATYPE` node.
    pub fn from_def(json: &Value) -> Datatype {
        // json.def.def holds the inner fields.
        let inner = &json["def"]["def"];
        let raw = get_str(inner, "datatype").unwrap_or("").to_string();
        let mut dt = Datatype {
            datatype: filter_datatype(&raw),
            display_width: opt_field(inner, "displayWidth"),
            digits: opt_field(inner, "digits"),
            decimals: opt_field(inner, "decimals"),
            length: opt_field(inner, "length"),
            fractional: opt_field(inner, "fractional"),
            values: inner.get("values").and_then(|v| {
                v.as_array().map(|a| {
                    a.iter()
                        .filter_map(|x| x.as_str().map(String::from))
                        .collect()
                })
            }),
            binary_collation: inner.get("binaryCollation").and_then(Value::as_bool),
        };
        // Clear values if the array was absent.
        if inner.get("values").is_none() {
            dt.values = None;
        }
        dt
    }

    /// Length that is indexable by this datatype.
    pub fn max_indexable_size(&self) -> i64 {
        let d = self.datatype.as_str();
        let non_indexable = [
            "tinyint",
            "smallint",
            "mediumint",
            "int",
            "bigint",
            "decimal",
            "float",
            "double",
            "bit",
            "boolean",
            "date",
            "time",
            "datetime",
            "timestamp",
            "year",
            "json",
            "uuid",
        ];
        if non_indexable.contains(&d) {
            return 0;
        }
        let spatial = [
            "geometry",
            "point",
            "linestring",
            "polygon",
            "multipoint",
            "multilinestring",
            "multipolygon",
            "geometrycollection",
        ];
        if spatial.contains(&d) {
            return 0;
        }
        let indexable = [
            "blob",
            "text",
            "char",
            "binary",
            "varchar",
            "nvarchar",
            "varbinary",
        ];
        if indexable.contains(&d) {
            return self.length.as_ref().and_then(Value::as_i64).unwrap_or(0);
        }
        0
    }

    /// Render to JSON in the source key order.
    pub fn to_json(&self) -> Value {
        let mut m = Map::new();
        m.insert("datatype".into(), Value::String(self.datatype.clone()));
        insert_defined(&mut m, "displayWidth", &self.display_width);
        insert_defined(&mut m, "digits", &self.digits);
        insert_defined(&mut m, "decimals", &self.decimals);
        insert_defined(&mut m, "length", &self.length);
        insert_defined(&mut m, "fractional", &self.fractional);
        if let Some(vals) = &self.values {
            m.insert(
                "values".into(),
                Value::Array(vals.iter().cloned().map(Value::String).collect()),
            );
        }
        if let Some(b) = self.binary_collation {
            m.insert("binaryCollation".into(), Value::Bool(b));
        }
        Value::Object(m)
    }

    /// Deep clone. Note the source clone drops `binaryCollation`.
    pub fn clone_model(&self) -> Datatype {
        Datatype {
            datatype: self.datatype.clone(),
            display_width: self.display_width.clone(),
            digits: self.digits.clone(),
            decimals: self.decimals.clone(),
            length: self.length.clone(),
            fractional: self.fractional.clone(),
            values: self.values.clone(),
            binary_collation: None,
        }
    }
}

/// Normalize a datatype name.
fn filter_datatype(term: &str) -> String {
    let lower = term.to_lowercase();
    match lower.as_str() {
        "integer" => "int".into(),
        "numeric" => "decimal".into(),
        "bool" => "boolean".into(),
        "tinyblob" | "mediumblob" | "longblob" => "blob".into(),
        "tinytext" | "mediumtext" | "longtext" => "text".into(),
        "national char" => "char".into(),
        "nvarchar" => "varchar".into(),
        "character" => "char".into(),
        "nchar" => "char".into(),
        "uniqueidentifier" => "uuid".into(),
        _ => lower,
    }
}

/// Column options.
#[derive(Debug, Clone, Default)]
pub struct ColumnOptions {
    fields: Map<String, Value>,
}

impl ColumnOptions {
    /// Build from an array of `O_COLUMN_DEFINITION` nodes.
    pub fn from_array(defs: &[Value]) -> ColumnOptions {
        let mut fields = Map::new();
        for d in defs {
            if let Some(obj) = d.get("def").and_then(Value::as_object) {
                for (k, v) in obj {
                    fields.insert(k.clone(), v.clone());
                }
            }
        }
        lowercase_field(&mut fields, "collation");
        lowercase_field(&mut fields, "charset");
        lowercase_field(&mut fields, "storage");
        lowercase_field(&mut fields, "format");

        if !is_defined(fields.get("nullable")) {
            fields.insert("nullable".into(), Value::Bool(true));
        }
        if fields.get("zerofill").and_then(Value::as_bool) == Some(true) {
            fields.insert("unsigned".into(), Value::Bool(true));
        }
        if fields.get("primary").and_then(Value::as_bool) == Some(true) {
            fields.insert("nullable".into(), Value::Bool(false));
        }
        ColumnOptions { fields }
    }

    /// Set a field.
    pub fn set(&mut self, key: &str, value: Value) {
        self.fields.insert(key.into(), value);
    }

    /// Remove a field.
    pub fn remove(&mut self, key: &str) {
        self.fields.remove(key);
    }

    /// Whether the column is autoincrement.
    pub fn is_autoincrement(&self) -> bool {
        self.fields.get("autoincrement").and_then(Value::as_bool) == Some(true)
    }

    /// Whether the column is primary.
    pub fn is_primary(&self) -> bool {
        self.fields.get("primary").and_then(Value::as_bool) == Some(true)
    }

    /// Whether the column is unique.
    pub fn is_unique(&self) -> bool {
        self.fields.get("unique").and_then(Value::as_bool) == Some(true)
    }

    /// Render to JSON in the source key order.
    pub fn to_json(&self) -> Value {
        let mut m = Map::new();
        let f = &self.fields;
        copy_defined(&mut m, f, "unsigned");
        copy_defined(&mut m, f, "zerofill");
        copy_defined(&mut m, f, "charset");
        copy_defined(&mut m, f, "collation");
        copy_defined(&mut m, f, "nullable");
        copy_defined(&mut m, f, "default");
        copy_defined(&mut m, f, "autoincrement");
        copy_defined(&mut m, f, "unique");
        copy_defined(&mut m, f, "primary");
        copy_defined(&mut m, f, "invisible");
        copy_defined(&mut m, f, "format");
        copy_defined(&mut m, f, "storage");
        copy_defined(&mut m, f, "comment");
        copy_defined(&mut m, f, "onUpdate");

        // A string default of "null" becomes JSON null.
        if let Some(Value::String(sd)) = m.get("default") {
            if sd.to_lowercase() == "null" {
                m.insert("default".into(), Value::Null);
            }
        }

        copy_defined(&mut m, f, "invisibleWithSystemVersioning");
        copy_defined(&mut m, f, "invisibleWithoutSystemVersioning");
        Value::Object(m)
    }

    /// Deep clone.
    pub fn clone_model(&self) -> ColumnOptions {
        ColumnOptions {
            fields: self.fields.clone(),
        }
    }
}

/// An index column entry.
#[derive(Debug, Clone)]
pub struct IndexColumn {
    pub column: Option<String>,
    pub length: Option<Value>,
    pub sort: Option<String>,
}

impl IndexColumn {
    /// Build from a `P_INDEX_COLUMN` node.
    pub fn from_def(json: &Value) -> IndexColumn {
        let def = &json["def"];
        IndexColumn::from_object(def)
    }

    /// Build from a plain object holding column, length, sort.
    pub fn from_object(def: &Value) -> IndexColumn {
        let column = get_str(def, "column").map(String::from);
        let length = def.get("length").filter(|v| truthy(v)).cloned();
        let sort = def
            .get("sort")
            .filter(|v| truthy(v))
            .and_then(Value::as_str)
            .map(String::from);
        IndexColumn {
            column,
            length,
            sort,
        }
    }

    /// Render to JSON.
    pub fn to_json(&self) -> Value {
        let mut m = Map::new();
        m.insert(
            "column".into(),
            self.column
                .clone()
                .map(Value::String)
                .unwrap_or(Value::Null),
        );
        if let Some(l) = &self.length {
            m.insert("length".into(), l.clone());
        }
        if let Some(s) = &self.sort {
            m.insert("sort".into(), Value::String(s.clone()));
        }
        Value::Object(m)
    }

    /// Deep clone.
    pub fn clone_model(&self) -> IndexColumn {
        self.clone()
    }
}

/// Index options.
#[derive(Debug, Clone, Default)]
pub struct IndexOptions {
    pub key_block_size: Option<Value>,
    pub index_type: Option<String>,
    pub parser: Option<String>,
    pub comment: Option<String>,
    pub algorithm: Option<String>,
    pub lock: Option<String>,
}

impl IndexOptions {
    /// Build from an array of `O_INDEX_OPTION` nodes.
    pub fn from_array(opts: &[Value]) -> IndexOptions {
        let mut o = IndexOptions::default();
        for opt in opts {
            let def = &opt["def"];
            if is_defined(def.get("comment")) {
                o.comment = def
                    .get("comment")
                    .and_then(Value::as_str)
                    .map(|c| c.to_lowercase());
            }
            if is_defined(def.get("indexType")) {
                o.index_type = def["indexType"]
                    .get("def")
                    .and_then(Value::as_str)
                    .map(|c| c.to_lowercase());
            }
            if is_defined(def.get("keyBlockSize")) {
                o.key_block_size = def.get("keyBlockSize").cloned();
            }
            if is_defined(def.get("parser")) {
                o.parser = def.get("parser").and_then(Value::as_str).map(String::from);
            }
            if is_defined(def.get("algorithm")) {
                o.algorithm = def
                    .get("algorithm")
                    .and_then(Value::as_str)
                    .map(String::from);
            }
            if is_defined(def.get("lock")) {
                o.lock = def.get("lock").and_then(Value::as_str).map(String::from);
            }
        }
        o
    }

    /// Render to JSON in the source key order.
    pub fn to_json(&self) -> Value {
        let mut m = Map::new();
        if let Some(v) = &self.key_block_size {
            m.insert("keyBlockSize".into(), v.clone());
        }
        if let Some(v) = &self.index_type {
            m.insert("indexType".into(), Value::String(v.clone()));
        }
        if let Some(v) = &self.algorithm {
            m.insert("algorithm".into(), Value::String(v.clone()));
        }
        if let Some(v) = &self.comment {
            m.insert("comment".into(), Value::String(v.clone()));
        }
        if let Some(v) = &self.parser {
            m.insert("parser".into(), Value::String(v.clone()));
        }
        if let Some(v) = &self.lock {
            m.insert("lock".into(), Value::String(v.clone()));
        }
        Value::Object(m)
    }
}

/// A reference-on trigger and action pair.
#[derive(Debug, Clone)]
pub struct ColumnReferenceOn {
    pub trigger: String,
    pub action: String,
}

impl ColumnReferenceOn {
    /// Build from a parsed object, lowercasing both fields.
    pub fn from_object(json: &Value) -> ColumnReferenceOn {
        ColumnReferenceOn {
            action: get_str(json, "action").unwrap_or("").to_lowercase(),
            trigger: get_str(json, "trigger").unwrap_or("").to_lowercase(),
        }
    }

    /// Render to JSON.
    pub fn to_json(&self) -> Value {
        json!({ "trigger": self.trigger, "action": self.action })
    }
}

/// A column reference for foreign keys.
#[derive(Debug, Clone)]
pub struct ColumnReference {
    pub table: String,
    pub r#match: Option<String>,
    pub columns: Vec<IndexColumn>,
    pub on: Vec<ColumnReferenceOn>,
}

impl ColumnReference {
    /// Build from a `P_COLUMN_REFERENCE` node.
    pub fn from_def(json: &Value) -> ColumnReference {
        let def = &json["def"];
        let table = get_str(def, "table").unwrap_or("").to_string();
        let r#match = def
            .get("match")
            .filter(|v| truthy(v))
            .and_then(Value::as_str)
            .map(|m| m.to_lowercase());
        let cols = def.get("columns").and_then(Value::as_array);
        let columns = match cols {
            Some(a) if !a.is_empty() => a.iter().map(IndexColumn::from_def).collect(),
            _ => Vec::new(),
        };
        let on_arr = def.get("on").and_then(Value::as_array);
        let on = match on_arr {
            Some(a) if !a.is_empty() => a.iter().map(ColumnReferenceOn::from_object).collect(),
            _ => Vec::new(),
        };
        ColumnReference {
            table,
            r#match,
            columns,
            on,
        }
    }

    /// Render to JSON in the source key order.
    pub fn to_json(&self) -> Value {
        let mut m = Map::new();
        m.insert("table".into(), Value::String(self.table.clone()));
        if let Some(mt) = &self.r#match {
            m.insert("match".into(), Value::String(mt.clone()));
        }
        if !self.on.is_empty() {
            m.insert(
                "on".into(),
                Value::Array(self.on.iter().map(ColumnReferenceOn::to_json).collect()),
            );
        }
        if !self.columns.is_empty() {
            m.insert(
                "columns".into(),
                Value::Array(self.columns.iter().map(IndexColumn::to_json).collect()),
            );
        }
        Value::Object(m)
    }

    /// Deep clone.
    pub fn clone_model(&self) -> ColumnReference {
        ColumnReference {
            table: self.table.clone(),
            r#match: self.r#match.clone(),
            columns: self.columns.iter().map(IndexColumn::clone_model).collect(),
            on: self.on.clone(),
        }
    }
}

/// Copy a defined field from `src` into `dst`.
fn copy_defined(dst: &mut Map<String, Value>, src: &Map<String, Value>, key: &str) {
    if let Some(v) = src.get(key) {
        if !v.is_null() {
            dst.insert(key.into(), v.clone());
        }
    }
}

/// Insert an optional JSON value if it is defined (present and non-null).
fn insert_defined(dst: &mut Map<String, Value>, key: &str, value: &Option<Value>) {
    if let Some(v) = value {
        if !v.is_null() {
            dst.insert(key.into(), v.clone());
        }
    }
}

/// Lowercase a string field in place if present.
fn lowercase_field(fields: &mut Map<String, Value>, key: &str) {
    if let Some(Value::String(s)) = fields.get(key) {
        let lowered = s.to_lowercase();
        fields.insert(key.into(), Value::String(lowered));
    }
}

/// Read an optional field, keeping null out.
fn opt_field(v: &Value, key: &str) -> Option<Value> {
    match v.get(key) {
        Some(x) if !x.is_null() => Some(x.clone()),
        _ => None,
    }
}

/// JavaScript truthiness for the JSON values used here.
fn truthy(v: &Value) -> bool {
    match v {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().map(|f| f != 0.0).unwrap_or(false),
        Value::String(s) => !s.is_empty(),
        _ => true,
    }
}
