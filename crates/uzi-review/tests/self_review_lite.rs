//! Profile-aware self-review: under `UZI_DEPTH=lite` only the 7 core dims are
//! required and sub-40% coverage is downgraded to `warning`.
//!
//! ## Port rewrites (intentional deviations from upstream)
//!
//! `lite_self_review.json` is upstream's recorded output. Two of its
//! `suggested_fix` strings name the Python entry points the port removed
//! (`run.py`, `generate_panel()`); an agent reading `_review_issues.json` must
//! not be told to run a script that does not exist here. `PORT_REWRITES` maps
//! the upstream text to the port's, is applied to the *fixture* at compare time
//! (so the fixture stays a faithful upstream record), and asserts every entry
//! actually fired — a stale rewrite fails loudly instead of silently masking a
//! real diff.

use serde_json::Value;
use std::path::PathBuf;

/// upstream `suggested_fix` → the port's replacement.
const PORT_REWRITES: &[(&str, &str)] = &[
    (
        "重跑 run.py <ticker> --no-resume 或手动 fetch_X",
        "重跑 uzi <ticker> --no-resume --stage1（该维度应跑的 fetcher 全部缺失）",
    ),
    (
        "重跑 generate_panel()",
        "重跑 uzi <ticker> --no-resume --stage1（评委面板由 stage1 重新生成）",
    ),
];

/// Apply [`PORT_REWRITES`] to every `suggested_fix` in a report tree, returning
/// how many entries fired.
fn apply_port_rewrites(v: &mut Value, fired: &mut Vec<&'static str>) {
    match v {
        Value::Object(map) => {
            if let Some(Value::String(s)) = map.get_mut("suggested_fix") {
                for (upstream, port) in PORT_REWRITES {
                    if s == upstream {
                        *s = (*port).to_string();
                        fired.push(upstream);
                    }
                }
            }
            for (_, child) in map.iter_mut() {
                apply_port_rewrites(child, fired);
            }
        }
        Value::Array(items) => {
            for item in items.iter_mut() {
                apply_port_rewrites(item, fired);
            }
        }
        _ => {}
    }
}

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

    let mut expected = golden("lite_self_review");
    let mut fired: Vec<&'static str> = Vec::new();
    apply_port_rewrites(&mut expected, &mut fired);
    for (upstream, _) in PORT_REWRITES {
        assert!(
            fired.contains(upstream),
            "PORT_REWRITES entry never matched the fixture: {:?} — the upstream \
             text changed; update the mapping (or drop it if the fix text is gone)",
            upstream
        );
    }
    uzi_core::testkit::assert_json_eq(&actual, &expected, "self_review/lite");

    let _ = std::fs::remove_dir_all(&root);
}
