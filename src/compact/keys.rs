//! Index and key models: primary key, unique key, index, fulltext, spatial,
//! and foreign key. Each mirrors the source model with the same mutations.

use serde_json::{Map, Value};

use super::models::{ColumnReference, IndexColumn, IndexOptions};
use super::table::Table;

/// Read columns from an object as index columns.
fn columns_from(json: &Value) -> Vec<IndexColumn> {
    json.get("columns")
        .and_then(Value::as_array)
        .map(|a| a.iter().map(IndexColumn::from_def).collect())
        .unwrap_or_default()
}

/// Read an optional name if truthy.
fn name_from(json: &Value) -> Option<String> {
    match json.get("name") {
        Some(Value::String(s)) if !s.is_empty() => Some(s.clone()),
        _ => None,
    }
}

/// Read an index type from `index.def` lowercased.
fn index_type_from(json: &Value) -> Option<String> {
    json.get("index")
        .filter(|v| !v.is_null())
        .and_then(|i| i.get("def"))
        .and_then(Value::as_str)
        .map(|s| s.to_lowercase())
}

/// Read options as `IndexOptions` if the array is non-empty.
fn options_from(json: &Value) -> Option<IndexOptions> {
    match json.get("options").and_then(Value::as_array) {
        Some(a) if !a.is_empty() => Some(IndexOptions::from_array(a)),
        _ => None,
    }
}

/// A primary key.
#[derive(Debug, Clone, Default)]
pub struct PrimaryKey {
    pub name: Option<String>,
    pub index_type: Option<String>,
    pub columns: Option<Vec<IndexColumn>>,
    pub options: Option<IndexOptions>,
}

impl PrimaryKey {
    /// Build from a create definition holding a `primaryKey`.
    pub fn from_def(json: &Value) -> PrimaryKey {
        PrimaryKey::from_object(&json["def"]["primaryKey"])
    }

    /// Build from a plain object.
    pub fn from_object(json: &Value) -> PrimaryKey {
        PrimaryKey {
            columns: Some(columns_from(json)),
            name: name_from(json),
            index_type: index_type_from(json),
            options: options_from(json),
        }
    }

    /// Render to JSON in the source key order.
    pub fn to_json(&self) -> Value {
        let mut m = Map::new();
        let cols = self.columns.clone().unwrap_or_default();
        m.insert(
            "columns".into(),
            Value::Array(cols.iter().map(IndexColumn::to_json).collect()),
        );
        if let Some(n) = &self.name {
            m.insert("name".into(), Value::String(n.clone()));
        }
        if let Some(o) = &self.options {
            m.insert("options".into(), o.to_json());
        }
        if let Some(it) = &self.index_type {
            m.insert("indexType".into(), Value::String(it.clone()));
        }
        Value::Object(m)
    }

    /// Deep clone.
    pub fn clone_model(&self) -> PrimaryKey {
        PrimaryKey {
            name: self.name.clone(),
            index_type: self.index_type.clone(),
            columns: self
                .columns
                .as_ref()
                .map(|c| c.iter().map(IndexColumn::clone_model).collect()),
            options: self.options.clone(),
        }
    }

    /// Push a single index column.
    pub fn push_column(&mut self, col: IndexColumn) {
        self.columns.get_or_insert_with(Vec::new).push(col);
    }

    /// Drop a column by name. Returns whether it was removed.
    pub fn drop_column(&mut self, name: &str) -> bool {
        match &mut self.columns {
            Some(cols) => drop_col(cols, name),
            None => false,
        }
    }

    /// Whether the table has all of this key's columns.
    pub fn has_all_columns(&self, table: &Table) -> bool {
        has_all_columns(self.columns.as_deref().unwrap_or(&[]), table)
    }

    /// Rename a column.
    pub fn rename_column(&mut self, old: &str, new: &str) {
        if let Some(cols) = &mut self.columns {
            for c in cols {
                if c.column.as_deref() == Some(old) {
                    c.column = Some(new.to_string());
                }
            }
        }
    }
}

/// Shared struct for unique key, index, fulltext, and spatial indexes, which
/// share the same shape but differ in `toJSON` key order.
#[derive(Debug, Clone, Default)]
pub struct KeyIndex {
    pub name: Option<String>,
    pub index_type: Option<String>,
    pub columns: Vec<IndexColumn>,
    pub options: Option<IndexOptions>,
}

impl KeyIndex {
    /// Build from a create definition holding the given sub-key, or from a
    /// `P_CREATE_INDEX` def.
    pub fn from_def(json: &Value, sub_key: &str) -> KeyIndex {
        let obj = match json.get("id").and_then(Value::as_str) {
            Some("P_CREATE_INDEX") => &json["def"],
            _ => &json["def"][sub_key],
        };
        KeyIndex::from_object(obj)
    }

    /// Build from a plain object.
    pub fn from_object(json: &Value) -> KeyIndex {
        KeyIndex {
            name: name_from(json),
            index_type: index_type_from(json),
            columns: columns_from(json),
            options: options_from(json),
        }
    }

    /// Drop a column by name. Returns whether it was removed.
    pub fn drop_column(&mut self, name: &str) -> bool {
        drop_col(&mut self.columns, name)
    }

    /// Whether the table has all of this key's columns.
    pub fn has_all_columns(&self, table: &Table) -> bool {
        has_all_columns(&self.columns, table)
    }

    /// Fill unset index column lengths from the table's datatypes.
    pub fn set_index_size_from_table(&mut self, table: &Table) {
        set_index_size(&mut self.columns, table);
    }

    /// Rename a column.
    pub fn rename_column(&mut self, old: &str, new: &str) {
        for c in &mut self.columns {
            if c.column.as_deref() == Some(old) {
                c.column = Some(new.to_string());
            }
        }
    }

    /// Deep clone.
    pub fn clone_model(&self) -> KeyIndex {
        KeyIndex {
            name: self.name.clone(),
            index_type: self.index_type.clone(),
            columns: self.columns.iter().map(IndexColumn::clone_model).collect(),
            options: self.options.clone(),
        }
    }

    /// Render a unique key to JSON: columns, name, indexType, options.
    pub fn to_json_unique(&self) -> Value {
        let mut m = self.base_columns();
        if let Some(n) = &self.name {
            m.insert("name".into(), Value::String(n.clone()));
        }
        if let Some(it) = &self.index_type {
            m.insert("indexType".into(), Value::String(it.clone()));
        }
        if let Some(o) = &self.options {
            m.insert("options".into(), o.to_json());
        }
        Value::Object(m)
    }

    /// Render an index to JSON: columns, options, indexType, name.
    pub fn to_json_index(&self) -> Value {
        let mut m = self.base_columns();
        if let Some(o) = &self.options {
            m.insert("options".into(), o.to_json());
        }
        if let Some(it) = &self.index_type {
            m.insert("indexType".into(), Value::String(it.clone()));
        }
        if let Some(n) = &self.name {
            m.insert("name".into(), Value::String(n.clone()));
        }
        Value::Object(m)
    }

    /// Render a fulltext or spatial index to JSON: columns, name, options.
    pub fn to_json_ft_spatial(&self) -> Value {
        let mut m = self.base_columns();
        if let Some(n) = &self.name {
            m.insert("name".into(), Value::String(n.clone()));
        }
        if let Some(o) = &self.options {
            m.insert("options".into(), o.to_json());
        }
        Value::Object(m)
    }

    fn base_columns(&self) -> Map<String, Value> {
        let mut m = Map::new();
        m.insert(
            "columns".into(),
            Value::Array(self.columns.iter().map(IndexColumn::to_json).collect()),
        );
        m
    }
}

/// A foreign key.
#[derive(Debug, Clone)]
pub struct ForeignKey {
    pub name: Option<String>,
    pub columns: Vec<IndexColumn>,
    pub reference: ColumnReference,
}

impl ForeignKey {
    /// Build from a create definition holding a `foreignKey`.
    pub fn from_def(json: &Value) -> ForeignKey {
        ForeignKey::from_object(&json["def"]["foreignKey"])
    }

    /// Build from a plain object.
    pub fn from_object(json: &Value) -> ForeignKey {
        ForeignKey {
            columns: columns_from(json),
            reference: ColumnReference::from_def(&json["reference"]),
            name: name_from(json),
        }
    }

    /// Render to JSON: columns, reference, name.
    pub fn to_json(&self) -> Value {
        let mut m = Map::new();
        m.insert(
            "columns".into(),
            Value::Array(self.columns.iter().map(IndexColumn::to_json).collect()),
        );
        m.insert("reference".into(), self.reference.to_json());
        if let Some(n) = &self.name {
            m.insert("name".into(), Value::String(n.clone()));
        }
        Value::Object(m)
    }

    /// Deep clone.
    pub fn clone_model(&self) -> ForeignKey {
        ForeignKey {
            name: self.name.clone(),
            columns: self.columns.iter().map(IndexColumn::clone_model).collect(),
            reference: self.reference.clone_model(),
        }
    }

    /// Drop a column by name. Returns whether it was removed.
    pub fn drop_column(&mut self, name: &str) -> bool {
        drop_col(&mut self.columns, name)
    }

    /// Fill unset index column lengths from the table's datatypes.
    pub fn set_index_size_from_table(&mut self, table: &Table) {
        set_index_size(&mut self.columns, table);
    }

    /// Whether this key references the given table and column.
    pub fn references_table_and_column(&self, table_name: &str, column_name: &str) -> bool {
        self.reference.table == table_name
            && self
                .reference
                .columns
                .iter()
                .any(|ic| ic.column.as_deref() == Some(column_name))
    }

    /// Whether this key references the given table.
    pub fn references_table(&self, table_name: &str) -> bool {
        self.reference.table == table_name
    }

    /// Rename a referenced column.
    pub fn rename_column(&mut self, old: &str, new: &str) {
        for c in &mut self.reference.columns {
            if c.column.as_deref() == Some(old) {
                c.column = Some(new.to_string());
            }
        }
    }

    /// Update the referenced table name.
    pub fn update_referenced_table_name(&mut self, new_name: &str) {
        self.reference.table = new_name.to_string();
    }
}

/// Drop a column by name from a list, returning whether it was removed. This
/// mirrors the source `some`-based index search and splice idiom.
fn drop_col(cols: &mut Vec<IndexColumn>, name: &str) -> bool {
    let pos = cols.iter().position(|c| c.column.as_deref() == Some(name));
    match pos {
        Some(p) => {
            cols.remove(p);
            true
        }
        None => false,
    }
}

/// Fill unset index column lengths from the table's datatypes.
fn set_index_size(cols: &mut [IndexColumn], table: &Table) {
    for ic in cols {
        if ic.length.is_some() {
            continue;
        }
        if let Some(name) = &ic.column {
            if let Some(col) = table.get_column(name) {
                let size = col.datatype.max_indexable_size();
                if size > 0 {
                    ic.length = Some(Value::from(size));
                }
            }
        }
    }
}

/// Whether the table has all of the given index columns.
fn has_all_columns(cols: &[IndexColumn], table: &Table) -> bool {
    let matched = table
        .columns_ref()
        .iter()
        .filter(|tc| {
            cols.iter()
                .any(|ic| ic.column.as_deref() == Some(tc.name.as_str()))
        })
        .count();
    matched == cols.len()
}
