//! Compact-model conformance: every compact case must match its golden.

mod common;

use common::{assert_json_eq, load_goldens, run_compact};

#[test]
fn compact_goldens_match() {
    let goldens = load_goldens("compact");
    assert!(!goldens.is_empty(), "expected compact goldens");
    let mut failures = Vec::new();
    for g in &goldens {
        let got = run_compact(&g.query);
        if got != g.value {
            failures.push(g.jest_key.clone());
            if failures.len() == 1 {
                assert_json_eq(&got, &g.value, &format!("{} [{}]", g.jest_key, g.spec));
            }
        }
    }
    assert!(
        failures.is_empty(),
        "{} compact goldens failed: {:?}",
        failures.len(),
        failures
    );
}
