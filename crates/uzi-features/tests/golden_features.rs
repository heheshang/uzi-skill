//! Differential tests: `extract_features` / `sanitize_features` vs the golden
//! artifacts produced by upstream Python (`tools/golden/dump_python.py`).

use serde_json::Value;
use uzi_core::testkit::{assert_json_eq, load_fixture, load_golden};
use uzi_features::{extract_features, sanitize_features};

fn check(case: &str) {
    let raw = load_fixture(&format!("raw_data_{}", case));
    let actual = extract_features(&raw, &raw["dimensions"]);
    let expected = load_golden(case, "features");
    assert_json_eq(&actual, &expected, &format!("extract_features/{}", case));

    let sanitized = sanitize_features(&actual);
    let expected_sanitized: Value = load_golden(case, "features_sanitized");
    assert_json_eq(
        &sanitized,
        &expected_sanitized,
        &format!("sanitize_features/{}", case),
    );
}

#[test]
fn synthetic_features_match_python() {
    check("synthetic");
}

#[test]
fn sparse_features_match_python() {
    check("sparse");
}

/// The golden `synthesis.json` embeds the full `friendly` block produced by
/// upstream `compute_friendly.main(ticker)`, so `compute_scenarios` /
/// `compute_exit_triggers` are differentially testable for both fixtures.
#[test]
fn friendly_matches_golden_synthesis_block() {
    for case in ["synthetic", "sparse"] {
        let raw = load_fixture(&format!("raw_data_{}", case));
        let dims = raw["dimensions"].clone();
        let synthesis = load_golden(case, "synthesis");
        let friendly = &synthesis["friendly"];

        assert_eq!(
            uzi_features::compute_scenarios(&raw, &dims),
            friendly["scenarios"],
            "compute_scenarios/{}",
            case
        );
        assert_eq!(
            uzi_features::compute_exit_triggers(&raw, &dims, &serde_json::json!({})),
            friendly["exit_triggers"],
            "compute_exit_triggers/{}",
            case
        );
    }
}

// NOTE: `detect_style` and `apply_style_weights` are covered in
// `quant_style_golden.rs`, not here — their quant branch reads the on-disk
// quant cache, which this test binary cannot seed without mutating
// `UZI_CACHE_ROOT` process-wide (and that is gitignored, so a clean
// checkout would otherwise fall through to `balanced`).
