//! Differential tests for the panel and synthesis stages against upstream Python.
//!
//! Golden artifacts come from `tools/golden/dump_python.py`. Upstream's persona
//! flavor line is chosen with an unseeded `random.choice`, so `assert_json_eq`
//! compares only the deterministic tail of `comment`; persona *membership* is
//! asserted separately below.

use serde_json::Value;
use uzi_core::testkit::{assert_json_eq, assert_persona_line_known, load_fixture, load_golden};
use uzi_pipeline::panel::generate_panel;
use uzi_pipeline::score::score_dimensions;
use uzi_pipeline::synthesis::generate_synthesis;

fn case_inputs(case: &str) -> (Value, Value, Value) {
    let raw = load_fixture(&format!("raw_data_{}", case));
    let dims = score_dimensions(&raw);
    let panel = generate_panel(&dims, &raw);
    (raw, dims, panel)
}

fn run_panel_case(case: &str) {
    let (_, _, panel) = case_inputs(case);
    let expected = load_golden(case, "panel");
    assert_json_eq(&panel, &expected, &format!("generate_panel/{}", case));
}

fn run_synthesis_case(case: &str) {
    let (raw, dims, panel) = case_inputs(case);
    let actual = generate_synthesis(&raw, &dims, &panel, None);
    let expected = load_golden(case, "synthesis");
    assert_json_eq(&actual, &expected, &format!("generate_synthesis/{}", case));
}

#[test]
fn synthetic_panel_matches_upstream() {
    run_panel_case("synthetic");
}

#[test]
fn sparse_panel_matches_upstream() {
    run_panel_case("sparse");
}

#[test]
fn empty_panel_matches_upstream() {
    run_panel_case("empty");
}

#[test]
fn synthetic_synthesis_matches_upstream() {
    run_synthesis_case("synthetic");
}

#[test]
fn sparse_synthesis_matches_upstream() {
    run_synthesis_case("sparse");
}

#[test]
fn empty_synthesis_matches_upstream() {
    run_synthesis_case("empty");
}

/// Every produced persona line must be one upstream could have produced, with the
/// context substituted exactly as `investor_personas.get_comment` does.
#[test]
fn persona_lines_come_from_the_upstream_pool() {
    let pools = load_golden("synthetic", "persona_pools");
    let (raw, dims, panel) = case_inputs("synthetic");
    let _ = dims;

    let dims_data = raw.get("dimensions").cloned().unwrap_or(Value::Null);
    let basic = dims_data
        .get("0_basic")
        .and_then(|d| d.get("data"))
        .cloned()
        .unwrap_or(Value::Null);
    let fin = dims_data
        .get("1_financials")
        .and_then(|d| d.get("data"))
        .cloned()
        .unwrap_or(Value::Null);
    let kline = dims_data
        .get("2_kline")
        .and_then(|d| d.get("data"))
        .cloned()
        .unwrap_or(Value::Null);
    let roe = fin
        .get("roe_history")
        .and_then(|v| v.as_array())
        .and_then(|a| a.last())
        .map(uzi_core::py::num_str)
        .unwrap_or_else(|| "—".to_string());
    let ctx = serde_json::json!({
        "name": basic.get("name").cloned().unwrap_or_else(|| serde_json::json!("这只票")),
        "industry": basic.get("industry").cloned().unwrap_or_else(|| serde_json::json!("该行业")),
        "price": basic.get("price").cloned().unwrap_or_else(|| serde_json::json!("—")),
        "pe": basic.get("pe_ttm").cloned().unwrap_or_else(|| serde_json::json!("—")),
        "roe": roe,
        "stage": kline.get("stage").cloned().unwrap_or_else(|| serde_json::json!("—")),
        "growth": fin.get("revenue_growth").cloned().unwrap_or_else(|| serde_json::json!("—")),
    });

    let empty: Vec<Value> = Vec::new();
    let investors = panel
        .get("investors")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or(empty);
    let mut checked = 0;
    for inv in investors {
        let signal = inv.get("signal").and_then(|v| v.as_str()).unwrap_or("");
        if signal == "skip" {
            continue; // skip path builds a fixed string, not a persona line
        }
        let id = inv.get("investor_id").and_then(|v| v.as_str()).unwrap_or("");
        let comment = inv.get("comment").and_then(|v| v.as_str()).unwrap_or("");
        let line = comment.split('\n').next().unwrap_or("");
        assert_persona_line_known(&pools, id, signal, line, &ctx);
        checked += 1;
    }
    assert!(checked > 0, "no persona lines were checked");
}
