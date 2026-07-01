//! Table options model. Field order and lowercasing follow the source.

use serde_json::{Map, Value};

use super::util::is_defined;

/// The table option fields in output order, with whether each is lowercased.
///
/// `Always` lowercases string values unconditionally. `IfString` lowercases
/// only when the value is a string. `No` copies as-is.
enum Lower {
    No,
    Always,
    IfString,
}

const FIELDS: &[(&str, Lower)] = &[
    ("autoincrement", Lower::No),
    ("avgRowLength", Lower::No),
    ("charset", Lower::Always),
    ("checksum", Lower::No),
    ("collation", Lower::Always),
    ("comment", Lower::No),
    ("compression", Lower::Always),
    ("connection", Lower::No),
    ("dataDirectory", Lower::No),
    ("indexDirectory", Lower::No),
    ("delayKeyWrite", Lower::No),
    ("encryption", Lower::Always),
    ("encryptionKeyId", Lower::No),
    ("ietfQuotes", Lower::Always),
    ("engine", Lower::No),
    ("insertMethod", Lower::Always),
    ("keyBlockSize", Lower::No),
    ("maxRows", Lower::No),
    ("minRows", Lower::No),
    ("packKeys", Lower::IfString),
    ("pageChecksum", Lower::No),
    ("password", Lower::No),
    ("rowFormat", Lower::Always),
    ("statsAutoRecalc", Lower::IfString),
    ("statsPersistent", Lower::IfString),
    ("statsSamplePages", Lower::IfString),
    ("transactional", Lower::No),
    ("withSystemVersioning", Lower::No),
    ("tablespaceName", Lower::No),
    ("tablespaceStorage", Lower::Always),
    ("union", Lower::No),
];

/// Table options.
#[derive(Debug, Clone, Default)]
pub struct TableOptions {
    fields: Map<String, Value>,
}

impl TableOptions {
    /// Build from a `P_CREATE_TABLE_OPTIONS` node.
    pub fn from_def(json: &Value) -> TableOptions {
        TableOptions::from_array(
            json.get("def")
                .and_then(Value::as_array)
                .unwrap_or(&Vec::new()),
        )
    }

    /// Build from an array of `O_CREATE_TABLE_OPTION` nodes.
    pub fn from_array(options: &[Value]) -> TableOptions {
        let mut fields = Map::new();
        for opt in options {
            let def = &opt["def"];
            for (name, lower) in FIELDS {
                if let Some(v) = def.get(*name) {
                    if !v.is_null() {
                        fields.insert((*name).into(), apply_lower(v, lower));
                    }
                }
            }
        }
        TableOptions { fields }
    }

    /// Render to JSON in field order.
    pub fn to_json(&self) -> Value {
        let mut m = Map::new();
        for (name, _) in FIELDS {
            if let Some(v) = self.fields.get(*name) {
                if !v.is_null() {
                    m.insert((*name).into(), v.clone());
                }
            }
        }
        Value::Object(m)
    }

    /// Deep clone.
    pub fn clone_model(&self) -> TableOptions {
        TableOptions {
            fields: self.fields.clone(),
        }
    }

    /// Merge another set of options in, overwriting shared fields.
    ///
    /// The merge reads already-built fields, so it does not re-lowercase.
    pub fn merge_with(&mut self, other: &TableOptions) {
        for (name, _) in FIELDS {
            if is_defined(other.fields.get(*name)) {
                self.fields
                    .insert((*name).into(), other.fields[*name].clone());
            }
        }
    }
}

/// Apply the lowercasing rule to a value.
fn apply_lower(v: &Value, lower: &Lower) -> Value {
    match lower {
        Lower::No => v.clone(),
        Lower::Always => match v {
            Value::String(s) => Value::String(s.to_lowercase()),
            other => other.clone(),
        },
        Lower::IfString => match v {
            Value::String(s) => Value::String(s.to_lowercase()),
            other => other.clone(),
        },
    }
}
