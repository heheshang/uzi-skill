//! Port of `lib/agent_analysis_validator.py` — schema validator for
//! agent_analysis.json.

use crate::pyfmt;
use serde_json::{json, Map, Value};

#[allow(dead_code)]
const VALID_SIGNALS: &[&str] = &["bullish", "bearish", "neutral", "skip"];
#[allow(dead_code)]
const REQUIRED_DIM_KEYS: &[&str] = &[
    "0_basic", "1_financials", "2_kline", "3_macro", "4_peers", "5_chain", "6_research", "7_industry",
    "8_materials", "9_futures", "10_valuation", "11_governance", "12_capital_flow", "13_policy",
    "14_moat", "15_events", "16_lhb", "17_sentiment", "18_trap", "19_contests",
];
const REQUIRED_BUY_ZONE_KEYS: &[&str] = &["value", "growth", "technical", "youzi"];

/// Python `type(v).__name__` for JSON values.
fn type_name(v: &Value) -> &'static str {
    match v {
        Value::Null => "NoneType",
        Value::Bool(_) => "bool",
        Value::Number(n) => {
            if n.is_i64() || n.is_u64() {
                "int"
            } else {
                "float"
            }
        }
        Value::String(_) => "str",
        Value::Array(_) => "list",
        Value::Object(_) => "dict",
    }
}

fn is_str(v: &Value, min_len: usize) -> bool {
    match v {
        Value::String(s) => s.trim().chars().count() >= min_len,
        _ => false,
    }
}

fn add(issues: &mut Vec<Value>, sev: &str, field: &str, msg: String, sugg: &str) {
    let mut m = Map::new();
    m.insert("severity".into(), json!(sev));
    m.insert("field".into(), json!(field));
    m.insert("message".into(), json!(msg));
    m.insert("suggestion".into(), json!(sugg));
    issues.push(Value::Object(m));
}

pub fn validate_agent_analysis(agent_analysis: &Value) -> Value {
    let mut issues: Vec<Value> = Vec::new();

    if !agent_analysis.is_object() {
        add(
            &mut issues,
            "error",
            "(root)",
            format!(
                "agent_analysis 必须是 dict，实际是 {}",
                type_name(agent_analysis)
            ),
            "整体重写，参考 SKILL.md 的 agent_analysis.json 示例",
        );
        return Value::Array(issues);
    }

    // ── agent_reviewed 标记 ──
    if !agent_analysis
        .get("agent_reviewed")
        .map(uzi_core::py::truthy)
        .unwrap_or(false)
    {
        add(
            &mut issues,
            "warning",
            "agent_reviewed",
            "缺少 agent_reviewed: true 标记".to_string(),
            "加 \"agent_reviewed\": true 在顶层",
        );
    }

    // ── dim_commentary ──
    if let Some(dc) = agent_analysis.get("dim_commentary") {
        if !dc.is_null() {
            if !dc.is_object() {
                add(
                    &mut issues,
                    "error",
                    "dim_commentary",
                    format!(
                        "dim_commentary 必须是 dict（key 是维度名），实际是 {}",
                        type_name(dc)
                    ),
                    "改为 {\"0_basic\": \"...\", \"1_financials\": \"...\", ...}",
                );
            } else {
                for (k, v) in dc.as_object().unwrap() {
                    if !v.is_string() {
                        add(
                            &mut issues,
                            "error",
                            &format!("dim_commentary.{}", k),
                            format!("评语必须是字符串，实际是 {}", type_name(v)),
                            "把评语改成一段连贯文字",
                        );
                    } else if v.as_str().unwrap().trim().chars().count() < 20 {
                        add(
                            &mut issues,
                            "warning",
                            &format!("dim_commentary.{}", k),
                            format!(
                                "评语太短（{} 字），低于 20 字门槛",
                                v.as_str().unwrap().trim().chars().count()
                            ),
                            "至少写 1-2 句话，引用具体数字",
                        );
                    }
                }
            }
        }
    }

    // ── panel_insights ──
    if let Some(pi) = agent_analysis.get("panel_insights") {
        if !pi.is_null() && !is_str(pi, 30) {
            add(
                &mut issues,
                "warning",
                "panel_insights",
                format!(
                    "panel_insights 应是 30+ 字字符串，实际 {} / 长度 {}",
                    type_name(pi),
                    pyfmt::str_exact(pi).chars().count()
                ),
                "用一段话概括 51 评委的投票结构和主要分歧",
            );
        }
    }

    // ── great_divide_override ──
    if let Some(gdo) = agent_analysis.get("great_divide_override") {
        if !gdo.is_null() {
            if !gdo.is_object() {
                add(
                    &mut issues,
                    "error",
                    "great_divide_override",
                    "必须是 dict（含 punchline / bull_say_rounds / bear_say_rounds）".to_string(),
                    "参考 SKILL.md 的格式",
                );
            } else {
                if let Some(pl) = gdo.get("punchline") {
                    if !pl.is_null() && !is_str(pl, 10) {
                        add(
                            &mut issues,
                            "warning",
                            "great_divide_override.punchline",
                            "punchline 应是 10+ 字冲突金句".to_string(),
                            "写一句能传播的话，含具体数字",
                        );
                    }
                }
                for side in ["bull_say_rounds", "bear_say_rounds"] {
                    if let Some(rounds) = gdo.get(side) {
                        if !rounds.is_null() {
                            if !rounds.is_array() {
                                add(
                                    &mut issues,
                                    "error",
                                    &format!("great_divide_override.{}", side),
                                    format!("必须是 list，实际是 {}", type_name(rounds)),
                                    "改为 [\"第 1 轮\", \"第 2 轮\", \"第 3 轮\"]",
                                );
                            } else if rounds.as_array().unwrap().len() < 3 {
                                add(
                                    &mut issues,
                                    "warning",
                                    &format!("great_divide_override.{}", side),
                                    format!(
                                        "应有 3 轮（至少），实际 {}",
                                        rounds.as_array().unwrap().len()
                                    ),
                                    "凑齐 3 句辩论",
                                );
                            }
                        }
                    }
                }
            }
        }
    }

    // ── narrative_override ──
    if let Some(no) = agent_analysis.get("narrative_override") {
        if !no.is_null() {
            if !no.is_object() {
                add(
                    &mut issues,
                    "error",
                    "narrative_override",
                    "必须是 dict".to_string(),
                    "参考 SKILL.md 的格式",
                );
            } else {
                if let Some(cc) = no.get("core_conclusion") {
                    if !cc.is_null() && !is_str(cc, 20) {
                        add(
                            &mut issues,
                            "warning",
                            "narrative_override.core_conclusion",
                            "core_conclusion 应是 20+ 字定论".to_string(),
                            "1-2 句结论 + 评分 + 关键证据",
                        );
                    }
                }
                if let Some(risks) = no.get("risks") {
                    if !risks.is_null() {
                        if !risks.is_array() {
                            add(
                                &mut issues,
                                "error",
                                "narrative_override.risks",
                                format!("必须是 list，实际是 {}", type_name(risks)),
                                "改为 [\"风险 1\", \"风险 2\", ...]",
                            );
                        } else if risks.as_array().unwrap().len() < 3 {
                            add(
                                &mut issues,
                                "warning",
                                "narrative_override.risks",
                                format!("建议至少 3 条风险，实际 {}", risks.as_array().unwrap().len()),
                                "补齐 Top 3 风险",
                            );
                        }
                    }
                }
                if let Some(bz) = no.get("buy_zones") {
                    if !bz.is_null() {
                        if !bz.is_object() {
                            add(
                                &mut issues,
                                "error",
                                "narrative_override.buy_zones",
                                "必须是 dict，含 value/growth/technical/youzi 4 key".to_string(),
                                "参考 SKILL.md 示例",
                            );
                        } else {
                            for k in REQUIRED_BUY_ZONE_KEYS {
                                match bz.get(k) {
                                    None | Some(Value::Null) => {
                                        add(
                                            &mut issues,
                                            "warning",
                                            &format!("narrative_override.buy_zones.{}", k),
                                            format!("缺少 {} 派系买入区间", k),
                                            &format!("加 \"{}\": {{\"price\": X, \"rationale\": \"...\"}}", k),
                                        );
                                    }
                                    Some(zone) if !zone.is_object() => {
                                        add(
                                            &mut issues,
                                            "error",
                                            &format!("narrative_override.buy_zones.{}", k),
                                            format!(
                                                "必须是 dict 含 price + rationale，实际 {}",
                                                type_name(zone)
                                            ),
                                            "改为 {\"price\": 10.5, \"rationale\": \"...\"}",
                                        );
                                    }
                                    Some(zone) => {
                                        if zone.get("price").map(|p| p.is_null()).unwrap_or(true) {
                                            add(
                                                &mut issues,
                                                "warning",
                                                &format!("narrative_override.buy_zones.{}.price", k),
                                                "缺 price 字段".to_string(),
                                                "加 \"price\": <数值>",
                                            );
                                        }
                                        let rationale = zone.get("rationale").cloned().unwrap_or(json!(""));
                                        if !is_str(&rationale, 5) {
                                            add(
                                                &mut issues,
                                                "warning",
                                                &format!("narrative_override.buy_zones.{}.rationale", k),
                                                "缺 rationale 解释".to_string(),
                                                "加 \"rationale\": \"...\"",
                                            );
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    // ── data_gap_acknowledged (v2.3 引入) ──
    if let Some(dga) = agent_analysis.get("data_gap_acknowledged") {
        if !dga.is_null() && !dga.is_object() {
            add(
                &mut issues,
                "error",
                "data_gap_acknowledged",
                format!("必须是 dict（key 是 dim 或 dim.field），实际 {}", type_name(dga)),
                "改为 {\"4_peers\": \"已尝试 X 但失败\", ...}",
            );
        }
    }

    // ── qualitative_deep_dive (v2.4 引入) ──
    if let Some(qdd) = agent_analysis.get("qualitative_deep_dive") {
        if !qdd.is_null() {
            if !qdd.is_object() {
                add(
                    &mut issues,
                    "error",
                    "qualitative_deep_dive",
                    format!("必须是 dict（key 是 6 个 dim），实际 {}", type_name(qdd)),
                    "参考 references/task2.5-qualitative-deep-dive.md 第 5 节",
                );
            } else {
                for (dim_k, dim_v) in qdd.as_object().unwrap() {
                    if !dim_v.is_object() {
                        add(
                            &mut issues,
                            "error",
                            &format!("qualitative_deep_dive.{}", dim_k),
                            format!(
                                "维度内容必须是 dict（含 evidence/associations/conclusion），实际 {}",
                                type_name(dim_v)
                            ),
                            "参考 task2.5 的输出 schema",
                        );
                    } else if let Some(ev) = dim_v.get("evidence") {
                        if !ev.is_null() && !ev.is_array() {
                            add(
                                &mut issues,
                                "error",
                                &format!("qualitative_deep_dive.{}.evidence", dim_k),
                                "evidence 必须是 list".to_string(),
                                "改为 [{\"source\": \"...\", \"url\": \"...\", \"finding\": \"...\"}, ...]",
                            );
                        }
                    }
                }
            }
        }
    }

    Value::Array(issues)
}

pub fn format_issues(issues: &Value) -> String {
    let arr: &[Value] = issues.as_array().map(|a| a.as_slice()).unwrap_or(&[]);
    if arr.is_empty() {
        return "✅ agent_analysis.json schema 校验通过".to_string();
    }
    let mut lines: Vec<String> = Vec::new();
    let errs: Vec<&Value> = arr
        .iter()
        .filter(|i| i.get("severity").and_then(|s| s.as_str()) == Some("error"))
        .collect();
    let warns: Vec<&Value> = arr
        .iter()
        .filter(|i| i.get("severity").and_then(|s| s.as_str()) == Some("warning"))
        .collect();
    if !errs.is_empty() {
        lines.push(format!(
            "🔴 schema 错误 {} 条（结构性，会导致 stage2 fallback）：",
            errs.len()
        ));
        for i in errs.iter().take(10) {
            lines.push(format!(
                "   · {}: {}",
                pyfmt::str_exact(i.get("field").unwrap_or(&Value::Null)),
                pyfmt::str_exact(i.get("message").unwrap_or(&Value::Null))
            ));
            lines.push(format!(
                "     → {}",
                pyfmt::str_exact(i.get("suggestion").unwrap_or(&Value::Null))
            ));
        }
        if errs.len() > 10 {
            lines.push(format!("   ... 还有 {} 条", errs.len() - 10));
        }
    }
    if !warns.is_empty() {
        lines.push(format!(
            "🟡 schema 警告 {} 条（质量问题，stage2 仍会用，但报告可能不达标）：",
            warns.len()
        ));
        for i in warns.iter().take(10) {
            lines.push(format!(
                "   · {}: {}",
                pyfmt::str_exact(i.get("field").unwrap_or(&Value::Null)),
                pyfmt::str_exact(i.get("message").unwrap_or(&Value::Null))
            ));
        }
        if warns.len() > 10 {
            lines.push(format!("   ... 还有 {} 条", warns.len() - 10));
        }
    }
    lines.join("\n")
}
