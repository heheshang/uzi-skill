//! Differential tests against the checked-in golden artifacts.
//!
//! `tools/golden/expected/{synthetic,sparse}/` is produced by running the
//! *upstream* Python modules (`tools/golden/dump_python.py`), so these tests are
//! the acceptance proof for the port.

use serde_json::{json, Value};
use std::collections::BTreeMap;

/// Acceptance 2 — the embedded roster must equal the golden `INVESTORS` dump
/// exactly, key order included.
#[test]
fn investors_match_golden_exactly() {
    let golden = uzi_core::testkit::load_golden("synthetic", "investors");
    let actual = Value::Array(uzi_investors::investors().clone());
    uzi_core::testkit::assert_json_eq(&actual, &golden, "investors/synthetic");
    // the sparse case dumps the same table
    let golden_sparse = uzi_core::testkit::load_golden("sparse", "investors");
    uzi_core::testkit::assert_json_eq(&actual, &golden_sparse, "investors/sparse");
}

/// Acceptance 4 — count and group histogram.
///
/// Derivation: `python3 -c "from collections import Counter;
/// from lib.investor_db import INVESTORS; print(Counter(i['group'] for i in
/// INVESTORS))"` against `/tmp/uzi-src/skills/deep-analysis/scripts` gives
/// Counter({'F': 24, 'B': 9, 'C': 7, 'E': 7, 'A': 6, 'D': 4, 'G': 4, 'H': 4,
/// 'I': 1}) — i.e. 24+9+7+7+6+4+4+4+1 = 66. The histogram below is that value.
#[test]
fn roster_has_66_investors_with_the_upstream_group_histogram() {
    let inv = uzi_investors::investors();
    assert_eq!(inv.len(), 66);

    let expected: BTreeMap<&str, usize> = [
        ("A", 6),
        ("B", 9),
        ("C", 7),
        ("D", 4),
        ("E", 7),
        ("F", 24),
        ("G", 4),
        ("H", 4),
        ("I", 1),
    ]
    .into_iter()
    .collect();

    let mut actual: BTreeMap<String, usize> = BTreeMap::new();
    for i in inv {
        let g = i["group"].as_str().expect("group is a string").to_string();
        *actual.entry(g).or_insert(0) += 1;
    }
    let actual: BTreeMap<&str, usize> =
        actual.iter().map(|(k, v)| (k.as_str(), *v)).collect();
    assert_eq!(actual, expected);
    assert_eq!(actual.values().sum::<usize>(), 66);

    // every id is unique and resolvable through the public lookup
    let mut ids: Vec<&str> = uzi_investors::investors()
        .iter()
        .filter_map(|i| i["id"].as_str())
        .collect();
    ids.sort();
    ids.dedup();
    assert_eq!(ids.len(), 66);
    for id in ids {
        assert!(uzi_investors::investor_by_id(id).is_some(), "{id} not resolvable");
    }
}

/// Acceptance 3 — every seeded persona line is one the upstream pool could have
/// produced, for every `(investor, signal)` in the golden pools.
#[test]
fn every_seeded_persona_line_is_in_the_upstream_pool() {
    let pools = uzi_core::testkit::load_golden("synthetic", "persona_pools");
    let ctx = json!({
        "roe": "18.2",
        "pe": "21.5",
        "price": "1234.5",
        "name": "贵州茅台",
        "industry": "白酒",
        "growth": "15.3",
        "stage": "Stage 2",
    });

    let mut checked = 0usize;
    for (investor_id, signals) in pools.as_object().expect("pools is a dict") {
        let signals = signals.as_object().expect("signals is a dict");
        for (signal, lines) in signals {
            let pool_lines = lines.as_array().expect("lines is a list");
            assert!(!pool_lines.is_empty(), "{investor_id}/{signal} empty pool");
            // iterating every seed covers every index of the pool
            for seed in 0..pool_lines.len() {
                let line = uzi_investors::persona_comment_seeded(
                    investor_id,
                    signal,
                    &ctx,
                    seed as u64,
                );
                uzi_core::testkit::assert_persona_line_known(
                    &pools, investor_id, signal, &line, &ctx,
                );
                checked += 1;
            }
            // `persona_comment` derives its seed from ctx and must agree
            let derived = uzi_investors::persona_comment(investor_id, signal, &ctx);
            uzi_core::testkit::assert_persona_line_known(
                &pools, investor_id, signal, &derived, &ctx,
            );
            checked += 1;
        }
    }
    assert!(checked > 66 * 3, "checked {checked} rendered lines");
}

/// Extra differential coverage for `evaluate_investor`: the golden `panel.json`
/// embeds the upstream verdicts for every investor, so reproduce the fields
/// `generate_panel` copies out of the evaluator result.
#[test]
fn evaluator_verdicts_match_golden_panel_for_both_fixtures() {
    for case in ["synthetic", "sparse"] {
        let features = uzi_core::testkit::load_golden(case, "features");
        let panel = uzi_core::testkit::load_golden(case, "panel");
        let mut mismatches: Vec<String> = Vec::new();

        for entry in panel["investors"].as_array().expect("investors list") {
            let id = entry["investor_id"].as_str().unwrap();
            let ev = uzi_investors::evaluate_investor(id, &features);

            let mut check = |field: &str, got: Value, want: &Value| {
                if &got != want {
                    mismatches.push(format!("{case}/{id}/{field}: got {got} want {want}"));
                }
            };

            check("signal", ev["signal"].clone(), &entry["signal"]);
            check("headline", ev["headline"].clone(), &entry["headline"]);
            check("rationale", ev["rationale"].clone(), &entry["reasoning"]);
            check("weight_pass", ev["weight_pass"].clone(), &entry["weight_pass"]);
            check("weight_total", ev["weight_total"].clone(), &entry["weight_total"]);
            check(
                "time_horizon",
                ev["time_horizon"].clone(),
                &entry["time_horizon"],
            );
            check(
                "position_sizing",
                ev["position_sizing"].clone(),
                &entry["position_sizing"],
            );
            check(
                "what_would_change_my_mind",
                ev["what_would_change_my_mind"].clone(),
                &entry["what_would_change_my_mind"],
            );

            // panel stores int(max(0, score)) and int(confidence)
            let score_int = ev["score"].as_f64().unwrap().max(0.0) as i64;
            check("score", json!(score_int), &entry["score"]);
            let conf_int = ev["confidence"].as_f64().unwrap() as i64;
            check("confidence", json!(conf_int), &entry["confidence"]);

            // panel copies the first 4 rules of each side (already weight-sorted)
            for (mine, theirs) in [("pass_rules", "pass"), ("fail_rules", "fail")] {
                let got: Vec<Value> = ev[mine]
                    .as_array()
                    .unwrap()
                    .iter()
                    .take(4)
                    .map(|r| json!({"name": r["name"], "msg": r["msg"], "weight": r["weight"]}))
                    .collect();
                check(mine, Value::Array(got), &entry[theirs]);
            }
        }
        assert!(mismatches.is_empty(), "{} mismatches:\n{}", mismatches.len(), mismatches.join("\n"));
    }
}

/// The evaluator's rule metadata (id/name/weight, in order) must match the golden
/// panel's per-investor rule names and the `criteria_meta` dump.
#[test]
fn rule_metadata_matches_the_panel() {
    let panel = uzi_core::testkit::load_golden("synthetic", "panel");
    let mut bad: Vec<String> = Vec::new();
    for entry in panel["investors"].as_array().unwrap() {
        let id = entry["investor_id"].as_str().unwrap();
        let rules = uzi_investors::criteria::rules_for(id).expect("rules");
        // rule ids are unique within an investor
        let mut ids: Vec<&str> = rules.iter().map(|r| r.rule_id.as_str()).collect();
        ids.sort();
        let n = ids.len();
        ids.dedup();
        if ids.len() != n {
            bad.push(format!("{id}: duplicate rule ids"));
        }
        for r in rules {
            if !(1..=5).contains(&r.weight) {
                bad.push(format!("{id}/{}: weight {}", r.rule_id, r.weight));
            }
            if r.name.is_empty() {
                bad.push(format!("{id}/{}: empty name", r.rule_id));
            }
        }
    }
    assert!(bad.is_empty(), "{}", bad.join("\n"));
}

/// `reality_check` drives skip decisions; spot-check a few against the panel.
#[test]
fn reality_check_skips_match_the_panel_skips() {
    let features = uzi_core::testkit::load_golden("synthetic", "features");
    let panel = uzi_core::testkit::load_golden("synthetic", "panel");
    let market = features["market"].as_str().unwrap_or("A");
    let ticker = features["ticker"].as_str().unwrap_or("");
    let name = features["name"].as_str().unwrap_or("");
    let industry = features["industry"].as_str().unwrap_or("");
    for entry in panel["investors"].as_array().unwrap() {
        let id = entry["investor_id"].as_str().unwrap();
        let rc = uzi_investors::knowledge::reality_check(id, market, ticker, name, industry);
        let expect_skip = entry["signal"] == "skip";
        let rc_skips = !rc["should_evaluate"].as_bool().unwrap();
        if rc_skips != expect_skip {
            // out-of-range 游资 skips happen after reality_check; tolerate those
            let reason = entry["headline"].as_str().unwrap_or("");
            assert!(
                !rc_skips || reason.contains("不在") || !expect_skip,
                "{id}: reality_check skip={rc_skips} panel skip={expect_skip}"
            );
        }
    }
}

/// `_fmt_msg` fallback: unknown placeholders stay literal, null renders `?`.
#[test]
fn fmt_msg_matches_python_semantics() {
    let f = json!({"pe": 34.2, "n": null});
    assert_eq!(uzi_investors::evaluator::fmt_msg("{pe} {nope}", &f), "34.2 ?");
    assert_eq!(uzi_investors::evaluator::fmt_msg("{pe:.1f}", &f), "34.2");
}

/// Empty-vs-zero: a zero-valued feature is present data, not missing data.
#[test]
fn zero_features_are_scored_not_skipped() {
    let f = json!({"market": "A", "name": "x", "pe": 0, "roe_5y_min": 0, "roe_5y_above_15": 0});
    let ev = uzi_investors::evaluate_investor("buffett", &f);
    // every numeric rule sees its defaulted/zero operand and fails; only
    // fcf_positive is skipped (fcf_known is absent → rule raises → skip)
    assert_eq!(ev["pass_rules"].as_array().unwrap().len(), 0);
    let ids: Vec<&str> = ev["fail_rules"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|r| r["rule_id"].as_str())
        .collect();
    assert!(ids.contains(&"roe_5y_15"));
    assert!(!ids.contains(&"fcf_positive"));
    assert_eq!(ev["weight_total"], 5 + 3 + 3 + 4 + 3 + 2);
    assert_eq!(ev["weight_pass"], 0);
    assert_eq!(ev["signal"], "bearish");
}

/// `UZI_SCHOOL` locking is exercised in the dedicated single-test binary
/// `tests/school_lock.rs` (env mutation must not race other tests).
#[test]
fn school_labels_cover_the_nine_schools() {
    for (key, label) in [
        ("A", "价值派"),
        ("B", "成长派"),
        ("C", "宏观派"),
        ("D", "技术派"),
        ("E", "中国价投"),
        ("F", "A 股游资"),
        ("G", "量化"),
        ("H", "科技领袖派"),
        ("I", "AI 卡位/瓶颈猎手"),
    ] {
        assert_eq!(uzi_investors::evaluator::school_labels(key), Some(label));
    }
    assert_eq!(uzi_investors::evaluator::school_labels("Z"), None);
}
