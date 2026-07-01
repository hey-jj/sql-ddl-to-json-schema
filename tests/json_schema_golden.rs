//! JSON Schema conformance: the ref and no-ref documents must match goldens.

mod common;

use common::{assert_json_eq, load_goldens, run_json_schema};

#[test]
fn json_schema_goldens_match() {
    let goldens = load_goldens("json-schema");
    assert_eq!(goldens.len(), 2, "expected ref and no-ref goldens");
    for g in &goldens {
        // The description names which mode to run.
        let use_ref = g.description.contains("with ref");
        let got = run_json_schema(&g.query, use_ref);
        assert_json_eq(&got, &g.value, &format!("{} [{}]", g.jest_key, g.spec));
    }
}
