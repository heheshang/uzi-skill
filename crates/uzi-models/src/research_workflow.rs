//! Port of `lib/research_workflow.py` — equity research workflow modules:
//! initiating coverage, post-earnings analysis, catalyst calendar, thesis
//! tracker, morning note, quant idea screens and sector overview.

use crate::{dim_data, get_or, pnum, py_str_py};
use chrono::{Datelike, Duration, NaiveDateTime};
use serde_json::{json, Map, Value};
use uzi_core::features::sanitize_features;
use uzi_core::py::round;

use crate::clock;

fn last_n(v: &Value, n: usize) -> Vec<Value> {
    match v.as_array() {
        Some(a) => a[a.len().saturating_sub(n)..].to_vec(),
        None => Vec::new(),
    }
}

// ═══════════════════════════════════════════════════════════════
// 1. INITIATING COVERAGE REPORT
// ═══════════════════════════════════════════════════════════════

/// `build_initiating_coverage` — institutional-style init report in 6 sections.
pub fn build_initiating_coverage(
    features: &Value,
    raw_data: &Value,
    dcf_result: Option<&Value>,
    comps_result: Option<&Value>,
) -> Value {
    let features = sanitize_features(features);
    let features = &features;
    let basic = dim_data(raw_data, "0_basic");
    let fin = dim_data(raw_data, "1_financials");
    let moat = dim_data(raw_data, "14_moat");
    let research = dim_data(raw_data, "6_research");

    let name = get_or(basic, "name", json!("—"));
    let industry = get_or(basic, "industry", json!("—"));
    let price = pnum(uzi_core::py::get(basic, "price"), 0.0);

    // Target price: blend DCF + comps if available
    let mut targets: Vec<(String, f64)> = Vec::new();
    let dcf_intrinsic = pnum(
        dcf_result
            .map(|d| uzi_core::py::get(d, "intrinsic_per_share"))
            .unwrap_or(&Value::Null),
        0.0,
    );
    let dcf_result = dcf_result.filter(|d| uzi_core::py::truthy(d));
    if dcf_intrinsic > 0.0 {
        targets.push(("DCF".to_string(), dcf_intrinsic));
    }
    if let Some(comps) = comps_result.filter(|c| uzi_core::py::truthy(c)) {
        let implied = get_or(comps, "implied_price", json!({}));
        if let Some(map) = implied.as_object() {
            for (k, v) in map {
                if pnum(v, 0.0) > 0.0 {
                    targets.push((k.clone(), pnum(v, 0.0)));
                }
            }
        }
    }
    let blended = if !targets.is_empty() {
        let sum: f64 = targets.iter().map(|t| t.1).sum();
        round(sum / targets.len() as f64, 2)
    } else {
        0.0
    };
    let has_targets = !targets.is_empty();
    let upside_pct = if has_targets && price > 0.0 {
        round((blended - price) / price * 100.0, 1)
    } else {
        0.0
    };

    // Rating logic
    let rating: String = if !has_targets {
        "未评级 (Not Rated)".to_string()
    } else if upside_pct >= 25.0 {
        "买入 (Overweight)".to_string()
    } else if upside_pct >= 10.0 {
        "增持 (Outperform)".to_string()
    } else if upside_pct >= -10.0 {
        "持有 (Neutral)".to_string()
    } else {
        "减持 (Underperform)".to_string()
    };

    // Executive summary
    let roe_hist = get_or(fin, "roe_history", json!([]));
    let roe_hist_arr = roe_hist.as_array().cloned().unwrap_or_default();
    let roe_last = if !roe_hist_arr.is_empty() {
        pnum(roe_hist_arr.last().unwrap(), 0.0)
    } else {
        0.0
    };
    let code_str = py_str_py(&get_or(basic, "code", json!("-")));
    let exec_summary = if has_targets {
        format!(
            "我们首次覆盖{}（{}），给予「{}」评级，目标价 ¥{:.2}，较现价 ¥{:.2} 空间 {:+.1}%。公司属于{}行业，最新 ROE {:.1}%。",
            py_str_py(&name),
            code_str,
            rating,
            blended,
            price,
            upside_pct,
            py_str_py(&industry),
            roe_last
        )
    } else {
        format!(
            "我们首次覆盖{}（{}），暂不给出目标价或方向评级。DCF 与可比公司估值均缺少有效结果，应待盈利恢复或补齐 PB/Comps 数据后重估。公司属于{}行业，最新 ROE {:.1}%。",
            py_str_py(&name),
            code_str,
            py_str_py(&industry),
            roe_last
        )
    };

    // Investment thesis (3-5 pillars)
    let thesis_pillars = build_thesis_pillars(features, moat);

    // Risks
    let risks = build_risks(features);

    // Valuation bridge
    let mut valuation_bridge: Vec<Value> = Vec::new();
    if dcf_intrinsic > 0.0 {
        let dcf = dcf_result.unwrap();
        let wacc_breakdown = get_or(dcf, "wacc_breakdown", json!({}));
        let assumptions = get_or(dcf, "assumptions", json!({}));
        valuation_bridge.push(json!({
            "method": "DCF",
            "value": crate::num_value(dcf_intrinsic),
            "rationale": format!(
                "WACC {:.1}% + 终值 g {:.1}%",
                pnum(&get_or(&wacc_breakdown, "wacc", json!(null)), 0.0) * 100.0,
                pnum(&get_or(&assumptions, "terminal_g", json!(null)), 0.0) * 100.0
            ),
        }));
    }
    if let Some(comps) = comps_result.filter(|c| uzi_core::py::truthy(c)) {
        let implied = get_or(comps, "implied_price", json!({}));
        if let Some(map) = implied.as_object() {
            for (k, v) in map {
                let value = pnum(v, 0.0);
                if value <= 0.0 {
                    continue;
                }
                valuation_bridge.push(json!({
                    "method": format!("Comps ({})", k),
                    "value": crate::num_value(value),
                    "rationale": "同行中位数估值法",
                }));
            }
        }
    }
    if has_targets {
        valuation_bridge.push(json!({
            "method": "Blended",
            "value": crate::num_value(blended),
            "rationale": format!("平均 {} 种估值方法", targets.len()),
        }));
    }

    // Key financial table (5yr hist)
    let rev_hist = get_or(fin, "revenue_history", json!([]));
    let ni_hist = get_or(fin, "net_profit_history", json!([]));

    let mut company = Map::new();
    company.insert("name".into(), name);
    company.insert("code".into(), get_or(basic, "code", Value::Null));
    company.insert("industry".into(), industry);

    let mut headline = Map::new();
    headline.insert("rating".into(), Value::String(rating.clone()));
    headline.insert(
        "target_price".into(),
        if has_targets {
            crate::num_value(blended)
        } else {
            json!(0)
        },
    );
    headline.insert("current_price".into(), crate::num_value(price));
    headline.insert(
        "upside_pct".into(),
        if has_targets && price > 0.0 {
            crate::num_value(upside_pct)
        } else {
            json!(0)
        },
    );
    headline.insert(
        "report_date".into(),
        Value::String(clock::date_str(&clock::now())),
    );

    let mut financial_snapshot = Map::new();
    financial_snapshot.insert(
        "revenue_history_yi".into(),
        if rev_hist.as_array().map(|a| !a.is_empty()).unwrap_or(false) {
            Value::Array(last_n(&rev_hist, 5))
        } else {
            Value::Array(Vec::new())
        },
    );
    financial_snapshot.insert(
        "net_profit_history_yi".into(),
        if ni_hist.as_array().map(|a| !a.is_empty()).unwrap_or(false) {
            Value::Array(last_n(&ni_hist, 5))
        } else {
            Value::Array(Vec::new())
        },
    );
    financial_snapshot.insert(
        "roe_history".into(),
        if !roe_hist_arr.is_empty() {
            Value::Array(last_n(&roe_hist, 5))
        } else {
            Value::Array(Vec::new())
        },
    );

    let mut out = Map::new();
    out.insert(
        "method".into(),
        Value::String("Initiating Coverage (JPM/GS/MS style)".into()),
    );
    out.insert("company".into(), Value::Object(company));
    out.insert("headline".into(), Value::Object(headline));
    out.insert("executive_summary".into(), Value::String(exec_summary));
    out.insert("investment_thesis".into(), thesis_pillars);
    out.insert("key_risks".into(), risks);
    out.insert("valuation_bridge".into(), Value::Array(valuation_bridge.clone()));
    out.insert("financial_snapshot".into(), Value::Object(financial_snapshot));
    out.insert("coverage_universe_pos".into(), coverage_positioning(research));
    out.insert(
        "methodology_log".into(),
        json!([
            "Task 1 · 公司研究 — 业务/管理层/行业扫描 ✓",
            "Task 2 · 财务模型 — 5 年历史 + 3 年预测 ✓",
            format!("Task 3 · 估值分析 — {} 种方法混合 ✓", valuation_bridge.len()),
            format!("Task 4 · 综合评级 — {}，目标价 ¥{:.2}", rating, blended),
        ]),
    );
    Value::Object(out)
}

/// `_build_thesis_pillars`.
fn build_thesis_pillars(features: &Value, moat: &Value) -> Value {
    let mut pillars: Vec<Value> = Vec::new();

    let roe_5y_above_15 = pnum(uzi_core::py::get(features, "roe_5y_above_15"), 0.0);
    if roe_5y_above_15 >= 3.0 {
        pillars.push(json!({
            "pillar": "盈利质量优秀",
            "evidence": format!(
                "过去 5 年有 {} 年 ROE > 15%，体现真实回报能力",
                py_str_py(&get_or(features, "roe_5y_above_15", json!(0)))
            ),
            "weight": "High",
        }));
    }

    let moat_scores = if moat.is_object() {
        get_or(moat, "scores", json!({}))
    } else {
        json!({})
    };
    let total_moat: f64 = if moat_scores
        .as_object()
        .map(|m| !m.is_empty())
        .unwrap_or(false)
    {
        moat_scores
            .as_object()
            .unwrap()
            .values()
            .map(|v| pnum(v, 0.0))
            .sum()
    } else {
        0.0
    };
    if total_moat >= 28.0 {
        pillars.push(json!({
            "pillar": "护城河清晰",
            "evidence": format!("无形资产 + 转换成本 + 规模 + 网络效应四项合计 {:.0}/40", total_moat),
            "weight": "High",
        }));
    }

    let rg = pnum(uzi_core::py::get(features, "rev_growth_3y"), 0.0);
    if rg > 15.0 {
        pillars.push(json!({
            "pillar": "营收高增长",
            "evidence": format!("3 年复合 {:.0}% 增速，高于行业中位数", rg),
            "weight": "Medium",
        }));
    }

    let nm = pnum(uzi_core::py::get(features, "net_margin"), 0.0);
    if nm > 15.0 {
        pillars.push(json!({
            "pillar": "高净利率定价能力",
            "evidence": format!("净利率 {:.0}%，反映定价权与成本控制", nm),
            "weight": "Medium",
        }));
    }

    if uzi_core::py::truthy(uzi_core::py::get(features, "fcf_known"))
        && uzi_core::py::truthy(uzi_core::py::get(features, "fcf_positive"))
    {
        pillars.push(json!({
            "pillar": "自由现金流健康",
            "evidence": "持续正 FCF 支撑分红与再投资",
            "weight": "Medium",
        }));
    }

    if pillars.is_empty() {
        pillars.push(json!({
            "pillar": "（暂未发现明确支柱）",
            "evidence": "基本面数据不足以支撑看多论点",
            "weight": "Low",
        }));
    }
    Value::Array(pillars.into_iter().take(5).collect())
}

/// `_build_risks`.
fn build_risks(features: &Value) -> Value {
    let mut risks: Vec<Value> = Vec::new();
    let debt = pnum(uzi_core::py::get(features, "debt_ratio"), 0.0);
    if debt > 60.0 {
        risks.push(json!({
            "risk": "财务杠杆偏高", "severity": "High",
            "detail": format!("资产负债率 {:.0}%", debt),
        }));
    }
    let roe_5y_min = pnum(&get_or(features, "roe_5y_min", json!(99)), 0.0);
    if roe_5y_min < 5.0 {
        risks.push(json!({
            "risk": "ROE 波动大", "severity": "Medium",
            "detail": format!("5 年 ROE 最低点 {:.1}%", pnum(&get_or(features, "roe_5y_min", json!(0)), 0.0)),
        }));
    }
    let pe = pnum(uzi_core::py::get(features, "pe"), 0.0);
    if pe > 60.0 {
        risks.push(json!({
            "risk": "估值偏高", "severity": "Medium",
            "detail": format!("PE {:.0}x 高于市场", pe),
        }));
    }
    if uzi_core::py::truthy(uzi_core::py::get(features, "fcf_known"))
        && !uzi_core::py::truthy(uzi_core::py::get(features, "fcf_positive"))
    {
        risks.push(json!({
            "risk": "自由现金流为负", "severity": "High",
            "detail": "长期依赖外部融资",
        }));
    }
    let pct_high = pnum(uzi_core::py::get(features, "pct_from_60d_high"), 0.0);
    if pct_high < -20.0 {
        risks.push(json!({
            "risk": "近期动量弱", "severity": "Low",
            "detail": format!("较 60 日高点 {:.0}%", pct_high),
        }));
    }
    // Generic risks
    risks.push(json!({
        "risk": "宏观 / 行业需求下行", "severity": "Medium", "detail": "景气周期风险"
    }));
    Value::Array(risks.into_iter().take(5).collect())
}

/// `_coverage_positioning`.
fn coverage_positioning(research: &Value) -> Value {
    json!({
        "analyst_count": crate::num_value(pnum(uzi_core::py::get(research, "coverage_count"), 0.0)),
        "ratings": get_or(research, "rating_distribution", json!({})),
        "consensus": get_or(research, "rating", json!("—")),
    })
}

// ═══════════════════════════════════════════════════════════════
// 2. EARNINGS ANALYSIS (beat/miss update)
// ═══════════════════════════════════════════════════════════════

/// `build_earnings_analysis` with the upstream default consensus.
pub fn build_earnings_analysis(features: &Value, raw_data: &Value) -> Value {
    build_earnings_analysis_with(features, raw_data, None)
}

/// `build_earnings_analysis` — quarterly post-earnings update.
pub fn build_earnings_analysis_with(
    features: &Value,
    raw_data: &Value,
    consensus: Option<&Value>,
) -> Value {
    let _ = features;
    let fin = dim_data(raw_data, "1_financials");
    let research = dim_data(raw_data, "6_research");

    let rev_hist = get_or(fin, "revenue_history", json!([]));
    let ni_hist = get_or(fin, "net_profit_history", json!([]));
    let rev_arr = rev_hist.as_array().cloned().unwrap_or_default();
    let ni_arr = ni_hist.as_array().cloned().unwrap_or_default();

    if rev_arr.is_empty() || ni_arr.is_empty() {
        return json!({
            "error": "财务历史数据不足",
            "method": "Earnings Analysis",
        });
    }

    let latest_rev = pnum(rev_arr.last().unwrap(), 0.0);
    let latest_ni = pnum(ni_arr.last().unwrap(), 0.0);
    let prev_rev = if rev_arr.len() >= 2 {
        pnum(&rev_arr[rev_arr.len() - 2], 0.0)
    } else {
        latest_rev
    };
    let prev_ni = if ni_arr.len() >= 2 {
        pnum(&ni_arr[ni_arr.len() - 2], 0.0)
    } else {
        latest_ni
    };

    let rev_yoy = if prev_rev > 0.0 {
        crate::num_value(round((latest_rev - prev_rev) / prev_rev * 100.0, 1))
    } else {
        json!(0)
    };
    let ni_yoy = if prev_ni > 0.0 {
        crate::num_value(round((latest_ni - prev_ni) / prev_ni * 100.0, 1))
    } else {
        json!(0)
    };

    // Consensus: pull from research dim if available
    let consensus = match consensus.filter(|c| uzi_core::py::truthy(c)) {
        Some(c) => c.clone(),
        None => json!({
            "rev": crate::num_value(pnum(&get_or(research, "consensus_rev_yi", Value::Null), latest_rev * 0.95)),
            "ni": crate::num_value(pnum(&get_or(research, "consensus_ni_yi", Value::Null), latest_ni * 0.95)),
        }),
    };
    let cons_rev = pnum(uzi_core::py::get(&consensus, "rev"), 0.0);
    let cons_ni = pnum(uzi_core::py::get(&consensus, "ni"), 0.0);

    let rev_vs_cons = if cons_rev > 0.0 {
        crate::num_value(round((latest_rev - cons_rev) / cons_rev * 100.0, 1))
    } else {
        json!(0)
    };
    let ni_vs_cons = if cons_ni > 0.0 {
        crate::num_value(round((latest_ni - cons_ni) / cons_ni * 100.0, 1))
    } else {
        json!(0)
    };
    let rev_vs_f = pnum(&rev_vs_cons, 0.0);
    let ni_vs_f = pnum(&ni_vs_cons, 0.0);

    let rev_tag = tag(rev_vs_f);
    let ni_tag = tag(ni_vs_f);

    // Headline
    let headline = if rev_vs_f > 2.0 && ni_vs_f > 2.0 {
        format!("双超预期：营收 +{:.1}% / 净利 +{:.1}%", rev_vs_f, ni_vs_f)
    } else if rev_vs_f < -2.0 && ni_vs_f < -2.0 {
        format!("双不及：营收 {:.1}% / 净利 {:.1}%", rev_vs_f, ni_vs_f)
    } else {
        format!(
            "分化：营收 {} / 净利 {}",
            strip_tag_prefix(rev_tag),
            strip_tag_prefix(ni_tag)
        )
    };

    let mut latest = Map::new();
    latest.insert("revenue_yi".into(), crate::num_value(latest_rev));
    latest.insert("net_profit_yi".into(), crate::num_value(latest_ni));
    latest.insert("revenue_yoy_pct".into(), rev_yoy.clone());
    latest.insert("net_profit_yoy_pct".into(), ni_yoy.clone());

    let mut beat_miss = Map::new();
    beat_miss.insert("revenue_vs_consensus_pct".into(), rev_vs_cons.clone());
    beat_miss.insert("revenue_tag".into(), Value::String(rev_tag.to_string()));
    beat_miss.insert("net_profit_vs_consensus_pct".into(), ni_vs_cons.clone());
    beat_miss.insert("net_profit_tag".into(), Value::String(ni_tag.to_string()));

    let thesis_impact = if rev_vs_f > 2.0 && ni_vs_f > 2.0 {
        "💪 强化看多"
    } else if rev_vs_f < -2.0 || ni_vs_f < -2.0 {
        "⚠️ 削弱看多"
    } else {
        "⚪ 保持观点"
    };

    json!({
        "method": "Post-Earnings Analysis",
        "headline": headline,
        "latest": Value::Object(latest),
        "consensus": consensus,
        "beat_miss": Value::Object(beat_miss),
        "thesis_impact": thesis_impact,
        "methodology_log": [
            format!("Step 1 · 最新季度营收 {:.1} 亿 vs 共识 {:.1} → {}", latest_rev, cons_rev, rev_tag),
            format!("Step 2 · 最新季度净利 {:.1} 亿 vs 共识 {:.1} → {}", latest_ni, cons_ni, ni_tag),
            format!("Step 3 · 同比：营收 {:+.1}% · 净利 {:+.1}%", pnum(&rev_yoy, 0.0), pnum(&ni_yoy, 0.0)),
            format!("Step 4 · 结论：{}", headline),
        ],
    })
}

/// `_tag` — the beat/miss emoji ladder.
fn tag(pct: f64) -> &'static str {
    if pct >= 5.0 {
        "🟢 大幅超预期"
    } else if pct >= 2.0 {
        "🟢 小幅超预期"
    } else if pct >= -2.0 {
        "⚪ 基本符合"
    } else if pct >= -5.0 {
        "🟠 小幅不及"
    } else {
        "🔴 大幅不及"
    }
}

/// Python `tag[2:]` — drop the emoji + following space.
fn strip_tag_prefix(tag: &str) -> String {
    tag.char_indices()
        .nth(2)
        .map(|(i, _)| tag[i..].to_string())
        .unwrap_or_default()
}

// ═══════════════════════════════════════════════════════════════
// 3. CATALYST CALENDAR
// ═══════════════════════════════════════════════════════════════

/// `build_catalyst_calendar`.
pub fn build_catalyst_calendar(_features: &Value, raw_data: &Value) -> Value {
    let events = dim_data(raw_data, "15_events");
    let now = clock::now();
    let mut catalysts: Vec<Value> = Vec::new();

    // Extract past events from multiple possible formats
    let mut past_sources: Vec<Value> = Vec::new();
    for key in ["event_timeline", "recent_news", "recent_notices"] {
        if let Some(Value::Array(a)) = events.get(key) {
            past_sources.extend(a.iter().cloned());
        }
    }

    let mut seen_titles: Vec<String> = Vec::new();
    for ev in past_sources.iter().take(20) {
        let Some(parsed) = parse_event(ev) else {
            continue;
        };
        let title = py_str_py(uzi_core::py::get(&parsed, "title"));
        if title.is_empty() {
            continue;
        }
        if seen_titles.contains(&title) {
            continue;
        }
        seen_titles.push(title.clone());
        let text = format!(
            "{} {}",
            title,
            py_str_py(uzi_core::py::get(&parsed, "body"))
        );
        catalysts.push(json!({
            "date": uzi_core::py::get(&parsed, "date").clone(),
            "event": title.chars().take(100).collect::<String>(),
            "category": "past",
            "impact": classify_impact(&text),
        }));
        if catalysts.len() >= 10 {
            break;
        }
    }

    // Extract forward-looking catalysts from dim
    if let Some(Value::Array(list)) = events.get("catalyst") {
        for c in list.iter().take(5) {
            match c {
                Value::Object(_) => {
                    catalysts.push(json!({
                        "date": get_or(c, "date", json!("—")),
                        "event": get_or(c, "event", get_or(c, "title", json!("—"))),
                        "category": "forward",
                        "impact": get_or(c, "impact", json!("medium")),
                        "expectation": get_or(c, "expectation", json!("")),
                    }));
                }
                Value::String(s) => {
                    let d = now + Duration::days(30);
                    catalysts.push(json!({
                        "date": clock::date_str(&d),
                        "event": s.chars().take(100).collect::<String>(),
                        "category": "forward",
                        "impact": "medium",
                    }));
                }
                _ => {}
            }
        }
    }

    // Include warnings as risk events
    if let Some(Value::Array(warnings)) = events.get("warnings") {
        for w in warnings.iter().take(3) {
            if let Value::String(s) = w {
                catalysts.push(json!({
                    "date": clock::date_str(&now),
                    "event": format!("⚠️ {}", s.chars().take(100).collect::<String>()),
                    "category": "risk",
                    "impact": "high",
                }));
            }
        }
    }

    // Scheduled future events — standard research calendar
    let q_end = next_quarter_end(&now);
    catalysts.push(json!({
        "date": clock::date_str(&q_end),
        "event": format!(
            "季报披露（预计 {} Q{}）",
            q_end.year(),
            (q_end.month() - 1) / 3 + 1
        ),
        "category": "earnings",
        "impact": "high",
        "expectation": "关注营收/净利超预期与否",
    }));
    catalysts.push(json!({
        "date": clock::date_str(&(now + Duration::days(30))),
        "event": "股东大会 / 投资者关系活动",
        "category": "corporate",
        "impact": "medium",
    }));
    catalysts.push(json!({
        "date": clock::date_str(&(now + Duration::days(60))),
        "event": "行业展会 / 新品发布窗口",
        "category": "industry",
        "impact": "medium",
    }));

    // Macro events
    catalysts.push(json!({
        "date": clock::date_str(&next_fomc(&now)),
        "event": "美联储 FOMC 会议 (参考)",
        "category": "macro",
        "impact": "low",
    }));

    // Sort by date (stable, like Python's list.sort with a key)
    catalysts.sort_by_key(|c| {
        clock::parse_date_or_now(
            uzi_core::py::get(c, "date").as_str().unwrap_or(""),
            &now,
        )
    });

    let past_count = catalysts
        .iter()
        .filter(|c| uzi_core::py::get(c, "category") == &json!("past"))
        .count();
    let forward_count = catalysts
        .iter()
        .filter(|c| uzi_core::py::get(c, "category") == &json!("forward"))
        .count();
    let high_impact = catalysts
        .iter()
        .filter(|c| uzi_core::py::get(c, "impact") == &json!("high"))
        .count();

    let cutoff = now + Duration::days(30);
    let next_30d: Vec<Value> = catalysts
        .iter()
        .filter(|c| {
            clock::parse_date_or_now(
                uzi_core::py::get(c, "date").as_str().unwrap_or(""),
                &now,
            ) <= cutoff
        })
        .cloned()
        .collect();

    json!({
        "method": "Catalyst Calendar",
        "generated_at": clock::date_str(&now),
        "events": catalysts,
        "high_impact_count": high_impact,
        "past_event_count": past_count,
        "forward_event_count": forward_count,
        "next_30d": next_30d,
        "methodology_log": [
            format!("Step 1 · 从 15_events 提取 {} 条历史事件", past_count),
            format!("Step 2 · 预排 {} 个未来节点（季报/股东会/展会/FOMC）", forward_count),
            format!("Step 3 · 共 {} 个节点，其中 {} 个高影响", catalysts.len(), high_impact),
        ],
    })
}

/// `_parse_event` — normalize an event record to `{date, title, body}`.
fn parse_event(ev: &Value) -> Option<Value> {
    match ev {
        Value::Object(_) => Some(json!({
            "date": get_or(ev, "date", get_or(ev, "pub_time", get_or(ev, "time", json!("—")))),
            "title": get_or(ev, "title", get_or(ev, "name", get_or(ev, "headline", json!("")))),
            "body": get_or(ev, "body", get_or(ev, "content", get_or(ev, "summary", json!("")))),
        })),
        Value::String(s) => Some(parse_event_string(s)),
        _ => None,
    }
}

/// The `^\d{4}-\d{2}-\d{2}\s*[·\-:|]?\s*(.+)$` branch for string events.
fn parse_event_string(ev: &str) -> Value {
    let s = ev.trim();
    let bytes = s.as_bytes();
    if bytes.len() >= 10
        && bytes[..10].iter().enumerate().all(|(i, b)| match i {
            4 | 7 => *b == b'-',
            _ => b.is_ascii_digit(),
        })
    {
        let date = &s[..10];
        let mut rest = &s[10..];
        rest = rest.trim_start();
        if let Some(c) = rest.chars().next() {
            if "·-:|".contains(c) {
                rest = &rest[c.len_utf8()..];
            }
        }
        rest = rest.trim_start();
        if !rest.is_empty() {
            return json!({"date": date, "title": rest, "body": ""});
        }
    }
    json!({
        "date": "—",
        "title": ev.chars().take(120).collect::<String>(),
        "body": "",
    })
}

/// `_classify_impact`.
fn classify_impact(text: &str) -> &'static str {
    let high_kws = ["重大", "收购", "并购", "停牌", "中标", "签约", "翻倍", "突破"];
    let med_kws = ["合作", "发布", "公告", "增持", "减持"];
    if high_kws.iter().any(|kw| text.contains(kw)) {
        return "high";
    }
    if med_kws.iter().any(|kw| text.contains(kw)) {
        return "medium";
    }
    "low"
}

/// `_next_quarter_end`.
fn next_quarter_end(dt: &NaiveDateTime) -> NaiveDateTime {
    let q_month = ((dt.month() - 1) / 3 + 1) * 3;
    if q_month == 12 {
        return chrono::NaiveDate::from_ymd_opt(dt.year() + 1, 3, 31)
            .unwrap()
            .and_hms_opt(0, 0, 0)
            .unwrap();
    }
    let m = q_month + 1;
    let day = if matches!(m, 4 | 6 | 9 | 11) { 30 } else { 31 };
    chrono::NaiveDate::from_ymd_opt(dt.year(), m, day)
        .unwrap()
        .and_hms_opt(0, 0, 0)
        .unwrap()
}

/// `_next_fomc` — rough 6-weekly FOMC cadence.
fn next_fomc(dt: &NaiveDateTime) -> NaiveDateTime {
    *dt + Duration::days(42)
}

// ═══════════════════════════════════════════════════════════════
// 4. THESIS TRACKER
// ═══════════════════════════════════════════════════════════════

/// `build_thesis_tracker` with the upstream default `direction="long"`.
pub fn build_thesis_tracker(features: &Value, raw_data: &Value) -> Value {
    build_thesis_tracker_with(features, raw_data, "long")
}

/// `build_thesis_tracker` — running thesis scorecard.
pub fn build_thesis_tracker_with(features: &Value, raw_data: &Value, direction: &str) -> Value {
    let features = sanitize_features(features);
    let features = &features;
    let kline = dim_data(raw_data, "2_kline");

    let rg = pnum(uzi_core::py::get(features, "rev_growth_3y"), 0.0);
    let roe = pnum(uzi_core::py::get(features, "roe_last"), 0.0);
    let pe = pnum(uzi_core::py::get(features, "pe"), 0.0);

    let pillars = json!([
        {
            "pillar": "营收增速 > 15%",
            "original_target": "维持 15%+",
            "current_status": format!("{:.1}%", rg),
            "trend": if rg >= 15.0 { "stable" } else { "concerning" },
            "verdict": if rg >= 15.0 { "✅" } else { "⚠️" },
        },
        {
            "pillar": "ROE > 15%",
            "original_target": "15%+",
            "current_status": format!("{:.1}%", roe),
            "trend": if roe >= 15.0 { "stable" } else { "concerning" },
            "verdict": if roe >= 15.0 { "✅" } else { "⚠️" },
        },
        {
            "pillar": "技术面处于 Stage 2",
            "original_target": "Stage 2 上升",
            "current_status": get_or(kline, "stage", json!("—")),
            "trend": if uzi_core::py::get(features, "stage_num") == &json!(2) { "stable" } else { "watch" },
            "verdict": if uzi_core::py::get(features, "stage_num") == &json!(2) { "✅" } else { "⚠️" },
        },
        {
            "pillar": "估值合理 (PE < 40)",
            "original_target": "< 40",
            "current_status": format!("{:.0}", pe),
            "trend": if pe < 40.0 { "stable" } else { "concerning" },
            "verdict": if pe < 40.0 { "✅" } else { "⚠️" },
        },
        {
            "pillar": "FCF 为正",
            "original_target": "持续正 FCF",
            "current_status": if uzi_core::py::truthy(uzi_core::py::get(features, "fcf_positive")) { "正" } else { "负" },
            "trend": if uzi_core::py::truthy(uzi_core::py::get(features, "fcf_positive")) { "stable" } else { "concerning" },
            "verdict": if uzi_core::py::truthy(uzi_core::py::get(features, "fcf_positive")) { "✅" } else { "⚠️" },
        },
    ]);

    let passed = pillars
        .as_array()
        .unwrap()
        .iter()
        .filter(|p| uzi_core::py::get(p, "verdict") == &json!("✅"))
        .count();
    let total = pillars.as_array().unwrap().len();
    let intact_pct = if total > 0 {
        crate::num_value(round(passed as f64 / total as f64 * 100.0, 0))
    } else {
        json!(0)
    };
    let intact_f = pnum(&intact_pct, 0.0);

    let (conviction, action) = if intact_f >= 80.0 {
        ("High", "Hold / Add")
    } else if intact_f >= 60.0 {
        ("Medium", "Hold")
    } else if intact_f >= 40.0 {
        ("Low", "Trim / Review")
    } else {
        ("Broken", "Exit")
    };

    json!({
        "method": "Thesis Tracker",
        "direction": direction,
        "pillars": pillars,
        "pillars_passed": passed,
        "pillars_total": total,
        "thesis_intact_pct": intact_pct,
        "conviction": conviction,
        "recommended_action": action,
        "methodology_log": [
            format!("Step 1 · 构建 {} 条核心假设支柱", total),
            format!("Step 2 · 当前命中 {}/{}，完好率 {}%", passed, total, py_str_py(&intact_pct)),
            format!("Step 3 · 信念度 {}，建议 {}", conviction, action),
        ],
    })
}

// ═══════════════════════════════════════════════════════════════
// 5. MORNING NOTE
// ═══════════════════════════════════════════════════════════════

/// `build_morning_note`.
pub fn build_morning_note(features: &Value, raw_data: &Value) -> Value {
    let basic = dim_data(raw_data, "0_basic");
    let kline = dim_data(raw_data, "2_kline");
    let lhb = dim_data(raw_data, "16_lhb");
    let sentiment = dim_data(raw_data, "17_sentiment");
    let capital = dim_data(raw_data, "12_capital_flow");

    let name = get_or(basic, "name", json!("—"));
    let price = pnum(uzi_core::py::get(basic, "price"), 0.0);
    let pe = pnum(uzi_core::py::get(basic, "pe_ttm"), 0.0);
    let stage = get_or(kline, "stage", json!("—"));

    let stage2 = uzi_core::py::get(features, "stage_num") == &json!(2);
    let pe_low = pnum(&get_or(features, "pe", json!(100)), 0.0) < 40.0;

    let (top_call, rec) = if stage2 && pe_low {
        (
            format!("{} · Stage 2 上升，PE {:.0} 合理 → 关注", py_str_py(&name), pe),
            "关注仓位建立",
        )
    } else if stage2 {
        (
            format!("{} · 技术面转强 (Stage 2)，但 PE {:.0} 偏高", py_str_py(&name), pe),
            "等待回踩",
        )
    } else {
        (
            format!("{} · 技术面 {}，暂观望", py_str_py(&name), py_str_py(&stage)),
            "无明确信号",
        )
    };

    let bullets = json!([
        format!("【价格】¥{:.2} · PE {:.0}x", price, pe),
        format!("【技术面】{} · 均线 {}", py_str_py(&stage), py_str_py(&get_or(kline, "ma_align", json!("—")))),
        format!(
            "【资金面】龙虎榜 {} 次 · 主力资金 {}",
            py_str_py(&get_or(lhb, "lhb_count_30d", json!(0))),
            py_str_py(&get_or(capital, "main_5d", json!("—")))
        ),
        format!(
            "【舆情】热度 {} · {}",
            py_str_py(&get_or(sentiment, "thermometer_value", json!(0))),
            py_str_py(&get_or(sentiment, "sentiment_label", json!("—")))
        ),
    ]);

    json!({
        "method": "Morning Note",
        "date": clock::date_str(&clock::now()),
        "top_call": top_call,
        "recommendation": rec,
        "bullets": bullets,
        "methodology_log": [
            "Step 1 · 扫描技术面 + 资金面 + 舆情",
            format!("Step 2 · Top Call: {}", top_call),
        ],
    })
}

// ═══════════════════════════════════════════════════════════════
// 6. IDEA SCREEN (quant filters)
// ═══════════════════════════════════════════════════════════════

/// `run_idea_screen` — one of the standard quant screens against this stock.
pub fn run_idea_screen(features: &Value, style: &str) -> Value {
    let features = sanitize_features(features);
    let features = &features;

    let pe = pnum(&get_or(features, "pe", json!(100)), 0.0);
    let pb = pnum(&get_or(features, "pb", json!(100)), 0.0);
    let debt = pnum(&get_or(features, "debt_ratio", json!(100)), 0.0);
    let rev_growth = pnum(uzi_core::py::get(features, "rev_growth_3y"), 0.0);
    let roe = pnum(uzi_core::py::get(features, "roe_last"), 0.0);
    let pe_raw = pnum(uzi_core::py::get(features, "pe"), 0.0);

    let checks: Vec<(&str, bool)> = match style {
        "value" => vec![
            ("PE < 15", pe < 15.0),
            ("PB < 1.5", pb < 1.5),
            (
                "股息率 > 3%",
                pnum(uzi_core::py::get(features, "dividend_yield"), 0.0) > 3.0,
            ),
            (
                "FCF > 0",
                uzi_core::py::truthy(&get_or(features, "fcf_positive", json!(false))),
            ),
            ("资产负债率 < 50%", debt > 0.0 && debt < 50.0),
        ],
        "growth" => vec![
            ("营收增速 > 15%", rev_growth > 15.0),
            (
                "净利增速 > 20%",
                pnum(uzi_core::py::get(features, "eps_growth_3y"), 0.0) > 20.0,
            ),
            (
                "毛利率扩张",
                uzi_core::py::truthy(&get_or(features, "gross_margin_expanding", json!(false))),
            ),
            ("ROE > 15%", roe > 15.0),
        ],
        "quality" => vec![
            (
                "ROE 连续 5 年 > 15%",
                pnum(uzi_core::py::get(features, "roe_5y_above_15"), 0.0) >= 4.0,
            ),
            (
                "净利率 > 15%",
                pnum(uzi_core::py::get(features, "net_margin"), 0.0) > 15.0,
            ),
            (
                "FCF 持续为正",
                uzi_core::py::truthy(&get_or(features, "fcf_positive", json!(false))),
            ),
            ("资产负债率 < 50%", debt > 0.0 && debt < 50.0),
            (
                "护城河 ≥ 28/40",
                pnum(uzi_core::py::get(features, "moat_total"), 0.0) >= 28.0,
            ),
        ],
        "gulp" => vec![
            ("PEG < 1.5", pnum(&get_or(features, "peg", json!(99)), 0.0) < 1.5),
            ("营收增速 > 15%", rev_growth > 15.0),
            ("ROE > 15%", roe > 15.0),
            (
                "Stage 2",
                uzi_core::py::get(features, "stage_num") == &json!(2),
            ),
        ],
        "short" => vec![
            ("PE > 60", pe_raw > 60.0),
            ("营收下滑", rev_growth < 0.0),
            ("ROE < 5%", roe < 5.0),
            ("资产负债率 > 70%", debt > 70.0),
        ],
        other => {
            return json!({"error": format!("unknown style: {}", other)});
        }
    };

    let total = checks.len();
    let passed = checks.iter().filter(|(_, ok)| *ok).count();
    let pct = if total > 0 {
        crate::num_value(round(passed as f64 / total as f64 * 100.0, 0))
    } else {
        json!(0)
    };
    let pct_f = pnum(&pct, 0.0);

    let checks_json: Vec<Value> = checks
        .iter()
        .map(|(c, ok)| json!({"criterion": c, "pass": ok}))
        .collect();

    json!({
        "method": format!("Idea Screen ({})", style),
        "checks": checks_json,
        "passed": passed,
        "total": total,
        "pass_rate_pct": pct,
        "fits_screen": pct_f >= 70.0,
        "verdict": if pct_f >= 70.0 {
            format!("🟢 命中 {} 筛选", style)
        } else {
            format!("🟡 部分命中 ({}/{})", passed, total)
        },
        "methodology_log": [
            format!("Step 1 · {} 筛选 — {} 条标准", style, total),
            format!("Step 2 · 命中 {}/{} ({}%)", passed, total, py_str_py(&pct)),
        ],
    })
}

// ═══════════════════════════════════════════════════════════════
// 7. SECTOR OVERVIEW
// ═══════════════════════════════════════════════════════════════

/// `build_sector_overview`.
pub fn build_sector_overview(features: &Value, raw_data: &Value) -> Value {
    let industry_dim = dim_data(raw_data, "7_industry");
    let peers = dim_data(raw_data, "4_peers");
    let chain = dim_data(raw_data, "5_chain");

    let industry_name = get_or(
        &industry_dim,
        "industry",
        get_or(features, "industry", json!("—")),
    );
    let growth = get_or(&industry_dim, "growth", json!("—"));
    let tam = get_or(&industry_dim, "tam", json!("—"));
    let lifecycle = get_or(&industry_dim, "lifecycle", json!("—"));

    let peer_list = get_or(
        &peers,
        "peer_table",
        get_or(&peers, "peer_comparison", json!([])),
    );
    let peer_count = peer_list.as_array().map(|a| a.len()).unwrap_or(0);

    json!({
        "method": "Sector Overview",
        "industry": industry_name,
        "market_size": {"tam": tam, "growth": growth, "lifecycle": lifecycle},
        "value_chain": {
            "upstream": get_or(&chain, "upstream", json!([])),
            "company": get_or(&chain, "main_business_breakdown", json!([])),
            "downstream": get_or(&chain, "downstream", json!([])),
        },
        "competitive_map": peer_list,
        "peer_count": peer_count,
        "methodology_log": [
            format!("Step 1 · 行业={}，生命周期={}", py_str_py(&industry_name), py_str_py(&lifecycle)),
            format!("Step 2 · TAM {} · 增速 {}", py_str_py(&tam), py_str_py(&growth)),
            format!("Step 3 · 识别同行 {} 家", peer_count),
        ],
    })
}
