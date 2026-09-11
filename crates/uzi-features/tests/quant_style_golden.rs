//! `detect_style` / `apply_style_weights` against the golden artifacts, with the
//! quant cache **seeded by the test**.
//!
//! `detect_style`'s quant branch is the only branch that reads the on-disk quant
//! cache (`_quant/<fund>/api_cache/top10_holdings*.json`). That cache is
//! gitignored, so a clean checkout has none — and without it the branch finds no
//! funds and the style falls through to `balanced`, which made these assertions
//! pass only on a machine that had already run the Python upstream.
//!
//! They now seed the minimal universe themselves, like `cache_glue.rs` does for
//! its own fixtures.
//!
//! The scenarios run sequentially inside one `#[test]`: each swaps
//! `UZI_CACHE_ROOT` process-wide, so running them as separate tests in this
//! binary would race (the same reason `name_resolution.rs` is structured this
//! way).

use serde_json::json;
use uzi_core::testkit::{load_fixture, load_golden, seed_quant_cache};

/// Seed a fresh cache root and point the process at it.
fn new_root(tag: &str) -> std::path::PathBuf {
    let root = std::env::temp_dir().join(format!(
        "uzi-features-quant-{}-{}",
        tag,
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    std::env::set_var("UZI_CACHE_ROOT", &root);
    root
}

#[test]
fn quant_style_scenarios_match_golden() {
    // ── Scenario A · the documented universe ────────────────────────────────
    //
    // Derivation for `synthetic` (crystal-opto, 002273): `0 < pb(=3.11) < 1`
    // fails → not `distressed`; the quant branch finds 3 quant-like holders →
    // `count = 3 >= QUANT_FACTOR_MIN_COUNT` → `quant_factor`.
    let root = new_root("full");
    // Three quant-like funds (top-1 = 0.87% of NAV) holding 002273.
    seed_quant_cache("002273", &[("013332", 0.87, 8), ("022953", 0.87, 8), ("161017", 0.87, 8)]);

    let raw = load_fixture("raw_data_synthetic");
    let features = load_golden("synthetic", "features");
    assert_eq!(
        uzi_features::detect_style(&features, &raw),
        "quant_factor",
        "detect_style/synthetic"
    );

    // Derivation for `sparse` (AAPL): pb=0, mcap=0, market="US" (so the A-share
    // small-cap branch cannot fire), industry="—", growth=0, dividend=0 → no
    // rule matches → `balanced`. It never consults the quant cache.
    let raw_us = load_fixture("raw_data_sparse");
    let features_us = load_golden("sparse", "features");
    assert_eq!(
        uzi_features::detect_style(&features_us, &raw_us),
        "balanced",
        "detect_style/sparse"
    );

    // ── Scenario B · weighted-scoring arithmetic ────────────────────────────
    //
    // `style_diagnostics` in the golden `synthesis.json` was produced under
    // `quant_factor`, so it only matches when `detect_style` resolves the same
    // way — hence the shared seed.
    let panel = load_golden("synthetic", "panel");
    let dims = load_golden("synthetic", "dimensions");
    let synthesis = load_golden("synthetic", "synthesis");
    let style = uzi_features::detect_style(&features, &raw);
    let adj = uzi_features::apply_style_weights(&panel["investors"], &dims, &style);

    assert_eq!(synthesis["detected_style"], json!(style));
    assert_eq!(adj["diagnostics"], synthesis["style_diagnostics"]);
    assert_eq!(uzi_features::style_label(&style), synthesis["style_label_cn"]);
    assert_eq!(
        uzi_features::style_explanation(&style),
        synthesis["style_explanation"]
    );
    let _ = std::fs::remove_dir_all(&root);

    // ── Scenario C · the threshold itself ───────────────────────────────────
    //
    // Fewer than `QUANT_FACTOR_MIN_COUNT` holders must NOT trigger the style;
    // otherwise the assertions above would pass even with an empty cache.
    let root = new_root("threshold");
    // Only two quant-like holders → count 2 < QUANT_FACTOR_MIN_COUNT.
    seed_quant_cache("002273", &[("013332", 0.87, 8), ("022953", 0.87, 8)]);
    assert_ne!(
        uzi_features::detect_style(&features, &raw),
        "quant_factor",
        "two quant-like holders must not reach QUANT_FACTOR_MIN_COUNT"
    );

    // A fund that holds the name but is not quant-like (top-1 far above 2%)
    // must not count towards the total either.
    seed_quant_cache("002273", &[("161017", 9.5, 3)]);
    assert_ne!(
        uzi_features::detect_style(&features, &raw),
        "quant_factor",
        "a concentrated fund is not quant-like"
    );

    let _ = std::fs::remove_dir_all(&root);

    // An empty cache is the clean-checkout case: no funds → `balanced`.
    let root = new_root("empty");
    assert_eq!(
        uzi_features::detect_style(&features, &raw),
        "balanced",
        "with no quant cache the branch must fall through"
    );

    std::env::remove_var("UZI_CACHE_ROOT");
    let _ = std::fs::remove_dir_all(&root);
}
