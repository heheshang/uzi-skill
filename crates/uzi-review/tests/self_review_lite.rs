//! Profile-aware self-review: under `UZI_DEPTH=lite` only the 7 core dims are
//! required and sub-40% coverage is downgraded to `warning`.

use serde_json::Value;
use std::path::PathBuf;

fn golden(name: &str) -> Value {
    uzi_core::testkit::load_json(
        &PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/golden")
            .join(format!("{}.json", name)),
    )
}

#[test]
fn review_all_lite_profile_matches_upstream() {
    let root = std::env::temp_dir().join(format!("uzi-review-lite-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::env::set_var("UZI_CACHE_ROOT", &root);
    std::env::set_var("UZI_DEPTH", "lite");
    for k in ["UZI_LITE", "UZI_CLI_ONLY", "CI"] {
        std::env::remove_var(k);
    }

    let ticker = "002273.SZ";
    let raw = serde_json::json!({
        "ticker": "002273.SZ", "market": "A",
        "dimensions": {
            "0_basic": {"data": {"name": "N", "price": 10, "industry": "I", "market_cap": 100}},
            "1_financials": {"data": {"roe_history": [1]}},
            "2_kline": {"data": {"stage": "S"}}
        }
    });
    uzi_core::cache::write_task_output(ticker, "raw_data", &raw).unwrap();

    let report = uzi_review::review_all(ticker, None);
    println!("{}", uzi_review::format_human(&report));

    let mut actual = report.clone();
    actual["reviewed_at"] = serde_json::json!("<TS>");
    uzi_core::testkit::assert_json_eq(&actual, &golden("lite_self_review"), "self_review/lite");

    let _ = std::fs::remove_dir_all(&root);
}
