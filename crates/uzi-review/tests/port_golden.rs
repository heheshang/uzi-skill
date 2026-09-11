//! Differential tests against golden output produced by the upstream Python
//! modules (`lib/data_integrity.py`, `lib/agent_analysis_validator.py`).

use serde_json::Value;
use std::path::PathBuf;

fn golden(name: &str) -> Value {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden")
        .join(format!("{}.json", name));
    let text = std::fs::read_to_string(&p)
        .unwrap_or_else(|e| panic!("cannot read {}: {}", p.display(), e));
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("invalid JSON in {}: {}", p.display(), e))
}

fn golden_text(name: &str) -> String {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden")
        .join(format!("{}.txt", name));
    std::fs::read_to_string(&p)
        .unwrap_or_else(|e| panic!("cannot read {}: {}", p.display(), e))
        .trim_end_matches('\n')
        .to_string()
}

// ── data_integrity: validate ────────────────────────────────────────────

#[test]
fn validate_synthetic_matches_upstream() {
    let raw = uzi_core::testkit::load_fixture("raw_data_synthetic");
    let actual = uzi_review::validate(&raw);
    uzi_core::testkit::assert_json_eq(
        &actual,
        &golden("synthetic_validate"),
        "data_integrity/validate/synthetic",
    );
}

#[test]
fn validate_sparse_matches_upstream() {
    let raw = uzi_core::testkit::load_fixture("raw_data_sparse");
    let actual = uzi_review::validate(&raw);
    uzi_core::testkit::assert_json_eq(
        &actual,
        &golden("sparse_validate"),
        "data_integrity/validate/sparse",
    );
}

// ── data_integrity: generate_recovery_tasks ─────────────────────────────

#[test]
fn recovery_tasks_synthetic_matches_upstream() {
    let raw = uzi_core::testkit::load_fixture("raw_data_synthetic");
    let integrity = uzi_review::validate(&raw);
    let actual = uzi_review::generate_recovery_tasks(&raw, &integrity);
    uzi_core::testkit::assert_json_eq(
        &actual,
        &golden("synthetic_recovery_tasks"),
        "data_integrity/generate_recovery_tasks/synthetic",
    );
}

#[test]
fn recovery_tasks_sparse_matches_upstream() {
    let raw = uzi_core::testkit::load_fixture("raw_data_sparse");
    let integrity = uzi_review::validate(&raw);
    let actual = uzi_review::generate_recovery_tasks(&raw, &integrity);
    uzi_core::testkit::assert_json_eq(
        &actual,
        &golden("sparse_recovery_tasks"),
        "data_integrity/generate_recovery_tasks/sparse",
    );
}

/// Raw with all 20 dims present but empty — exercises every
/// `CRITICAL_CHECKS`/`ENRICHMENT_DIMS` entry and every hint template.
fn all_missing_raw() -> Value {
    const DIMS: &[&str] = &[
        "0_basic", "1_financials", "2_kline", "3_macro", "4_peers", "5_chain", "6_research",
        "7_industry", "8_materials", "9_futures", "10_valuation", "11_governance", "12_capital_flow",
        "13_policy", "14_moat", "15_events", "16_lhb", "17_sentiment", "18_trap", "19_contests",
    ];
    let mut dimensions = serde_json::Map::new();
    for d in DIMS {
        dimensions.insert((*d).to_string(), serde_json::json!({"data": {}}));
    }
    serde_json::json!({
        "ticker": "600519.SH", "code": "600519.SH", "market": "A",
        "dimensions": Value::Object(dimensions),
    })
}

#[test]
fn validate_all_missing_matches_upstream() {
    let raw = all_missing_raw();
    let actual = uzi_review::validate(&raw);
    uzi_core::testkit::assert_json_eq(
        &actual,
        &golden("all_missing_validate"),
        "data_integrity/validate/all_missing",
    );
    let tasks = uzi_review::generate_recovery_tasks(&raw, &actual);
    uzi_core::testkit::assert_json_eq(
        &tasks,
        &golden("all_missing_recovery_tasks"),
        "data_integrity/generate_recovery_tasks/all_missing",
    );
}

#[test]
fn missing_semantics_match_upstream() {
    // "" "0" "0.0" "—" "-" "N/A" "None" [] {} are missing; the *numbers* 0/0.0,
    // a non-empty list, and non-placeholder strings are present.
    let raw = serde_json::json!({
        "ticker": "X",
        "dimensions": {
            "0_basic": {"data": {"name": "", "price": "0", "industry": "0.0", "market_cap": "—", "pe_ttm": "-", "pb": "N/A"}},
            "1_financials": {"data": {"roe_history": "None", "revenue_history": 0, "net_profit_history": 0.0, "financial_health": []}},
            "2_kline": {"data": {"stage": {}, "ma_align": [1], "macd": "MACD"}},
            "7_industry": {"data": {"growth": "N/A"}},
            "14_moat": {"data": {"scores": "x"}}
        }
    });
    let actual = uzi_review::validate(&raw);
    uzi_core::testkit::assert_json_eq(
        &actual,
        &golden("missing_semantics_validate"),
        "data_integrity/validate/missing_semantics",
    );
    let tasks = uzi_review::generate_recovery_tasks(&raw, &actual);
    uzi_core::testkit::assert_json_eq(
        &tasks,
        &golden("missing_semantics_recovery_tasks"),
        "data_integrity/generate_recovery_tasks/missing_semantics",
    );
}

// ── data_integrity: format_report ───────────────────────────────────────

#[test]
fn format_report_matches_upstream_console() {
    for case in ["synthetic", "sparse"] {
        let raw = uzi_core::testkit::load_fixture(&format!("raw_data_{}", case));
        let text = uzi_review::format_report(&uzi_review::validate(&raw));
        assert_eq!(text, golden_text(&format!("{}_format_report", case)));
    }
}

// ── agent_analysis_validator ────────────────────────────────────────────

fn validator_case(name: &str, payload: Value) {
    let actual = uzi_review::validate_agent_analysis(&payload);
    uzi_core::testkit::assert_json_eq(
        &actual,
        &golden(name),
        &format!("agent_analysis_validator/{}", name),
    );
}

#[test]
fn dim_commentary_as_list_is_error() {
    validator_case(
        "validator_case1",
        serde_json::json!({"dim_commentary": ["wrong"]}),
    );
}

#[test]
fn missing_buy_zone_value_is_warning() {
    validator_case(
        "validator_case2",
        serde_json::json!({"narrative_override": {"buy_zones": {"growth": {"price": 10}}}}),
    );
}

#[test]
fn clean_payload_matches_upstream_acceptable_issues() {
    let payload = serde_json::json!({
        "agent_reviewed": true,
        "dim_commentary": {"0_basic": "公司是港口龙头，市值 270 亿，PE 25 倍偏高。"},
        "panel_insights": "51 评委里 12 人看多 19 中性 19 看空，分歧主要在估值和催化剂之间。",
        "great_divide_override": {
            "punchline": "PE 25 买 ROE 6% 是为运河支付溢价",
            "bull_say_rounds": ["a", "b", "c"],
            "bear_say_rounds": ["a", "b", "c"]
        },
        "narrative_override": {
            "core_conclusion": "综合 48 分谨慎评级，等待回调再介入。",
            "risks": ["风险 1", "风险 2", "风险 3"],
            "buy_zones": {
                "value": {"price": 10.0, "rationale": "test"},
                "growth": {"price": 10.0, "rationale": "test"},
                "technical": {"price": 10.0, "rationale": "test"},
                "youzi": {"price": 10.0, "rationale": "test"}
            }
        }
    });
    validator_case("validator_case3", payload);
}

#[test]
fn kitchen_sink_messages_match_upstream() {
    let payload = serde_json::json!({
        "agent_reviewed": "yes",
        "dim_commentary": {"0_basic": "太短", "1_financials": 123, "2_kline": "这是一段足够长的评语，引用具体数字 123 与 456。"},
        "panel_insights": 123,
        "great_divide_override": {"punchline": "短", "bull_say_rounds": ["a"], "bear_say_rounds": "nope"},
        "narrative_override": {
            "core_conclusion": "短",
            "risks": "bad",
            "buy_zones": {
                "value": "bad",
                "growth": {"price": null, "rationale": "x"},
                "technical": {},
                "youzi": {"price": 1, "rationale": "足够长的理由"}
            }
        },
        "data_gap_acknowledged": ["x"],
        "qualitative_deep_dive": {"3_macro": "bad", "13_policy": {"evidence": "bad"}, "18_trap": {"evidence": []}}
    });
    validator_case("validator_case4", payload);
}

#[test]
fn format_issues_renders_errors_and_warnings() {
    let issues = uzi_review::validate_agent_analysis(
        &serde_json::json!({"dim_commentary": ["wrong"]}),
    );
    let text = uzi_review::format_issues(&issues);
    assert!(text.starts_with("🔴 schema 错误 1 条（结构性，会导致 stage2 fallback）："));
    assert!(text.contains("   · dim_commentary: dim_commentary 必须是 dict（key 是维度名），实际是 list"));
    assert!(text.contains("🟡 schema 警告 1 条（质量问题，stage2 仍会用，但报告可能不达标）："));
    assert_eq!(
        uzi_review::format_issues(&serde_json::json!([])),
        "✅ agent_analysis.json schema 校验通过"
    );
}

// ── data_integrity: refresh_recovery_artifact ───────────────────────────

#[test]
fn refresh_recovery_artifact_writes_upstream_document() {
    let dir = std::env::temp_dir().join(format!("uzi-review-refresh-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let gaps = dir.join("gaps.json");

    let mut raw = all_missing_raw();
    let integrity = uzi_review::refresh_recovery_artifact(&mut raw, "600519.SH", &gaps);
    uzi_core::testkit::assert_json_eq(&raw["_integrity"], &integrity, "refresh/raw._integrity");
    uzi_core::testkit::assert_json_eq(
        &integrity,
        &uzi_review::validate(&all_missing_raw()),
        "refresh/integrity",
    );

    let mut doc = uzi_core::testkit::load_json(&gaps);
    let ts = doc["generated_at"].as_str().unwrap().to_string();
    assert_eq!(ts.len(), 25, "unexpected generated_at: {}", ts);
    assert!(ts.ends_with("+00:00"), "unexpected generated_at: {}", ts);
    doc["generated_at"] = serde_json::json!("<TS>");
    uzi_core::testkit::assert_json_eq(
        &doc,
        &golden("all_missing_gaps_document"),
        "refresh/gaps_document",
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn refresh_removes_artifact_when_no_tasks() {
    let dir = std::env::temp_dir().join(format!("uzi-review-refresh2-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let gaps = dir.join("gaps.json");
    std::fs::write(&gaps, "{}").unwrap();

    let mut raw = all_missing_raw();
    for (_d, v) in raw["dimensions"].as_object_mut().unwrap().iter_mut() {
        v["data"]["filler"] = serde_json::json!(1);
    }
    let req: [(&str, &[&str]); 6] = [
        ("0_basic", &["name", "price", "industry", "market_cap", "pe_ttm", "pb"]),
        ("1_financials", &["roe_history", "revenue_history", "net_profit_history", "financial_health"]),
        ("2_kline", &["stage", "ma_align", "macd"]),
        ("10_valuation", &["pe", "pe_quantile", "pb_quantile"]),
        ("7_industry", &["growth"]),
        ("14_moat", &["scores"]),
    ];
    for (dim, keys) in req {
        for k in keys {
            raw["dimensions"][dim]["data"][k] = serde_json::json!(1);
        }
    }

    let integrity = uzi_review::refresh_recovery_artifact(&mut raw, "600519.SH", &gaps);
    assert_eq!(integrity["coverage_pct"], 100.0);
    assert!(!gaps.exists(), "stale gaps artifact should be removed");
    let _ = std::fs::remove_dir_all(&dir);
}
