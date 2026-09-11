//! Port of `lib/deep_analysis_methods.py` — deep-analysis methods adapted from
//! financial-services-plugins:
//!
//! * private-equity: ic-memo, unit-economics, value-creation-plan, dd-checklist
//! * financial-analysis: competitive-analysis (Porter 5 Forces + BCG)
//! * wealth-management: portfolio-rebalance
//!
//! All pure computation returning structured JSON dicts.

use crate::{dim_data, get_or, pnum, py_str_py};
use serde_json::{json, Map, Value};
use uzi_core::features::sanitize_features;
use uzi_core::py::round;

fn last_n(v: &Value, n: usize) -> Value {
    match v.as_array() {
        Some(a) => Value::Array(a[a.len().saturating_sub(n)..].to_vec()),
        None => Value::Array(Vec::new()),
    }
}

/// `build_ic_memo` — structured IC memo for a formal investment decision.
pub fn build_ic_memo(
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

    let name = get_or(basic, "name", json!("—"));
    let price = pnum(uzi_core::py::get(basic, "price"), 0.0);

    // I. Executive Summary
    let (rec_headline, recommendation) = ic_recommendation(features, dcf_result);

    // II. Company Overview
    let mut company = Map::new();
    company.insert("name".into(), name.clone());
    company.insert(
        "industry".into(),
        get_or(basic, "industry", json!("—")),
    );
    let chain_data = dim_data(raw_data, "5_chain");
    company.insert(
        "business_model".into(),
        get_or(chain_data, "main_business_raw", json!("—")),
    );
    company.insert(
        "market_cap_yi".into(),
        crate::num_value(pnum(uzi_core::py::get(features, "market_cap_yi"), 0.0)),
    );
    company.insert(
        "revenue_last_yi".into(),
        crate::num_value(pnum(uzi_core::py::get(features, "revenue_latest_yi"), 0.0)),
    );

    // III. Industry & Market
    let industry7 = dim_data(raw_data, "7_industry");
    let mut industry_info = Map::new();
    industry_info.insert("industry_name".into(), get_or(basic, "industry", json!("—")));
    industry_info.insert("tam".into(), get_or(industry7, "tam", json!("—")));
    industry_info.insert("growth".into(), get_or(industry7, "growth", json!("—")));
    industry_info.insert(
        "lifecycle".into(),
        get_or(industry7, "lifecycle", json!("—")),
    );

    // IV. Financial Analysis
    let mut financial_snapshot = Map::new();
    financial_snapshot.insert(
        "roe_5yr".into(),
        last_n(&get_or(fin, "roe_history", json!([])), 5),
    );
    financial_snapshot.insert(
        "revenue_hist_yi".into(),
        last_n(&get_or(fin, "revenue_history", json!([])), 5),
    );
    financial_snapshot.insert(
        "net_profit_hist_yi".into(),
        last_n(&get_or(fin, "net_profit_history", json!([])), 5),
    );
    financial_snapshot.insert(
        "net_margin".into(),
        get_or(features, "net_margin", json!(0)),
    );
    financial_snapshot.insert(
        "debt_ratio".into(),
        get_or(features, "debt_ratio", json!(0)),
    );
    financial_snapshot.insert(
        "fcf_positive".into(),
        get_or(features, "fcf_positive", json!(false)),
    );

    // V. Valuation
    let mut valuation = Map::new();
    if let Some(dcf) = dcf_result.filter(|d| uzi_core::py::truthy(d)) {
        let mut entry = Map::new();
        entry.insert(
            "intrinsic_per_share".into(),
            get_or(dcf, "intrinsic_per_share", json!(0)),
        );
        entry.insert(
            "safety_margin_pct".into(),
            get_or(dcf, "safety_margin_pct", json!(0)),
        );
        entry.insert("verdict".into(), get_or(dcf, "verdict", json!("")));
        valuation.insert("dcf".into(), Value::Object(entry));
    }
    if let Some(comps) = comps_result.filter(|d| uzi_core::py::truthy(d)) {
        let mut entry = Map::new();
        entry.insert(
            "target_percentile".into(),
            get_or(comps, "target_percentile", json!({})),
        );
        entry.insert(
            "implied_price".into(),
            get_or(comps, "implied_price", json!({})),
        );
        entry.insert(
            "verdict".into(),
            get_or(comps, "valuation_verdict", json!("")),
        );
        valuation.insert("comps".into(), Value::Object(entry));
    }

    // VI. Key Risks & Mitigants
    let risks = ic_risks(features, moat);

    // VII. Returns Analysis (3 scenarios)
    let scenarios = ic_scenarios(price, dcf_result);

    // VIII. Top 3 risks + mitigants
    let top_3_risks = Value::Array(
        risks
            .as_array()
            .map(|a| a.iter().take(3).cloned().collect())
            .unwrap_or_default(),
    );

    let mut exec_summary = Map::new();
    exec_summary.insert("headline".into(), Value::String(rec_headline));
    exec_summary.insert("recommendation".into(), Value::String(recommendation.clone()));
    exec_summary.insert("top_3_risks".into(), top_3_risks);

    let mut sections = Map::new();
    sections.insert("I_exec_summary".into(), Value::Object(exec_summary));
    sections.insert("II_company_overview".into(), Value::Object(company));
    sections.insert("III_industry_market".into(), Value::Object(industry_info));
    sections.insert("IV_financial_analysis".into(), Value::Object(financial_snapshot));
    sections.insert("V_valuation".into(), Value::Object(valuation));
    sections.insert("VI_risks_mitigants".into(), risks);
    sections.insert("VII_returns_scenarios".into(), scenarios);
    sections.insert(
        "VIII_recommendation".into(),
        Value::String(recommendation.clone()),
    );

    let mut out = Map::new();
    out.insert(
        "method".into(),
        Value::String("Investment Committee Memo".into()),
    );
    out.insert("sections".into(), Value::Object(sections));
    out.insert(
        "methodology_log".into(),
        json!([
            "Step 1 · 汇总公司/行业/财务快照",
            "Step 2 · 结合 DCF/Comps 形成估值结论",
            "Step 3 · 构建三情景回报",
            format!("Step 4 · 出具建议: {}", recommendation),
        ]),
    );
    Value::Object(out)
}

/// `_ic_recommendation` — simple recommendation logic based on quality + valuation.
fn ic_recommendation(features: &Value, dcf: Option<&Value>) -> (String, String) {
    let mut quality_score = 0;
    if pnum(uzi_core::py::get(features, "roe_5y_above_15"), 0.0) >= 3.0 {
        quality_score += 2;
    }
    if uzi_core::py::truthy(uzi_core::py::get(features, "fcf_positive")) {
        quality_score += 1;
    }
    if pnum(uzi_core::py::get(features, "net_margin"), 0.0) > 15.0 {
        quality_score += 1;
    }
    if uzi_core::py::truthy(uzi_core::py::get(features, "moat_known"))
        && pnum(uzi_core::py::get(features, "moat_total"), 0.0) >= 28.0
    {
        quality_score += 2;
    }

    let mut val_score = 0;
    if let Some(dcf) = dcf {
        let sm = pnum(uzi_core::py::get(dcf, "safety_margin_pct"), 0.0);
        val_score = if sm > 20.0 {
            2
        } else if sm > 0.0 {
            1
        } else if sm > -20.0 {
            0
        } else {
            -1
        };
    }

    let total = quality_score + val_score;
    if total >= 5 {
        (
            "🟢 强烈建议通过 (PASS)".to_string(),
            "推荐投委会批准建仓 — 高质量 × 安全边际充足".to_string(),
        )
    } else if total >= 3 {
        (
            "🟡 建议通过 (CONDITIONAL PASS)".to_string(),
            "可批准但建议分批建仓，控制初始仓位".to_string(),
        )
    } else if total >= 0 {
        (
            "⚪ 观望 (HOLD)".to_string(),
            "暂不建议建仓，等待估值回落或信号强化".to_string(),
        )
    } else {
        (
            "🔴 建议回避 (PASS)".to_string(),
            "质量或估值不达标 — 投委会建议不进场".to_string(),
        )
    }
}

/// `_ic_risks`.
fn ic_risks(features: &Value, moat: &Value) -> Value {
    let _ = moat;
    let mut risks: Vec<Value> = Vec::new();
    let debt_ratio = pnum(uzi_core::py::get(features, "debt_ratio"), 0.0);
    if debt_ratio > 60.0 {
        risks.push(json!({
            "risk": "财务杠杆风险",
            "detail": format!("资产负债率 {:.0}% 偏高", debt_ratio),
            "severity": "High",
            "mitigant": "监控利息覆盖倍数与再融资窗口",
        }));
    }
    if uzi_core::py::truthy(uzi_core::py::get(features, "moat_known"))
        && pnum(uzi_core::py::get(features, "moat_total"), 0.0) < 20.0
    {
        risks.push(json!({
            "risk": "护城河偏弱",
            "detail": format!("4 项合计 {:.0}/40", pnum(uzi_core::py::get(features, "moat_total"), 0.0)),
            "severity": "Medium",
            "mitigant": "密切跟踪市场份额变化",
        }));
    }
    let pe = pnum(uzi_core::py::get(features, "pe"), 0.0);
    if pe > 60.0 {
        risks.push(json!({
            "risk": "估值偏贵",
            "detail": format!("PE {:.0}x", pe),
            "severity": "Medium",
            "mitigant": "等待 PE 回归 40 以下再建仓",
        }));
    }
    if uzi_core::py::truthy(uzi_core::py::get(features, "fcf_known"))
        && !uzi_core::py::truthy(uzi_core::py::get(features, "fcf_positive"))
    {
        risks.push(json!({
            "risk": "现金流为负",
            "detail": "依赖外部融资",
            "severity": "High",
            "mitigant": "要求管理层提供扭转路线图",
        }));
    }
    // v3.9.4 · 行业周期下行只在生命周期判定为衰退时才算风险。
    if uzi_core::py::truthy(uzi_core::py::get(features, "industry_in_decline")) {
        risks.push(json!({
            "risk": "行业周期下行",
            "detail": "需求侧宏观冲击",
            "severity": "Medium",
            "mitigant": "行业景气度月度跟踪",
        }));
    }
    Value::Array(risks)
}

/// `_ic_scenarios`.
fn ic_scenarios(price: f64, dcf: Option<&Value>) -> Value {
    let Some(dcf) = dcf.filter(|d| uzi_core::py::truthy(d)) else {
        return Value::Array(Vec::new());
    };
    if price <= 0.0 {
        return Value::Array(Vec::new());
    }
    let intrinsic = pnum(uzi_core::py::get(dcf, "intrinsic_per_share"), 0.0);
    if intrinsic <= 0.0 {
        return Value::Array(Vec::new());
    }
    json!([
        {
            "scenario": "Bull (乐观)",
            "price_target": crate::num_value(round(intrinsic * 1.3, 2)),
            "return_pct": crate::num_value(round((intrinsic * 1.3 - price) / price * 100.0, 1)),
            "probability_pct": 25,
            "assumptions": "超预期增速 + 估值扩张",
        },
        {
            "scenario": "Base (中性)",
            "price_target": crate::num_value(round(intrinsic, 2)),
            "return_pct": crate::num_value(round((intrinsic - price) / price * 100.0, 1)),
            "probability_pct": 50,
            "assumptions": "DCF 基础假设",
        },
        {
            "scenario": "Bear (悲观)",
            "price_target": crate::num_value(round(intrinsic * 0.7, 2)),
            "return_pct": crate::num_value(round((intrinsic * 0.7 - price) / price * 100.0, 1)),
            "probability_pct": 25,
            "assumptions": "增速放缓 + 估值压缩",
        },
    ])
}

/// `build_unit_economics` — ARR / LTV / CAC for recurring businesses, otherwise a
/// gross-margin decomposition.
pub fn build_unit_economics(features: &Value, raw_data: &Value) -> Value {
    let industry = get_or(
        dim_data(raw_data, "0_basic"),
        "industry",
        json!(""),
    );
    let industry_str = industry.as_str().unwrap_or("");

    let is_recurring = ["软件", "服务", "云", "互联网", "SaaS"]
        .iter()
        .any(|kw| industry_str.contains(kw));

    if is_recurring {
        // SaaS-style cohort metrics
        let arpu = pnum(uzi_core::py::get(features, "revenue_latest_yi"), 0.0)
            / pnum(uzi_core::py::get(features, "customer_count"), 1.0).max(1.0);
        let gross_margin = pnum(&get_or(features, "gross_margin", json!(50)), 0.0) / 100.0;
        let churn_rate = 0.15;
        let ltv = if churn_rate > 0.0 {
            (arpu * gross_margin) / churn_rate
        } else {
            0.0
        };
        let cac = arpu * 0.5;
        let ltv_cac = if cac > 0.0 { ltv / cac } else { 0.0 };
        let payback_months = if arpu > 0.0 {
            cac / (arpu * gross_margin / 12.0)
        } else {
            0.0
        };

        let mut metrics = Map::new();
        metrics.insert("arpu_yi".into(), crate::num_value(round(arpu, 3)));
        metrics.insert(
            "gross_margin_pct".into(),
            crate::num_value(round(gross_margin * 100.0, 1)),
        );
        metrics.insert(
            "churn_rate_pct".into(),
            crate::num_value(round(churn_rate * 100.0, 1)),
        );
        metrics.insert("ltv_yi".into(), crate::num_value(round(ltv, 3)));
        metrics.insert("cac_yi".into(), crate::num_value(round(cac, 3)));
        metrics.insert("ltv_cac_ratio".into(), crate::num_value(round(ltv_cac, 2)));
        metrics.insert(
            "payback_months".into(),
            crate::num_value(round(payback_months, 1)),
        );

        return json!({
            "method": "Unit Economics (SaaS/recurring)",
            "business_type": "recurring",
            "metrics": Value::Object(metrics),
            "healthy": ltv_cac >= 3.0 && payback_months <= 24.0,
            "verdict": if ltv_cac >= 3.0 { "🟢 健康" } else { "🔴 不健康" },
            "methodology_log": [
                format!("Step 1 · ARPU {:.3} 亿 · 毛利率 {:.0}%", arpu, gross_margin * 100.0),
                format!("Step 2 · LTV {:.2} / CAC {:.2} = {:.1}x", ltv, cac, ltv_cac),
                format!("Step 3 · 回本周期 {:.0} 个月", payback_months),
            ],
        });
    }

    // Non-recurring: gross margin decomposition
    let rev = pnum(uzi_core::py::get(features, "revenue_latest_yi"), 0.0);
    let gm_pct = pnum(&get_or(features, "gross_margin", json!(30)), 0.0);
    let nm_pct = pnum(&get_or(features, "net_margin", json!(10)), 0.0);
    let opex_pct = gm_pct - nm_pct;

    json!({
        "method": "Margin Decomposition",
        "business_type": "non-recurring",
        "revenue_yi": crate::num_value(rev),
        "gross_margin_pct": crate::num_value(gm_pct),
        "opex_pct_of_rev": crate::num_value(round(opex_pct, 1)),
        "net_margin_pct": crate::num_value(nm_pct),
        "waterfall": [
            {"stage": "收入", "value": 100, "label": "100%"},
            {"stage": "毛利", "value": crate::num_value(gm_pct), "label": format!("{:.0}%", gm_pct)},
            {"stage": "税前", "value": crate::num_value(nm_pct / 0.75), "label": format!("{:.0}%", nm_pct / 0.75)},
            {"stage": "净利", "value": crate::num_value(nm_pct), "label": format!("{:.0}%", nm_pct)},
        ],
        "methodology_log": [
            format!("Step 1 · 营收 {:.1} 亿", rev),
            format!(
                "Step 2 · 毛利率 {:.0}% · 运营费率 {:.0}% · 净利率 {:.0}%",
                gm_pct, opex_pct, nm_pct
            ),
        ],
    })
}

/// `build_value_creation_plan` — post-investment value-creation roadmap, 5 years.
pub fn build_value_creation_plan(features: &Value, raw_data: &Value) -> Value {
    let features = sanitize_features(features);
    let features = &features;
    let _ = raw_data;

    let rev = pnum(uzi_core::py::get(features, "revenue_latest_yi"), 0.0);
    let ebitda_est = if rev > 0.0 { rev * 0.20 } else { 1.0 };
    let current_ebitda = pnum(uzi_core::py::get(features, "ebitda_yi"), ebitda_est);
    let margin_taken = rev > 0.0;
    let current_ebitda_margin = if margin_taken {
        current_ebitda / rev * 100.0
    } else {
        0.0
    };

    let market_share = get_or(features, "market_share", json!("—"));
    let gross_margin = pnum(&get_or(features, "gross_margin", json!(30)), 0.0);

    let levers = json!([
        {
            "category": "Revenue · Organic Growth",
            "lever": "现有市场渗透率提升",
            "current_state": format!("市场份额 ~{}%", py_str_py(&market_share)),
            "target_state": "5 年内提升 3pp",
            "ebitda_impact_yi": crate::num_value(round(rev * 0.03 * 0.25, 2)),
            "timeline": "Y1-Y5",
            "confidence": "Medium",
        },
        {
            "category": "Revenue · Cross-Sell",
            "lever": "新产品交叉销售",
            "current_state": "核心产品",
            "target_state": "5 年新增 20% 营收占比",
            "ebitda_impact_yi": crate::num_value(round(rev * 0.20 * 0.20, 2)),
            "timeline": "Y2-Y5",
            "confidence": "Medium",
        },
        {
            "category": "Margin · Pricing Power",
            "lever": "定价优化",
            "current_state": format!("毛利率 {:.0}%", gross_margin),
            "target_state": "+300bps",
            "ebitda_impact_yi": crate::num_value(round(rev * 0.03, 2)),
            "timeline": "Y1-Y3",
            "confidence": "High",
        },
        {
            "category": "Margin · COGS",
            "lever": "采购集中 + 供应链优化",
            "current_state": "多点采购",
            "target_state": "−200bps COGS",
            "ebitda_impact_yi": crate::num_value(round(rev * 0.02, 2)),
            "timeline": "Y1-Y2",
            "confidence": "High",
        },
        {
            "category": "Capital Efficiency",
            "lever": "营运资本优化",
            "current_state": "存货周转 —",
            "target_state": "存货周转 +20%",
            "ebitda_impact_yi": crate::num_value(round(rev * 0.01, 2)),
            "timeline": "Y1-Y3",
            "confidence": "Medium",
        },
    ]);

    let total_uplift: f64 = levers
        .as_array()
        .unwrap()
        .iter()
        .map(|l| pnum(uzi_core::py::get(l, "ebitda_impact_yi"), 0.0))
        .sum();
    let target_ebitda = current_ebitda + total_uplift;
    let target_margin_pct = if rev > 0.0 {
        crate::num_value(round(target_ebitda / rev * 100.0, 1))
    } else {
        json!(0)
    };

    json!({
        "method": "Value Creation Plan (EBITDA Bridge)",
        "current_ebitda_yi": crate::num_value(round(current_ebitda, 2)),
        "current_margin_pct": if margin_taken {
            crate::num_value(round(current_ebitda_margin, 1))
        } else {
            json!(0)
        },
        "levers": levers,
        "total_uplift_yi": crate::num_value(round(total_uplift, 2)),
        "target_ebitda_yi": crate::num_value(round(target_ebitda, 2)),
        "target_margin_pct": target_margin_pct,
        "hundred_day_priorities": [
            "Day 30 · 财务 QoE 验证",
            "Day 60 · 新管理层招募",
            "Day 90 · 季度 KPI 仪表盘上线",
        ],
        "methodology_log": [
            format!(
                "Step 1 · 现 EBITDA {:.1} 亿 ({:.0}% 利润率)",
                current_ebitda, current_ebitda_margin
            ),
            format!("Step 2 · 5 大杠杆合计加厚 {:.1} 亿", total_uplift),
            format!(
                "Step 3 · 目标 EBITDA {:.1} 亿 ({:.0}%)",
                target_ebitda,
                if rev > 0.0 { target_ebitda / rev * 100.0 } else { 0.0 }
            ),
        ],
    })
}

/// `build_dd_checklist`.
pub fn build_dd_checklist(features: &Value, raw_data: &Value) -> Value {
    let check = |has: bool| -> &'static str {
        if has {
            "✅ 已有数据"
        } else {
            "❌ 缺失"
        }
    };
    let fin = dim_data(raw_data, "1_financials");
    let ind = dim_data(raw_data, "7_industry");
    let peers = dim_data(raw_data, "4_peers");
    let chain = dim_data(raw_data, "5_chain");
    let gov = dim_data(raw_data, "11_governance");
    let policy = dim_data(raw_data, "13_policy");
    let sent = dim_data(raw_data, "17_sentiment");
    let events = dim_data(raw_data, "15_events");
    let trap = dim_data(raw_data, "18_trap");

    let workstreams = json!([
        {
            "workstream": "财务尽调 (Financial DD)",
            "items": [
                {"item": "5 年营收 / 净利历史", "status": check(uzi_core::py::truthy(&get_or(fin, "revenue_history", json!([]))))},
                {"item": "ROE / 毛利 / 净利率", "status": check(uzi_core::py::truthy(&get_or(features, "roe_last", json!(0))))},
                {"item": "资产负债率", "status": check(uzi_core::py::truthy(&get_or(features, "debt_ratio", json!(0))))},
                {"item": "自由现金流", "status": check(uzi_core::py::truthy(&get_or(features, "fcf_known", json!(false))))},
                {"item": "审计意见 / 会计政策", "status": "⚪ 需人工核查"},
            ],
        },
        {
            "workstream": "商业尽调 (Commercial DD)",
            "items": [
                {"item": "市场规模 (TAM)", "status": check(uzi_core::py::truthy(&get_or(ind, "tam", json!(null))))},
                {"item": "竞争格局", "status": check(uzi_core::py::truthy(&get_or(peers, "peer_table", json!(null))))},
                {"item": "客户集中度", "status": "⚪ 需年报披露"},
                {"item": "上下游分析", "status": check(uzi_core::py::truthy(&get_or(chain, "upstream", json!(null))))},
            ],
        },
        {
            "workstream": "法律尽调 (Legal DD)",
            "items": [
                {"item": "股权结构", "status": check(uzi_core::py::truthy(&get_or(gov, "pledge", json!(null))))},
                {"item": "重大诉讼", "status": "⚪ 需披露核查"},
                {"item": "关联交易", "status": "⚪ 需年报披露"},
                {"item": "股权质押 / 内部交易", "status": check(uzi_core::py::truthy(&get_or(gov, "insider_trades_1y", json!(null))))},
            ],
        },
        {
            "workstream": "运营尽调 (Operational DD)",
            "items": [
                {"item": "护城河评估", "status": check(uzi_core::py::truthy(&get_or(features, "moat_known", json!(false))))},
                {"item": "研发投入", "status": "⚪ 需年报披露"},
                {"item": "管理层背景", "status": "⚪ 需人工核查"},
                {"item": "ESG 评级", "status": "⚪ 需第三方数据"},
            ],
        },
        {
            "workstream": "市场尽调 (Market DD)",
            "items": [
                {"item": "政策环境", "status": check(uzi_core::py::truthy(&get_or(policy, "snippets", json!(null))))},
                {"item": "舆情扫描", "status": check(uzi_core::py::truthy(&get_or(sent, "thermometer_value", json!(null))))},
                {"item": "事件驱动监控", "status": check(uzi_core::py::truthy(&get_or(events, "recent_news", json!(null))))},
                {"item": "杀猪盘排查", "status": check(uzi_core::py::truthy(&get_or(trap, "trap_level", json!(null))))},
            ],
        },
    ]);

    let total_items: usize = workstreams
        .as_array()
        .unwrap()
        .iter()
        .map(|ws| {
            uzi_core::py::get(ws, "items")
                .as_array()
                .map(|a| a.len())
                .unwrap_or(0)
        })
        .sum();
    let done = workstreams
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|ws| {
            uzi_core::py::get(ws, "items")
                .as_array()
                .cloned()
                .unwrap_or_default()
        })
        .filter(|it| {
            uzi_core::py::get(it, "status")
                .as_str()
                .map(|s| s.contains('✅'))
                .unwrap_or(false)
        })
        .count();
    let pct = if total_items > 0 {
        crate::num_value(round(done as f64 / total_items as f64 * 100.0, 0))
    } else {
        json!(0)
    };

    json!({
        "method": "Due Diligence Checklist",
        "workstreams": workstreams,
        "total_items": total_items,
        "items_auto_verified": done,
        "completion_pct": pct,
        "manual_review_required": total_items - done,
        "methodology_log": [
            format!("Step 1 · 生成 5 大工作流 {} 条清单", total_items),
            format!("Step 2 · 自动命中 {} 条 ({}%)", done, py_str_py(&pct)),
            format!("Step 3 · 剩余 {} 条需人工复核", total_items - done),
        ],
    })
}

/// `build_competitive_analysis` — Porter 5 Forces + BCG position.
pub fn build_competitive_analysis(features: &Value, raw_data: &Value) -> Value {
    let moat = dim_data(raw_data, "14_moat");
    let moat_scores = if moat.is_object() {
        get_or(moat, "scores", json!({}))
    } else {
        json!({})
    };

    // Porter 5 Forces — 1-5 scale (1 = low threat / 5 = high threat)
    let barriers = pnum(&get_or(&moat_scores, "intangible", json!(5)), 0.0);
    let switching = pnum(&get_or(&moat_scores, "switching", json!(5)), 0.0);
    let scale = pnum(&get_or(&moat_scores, "scale", json!(5)), 0.0);

    let new_entrants_threat = (6 - (barriers / 2.0) as i64).max(1);
    let substitutes_threat = (6 - (switching / 2.0) as i64).max(1);
    let supplier_power = 3i64;
    let buyer_power = 3i64;
    let rivalry = (6 - (scale / 2.0) as i64).max(1);

    let total_threat = new_entrants_threat
        + substitutes_threat
        + supplier_power
        + buyer_power
        + rivalry;
    let attractiveness = crate::num_value(round(
        (25 - total_threat) as f64 / 20.0 * 100.0,
        0,
    ));

    // v2.12.1 · BCG matrix positioning.
    let market_share = pnum(&get_or(features, "market_share", json!(0)), 0.0);
    let market_growth = pnum(&get_or(features, "industry_growth", json!(0)), 0.0);

    let (bcg, bcg_action) = if market_share > 3.0 && market_growth > 15.0 {
        ("Star (明星)", "继续投入，抢占市场")
    } else if market_share > 3.0 {
        ("Cash Cow (现金牛)", "维持运营，最大化现金回收")
    } else if market_growth > 15.0 {
        ("Question Mark (问号)", "选择性投入 / 或退出")
    } else {
        ("Dog (瘦狗)", "考虑剥离 / 收缩")
    };

    json!({
        "method": "Competitive Analysis (Porter + BCG)",
        "porter_five_forces": {
            "new_entrants_threat": {"score": new_entrants_threat, "rationale": format!("进入壁垒分 {:.0}/10 (无形资产)", barriers)},
            "substitutes_threat": {"score": substitutes_threat, "rationale": format!("转换成本分 {:.0}/10", switching)},
            "supplier_power": {"score": supplier_power, "rationale": "中性（未细分）"},
            "buyer_power": {"score": buyer_power, "rationale": "中性（未细分）"},
            "rivalry_intensity": {"score": rivalry, "rationale": format!("规模优势分 {:.0}/10", scale)},
        },
        "industry_attractiveness_pct": attractiveness,
        "bcg_position": {
            "category": bcg,
            "market_share_pct": crate::num_value(market_share),
            "market_growth_pct": crate::num_value(market_growth),
            "strategic_action": bcg_action,
        },
        "methodology_log": [
            format!("Step 1 · Porter 5 力合计威胁分 {}/25，行业吸引力 {}%", total_threat, py_str_py(&attractiveness)),
            format!("Step 2 · BCG 定位 {} — {}", bcg, bcg_action),
        ],
    })
}

/// `build_portfolio_rebalance` — retail portfolio drift analyzer.
pub fn build_portfolio_rebalance(positions: &[Value], target_allocation: Option<&Value>) -> Value {
    if positions.is_empty() {
        return json!({"error": "no positions provided"});
    }

    let default_target = json!({
        "A股蓝筹": 30, "A股成长": 25, "港股": 15,
        "美股": 10, "债券/货币": 15, "现金": 5,
    });
    let target_allocation = target_allocation
        .filter(|t| uzi_core::py::truthy(t))
        .unwrap_or(&default_target);
    let target_map = match target_allocation.as_object() {
        Some(m) => m.clone(),
        None => return json!({"error": "portfolio total is 0"}),
    };

    let total: f64 = positions
        .iter()
        .map(|p| pnum(uzi_core::py::get(p, "market_value_yuan"), 0.0))
        .sum();
    if total <= 0.0 {
        return json!({"error": "portfolio total is 0"});
    }

    // Current by asset class
    let mut by_class: Map<String, Value> = Map::new();
    for p in positions {
        let cls = get_or(p, "asset_class", json!("A股蓝筹"));
        let key = py_str_py(&cls);
        let prev = by_class
            .get(&key)
            .map(|v| pnum(v, 0.0))
            .unwrap_or(0.0);
        by_class.insert(
            key,
            crate::num_value(prev + pnum(uzi_core::py::get(p, "market_value_yuan"), 0.0)),
        );
    }

    let mut drift_rows: Vec<Value> = Vec::new();
    for (cls, target_pct_v) in target_map.iter() {
        let target_pct = pnum(target_pct_v, 0.0);
        let cur_value = by_class
            .get(cls)
            .map(|v| pnum(v, 0.0))
            .unwrap_or(0.0);
        let cur_pct = cur_value / total * 100.0;
        let drift = cur_pct - target_pct;
        let target_value = total * target_pct / 100.0;
        let dollar_drift = cur_value - target_value;
        drift_rows.push(json!({
            "asset_class": cls,
            "target_pct": target_pct_v,
            "current_pct": crate::num_value(round(cur_pct, 1)),
            "drift_pct": crate::num_value(round(drift, 1)),
            "dollar_drift_yuan": crate::num_value(round(dollar_drift, 0)),
            "action": if drift > 5.0 { "减持" } else if drift < -5.0 { "买入" } else { "维持" },
        }));
    }

    let needs_rebalance = drift_rows.iter().any(|r| {
        pnum(uzi_core::py::get(r, "drift_pct"), 0.0).abs() > 5.0
    });
    let rebalance_trades: Vec<Value> = drift_rows
        .iter()
        .filter(|r| pnum(uzi_core::py::get(r, "drift_pct"), 0.0).abs() > 5.0)
        .cloned()
        .collect();

    json!({
        "method": "Portfolio Rebalance Analysis",
        "portfolio_total_yuan": crate::num_value(round(total, 0)),
        "drift_rows": drift_rows,
        "needs_rebalance": needs_rebalance,
        "rebalance_trades": rebalance_trades,
        "methodology_log": [
            format!("Step 1 · 组合总值 ¥{}", crate::py_thousands0(total)),
            format!("Step 2 · {} 个资产类别漂移检查", target_map.len()),
            format!("Step 3 · 需再平衡: {}", if needs_rebalance { "True" } else { "False" }),
        ],
    })
}
