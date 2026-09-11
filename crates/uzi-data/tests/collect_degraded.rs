//! Integration test: `collect()` degradation contract.
//!
//! Runs with the network deliberately unreachable (all HTTP proxied to a closed
//! port) and asserts `collect` still returns a well-formed legacy raw dict:
//! `ticker` plus every registered dim as `{data, source, fallback, _pipeline}`,
//! with no panic. The live counterpart is `tests/live_quote.rs`, gated by
//! `UZI_LIVE_TEST=1`.

use serde_json::Value;

fn dim_keys() -> Vec<&'static str> {
    vec![
        "0_basic",
        "1_financials",
        "2_kline",
        "3_macro",
        "4_peers",
        "5_chain",
        "6_fund_holders",
        "6_research",
        "7_industry",
        "8_materials",
        "9_futures",
        "10_valuation",
        "11_governance",
        "12_capital_flow",
        "13_policy",
        "14_moat",
        "15_events",
        "16_lhb",
        "17_sentiment",
        "18_trap",
        "19_contests",
    ]
}

/// Point every HTTP call at a closed port so the whole run is offline.
fn force_offline() {
    std::env::set_var("UZI_SKIP_PREFLIGHT", "1");
    std::env::set_var("UZI_HTTP_TIMEOUT", "2");
    std::env::set_var("HTTP_PROXY", "http://127.0.0.1:9");
    std::env::set_var("HTTPS_PROXY", "http://127.0.0.1:9");
    std::env::set_var("ALL_PROXY", "http://127.0.0.1:9");
    let dir = std::env::temp_dir().join(format!("uzi-data-test-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    std::env::set_var("UZI_CACHE_ROOT", dir);
}

fn assert_dim_shape(raw: &Value, key: &str) {
    let dim = &raw["dimensions"][key];
    assert!(
        dim.is_object(),
        "dim {key} must be an object, got {dim}"
    );
    for field in ["data", "source", "fallback"] {
        assert!(
            dim.get(field).is_some(),
            "dim {key} missing `{field}`: {dim}"
        );
    }
    assert!(
        dim.get("data").unwrap().is_object(),
        "dim {key} data must be an object"
    );
    assert!(
        dim.get("fallback").unwrap().is_boolean(),
        "dim {key} fallback must be a bool"
    );
    assert!(
        dim.get("_pipeline")
            .and_then(|p| p.get("quality"))
            .map(|q| q.is_string())
            .unwrap_or(false),
        "dim {key} missing _pipeline.quality"
    );
}

#[test]
fn collect_offline_is_well_formed_and_never_panics() {
    force_offline();
    let raw = uzi_data::collect("600519.SH", None, 4, None);
    assert!(raw.is_object(), "collect must return an object");
    assert_eq!(raw["ticker"], serde_json::json!("600519.SH"));

    for key in dim_keys() {
        assert_dim_shape(&raw, key);
    }

    // Every dim is accounted for under the nested contract, and the metadata /
    // overflow keys upstream keeps at the top level stay there.
    let dims = raw["dimensions"]
        .as_object()
        .expect("raw must carry a nested `dimensions` map");
    for key in dim_keys() {
        assert!(dims.contains_key(key), "missing dim {key}");
    }
    for key in ["ticker", "fund_managers", "similar_stocks"] {
        assert!(
            raw.get(key).is_some(),
            "top-level key {key} must survive nesting"
        );
    }
    // A dim must not also leak to the top level.
    assert!(
        raw.get("0_basic").is_none(),
        "dims belong under `dimensions`, not at the top level"
    );

    // Degraded dims must be honest about it.
    assert_eq!(
        raw["dimensions"]["0_basic"]["fallback"],
        serde_json::json!(true)
    );
    assert!(raw["dimensions"]["0_basic"]["data"].is_object());
}

#[test]
fn collect_offline_honours_resume_cache() {
    force_offline();
    let previous = serde_json::json!({
        "dimensions": {
            "0_basic": {
                "data": {"name": "贵州茅台", "price": 1500.0},
                "source": "resume:fixture",
                "fallback": false,
                "_pipeline": {"quality": "full", "fetched_at": now_epoch()}
            }
        }
    });
    let raw = uzi_data::collect("600519.SH", Some(&previous), 2, None);
    assert_dim_shape(&raw, "0_basic");
    assert_eq!(
        raw["dimensions"]["0_basic"]["source"],
        serde_json::json!("resume:fixture")
    );
    assert_eq!(
        raw["dimensions"]["0_basic"]["data"]["name"],
        serde_json::json!("贵州茅台")
    );
    assert_eq!(
        raw["dimensions"]["0_basic"]["fallback"],
        serde_json::json!(false)
    );
}

/// `collect` must honour the profile's fetcher whitelist.
///
/// Regression: `should_run_fetcher` / `fetchers_enabled` was defined but never
/// consulted, so `--depth lite` fetched all 20 dims — the documented "30 秒速判"
/// took as long as a full run, and the banner's `7/20 维` was false.
///
/// Skipped dims are still *present* (as `quality == "error"` placeholders) so
/// downstream code can iterate every registered dim; they are simply not fetched.
#[test]
fn collect_skips_dims_outside_the_profile_whitelist() {
    force_offline();
    let lite: std::collections::BTreeSet<String> = ["0_basic", "1_financials", "2_kline"]
        .iter()
        .map(|s| s.to_string())
        .collect();

    let raw = uzi_data::collect("600519.SH", None, 2, Some(&lite));
    let dims = raw["dimensions"].as_object().expect("nested dimensions");

    // Everything is still present, so the shape contract holds.
    for key in dim_keys() {
        assert!(dims.contains_key(key), "dim {key} should still be present");
    }

    // A dim outside the whitelist must be an unfetched placeholder.
    for key in ["3_macro", "18_trap", "19_contests", "4_peers"] {
        let quality = dims[key]
            .get("_pipeline")
            .and_then(|p| p.get("quality"))
            .and_then(|q| q.as_str())
            .unwrap_or("");
        assert!(
            matches!(quality, "error" | "missing"),
            "{key} was fetched despite being outside the whitelist (quality={quality})"
        );
    }

    // `None` means "no filtering" — the backward-compatible path.
    let unfiltered = uzi_data::collect("600519.SH", None, 2, None);
    assert!(unfiltered["dimensions"].as_object().unwrap().len() >= dims.len());
}

fn now_epoch() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

/// `collect` must emit the nested raw contract every consumer reads.
///
/// Regression: dims used to be emitted at the top level, so `score_dimensions`
/// (which reads `raw["dimensions"]`) scored defaults on every live run — "ROE
/// 0.0%" — while the fixture-based parity tests passed, because the fixtures were
/// already nested. This pins the shape on the live path.
#[test]
fn collect_emits_dims_under_the_nested_contract() {
    force_offline();
    let raw = uzi_data::collect("600519.SH", None, 2, None);

    let dims = raw["dimensions"].as_object().expect("nested dimensions");
    assert!(!dims.is_empty(), "nested map must hold the dims");

    // Dim-like keys must NOT also appear at the top level.
    for key in raw.as_object().unwrap().keys() {
        assert!(
            !(key.split_once('_')
                .map(|(i, n)| {
                    !i.is_empty()
                        && i.bytes().all(|b| b.is_ascii_digit())
                        && !n.is_empty()
                        && n.bytes().all(|b| b.is_ascii_lowercase() || b == b'_')
                })
                .unwrap_or(false)),
            "dim {key} leaked to the top level"
        );
    }

    // The shape consumers require: `score_dimensions` / `extract_features` /
    // `data_integrity` all read `raw["dimensions"]["<key>"]`, so a populated map
    // here is what makes them see data instead of an empty object.
    assert!(
        dims.contains_key("1_financials") && dims.contains_key("2_kline"),
        "expected the scored dims to be nested: {:?}",
        dims.keys().take(8).collect::<Vec<_>>()
    );
    assert!(
        dims["0_basic"].get("data").map(|d| d.is_object()).unwrap_or(false),
        "nested dims must keep their {{data, source, fallback}} shape"
    );
}
