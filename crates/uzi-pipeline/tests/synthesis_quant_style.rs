//! `generate_synthesis` for the fixture whose style is `quant_factor`.
//!
//! `generate_synthesis` calls `detect_style`, whose quant branch reads the
//! gitignored `_quant/<fund>/api_cache/top10_holdings*.json` cache. On a clean
//! checkout that cache is absent, the branch finds no funds, the style falls
//! through to `balanced`, and the whole synthesis tree diverges from the golden
//! `quant_factor` artifact — so the universe is seeded here.
//!
//! Lives in its own integration binary: it mutates `UZI_CACHE_ROOT`
//! process-wide, which would race with the other tests in `golden_panel.rs`.
//! The fixtures whose style does not depend on the cache (`sparse` / `empty`)
//! stay in `golden_panel.rs`.

use serde_json::Value;
use uzi_core::testkit::{assert_json_eq, load_fixture, load_golden, seed_quant_cache};
use uzi_pipeline::panel::generate_panel;
use uzi_pipeline::score::score_dimensions;
use uzi_pipeline::synthesis::generate_synthesis;

/// The three quant-like funds holding 002273 in the documented fixture: top-1
/// holding is 0.87% of NAV (well under the 2% rule) and the name sits at rank 8.
const QUANT_FUNDS: &[(&str, f64, usize)] = &[
    ("013332", 0.87, 8),
    ("022953", 0.87, 8),
    ("161017", 0.87, 8),
];

#[test]
fn synthetic_synthesis_matches_upstream_with_seeded_quant_cache() {
    let root = std::env::temp_dir().join(format!("uzi-pipeline-quant-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    std::env::set_var("UZI_CACHE_ROOT", &root);

    seed_quant_cache("002273", QUANT_FUNDS);

    let raw = load_fixture("raw_data_synthetic");
    let dims = score_dimensions(&raw);
    let panel = generate_panel(&dims, &raw);
    let actual = generate_synthesis(&raw, &dims, &panel, None);
    let expected: Value = load_golden("synthetic", "synthesis");

    // The premise: this fixture is only `quant_factor` because the cache says so.
    // If that ever changes, the comparison below would silently stop testing the
    // quant path.
    assert_eq!(
        expected["detected_style"],
        serde_json::json!("quant_factor"),
        "golden fixture no longer pins the quant style — re-derive this test"
    );
    assert_eq!(actual["detected_style"], expected["detected_style"]);

    assert_json_eq(&actual, &expected, "generate_synthesis/synthetic");

    std::env::remove_var("UZI_CACHE_ROOT");
    let _ = std::fs::remove_dir_all(&root);
}
