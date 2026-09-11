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

/// `detect_style` must return the style upstream returns for each fixture.
///
/// Derivation for `synthetic` (crystal-opto):
///   * `0 < pb(=3.11) < 1` fails → not `distressed`;
///   * the quant branch consults `lib.quant_signal.detect_quant_signal("002273",
///     raw["fund_managers"])`. That module's structural rule is "top-1 holding
///     < 2% of NAV → quant-like"; the cached holdings under `.cache/_quant`
///     contain exactly 3 quant-like funds holding 002273 (013332 / 022953 /
///     161017, all top-1 = 0.87%) → `count = 3 >= QUANT_FACTOR_MIN_COUNT` →
///     `quant_factor`. This is also what the golden `synthesis.json` pins
///     (`detected_style: "quant_factor"`), and it is the only style whose
///     `apply_style_weights` diagnostics reproduce the golden values
///     (active_weight 51.6 / bullish_weight 25.75 / neutral_weight 8.37).
///
/// Derivation for `sparse` (AAPL): pb=0, mcap=0, market="US" (so the A-share
/// small-cap branch cannot fire), industry="—", growth=0, dividend=0 → no rule
/// matches → `balanced` (confirmed by running upstream `detect_style`).
#[test]
fn detect_style_matches_upstream_for_fixtures() {
    // `detect_style`'s quant branch reads the on-disk quant cache, exactly like
    // upstream (which populates/reads `.cache`); no network is involved.
    for (case, expected) in [("synthetic", "quant_factor"), ("sparse", "balanced")] {
        let raw = load_fixture(&format!("raw_data_{}", case));
        let features = load_golden(case, "features");
        let style = uzi_features::detect_style(&features, &raw);
        assert_eq!(style, expected, "detect_style/{}", case);
    }
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

/// `apply_style_weights` diagnostics are embedded verbatim in the golden
/// `synthesis.json` (`style_diagnostics`), so they pin the weighted-scoring
/// arithmetic for the `synthetic` fixture.
#[test]
fn apply_style_weights_matches_golden_synthesis_diagnostics() {
    let raw = load_fixture("raw_data_synthetic");
    let features = load_golden("synthetic", "features");
    let panel = load_golden("synthetic", "panel");
    let dims = load_golden("synthetic", "dimensions");
    let synthesis = load_golden("synthetic", "synthesis");

    let style = uzi_features::detect_style(&features, &raw);
    let adj = uzi_features::apply_style_weights(&panel["investors"], &dims, &style);

    assert_eq!(synthesis["detected_style"], serde_json::json!(style));
    assert_eq!(adj["diagnostics"], synthesis["style_diagnostics"]);
    assert_eq!(
        uzi_features::style_label(&style),
        synthesis["style_label_cn"]
    );
    assert_eq!(
        uzi_features::style_explanation(&style),
        synthesis["style_explanation"]
    );
}
