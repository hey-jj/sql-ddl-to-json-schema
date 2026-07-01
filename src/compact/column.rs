//! Column model with key extraction.

use serde_json::{Map, Value};

use super::keys::{ForeignKey, KeyIndex, PrimaryKey};
use super::models::{ColumnOptions, ColumnReference, Datatype, IndexColumn};

/// A table column.
#[derive(Debug, Clone)]
pub struct Column {
    pub name: String,
    pub datatype: Datatype,
    pub reference: Option<ColumnReference>,
    pub options: Option<ColumnOptions>,
}

impl Column {
    /// Build from an `O_CREATE_TABLE_CREATE_DEFINITION` holding a column.
    pub fn from_def(json: &Value) -> Column {
        let column = &json["def"]["column"];
        let obj = ColumnObject {
            name: column["name"].as_str().unwrap_or("").to_string(),
            datatype: column["def"]["datatype"].clone(),
            reference: column["def"].get("reference").cloned(),
            column_definition: column["def"].get("columnDefinition").cloned(),
        };
        Column::from_object(&obj)
    }

    /// Build from a plain object with name, datatype, reference, definitions.
    pub fn from_object(json: &ColumnObject) -> Column {
        let datatype = Datatype::from_def(&json.datatype);
        let reference = json
            .reference
            .as_ref()
            .filter(|v| !v.is_null())
            .map(ColumnReference::from_def);
        let options = json.column_definition.as_ref().map(|cd| {
            let arr = cd.as_array().cloned().unwrap_or_default();
            ColumnOptions::from_array(&arr)
        });
        Column {
            name: json.name.clone(),
            datatype,
            reference,
            options,
        }
    }

    /// Render to JSON: name, type, options, reference.
    pub fn to_json(&self) -> Value {
        let mut m = Map::new();
        m.insert("name".into(), Value::String(self.name.clone()));
        m.insert("type".into(), self.datatype.to_json());
        if let Some(o) = &self.options {
            m.insert("options".into(), o.to_json());
        }
        if let Some(r) = &self.reference {
            m.insert("reference".into(), r.to_json());
        }
        Value::Object(m)
    }

    /// Deep clone. The source clone drops the reference.
    pub fn clone_model(&self) -> Column {
        Column {
            name: self.name.clone(),
            datatype: self.datatype.clone_model(),
            reference: None,
            options: self.options.as_ref().map(ColumnOptions::clone_model),
        }
    }

    /// Whether this column is a primary key by option.
    pub fn is_primary_key(&self) -> bool {
        self.options
            .as_ref()
            .map(|o| o.is_primary())
            .unwrap_or(false)
    }

    /// Whether this column is a unique key by option.
    pub fn is_unique_key(&self) -> bool {
        self.options
            .as_ref()
            .map(|o| o.is_unique())
            .unwrap_or(false)
    }

    /// Whether this column has a reference.
    pub fn is_foreign_key(&self) -> bool {
        self.reference.is_some()
    }

    /// Extract a primary key if this column is primary. Removes `primary`.
    pub fn extract_primary_key(&mut self) -> Option<PrimaryKey> {
        if !self.is_primary_key() {
            return None;
        }
        if let Some(o) = &mut self.options {
            o.remove("primary");
        }
        let mut pk = PrimaryKey::default();
        pk.push_column(IndexColumn {
            column: Some(self.name.clone()),
            length: None,
            sort: None,
        });
        Some(pk)
    }

    /// Extract a foreign key if this column has a reference. Removes it.
    pub fn extract_foreign_key(&mut self) -> Option<ForeignKey> {
        if !self.is_foreign_key() {
            return None;
        }
        let reference = self.reference.take().unwrap();
        let fk = ForeignKey {
            name: None,
            columns: vec![IndexColumn {
                column: Some(self.name.clone()),
                length: None,
                sort: None,
            }],
            reference,
        };
        Some(fk)
    }

    /// Extract a unique key if this column is unique. Removes `unique`.
    pub fn extract_unique_key(&mut self) -> Option<KeyIndex> {
        if !self.is_unique_key() {
            return None;
        }
        if let Some(o) = &mut self.options {
            o.remove("unique");
        }
        let uk = KeyIndex {
            name: None,
            index_type: None,
            columns: vec![IndexColumn {
                column: Some(self.name.clone()),
                length: None,
                sort: None,
            }],
            options: None,
        };
        Some(uk)
    }
}

/// A plain object used to build a column.
pub struct ColumnObject {
    pub name: String,
    pub datatype: Value,
    pub reference: Option<Value>,
    pub column_definition: Option<Value>,
}
