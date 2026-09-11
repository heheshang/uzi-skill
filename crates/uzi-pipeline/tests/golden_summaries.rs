//! Differential test for `auto_summarize_dim` against the upstream Python engine.
//!
//! Golden text is produced by `tools/golden/dump_summarize.py`, which calls the
//! upstream `_auto_summarize_dim` with the same dimension dict and the score from
//! `score_dimensions` — exactly what `generate_synthesis` passes.

use serde_json::Value;
use uzi_core::testkit::{assert_json_eq, load_fixture, load_golden};
use uzi_pipeline::score::score_dimensions;
use uzi_pipeline::summarize::auto_summarize_dim;

const DIM_LABELS: &[(&str, &str)] = &[
    ("0_basic", "基础信息"),
    ("1_financials", "财报"),
    ("2_kline", "K线技术面"),
    ("3_macro", "宏观环境"),
    ("4_peers", "同行对比"),
    ("5_chain", "产业链"),
    ("6_research", "券商研报"),
    ("7_industry", "行业景气"),
    ("8_materials", "原材料"),
    ("9_futures", "期货关联"),
    ("10_valuation", "估值分位"),
    ("11_governance", "治理/减持"),
    ("12_capital_flow", "资金面"),
    ("13_policy", "政策与监管"),
    ("14_moat", "护城河"),
    ("15_events", "事件驱动"),
    ("16_lhb", "龙虎榜"),
    ("17_sentiment", "舆情"),
    ("18_trap", "杀猪盘"),
    ("19_contests", "实盘比赛"),
];

fn run_case(case: &str) {
    let raw = load_fixture(&format!("raw_data_{}", case));
    let dims_scored = score_dimensions(&raw);
    let expected = load_golden(case, "summaries");

    let mut actual = serde_json::Map::new();
    for (dim_key, label) in DIM_LABELS {
        // upstream: `dim = (raw.get("dimensions", {}).get(dim_key) or {})`
        let dim = match raw.get("dimensions").and_then(|d| d.get(*dim_key)) {
            Some(v) if uzi_core::py::truthy(v) => v.clone(),
            _ => Value::Object(serde_json::Map::new()),
        };
        let score = dims_scored
            .get("dimensions")
            .and_then(|d| d.get(*dim_key))
            .and_then(|d| d.get("score"))
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0);
        let text = auto_summarize_dim(dim_key, label, &dim, score);
        actual.insert((*dim_key).to_string(), Value::String(text));
    }

    assert_json_eq(&Value::Object(actual), &expected, &format!("summaries/{}", case));
}

#[test]
fn synthetic_summaries_match_upstream() {
    run_case("synthetic");
}

#[test]
fn sparse_summaries_match_upstream() {
    run_case("sparse");
}

#[test]
fn empty_summaries_match_upstream() {
    run_case("empty");
}
