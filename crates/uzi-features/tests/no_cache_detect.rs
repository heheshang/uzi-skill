//! `detect_style` must stay total when the quant cache is absent (fresh
//! checkout, CI, temp cwd): the quant branch finds no funds and the style falls
//! through to the deterministic rules. Lives in its own integration binary
//! because it mutates `UZI_CACHE_ROOT` process-wide.

use uzi_core::testkit::{load_fixture, load_golden};

#[test]
fn detect_style_defaults_without_any_cache() {
    let missing = std::env::temp_dir().join(format!("uzi_features_absent_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&missing);
    std::env::set_var("UZI_CACHE_ROOT", &missing);

    let raw = load_fixture("raw_data_synthetic");
    let features = load_golden("synthetic", "features");
    // No quant funds discoverable → no style rule matches → balanced, no panic.
    assert_eq!(uzi_features::detect_style(&features, &raw), "balanced");

    // A caller-supplied fund_managers list whose holdings are not cached is
    // equally harmless.
    let raw_with_funds = serde_json::json!({
        "ticker": "002273.SZ",
        "fund_managers": [{"fund_code": "013332", "fund_name": "x"}]
    });
    assert_eq!(uzi_features::detect_style(&features, &raw_with_funds), "balanced");

    std::env::remove_var("UZI_CACHE_ROOT");
}
