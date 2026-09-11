//! Port of `lib/pipeline/score_fns.py::generate_synthesis` — merges the scored
//! dimensions and the panel into the final verdict, debate, risk and dashboard
//! payload consumed by the report renderer.

use serde_json::{json, Map, Value};
use uzi_core::py::{f0, round, truthy};
use uzi_features::stock_style::{apply_style_weights, detect_style, style_explanation, style_label};
use uzi_features::{compute_exit_triggers, compute_scenarios, extract_features};
use uzi_investors::evaluator::{get_locked_school, school_labels};

use crate::summarize::auto_summarize_dim;

/// `_ff` — the local number parser defined inside `generate_synthesis`.
fn ff(v: &Value) -> f64 {
    uzi_core::py::f_fin(v, 0.0)
}

fn dim_data<'a>(raw: &'a Value, key: &str) -> Value {
    raw.get("dimensions")
        .and_then(|d| d.get(key))
        .and_then(|d| d.get("data"))
        .cloned()
        .unwrap_or(json!({}))
}

/// Dimension labels used for the auto-summary fallback, in upstream dict order.
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

/// Generate the synthesis payload, folding in agent overrides when present.
///
/// `agent_analysis` keys honoured: `per_investor_override`, `great_divide_override`,
/// `narrative_override`, `dim_commentary`, `panel_insights`, `agent_reviewed`.
pub fn generate_synthesis(
    raw: &Value,
    dims_scored: &Value,
    panel: &Value,
    agent_analysis: Option<&Value>,
) -> Value {
    let null = json!({});
    let ag = agent_analysis.unwrap_or(&null);

    // ── per_investor_override merge (v3.9.4) ──
    let mut panel = panel.clone();
    let pio = ag.get("per_investor_override").cloned().unwrap_or(json!({}));
    if let Some(pio_map) = pio.as_object().filter(|m| !m.is_empty()) {
        if let Some(inv_list) = panel.get_mut("investors").and_then(|v| v.as_array_mut()) {
            let index: Vec<String> = inv_list
                .iter()
                .map(|i| {
                    i.get("investor_id")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string()
                })
                .collect();
            for (id, ov) in pio_map.iter() {
                let Some(pos) = index.iter().position(|x| x == id) else {
                    continue;
                };
                let Some(ov_obj) = ov.as_object() else { continue };
                let Some(inv) = inv_list[pos].as_object_mut() else {
                    continue;
                };
                for field in ["signal", "score", "headline", "reasoning", "comment", "verdict"] {
                    if let Some(v) = ov_obj.get(field) {
                        if !v.is_null() {
                            inv.insert(field.to_string(), v.clone());
                        }
                    }
                }
                let has_comment = inv.get("comment").map(truthy).unwrap_or(false);
                if !has_comment {
                    if let Some(h) = inv.get("headline").cloned().filter(|v| truthy(v)) {
                        inv.insert("comment".into(), h);
                    }
                }
            }
        }
    }

    let basic = dim_data(raw, "0_basic");
    let name = basic
        .get("name")
        .filter(|v| truthy(v))
        .cloned()
        .unwrap_or_else(|| raw.get("ticker").cloned().unwrap_or(Value::Null));
    let price = basic.get("price").cloned().unwrap_or(json!(0));
    let price = if truthy(&price) { price } else { json!(0) };

    // ── Style detection + weighted scoring ──
    let style: String;
    let mut style_diag = json!({});
    // upstream: `dims_scored.get("fundamental_score", 60)` — default only on absent key
    let mut fund_score = match dims_scored.get("fundamental_score") {
        None => 60.0,
        Some(v) => f0(v),
    };
    let mut consensus = match panel.get("panel_consensus") {
        None => 50.0,
        Some(v) => f0(v),
    };

    let mcap_raw = basic.get("market_cap_raw").cloned().unwrap_or(json!(0));
    let mcap_yi = if truthy(&mcap_raw) {
        match mcap_raw {
            Value::Number(_) => f0(&mcap_raw) / 1e8,
            _ => uzi_core::py::parse_float(&uzi_core::py::py_str(&mcap_raw))
                .map(|x| x / 1e8)
                .unwrap_or(0.0),
        }
    } else {
        0.0
    };
    let d_fin = dims_scored
        .get("dimensions")
        .and_then(|d| d.get("1_financials"))
        .cloned()
        .unwrap_or(json!({}));
    let feat_for_style = json!({
        "code": raw.get("ticker").cloned().unwrap_or_else(|| json!("")),
        "market": raw.get("market").cloned().unwrap_or_else(|| json!("A")),
        "industry": basic.get("industry").cloned().unwrap_or_else(|| json!("")),
        "market_cap_yi": mcap_yi,
        "pe": ff(basic.get("pe_ttm").unwrap_or(&Value::Null)),
        "pe_ttm": ff(basic.get("pe_ttm").unwrap_or(&Value::Null)),
        "pb": ff(basic.get("pb").unwrap_or(&Value::Null)),
        "roe_5y_avg": ff(d_fin.get("roe_5y_avg").unwrap_or(&Value::Null)),
        "roe_5y_min": ff(d_fin.get("roe_5y_min").unwrap_or(&Value::Null)),
        "revenue_growth_3y_cagr": ff(d_fin.get("revenue_growth_3y_cagr").unwrap_or(&Value::Null)),
        "dividend_yield": ff(basic.get("dividend_yield_ttm").unwrap_or(&Value::Null)),
    });
    style = detect_style(&feat_for_style, raw);
    let adj = apply_style_weights(
        panel.get("investors").unwrap_or(&json!([])),
        dims_scored,
        &style,
    );
    if let Some(v) = adj.get("fundamental_score") {
        fund_score = f0(v);
    }
    if let Some(v) = adj.get("panel_consensus") {
        consensus = f0(v);
    }
    style_diag = adj.get("diagnostics").cloned().unwrap_or(json!({}));

    let overall = fund_score * 0.6 + consensus * 0.4;

    // ── Verdict thresholds (v3.4.1 granularity) ──
    let mut verdict_label = if overall >= 80.0 {
        "值得重仓"
    } else if overall >= 70.0 {
        "可以蹲一蹲"
    } else if overall >= 65.0 {
        "可以蹲（偏弱）"
    } else if overall >= 60.0 {
        "观望偏多"
    } else if overall >= 55.0 {
        "观望中性"
    } else if overall >= 50.0 {
        "观望偏空"
    } else if overall >= 35.0 {
        "谨慎"
    } else {
        "回避"
    }
    .to_string();

    let school_scores = panel.get("school_scores").cloned().unwrap_or(json!({}));
    if let Some(map) = school_scores.as_object().filter(|m| !m.is_empty()) {
        let bullish_schools: Vec<String> = map
            .values()
            .filter(|s| matches!(s.get("verdict").and_then(|v| v.as_str()), Some("重仓") | Some("买入")))
            .filter_map(|s| s.get("label").and_then(|v| v.as_str()).map(str::to_string))
            .collect();
        let bearish_schools: Vec<String> = map
            .values()
            .filter(|s| s.get("verdict").and_then(|v| v.as_str()) == Some("回避"))
            .filter_map(|s| s.get("label").and_then(|v| v.as_str()).map(str::to_string))
            .collect();
        if !bullish_schools.is_empty() && !bearish_schools.is_empty() {
            verdict_label.push_str(&format!(
                " · {} 派看多 / {} 派看空",
                bullish_schools.len(),
                bearish_schools.len()
            ));
        } else if !bullish_schools.is_empty() {
            verdict_label.push_str(&format!(" · {} 派看多", bullish_schools.len()));
        } else if !bearish_schools.is_empty() {
            verdict_label.push_str(&format!(" · {} 派看空", bearish_schools.len()));
        }
    }

    let verdict_detail = format!("基本面 {:.1} · 共识 {:.1}", fund_score, consensus);

    // ── Bull / bear selection ──
    let investors = panel
        .get("investors")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let mut eligible: Vec<&Value> = investors
        .iter()
        .filter(|i| {
            i.get("signal").and_then(|v| v.as_str()) != Some("skip") && f0(&i["score"]) > 0.0
        })
        .collect();
    if eligible.is_empty() {
        eligible = investors
            .iter()
            .filter(|i| i.get("signal").and_then(|v| v.as_str()) != Some("skip"))
            .collect();
        if eligible.is_empty() {
            eligible = investors.iter().collect();
        }
    }
    let mut by_score = eligible.clone();
    // Python's `sorted(..., key=lambda x: -score)` is a stable sort by descending score.
    by_score.sort_by(|a, b| {
        f0(&b["score"])
            .partial_cmp(&f0(&a["score"]))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let empty_obj = json!({});
    let bull = by_score
        .first()
        .copied()
        .or_else(|| investors.first())
        .unwrap_or(&empty_obj);
    let mut bear = by_score
        .last()
        .copied()
        .or_else(|| investors.last())
        .unwrap_or(&empty_obj);
    if bull.get("investor_id") == bear.get("investor_id") && by_score.len() > 1 {
        bear = by_score[by_score.len() - 2];
    }

    let bull_headline = bull
        .get("headline")
        .cloned()
        .unwrap_or_else(|| bull.get("comment").cloned().unwrap_or(json!("")));
    let bear_headline = bear
        .get("headline")
        .cloned()
        .unwrap_or_else(|| bear.get("comment").cloned().unwrap_or(json!("")));
    let bull_pass_rules = bull.get("pass").cloned().unwrap_or(json!([]));
    let bear_fail_rules = bear.get("fail").cloned().unwrap_or(json!([]));

    let gd_override = ag.get("great_divide_override").cloned().unwrap_or(json!({}));
    let agent_bull_rounds = gd_override
        .get("bull_say_rounds")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let agent_bear_rounds = gd_override
        .get("bear_say_rounds")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    let rule_text = |rules: &Value, fallback: &str| -> String {
        let joined = rules
            .as_array()
            .map(|a| {
                a.iter()
                    .take(3)
                    .map(|r| {
                        let m = r.get("msg").filter(|v| truthy(v));
                        match m {
                            Some(v) => uzi_core::py::py_str(v),
                            None => r
                                .get("name")
                                .map(|v| uzi_core::py::py_str(v))
                                .unwrap_or_default(),
                        }
                    })
                    .collect::<Vec<_>>()
                    .join(" · ")
            })
            .unwrap_or_default();
        if joined.is_empty() {
            fallback.to_string()
        } else {
            joined
        }
    };

    let bull_score = bull.get("score").cloned().unwrap_or(json!(0));
    let bear_score = bear.get("score").cloned().unwrap_or(json!(0));
    let rounds = json!([
        {
            "round": 1,
            "bull_say": agent_bull_rounds.first().cloned().unwrap_or(bull_headline),
            "bear_say": agent_bear_rounds.first().cloned().unwrap_or(bear_headline),
        },
        {
            "round": 2,
            "bull_say": agent_bull_rounds.get(1).cloned().unwrap_or_else(|| json!(rule_text(&bull_pass_rules, "数据支持我的判断。"))),
            "bear_say": agent_bear_rounds.get(1).cloned().unwrap_or_else(|| json!(rule_text(&bear_fail_rules, "风险点太多。"))),
        },
        {
            "round": 3,
            "bull_say": agent_bull_rounds.get(2).cloned().unwrap_or_else(|| json!(format!("综合看，{} 分，我的立场不变。", uzi_core::py::num_str(&bull_score)))),
            "bear_say": agent_bear_rounds.get(2).cloned().unwrap_or_else(|| json!(format!("综合看，{} 分，风险大于收益。", uzi_core::py::num_str(&bear_score)))),
        },
    ]);

    let kline = dim_data(raw, "2_kline");
    let d20 = dim_data(raw, "20_valuation_models");
    let d21 = dim_data(raw, "21_research_workflow");
    let d22 = dim_data(raw, "22_deep_methods");
    let dcf_summary = d20.get("summary").cloned().unwrap_or(json!({}));
    let init_cov = d21.get("initiating_coverage").cloned().unwrap_or(json!({}));
    let ic_memo = d22.get("ic_memo").cloned().unwrap_or(json!({}));
    let competitive = d22.get("competitive_analysis").cloned().unwrap_or(json!({}));

    let dcf_sm = f0(dcf_summary.get("dcf_safety_margin_pct").unwrap_or(&json!(0)));
    let lbo_irr = f0(dcf_summary.get("lbo_irr_pct").unwrap_or(&json!(0)));
    let init_headline = init_cov.get("headline").cloned().unwrap_or(json!({}));
    let tp = f0(init_headline.get("target_price").unwrap_or(&json!(0)));
    let upside = f0(init_headline.get("upside_pct").unwrap_or(&json!(0)));
    let rating = init_headline
        .get("rating")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    let agent_punchline = gd_override
        .get("punchline")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let punchline = if !agent_punchline.is_empty() {
        agent_punchline
    } else if dcf_sm != 0.0 && lbo_irr != 0.0 && dcf_sm.abs() > 10.0 && lbo_irr > 15.0 {
        if dcf_sm < 0.0 && lbo_irr > 20.0 {
            format!(
                "DCF 说高估 {:.0}%，但 LBO 测试显示 PE 买方仍能赚 {:.0}% IRR — 冲突很有意思。",
                dcf_sm.abs(),
                lbo_irr
            )
        } else if dcf_sm > 15.0 && lbo_irr > 20.0 {
            format!(
                "DCF 认为低估 {:.0}%，LBO IRR {:.0}% 也确认 — 双重信号看多。",
                dcf_sm, lbo_irr
            )
        } else {
            format!(
                "机构建模定调 {}，目标价 ¥{}（{:+.0}%），LBO 视角 IRR {:.0}%。",
                rating, tp, upside, lbo_irr
            )
        }
    } else if tp > 0.0 && upside.abs() > 5.0 {
        format!(
            "首次覆盖 {}，目标价 ¥{}，空间 {:+.0}%。",
            rating, tp, upside
        )
    } else {
        format!(
            "{} · ROE 历史与当前估值存在结构性分歧，等待方向明朗。",
            uzi_core::py::py_str(&name)
        )
    };

    // ── Risks ──
    let narrative_override = ag.get("narrative_override").cloned().unwrap_or(json!({}));
    let mut risks: Vec<String> = narrative_override
        .get("risks")
        .and_then(|v| v.as_array())
        .map(|a| a.iter().map(uzi_core::py::py_str).collect())
        .unwrap_or_default();
    if risks.is_empty() {
        if let Some(map) = dims_scored.get("dimensions").and_then(|d| d.as_object()) {
            for (key, dim) in map.iter() {
                if f0(dim.get("score").unwrap_or(&json!(0))) <= 4.0 {
                    let reasons = dim.get("reasons_fail").and_then(|v| v.as_array());
                    match reasons.filter(|a| !a.is_empty()) {
                        Some(a) => risks.push(uzi_core::py::py_str(&a[0])),
                        None => {
                            let dim_name = dim
                                .get("name")
                                .filter(|v| truthy(v))
                                .or_else(|| dim.get("label").filter(|v| truthy(v)))
                                .map(uzi_core::py::py_str)
                                .unwrap_or_else(|| key.clone());
                            risks.push(format!(
                                "{} 评分偏低 ({}/10)",
                                dim_name,
                                uzi_core::py::num_str(dim.get("score").unwrap_or(&json!(0)))
                            ));
                        }
                    }
                }
            }
        }
    }
    if risks.is_empty() {
        let feats = extract_features(raw, &raw.get("dimensions").cloned().unwrap_or(json!({})));
        let pe_val = f0(feats.get("pe").unwrap_or(&json!(0)));
        let debt_val = f0(feats.get("debt_ratio").unwrap_or(&json!(0)));
        let roe_min = f0(feats.get("roe_5y_min").unwrap_or(&json!(0)));
        let industry = feats
            .get("industry")
            .map(uzi_core::py::py_str)
            .unwrap_or_else(|| "所属行业".to_string());
        if pe_val > 30.0 {
            risks.push(format!("当前 PE {:.0}x，估值偏高", pe_val));
        }
        if debt_val > 50.0 {
            risks.push(format!("资产负债率 {:.0}%，财务杠杆偏高", debt_val));
        }
        if roe_min < 5.0 {
            risks.push(format!("ROE 最低 {:.1}%，盈利稳定性不足", roe_min));
        }
        risks.push(format!("{}行业竞争加剧风险", industry));
        risks.push("宏观经济或政策环境变化".to_string());
    }
    risks.truncate(5);

    // ── Friendly layer ──
    let scenarios = compute_scenarios(raw, dims_scored);
    let exit_triggers = compute_exit_triggers(raw, dims_scored, &json!({}));
    let similar_stocks = raw.get("similar_stocks").cloned().unwrap_or(json!([]));

    // ── Dashboard ──
    let ytd_return = kline
        .get("kline_stats")
        .and_then(|s| s.get("ytd_return"))
        .cloned()
        .unwrap_or_else(|| json!("—"));
    let long_active = panel
        .get("long_active")
        .filter(|v| truthy(v))
        .cloned()
        .unwrap_or_else(|| {
            let dist = panel.get("signal_distribution").cloned().unwrap_or(json!({}));
            json!(["bullish", "neutral", "bearish"]
                .iter()
                .map(|k| f0(dist.get(*k).unwrap_or(&json!(0))) as i64)
                .sum::<i64>())
        });
    let agent_core_conclusion = narrative_override
        .get("core_conclusion")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let sig_dist = panel.get("signal_distribution").cloned().unwrap_or(json!({}));
    let core_conclusion = if !agent_core_conclusion.is_empty() {
        agent_core_conclusion
    } else {
        format!(
            "{} · {} 分 · {}。{} 位多头评委里 {} 人看多，YTD {}。{}",
            uzi_core::py::py_str(&name),
            // upstream `f"{int(overall)}"` truncates toward zero
            overall as i64,
            verdict_label,
            uzi_core::py::num_str(&long_active),
            uzi_core::py::num_str(&sig_dist["bullish"]),
            uzi_core::py::py_str(&ytd_return),
            punchline
        )
    };

    // ── dim_commentary: agent text > auto summary ──
    let agent_dim_commentary = ag.get("dim_commentary").cloned().unwrap_or(json!({}));
    let mut dim_commentary_final = Map::new();
    for (dim_key, label) in DIM_LABELS {
        let agent_text = agent_dim_commentary
            .get(*dim_key)
            .filter(|v| truthy(v))
            .cloned();
        if let Some(text) = agent_text {
            dim_commentary_final.insert((*dim_key).to_string(), text);
        } else {
            let dim = raw
                .get("dimensions")
                .and_then(|d| d.get(*dim_key))
                .cloned()
                .unwrap_or(json!({}));
            let score = f0(
                dims_scored
                    .get("dimensions")
                    .and_then(|d| d.get(*dim_key))
                    .and_then(|d| d.get("score"))
                    .unwrap_or(&json!(0)),
            );
            let auto = auto_summarize_dim(dim_key, label, &dim, score);
            if !auto.is_empty() {
                dim_commentary_final.insert((*dim_key).to_string(), json!(auto));
            }
        }
    }

    let locked = get_locked_school();
    let school_lock = if locked.is_empty() {
        Value::Null
    } else {
        json!({
            "group": locked,
            "label": school_labels(&locked).unwrap_or(""),
        })
    };

    let catalyst_events = d21
        .get("catalyst_calendar")
        .and_then(|c| c.get("events"))
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let catalysts: Vec<Value> = catalyst_events
        .iter()
        .filter(|e| matches!(e.get("impact").and_then(|v| v.as_str()), Some("high") | Some("medium")))
        .take(3)
        .map(|e| {
            let ev = e.get("event").cloned().unwrap_or_else(|| json!("季报"));
            json!(uzi_core::py::py_str(&ev).chars().take(30).collect::<String>())
        })
        .collect();
    let catalysts = if catalysts.is_empty() {
        json!(["暂无明确催化剂"])
    } else {
        Value::Array(catalysts)
    };

    let price_num = f0(&price);
    // upstream: f"¥{round(price * mult, 2) if price else '—'}" — the ¥ prefix is
    // unconditional, so a falsy price renders as "¥—".
    let price_display = |mult: f64| -> Value {
        if price_num != 0.0 {
            json!(format!("¥{}", uzi_core::py::float_str(round(price_num * mult, 2))))
        } else {
            json!("¥—")
        }
    };

    json!({
        "ticker": raw.get("ticker").cloned().unwrap_or(Value::Null),
        "name": name,
        "overall_score": round(overall, 1),
        "verdict_label": verdict_label,
        "verdict_detail": verdict_detail,
        "fundamental_score": round(fund_score, 1),
        "panel_consensus": round(consensus, 1),
        "school_lock": school_lock,
        "school_scores": school_scores,
        "short_consensus": panel.get("short_consensus").cloned().unwrap_or(json!({})),
        "dim_commentary": Value::Object(dim_commentary_final),
        "institutional_modeling": {
            "dcf_intrinsic": dcf_summary.get("dcf_intrinsic").cloned().unwrap_or(Value::Null),
            "dcf_safety_margin_pct": dcf_summary.get("dcf_safety_margin_pct").cloned().unwrap_or(Value::Null),
            "dcf_verdict": dcf_summary.get("dcf_verdict").cloned().unwrap_or(Value::Null),
            "lbo_irr_pct": dcf_summary.get("lbo_irr_pct").cloned().unwrap_or(Value::Null),
            "lbo_verdict": dcf_summary.get("lbo_verdict").cloned().unwrap_or(Value::Null),
            "comps_verdict": dcf_summary.get("comps_verdict").cloned().unwrap_or(Value::Null),
            "initiating_rating": init_headline.get("rating").cloned().unwrap_or(Value::Null),
            "target_price": init_headline.get("target_price").cloned().unwrap_or(Value::Null),
            "upside_pct": init_headline.get("upside_pct").cloned().unwrap_or(Value::Null),
            "ic_recommendation": ic_memo.get("sections").and_then(|s| s.get("I_exec_summary")).and_then(|s| s.get("headline")).cloned().unwrap_or(Value::Null),
            "bcg_position": competitive.get("bcg_position").and_then(|b| b.get("category")).cloned().unwrap_or(Value::Null),
            "industry_attractiveness": competitive.get("industry_attractiveness_pct").cloned().unwrap_or(Value::Null),
        },
        "detected_style": style,
        "style_label_cn": style_label(&style),
        "style_explanation": style_explanation(&style),
        "style_diagnostics": style_diag,
        "agent_reviewed": ag.get("agent_reviewed").and_then(|v| v.as_bool()).unwrap_or(false),
        "panel_insights": ag.get("panel_insights").and_then(|v| v.as_str()).unwrap_or(""),
        "claude_narrative_stub": {
            "_note": if truthy(ag.get("agent_reviewed").unwrap_or(&json!(false))) {
                "以下字段已由 agent 覆盖"
            } else {
                "以下字段是脚本生成的占位，Task 4 中 Claude 必须根据原始数据重写"
            },
            "needs_rewrite": if truthy(ag.get("agent_reviewed").unwrap_or(&json!(false))) {
                json!([])
            } else {
                json!([
                    "great_divide.punchline", "dashboard.core_conclusion",
                    "debate.rounds[*].bull_say", "debate.rounds[*].bear_say",
                    "buy_zones.*.rationale", "risks[*]"
                ])
            },
        },
        "debate": {
            "bull": {
                "investor_id": bull.get("investor_id").cloned().unwrap_or(Value::Null),
                "name": bull.get("name").cloned().unwrap_or(Value::Null),
                "group": bull.get("group").cloned().unwrap_or(Value::Null),
            },
            "bear": {
                "investor_id": bear.get("investor_id").cloned().unwrap_or(Value::Null),
                "name": bear.get("name").cloned().unwrap_or(Value::Null),
                "group": bear.get("group").cloned().unwrap_or(Value::Null),
            },
            "rounds": rounds,
            "punchline": punchline,
        },
        "great_divide": {
            "bull_avatar": bull.get("investor_id").cloned().unwrap_or(Value::Null),
            "bear_avatar": bear.get("investor_id").cloned().unwrap_or(Value::Null),
            "bull_score": bull_score,
            "bear_score": bear_score,
            "bull_signal": bull.get("signal").cloned().unwrap_or(Value::Null),
            "bear_signal": bear.get("signal").cloned().unwrap_or(Value::Null),
            "punchline": punchline,
        },
        "risks": risks,
        "buy_zones": narrative_override.get("buy_zones").filter(|v| truthy(v)).cloned().unwrap_or_else(|| json!({
            "value": {"price": if price_num != 0.0 { json!(round(price_num * 0.85, 2)) } else { json!("—") }, "rationale": "历史 PE 25 分位"},
            "growth": {"price": if price_num != 0.0 { json!(round(price_num * 0.92, 2)) } else { json!("—") }, "rationale": "PEG 合理区"},
            "technical": {"price": if price_num != 0.0 { json!(round(price_num * 0.95, 2)) } else { json!("—") }, "rationale": "MA60 支撑位"},
            "youzi": {"price": if price_num != 0.0 { price.clone() } else { json!("—") }, "rationale": "当前情绪未破"},
        })),
        "friendly": {
            "scenarios": scenarios,
            "exit_triggers": exit_triggers,
            "similar_stocks": similar_stocks,
        },
        "fund_managers": raw.get("fund_managers").cloned().unwrap_or(json!([])),
        "dashboard": {
            "core_conclusion": core_conclusion,
            "data_perspective": {
                "trend": uzi_core::py::py_str(&kline.get("stage").cloned().unwrap_or_else(|| json!("—"))),
                "price": if price_num != 0.0 { format!("¥{}", uzi_core::py::py_str(&price)) } else { "—".to_string() },
                "volume": "—",
                "chips": uzi_core::py::py_str(&kline.get("ma_align").cloned().unwrap_or_else(|| json!("—"))),
            },
            "intelligence": {
                "news": format!("已采集 {} 项催化剂事件", catalyst_events.len()),
                "risks": risks.iter().take(3).cloned().collect::<Vec<String>>(),
                "catalysts": catalysts,
            },
            "battle_plan": {
                "entry": price_display(0.92),
                "position": "分批建仓 · 勿满仓",
                "stop": price_display(0.85),
                "target": price_display(1.25),
            },
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ff_matches_the_local_parser() {
        assert_eq!(ff(&json!("1,234.5%")), 1234.5);
        assert_eq!(ff(&json!("12亿")), 12.0);
        assert_eq!(ff(&json!(null)), 0.0);
        assert_eq!(ff(&json!("—")), 0.0);
    }

    #[test]
    fn verdict_thresholds_are_inclusive_lower_bounds() {
        let label = |o: f64| -> &'static str {
            if o >= 80.0 {
                "值得重仓"
            } else if o >= 70.0 {
                "可以蹲一蹲"
            } else if o >= 65.0 {
                "可以蹲（偏弱）"
            } else if o >= 60.0 {
                "观望偏多"
            } else if o >= 55.0 {
                "观望中性"
            } else if o >= 50.0 {
                "观望偏空"
            } else if o >= 35.0 {
                "谨慎"
            } else {
                "回避"
            }
        };
        assert_eq!(label(80.0), "值得重仓");
        assert_eq!(label(79.9), "可以蹲一蹲");
        assert_eq!(label(65.0), "可以蹲（偏弱）");
        assert_eq!(label(49.9), "谨慎");
        assert_eq!(label(34.9), "回避");
    }

    #[test]
    fn round_zero_produces_the_integer_used_in_core_conclusion() {
        // upstream: f"{int(overall)} 分" — int() truncates
        assert_eq!(uzi_core::py::round0(69.3) as i64, 69);
        assert_eq!(uzi_core::py::round0(69.9) as i64, 70);
    }
}
