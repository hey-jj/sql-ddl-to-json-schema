//! Parse-tree conformance: every parser case must match its golden.

mod common;

use common::{assert_json_eq, load_goldens, run_parsed};

#[test]
fn parser_goldens_match() {
    let goldens = load_goldens("parser");
    assert!(!goldens.is_empty(), "expected parser goldens");
    let mut failures = Vec::new();
    let mut first_mismatch: Option<(String, serde_json::Value, serde_json::Value)> = None;
    for g in &goldens {
        match run_parsed(&g.query) {
            Ok(got) => {
                if got != g.value {
                    failures.push(format!("{} [{}] MISMATCH", g.jest_key, g.spec));
                    if first_mismatch.is_none() {
                        first_mismatch =
                            Some((format!("{} [{}]", g.jest_key, g.spec), got, g.value.clone()));
                    }
                }
            }
            Err(e) => failures.push(format!("{} [{}] ERROR {}", g.jest_key, g.spec, e)),
        }
    }
    if !failures.is_empty() {
        eprintln!(
            "{} parser goldens failed:\n{}",
            failures.len(),
            failures.join("\n")
        );
        if let Some((label, got, want)) = first_mismatch {
            assert_json_eq(&got, &want, &label);
        }
        panic!("{} parser goldens failed", failures.len());
    }
}
