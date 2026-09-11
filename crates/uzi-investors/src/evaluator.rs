//! Port of `lib/investor_evaluator.py` — the rule-engine executor that turns
//! `(investor_id, features)` into a quantified verdict.
//!
//! Three layers, exactly like upstream:
//!   1. reality check (`investor_knowledge.reality_check`)
//!   2. rule engine (`investor_criteria.INVESTOR_RULES`)
//!   3. composite: holding bonus + affinity adjustment

use crate::pyhelp::{self as h, Missing};
use crate::{criteria, db, knowledge, profile, seat_db};
use serde_json::{Map, Value};
use uzi_core::py;

/// score ≥ 65 → bullish.
pub const BULLISH_THRESHOLD: f64 = 65.0;
/// score < 35 → bearish.
pub const BEARISH_THRESHOLD: f64 = 35.0;

/// v3.5.0 · 流派标签 · `--school` locks a single school and skips the rest.
pub fn school_labels(key: &str) -> Option<&'static str> {
    Some(match key {
        "A" => "价值派",
        "B" => "成长派",
        "C" => "宏观派",
        "D" => "技术派",
        "E" => "中国价投",
        "F" => "A 股游资",
        "G" => "量化",
        "H" => "科技领袖派",
        "I" => "AI 卡位/瓶颈猎手",
        _ => return None,
    })
}

/// `investor_evaluator.get_locked_school` — `UZI_SCHOOL` env, uppercase, valid
/// single letter or `""`.
pub fn get_locked_school() -> String {
    let raw = std::env::var("UZI_SCHOOL").unwrap_or_default();
    let raw = raw.trim().to_uppercase();
    if school_labels(&raw).is_some() {
        raw
    } else {
        String::new()
    }
}

fn group_of(investor_id: &str) -> String {
    db::investor_by_id(investor_id)
        .and_then(|i| i.get("group"))
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string()
}

fn name_of(investor_id: &str) -> String {
    db::investor_by_id(investor_id)
        .and_then(|i| i.get("name"))
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string()
}

/// Python `f.get(key, default)` as a display string (numbers via `str()`).
fn str_default(f: &Value, key: &str, default: &str) -> String {
    match f.get(key) {
        None => default.to_string(),
        Some(v) => py::num_str(v),
    }
}

/// `float(v)` best effort for `market_cap_yi` (Python `float()` accepts strings).
fn to_float(v: &Value) -> f64 {
    match v {
        Value::Number(n) => n.as_f64().unwrap_or(0.0),
        Value::Bool(b) => {
            if *b {
                1.0
            } else {
                0.0
            }
        }
        Value::String(s) => py::parse_float(s).unwrap_or(0.0),
        _ => 0.0,
    }
}

/// v2.13.3 · F 组游资射程前置检查 (v3.4.5 LHB 反查覆盖).
fn is_youzi_out_of_range(investor_id: &str, features: &Value) -> (bool, String) {
    if group_of(investor_id) != "F" {
        return (false, String::new());
    }
    let nickname = name_of(investor_id);
    if nickname.is_empty() || !seat_db::seats().contains_key(&nickname) {
        return (false, String::new());
    }

    // `mc = features.get("market_cap") or 0` then the 亿 → 元 fallback
    let mut mc = match features.get("market_cap") {
        Some(v) if py::truthy(v) => to_float(v),
        _ => 0.0,
    };
    if mc == 0.0 {
        if let Some(yi) = features.get("market_cap_yi") {
            if py::truthy(yi) {
                mc = to_float(yi) * 1e8;
            }
        }
    }

    let mut probe = features.clone();
    if let Some(obj) = probe.as_object_mut() {
        obj.insert("market_cap".into(), Value::from(mc));
    } else {
        probe = Value::Object(Map::new());
        probe["market_cap"] = Value::from(mc);
    }
    if seat_db::is_in_range(&nickname, &probe) {
        return (false, String::new());
    }

    // v3.4.5 · out of range but the LHB shows the seat actually traded → keep scoring
    let matched = features.get("matched_youzi").and_then(Value::as_array);
    if let Some(matched) = matched {
        if matched.iter().any(|m| m.as_str() == Some(nickname.as_str())) {
            return (false, String::new());
        }
    }

    let mc_yi = if mc != 0.0 { mc / 1e8 } else { 0.0 };
    (true, format!("市值 {:.0} 亿不在 {} 射程", mc_yi, nickname))
}

/// `investor_evaluator._fmt_msg` — f-string over the feature dict; unknown or
/// null placeholders render as `?`; a template that cannot be formatted is
/// returned verbatim.
pub fn fmt_msg(template: &str, features: &Value) -> String {
    if template.is_empty() {
        return String::new();
    }
    h::format_map(
        template,
        &|key| match features.get(key) {
            None | Some(Value::Null) => None,
            Some(v) => Some(v.clone()),
        },
        Missing::Literal("?"),
    )
    .unwrap_or_else(|_| template.to_string())
}

fn rule_entry(rule: &criteria::Rule, msg: String) -> Value {
    let mut m = Map::new();
    m.insert("rule_id".into(), Value::String(rule.rule_id.clone()));
    m.insert("name".into(), Value::String(rule.name.clone()));
    m.insert("weight".into(), Value::from(rule.weight));
    m.insert("msg".into(), Value::String(msg));
    Value::Object(m)
}

/// `investor_evaluator._build_headline`.
fn build_headline(signal: &str, pass_list: &[Value], fail_list: &[Value]) -> String {
    let msg = |v: &Value| v["msg"].as_str().unwrap_or("").to_string();
    if signal == "bullish" && !pass_list.is_empty() {
        return format!("看多核心：{}", msg(&pass_list[0]));
    }
    if signal == "bearish" && !fail_list.is_empty() {
        return format!("看空核心：{}", msg(&fail_list[0]));
    }
    if !pass_list.is_empty() && !fail_list.is_empty() {
        return format!("观望：{}；但 {}", msg(&pass_list[0]), msg(&fail_list[0]));
    }
    if !pass_list.is_empty() {
        return format!("中性：{}", msg(&pass_list[0]));
    }
    if !fail_list.is_empty() {
        return format!("中性：{}", msg(&fail_list[0]));
    }
    "数据不足，暂无判断".to_string()
}

/// `investor_evaluator._build_rationale`.
fn build_rationale(pass_list: &[Value], fail_list: &[Value]) -> String {
    let mut lines: Vec<String> = Vec::new();
    if !pass_list.is_empty() {
        lines.push("✅ 符合标准：".into());
        for r in pass_list.iter().take(4) {
            lines.push(format!(
                "  • [权{}] {}",
                r["weight"].as_i64().unwrap_or(0),
                r["msg"].as_str().unwrap_or("")
            ));
        }
    }
    if !fail_list.is_empty() {
        lines.push("❌ 未达标准：".into());
        for r in fail_list.iter().take(4) {
            lines.push(format!(
                "  • [权{}] {}",
                r["weight"].as_i64().unwrap_or(0),
                r["msg"].as_str().unwrap_or("")
            ));
        }
    }
    if lines.is_empty() {
        "无有效规则命中".to_string()
    } else {
        lines.join("\n")
    }
}

fn profile_fields(investor_id: &str) -> (Value, Value, Value) {
    let p = profile::get_profile(investor_id, &group_of(investor_id));
    (
        p["time_horizon"].clone(),
        p["position_sizing"].clone(),
        p["what_would_change_my_mind"].clone(),
    )
}

/// `investor_evaluator._skip_result`.
fn skip_result(investor_id: &str, reason: &str) -> Value {
    let (th, ps, ww) = profile_fields(investor_id);
    let mut m = Map::new();
    m.insert("investor_id".into(), Value::String(investor_id.into()));
    m.insert("score".into(), Value::from(-1));
    m.insert("signal".into(), Value::String("skip".into()));
    m.insert("confidence".into(), Value::from(0));
    m.insert("weight_pass".into(), Value::from(0));
    m.insert("weight_total".into(), Value::from(0));
    m.insert("pass_count".into(), Value::from(0));
    m.insert("fail_count".into(), Value::from(0));
    m.insert("pass_rules".into(), Value::Array(vec![]));
    m.insert("fail_rules".into(), Value::Array(vec![]));
    m.insert("headline".into(), Value::String(format!("不适合 — {}", reason)));
    m.insert(
        "rationale".into(),
        Value::String(format!("该投资者{}，不对此股票发表意见。", reason)),
    );
    m.insert("skip_reason".into(), Value::String(reason.into()));
    m.insert("time_horizon".into(), th);
    m.insert("position_sizing".into(), ps);
    m.insert("what_would_change_my_mind".into(), ww);
    Value::Object(m)
}

/// `investor_evaluator._unknown_result`.
fn unknown_result(investor_id: &str) -> Value {
    let (th, ps, ww) = profile_fields(investor_id);
    let mut m = Map::new();
    m.insert("investor_id".into(), Value::String(investor_id.into()));
    m.insert("score".into(), Value::from(50.0));
    m.insert("signal".into(), Value::String("neutral".into()));
    m.insert("confidence".into(), Value::from(30));
    m.insert("weight_pass".into(), Value::from(0));
    m.insert("weight_total".into(), Value::from(0));
    m.insert("pass_count".into(), Value::from(0));
    m.insert("fail_count".into(), Value::from(0));
    m.insert("pass_rules".into(), Value::Array(vec![]));
    m.insert("fail_rules".into(), Value::Array(vec![]));
    m.insert("headline".into(), Value::String("该投资者暂无量化评估规则".into()));
    m.insert("rationale".into(), Value::String("此投资者未配置规则库，使用默认中性判断。".into()));
    m.insert("time_horizon".into(), th);
    m.insert("position_sizing".into(), ps);
    m.insert("what_would_change_my_mind".into(), ww);
    Value::Object(m)
}

/// `investor_evaluator.evaluate`.
pub fn evaluate_investor(investor_id: &str, features: &Value) -> Value {
    // v3.5.0 · 用户锁定单一流派视角
    let locked = get_locked_school();
    if !locked.is_empty() {
        if group_of(investor_id) != locked {
            let label = school_labels(&locked).unwrap_or(&locked);
            return skip_result(
                investor_id,
                &format!("用户锁定 {} 派视角 · 非该派评委不参与", label),
            );
        }
    }

    // ─── Layer 1: Reality Check ───
    let market = str_default(features, "market", "A");
    let ticker = str_default(features, "ticker", "");
    let name = str_default(features, "name", "");
    let industry = str_default(features, "industry", "");
    let rc = knowledge::reality_check(investor_id, &market, &ticker, &name, &industry);

    if !py::truthy(&rc["should_evaluate"]) {
        let reason = rc["skip_reason"]
            .as_str()
            .filter(|s| !s.is_empty())
            .unwrap_or("不在能力圈");
        return skip_result(investor_id, reason);
    }

    let (out_of_range, range_reason) = is_youzi_out_of_range(investor_id, features);
    if out_of_range {
        return skip_result(investor_id, &range_reason);
    }

    let Some(rules) = criteria::rules_for(investor_id) else {
        return unknown_result(investor_id);
    };
    if rules.is_empty() {
        return unknown_result(investor_id);
    }

    // ─── Layer 2: Rule Engine ───
    let mut pass_list: Vec<Value> = Vec::new();
    let mut fail_list: Vec<Value> = Vec::new();
    let mut weight_pass: i64 = 0;
    let mut weight_total: i64 = 0;

    for rule in rules {
        let Some(ok) = criteria::safe_check(rule, features) else {
            continue; // data missing → rule skipped, weight not counted
        };
        weight_total += rule.weight;
        if ok {
            weight_pass += rule.weight;
            let template = if rule.pass_msg.is_empty() { &rule.name } else { &rule.pass_msg };
            pass_list.push(rule_entry(rule, fmt_msg(template, features)));
        } else {
            let fallback = format!("未达{}", rule.name);
            let template = if rule.fail_msg.is_empty() { fallback.as_str() } else { &rule.fail_msg };
            fail_list.push(rule_entry(rule, fmt_msg(template, features)));
        }
    }

    // ─── Layer 3: Reality Adjustment ───
    let affinity_adj = rc["affinity_adjust"].as_f64().unwrap_or(0.0);
    let holding_match = rc["holding_match"].as_array().cloned();

    if let Some(hm) = &holding_match {
        let attitude = hm.first().and_then(Value::as_str).unwrap_or("");
        let note = hm.get(1).and_then(Value::as_str).unwrap_or("");
        if attitude == "held" || attitude == "bullish_known" {
            pass_list.insert(
                0,
                {
                    let mut m = Map::new();
                    m.insert("rule_id".into(), Value::String("known_holding".into()));
                    m.insert("name".into(), Value::String("实际持仓 / 公开看好".into()));
                    m.insert("weight".into(), Value::from(6));
                    m.insert("msg".into(), Value::String(format!("📌 {}", note)));
                    Value::Object(m)
                },
            );
            weight_pass += 6;
            weight_total += 6;
        }
    }

    let score = if weight_total > 0 {
        py::round(
            (weight_pass as f64 / weight_total as f64) * 100.0 + affinity_adj,
            1,
        )
    } else {
        py::round(50.0 + affinity_adj, 1)
    };
    let score = score.max(0.0).min(100.0);

    let override_signal = rc["override_signal"].as_str().filter(|s| !s.is_empty());
    let signal = if let Some(s) = override_signal {
        s.to_string()
    } else if score >= BULLISH_THRESHOLD {
        "bullish".to_string()
    } else if score < BEARISH_THRESHOLD {
        "bearish".to_string()
    } else {
        "neutral".to_string()
    };

    let n_rules = rules.len() as f64 + if holding_match.is_some() { 1.0 } else { 0.0 };
    let base_conf = (50.0 + n_rules * 8.0).min(100.0);
    let extremeness = (score - 50.0).abs() * 0.6;
    let confidence = py::round((base_conf * 0.6 + 40.0 + extremeness * 0.4).min(100.0), 0);

    pass_list.sort_by(|a, b| {
        b["weight"].as_i64().cmp(&a["weight"].as_i64())
    });
    fail_list.sort_by(|a, b| {
        b["weight"].as_i64().cmp(&a["weight"].as_i64())
    });

    let headline = build_headline(&signal, &pass_list, &fail_list);
    let rationale = build_rationale(&pass_list, &fail_list);
    let (th, ps, ww) = profile_fields(investor_id);

    let mut m = Map::new();
    m.insert("investor_id".into(), Value::String(investor_id.into()));
    m.insert("score".into(), Value::from(score));
    m.insert("signal".into(), Value::String(signal));
    m.insert("confidence".into(), Value::from(confidence));
    m.insert("weight_pass".into(), Value::from(weight_pass));
    m.insert("weight_total".into(), Value::from(weight_total));
    m.insert("pass_count".into(), Value::from(pass_list.len()));
    m.insert("fail_count".into(), Value::from(fail_list.len()));
    m.insert("pass_rules".into(), Value::Array(pass_list));
    m.insert("fail_rules".into(), Value::Array(fail_list));
    m.insert("headline".into(), Value::String(headline));
    m.insert("rationale".into(), Value::String(rationale));
    m.insert("time_horizon".into(), th);
    m.insert("position_sizing".into(), ps);
    m.insert("what_would_change_my_mind".into(), ww);
    Value::Object(m)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn fmt_msg_handles_missing_and_null_placeholders() {
        let f = json!({"pe": 18.5, "name": null});
        assert_eq!(fmt_msg("PE {pe:.1f} 在 {pe_quantile_5y} 分位", &f), "PE 18.5 在 ? 分位");
        assert_eq!(fmt_msg("{name}", &f), "?"); // present null → ?
        assert_eq!(fmt_msg("{unknown} {pe}", &f), "? 18.5");
        assert_eq!(fmt_msg("no placeholders", &f), "no placeholders");
        assert_eq!(fmt_msg("", &f), "");
        // unsupported spec → raw template (Python ValueError fallback)
        assert_eq!(fmt_msg("{pe:.1f} {name:.2f}", &json!({"pe": 1.0, "name": "x"})), "{pe:.1f} {name:.2f}");
    }

    #[test]
    fn signals_follow_the_thresholds() {
        let features = json!({
            "market": "A",
            "name": "测试",
            "industry": "白酒",
            "pe": 10, "pe_quantile_5y": 5, "pb": 1.0, "pe_x_pb": 10,
            "net_margin": 30, "debt_ratio": 20, "moat_total": 30,
            "consecutive_dividend_years": 8, "roe_5y_above_15": 5, "roe_5y_min": 18,
            "fcf_known": true, "fcf_positive": true, "fcf_margin": 12, "current_ratio": 3,
            "consecutive_profit_years": 8, "is_safe": true,
        });
        let r = evaluate_investor("buffett", &features);
        assert_eq!(r["signal"], "bullish");
        assert_eq!(r["weight_pass"], r["weight_total"]);
        assert!(r["score"].as_f64().unwrap() >= 65.0);
        assert_eq!(r["pass_rules"].as_array().unwrap().len(), 7);
        assert!(r["headline"].as_str().unwrap().starts_with("看多核心："));
    }

    #[test]
    fn youzi_out_of_range_is_a_skip_with_the_reason() {
        let features = json!({"market": "A", "name": "宁德时代", "industry": "电池", "market_cap_yi": 9000});
        let r = evaluate_investor("zhao_lg", &features);
        assert_eq!(r["signal"], "skip");
        assert_eq!(r["score"], -1);
        assert_eq!(r["skip_reason"], "市值 9000 亿不在 赵老哥 射程");
        assert_eq!(r["headline"], "不适合 — 市值 9000 亿不在 赵老哥 射程");

        // LHB override: the seat actually traded → evaluate anyway
        let mut with_lhb = features.clone();
        with_lhb["matched_youzi"] = json!(["赵老哥"]);
        let r2 = evaluate_investor("zhao_lg", &with_lhb);
        assert_ne!(r2["signal"], "skip");
    }

    #[test]
    fn market_scope_skips_youzi_outside_a_shares() {
        let r = evaluate_investor("zhao_lg", &json!({"market": "US", "name": "Apple"}));
        assert_eq!(r["signal"], "skip");
        assert_eq!(r["skip_reason"], "不看US市场");
    }

    #[test]
    fn output_key_order_matches_the_documented_schema() {
        let r = evaluate_investor("buffett", &json!({"market": "A", "name": "x"}));
        let keys: Vec<&str> = r.as_object().unwrap().keys().map(String::as_str).collect();
        assert_eq!(
            keys,
            vec![
                "investor_id",
                "score",
                "signal",
                "confidence",
                "weight_pass",
                "weight_total",
                "pass_count",
                "fail_count",
                "pass_rules",
                "fail_rules",
                "headline",
                "rationale",
                "time_horizon",
                "position_sizing",
                "what_would_change_my_mind",
            ]
        );
        let rule_keys: Vec<&str> = r["pass_rules"]
            .as_array()
            .and_then(|a| a.first())
            .and_then(Value::as_object)
            .map(|o| o.keys().map(String::as_str).collect())
            .unwrap_or_default();
        if !rule_keys.is_empty() {
            assert_eq!(rule_keys, vec!["rule_id", "name", "weight", "msg"]);
        }
    }

    #[test]
    fn holding_bonus_adds_a_virtual_rule() {
        let r = evaluate_investor("buffett", &json!({"market": "US", "ticker": "AAPL", "name": "苹果", "industry": "消费电子"}));
        assert_eq!(r["signal"], "bullish"); // override_signal from the known holding
        let first = &r["pass_rules"][0];
        assert_eq!(first["rule_id"], "known_holding");
        assert_eq!(first["weight"], 6);
        assert!(first["msg"].as_str().unwrap().starts_with("📌 "));
    }
}
