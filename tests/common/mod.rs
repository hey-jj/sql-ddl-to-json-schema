//! Shared helpers for the golden-driven conformance tests.
//!
//! Each integration test binary links this module and uses a subset of it, so
//! some helpers look unused from any single binary.
#![allow(dead_code)]

use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use serde_json::Value;

use sql_ddl_to_json_schema::{JsonSchemaOptions, Parser};

/// A golden record: the query, the mode, and the expected output.
#[derive(Deserialize)]
pub struct Golden {
    pub group: String,
    pub spec: String,
    pub description: String,
    #[serde(rename = "jestKey")]
    pub jest_key: String,
    pub query: String,
    pub value: Value,
}

/// Directory holding the golden JSON files for a group.
fn golden_dir(group: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/goldens")
        .join(group)
}

/// Load every golden file in a group.
pub fn load_goldens(group: &str) -> Vec<Golden> {
    let dir = golden_dir(group);
    let mut out = Vec::new();
    for entry in fs::read_dir(&dir).expect("golden dir exists") {
        let path = entry.unwrap().path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let text = fs::read_to_string(&path).unwrap();
        let golden: Golden = serde_json::from_str(&text)
            .unwrap_or_else(|e| panic!("parse {}: {}", path.display(), e));
        out.push(golden);
    }
    // Deterministic order for readable failure output.
    out.sort_by(|a, b| a.jest_key.cmp(&b.jest_key));
    out
}

/// Run a query through the parse-tree handler.
pub fn run_parsed(query: &str) -> Result<Value, String> {
    let mut p = Parser::new("mysql").unwrap();
    p.feed(query);
    p.results().map_err(|e| e.to_string())
}

/// Run a query through the compact handler.
pub fn run_compact(query: &str) -> Result<Value, String> {
    let mut p = Parser::new("mysql").unwrap();
    p.feed(query);
    p.to_compact_json(None)
        .map(Value::Array)
        .map_err(|e| e.to_string())
}

/// Run a query through the JSON Schema handler with the given options.
pub fn run_json_schema(query: &str, use_ref: bool) -> Value {
    let mut p = Parser::new("mysql").unwrap();
    p.feed(query);
    let opts = JsonSchemaOptions { use_ref };
    Value::Array(
        p.to_json_schema_array(Some(opts), None)
            .expect("json schema should succeed"),
    )
}

/// Assert two JSON values are structurally equal, printing a compact diff.
pub fn assert_json_eq(got: &Value, want: &Value, label: &str) {
    if got != want {
        let got_s = serde_json::to_string_pretty(got).unwrap();
        let want_s = serde_json::to_string_pretty(want).unwrap();
        panic!(
            "mismatch for {}\n--- got ---\n{}\n--- want ---\n{}",
            label, got_s, want_s
        );
    }
}
