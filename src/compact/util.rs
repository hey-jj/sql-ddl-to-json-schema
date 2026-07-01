//! Small helpers shared by the compact models.

use serde_json::Value;

/// Read a string field from a JSON object.
pub fn get_str<'a>(v: &'a Value, key: &str) -> Option<&'a str> {
    v.get(key).and_then(Value::as_str)
}

/// Whether a JSON value counts as defined: present and not null.
pub fn is_defined(v: Option<&Value>) -> bool {
    matches!(v, Some(x) if !x.is_null())
}
