//! Compact formatter. Replays DDL statements into a table model.

mod column;
mod database;
mod keys;
mod models;
mod table;
mod table_options;
mod util;

use serde_json::Value;

use database::Database;

/// Format a `MAIN` parse tree into an array of compact table objects.
///
/// Returns an error if the tree root is not `{ id: "MAIN", ... }`.
pub fn format(json: &Value) -> Result<Vec<Value>, String> {
    if json.get("id").and_then(Value::as_str) != Some("MAIN") {
        return Err(
            "Invalid JSON format provided for CompactFormatter. Please provide JSON from root element, containing { id: MAIN }."
                .to_string(),
        );
    }
    let dds = json.get("def").and_then(Value::as_array).cloned().unwrap_or_default();
    let mut database = Database::new();
    database.parse_dds_collection(&dds);
    Ok(database.tables.iter().map(table::Table::to_json).collect())
}
