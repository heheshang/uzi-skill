//! Port of `lib/investor_criteria.py` — the quantified rule table for all 66
//! investors.
//!
//! Rule metadata (`rule_id` / `name` / `weight` / `pass_msg` / `fail_msg`) is
//! dumped verbatim from the upstream module into `src/data/criteria_meta.json`;
//! the `check` callables are hand-ported here. Every check mirrors the Python
//! lambda exactly, including the "missing vs `None`" distinction that decides
//! whether the evaluator skips a rule (`Ok(Err(PyErr))` semantics): a numeric
//! comparison against a present `None` raises, while equality and truthiness do
//! not.

use crate::pyhelp::{self as h, R};
use crate::pyhelp::PyErr;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::LazyLock;

/// One rule's check, ported from the upstream lambda.
pub type Check = fn(&Value) -> R<bool>;

/// A fully-resolved rule (metadata + behaviour).
pub struct Rule {
    pub rule_id: String,
    pub name: String,
    pub weight: i64,
    pub check: Check,
    pub pass_msg: String,
    pub fail_msg: String,
}

const META_JSON: &str = include_str!("data/criteria_meta.json");

fn meta() -> &'static Value {
    static META: LazyLock<Value> =
        LazyLock::new(|| serde_json::from_str(META_JSON).expect("embedded criteria metadata json"));
    &META
}

fn contains_any(hay: &str, kws: &[&str]) -> bool {
    kws.iter().any(|k| hay.contains(k))
}

/// `any(k in (f.get(a,"") + f.get(b,"")).lower() for k in KWS)`
fn any_lower_2(f: &Value, a: &str, b: &str, kws: &[&str]) -> R<bool> {
    let s = h::concat2(f, a, "", b, "")?.to_lowercase();
    Ok(contains_any(&s, kws))
}

/// `any(k in f.get(a,"") for k in KWS)`
fn any_in_1(f: &Value, a: &str, kws: &[&str]) -> R<bool> {
    let s = h::text(f, a, "")?;
    Ok(contains_any(&s, kws))
}

/// `not any(k in f.get(a,"") for k in KWS)`
fn none_in_1(f: &Value, a: &str, kws: &[&str]) -> R<bool> {
    Ok(!any_in_1(f, a, kws)?)
}

// ────────────────────────────────────────────────────────────────
// A 组 · 经典价值派
// ────────────────────────────────────────────────────────────────

fn checks_buffett() -> Vec<Check> {
    vec![
        // roe_5y_15
        |f| Ok(h::num(f, "roe_5y_above_15", 0.0)? >= 4.0 && h::num(f, "roe_5y_min", 0.0)? > 12.0),
        // net_margin_15
        |f| Ok(h::num(f, "net_margin", 0.0)? > 15.0),
        // debt_ratio_50
        |f| {
            let x = h::num(f, "debt_ratio", 100.0)?;
            Ok(0.0 < x && x < 50.0)
        },
        // fcf_positive
        |f| h::known_fcf(f),
        // moat_clear
        |f| Ok(h::num(f, "moat_total", 0.0)? >= 24.0),
        // safety_margin_pe
        |f| Ok(h::num(f, "pe_quantile_5y", 100.0)? < 50.0),
        // dividend_history
        |f| Ok(h::num(f, "consecutive_dividend_years", 0.0)? >= 5.0),
    ]
}

fn checks_graham() -> Vec<Check> {
    vec![
        // pe_under_15
        |f| {
            let x = h::num(f, "pe", 100.0)?;
            Ok(0.0 < x && x < 15.0)
        },
        // pb_under_1_5
        |f| {
            let x = h::num(f, "pb", 100.0)?;
            Ok(0.0 < x && x < 1.5)
        },
        // pe_pb_22_5
        |f| {
            let x = h::num(f, "pe_x_pb", 100.0)?;
            Ok(0.0 < x && x < 22.5)
        },
        // current_ratio_2
        |f| Ok(h::num(f, "current_ratio", 0.0)? > 2.0),
        // profit_10y (relaxed from 10 to 5 upstream)
        |f| Ok(h::num(f, "consecutive_profit_years", 0.0)? >= 5.0),
        // dividend_history
        |f| Ok(h::num(f, "consecutive_dividend_years", 0.0)? >= 5.0),
    ]
}

fn checks_fisher() -> Vec<Check> {
    vec![
        // industry_growing
        |f| Ok(h::truth(f, "industry_is_growing", false)),
        // profitability
        |f| Ok(h::num(f, "net_margin", 0.0)? > 15.0),
        // moat_quality
        |f| Ok(h::num(f, "moat_total", 0.0)? >= 24.0),
        // sell_side_confirm
        |f| Ok(h::num(f, "buy_rating_pct", 0.0)? >= 70.0),
        // growth_sustainable
        |f| Ok(h::num(f, "revenue_growth_3y_cagr", 0.0)? > 15.0),
    ]
}

fn checks_munger() -> Vec<Check> {
    vec![
        // simple_business
        |_f| Ok(true),
        // moat_strong
        |f| Ok(h::num(f, "moat_total", 0.0)? >= 28.0),
        // financial_strength
        |f| Ok(h::num(f, "debt_ratio", 100.0)? < 40.0 && h::known_fcf(f)?),
        // wait_for_price
        |f| Ok(h::num(f, "pe_quantile_5y", 100.0)? < 40.0),
        // psych_no_mania
        |f| Ok(h::truth(f, "is_safe", true) && !h::truth(f, "rsi_overbought", false)),
    ]
}

fn checks_templeton() -> Vec<Check> {
    vec![
        // pe_extreme_low
        |f| Ok(h::num(f, "pe_quantile_5y", 100.0)? < 25.0),
        // vs_industry_cheap
        |f| Ok(h::num(f, "vs_peer_avg_pe", 100.0)? < -10.0),
        // not_crowded
        |f| Ok(h::num(f, "sentiment_heat", 100.0)? < 50.0),
    ]
}

fn checks_klarman() -> Vec<Check> {
    vec![
        // margin_of_safety
        |f| Ok(h::num(f, "safety_margin", 0.0)? > 30.0),
        // downside_protected
        |f| Ok(h::num(f, "debt_ratio", 100.0)? < 40.0 && h::known_fcf(f)?),
        // catalyst_clear
        |f| Ok(h::truth(f, "has_positive_catalyst", false)),
    ]
}

// ────────────────────────────────────────────────────────────────
// B 组 · 成长投资派
// ────────────────────────────────────────────────────────────────

fn checks_lynch() -> Vec<Check> {
    vec![
        // peg_ideal
        |f| {
            let p = h::peg(f)?;
            Ok(0.0 < p && p < 1.0)
        },
        // peg_acceptable
        |f| {
            let p = h::peg(f)?;
            Ok(1.0 <= p && p < 1.5)
        },
        // pe_not_rolls_royce
        |f| {
            let x = h::num(f, "pe", 999.0)?;
            Ok(0.0 < x && x < 40.0)
        },
        // fast_grower_zone
        |f| {
            let x = h::num(f, "revenue_growth_latest", 0.0)?;
            Ok(20.0 < x && x < 50.0)
        },
        // understandable
        |_f| Ok(true),
        // research_support
        |f| {
            Ok(h::num(f, "research_coverage", 0.0)? >= 5.0
                && h::num(f, "buy_rating_pct", 0.0)? >= 60.0)
        },
    ]
}

fn checks_oneill() -> Vec<Check> {
    vec![
        // c_eps_growth
        |f| Ok(h::num(f, "net_profit_growth_latest", 0.0)? > 25.0),
        // a_annual_growth
        |f| Ok(h::num(f, "revenue_growth_3y_cagr", 0.0)? > 20.0),
        // n_near_high
        |f| Ok(h::num(f, "pct_from_60d_high", -100.0)? > -10.0),
        // l_industry_leader
        |f| Ok(h::truth(f, "industry_is_growing", false)),
        // i_institutional
        |f| Ok(h::num(f, "fund_manager_count", 0.0)? >= 3.0),
        // m_market_trend
        |f| Ok(h::eq_num(f, "stage_num", 2.0)),
    ]
}

fn checks_thiel() -> Vec<Check> {
    vec![
        // monopoly_leader
        |f| Ok(h::num(f, "moat_total", 0.0)? >= 28.0),
        // network_effect
        |f| Ok(h::num(f, "moat_network", 0.0)? >= 7.0),
        // scale_advantage
        |f| Ok(h::num(f, "moat_scale", 0.0)? >= 7.0),
    ]
}

fn checks_wood() -> Vec<Check> {
    const KWS: &[&str] = &[
        "光学", "半导体", "电池", "锂电", "AI", "人工智能", "生物", "基因", "机器人", "量子", "空间",
        "卫星", "AR", "VR", "自动驾驶", "新能源", "储能", "3D打印", "区块链", "数字货币", "mRNA",
        "脑机", "核聚变", "光模块", "CPO", "光芯片", "算力", "数据中心", "IDC", "HBM", "gpu",
        "通信设备", "光通信", "云计算", "服务器", "存储芯片",
    ];
    vec![
        // s_curve
        |f| Ok(h::or_num(f, "industry_growth", "industry_growth_pct", 0.0)? > 20.0),
        // innovation_platform (haystack lowered, keywords kept verbatim like upstream)
        |f| any_lower_2(f, "industry", "name", KWS),
        // revenue_acceleration
        |f| Ok(h::num(f, "rev_growth_3y", 0.0)? > 15.0),
        // long_term_view
        |f| Ok(h::num(f, "max_drawdown_1y", -100.0)? > -40.0),
    ]
}

fn checks_andreessen() -> Vec<Check> {
    const KWS: &[&str] = &["软件", "SaaS", "AI", "云", "半导体", "互联网", "platform"];
    vec![
        // software_or_ai_native
        |f| any_in_2(f, "industry", "name", KWS),
        // rev_growth_30
        |f| {
            Ok(h::num(f, "rev_growth_3y", 0.0)? > 30.0 || h::num(f, "rev_growth_3y_pct", 0.0)? > 30.0)
        },
        // network_effects
        |f| {
            Ok(h::num(f, "moat_total", 0.0)? >= 26.0
                || h::num(f, "network_effect_score", 0.0)? >= 6.0)
        },
        // market_size_huge
        |f| {
            Ok(h::num(f, "tam_usd_bn", 0.0)? >= 100.0 || h::num(f, "market_cap_yi", 0.0)? >= 5000.0)
        },
        // founder_led
        |f| {
            Ok(h::truth(f, "founder_active", false)
                || h::num(f, "founder_ownership_pct", 0.0)? >= 5.0)
        },
    ]
}

fn checks_gurley() -> Vec<Check> {
    const KWS: &[&str] = &["marketplace", "SaaS", "平台", "软件", "订阅"];
    vec![
        // marketplace_or_saas
        |f| any_in_2(f, "industry", "name", KWS),
        // unit_economics_positive
        |f| {
            Ok(h::num(f, "gross_margin", 0.0)? >= 50.0 && h::num(f, "net_margin", 0.0)? > 0.0)
        },
        // magnitude_of_demand
        |f| Ok(h::num(f, "rev_growth_3y", 0.0)? > 25.0),
        // burn_multiple_ok
        |f| {
            if h::known_fcf(f)? {
                Ok(true)
            } else {
                Ok(h::num(f, "fcf_margin", 0.0)? > -20.0)
            }
        },
        // valuation_reasonable
        |f| Ok(h::num(f, "ev_to_revenue", 100.0)? < 20.0),
    ]
}

fn checks_naval() -> Vec<Check> {
    const KWS: &[&str] = &["软件", "平台", "内容", "互联网", "SaaS", "AI"];
    vec![
        // permissionless_leverage
        |f| any_in_2(f, "industry", "name", KWS),
        // specific_knowledge
        |f| Ok(h::num(f, "moat_total", 0.0)? >= 28.0),
        // long_holding_horizon
        |f| {
            Ok(h::num(f, "roe_5y_above_15", 0.0)? >= 4.0 || h::num(f, "roe", 0.0)? >= 18.0)
        },
        // not_zero_sum
        |f| none_in_1(f, "industry", &["博彩", "期货", "加密"]),
        // compound_interest
        |f| Ok(h::num(f, "net_profit_growth_3y", 0.0)? > 15.0),
    ]
}

fn checks_gerstner() -> Vec<Check> {
    const KWS: &[&str] = &[
        "AI", "云", "算力", "芯片", "半导体", "CPO", "光模块", "数据库", "SaaS",
    ];
    vec![
        // ai_or_cloud_native
        |f| any_in_2(f, "industry", "name", KWS),
        // revenue_acceleration
        |f| {
            Ok(h::num(f, "rev_growth_yoy", 0.0)? > h::num(f, "rev_growth_3y", 100.0)? + 5.0)
        },
        // rule_of_40
        |f| {
            Ok(h::num(f, "rev_growth_3y", 0.0)? + h::num(f, "net_margin", 0.0)? >= 40.0)
        },
        // category_leader
        |f| Ok(h::num(f, "industry_rank", 99.0)? <= 3.0),
        // expensive_but_growing
        |f| {
            Ok(h::num(f, "peg", 100.0)? < 1.5 || h::num(f, "rev_growth_3y", 0.0)? > 35.0)
        },
    ]
}

fn checks_chamath() -> Vec<Check> {
    const KWS: &[&str] = &["AI", "新能源", "生物", "加密", "太空", "元宇宙", "SaaS"];
    vec![
        // disruptor_thesis
        |f| any_in_2(f, "industry", "name", KWS),
        // tam_centibillion
        |f| Ok(h::num(f, "tam_usd_bn", 0.0)? >= 100.0),
        // path_to_profit
        |f| {
            Ok(h::num(f, "gross_margin", 0.0)? > 35.0 || h::num(f, "net_margin", 0.0)? > 0.0)
        },
        // not_a_meme
        |f| Ok(h::num(f, "rev", 0.0)? > 5.0 || h::num(f, "revenue_b", 0.0)? > 0.5),
        // transparent_metrics
        |f| Ok(h::num(f, "governance_score", 0.0)? >= 6.0),
    ]
}

// ────────────────────────────────────────────────────────────────
// C 组 · 宏观对冲派
// ────────────────────────────────────────────────────────────────

fn checks_soros() -> Vec<Check> {
    vec![
        // sentiment_long_reflex
        |f| Ok(h::num(f, "upside_to_target", 0.0)? > 10.0),
        // sentiment_short_reflex_penalty
        |f| Ok(h::num(f, "upside_to_target", 0.0)? > -15.0),
        // macro_tailwind
        |f| Ok(h::truth(f, "macro_rate_easing", false)),
        // trend_clear
        |f| Ok(h::eq_num(f, "stage_num", 2.0)),
    ]
}

fn checks_dalio() -> Vec<Check> {
    vec![
        // rate_cycle_pos
        |f| Ok(h::truth(f, "macro_rate_easing", false)),
        // low_debt
        |f| Ok(h::num(f, "debt_ratio", 100.0)? < 40.0),
        // dividend_income
        |f| Ok(h::num(f, "dividend_yield", 0.0)? > 1.0),
    ]
}

fn checks_marks() -> Vec<Check> {
    vec![
        // market_fear
        |f| Ok(h::num(f, "sentiment_heat", 100.0)? < 60.0),
        // cheap_vs_history
        |f| Ok(h::num(f, "pe_quantile_5y", 100.0)? < 40.0),
        // risk_priced_in
        |f| Ok(h::num(f, "max_drawdown_1y", 0.0)? < -15.0),
    ]
}

fn checks_druck() -> Vec<Check> {
    vec![
        // liquidity_tailwind
        |f| Ok(h::truth(f, "macro_rate_easing", false)),
        // macro_theme
        |f| Ok(h::truth(f, "industry_is_growing", false)),
        // high_conviction
        |f| Ok(h::num(f, "consensus_growth_to_2026", 0.0)? > 15.0),
    ]
}

fn checks_robertson() -> Vec<Check> {
    vec![
        // best_in_class — `rank <= 2 if rank_defaulted_0 > 0 else False`
        |f| {
            if h::num(f, "industry_rank", 0.0)? > 0.0 {
                Ok(h::num(f, "industry_rank", 99.0)? <= 2.0)
            } else {
                Ok(false)
            }
        },
        // fundamentals_strong
        |f| {
            Ok(h::num(f, "roe_latest", 0.0)? > 12.0 && h::num(f, "net_margin", 0.0)? > 12.0)
        },
    ]
}

fn checks_burry() -> Vec<Check> {
    vec![
        // not_in_bubble_basket
        |f| Ok(h::num(f, "pe_ttm", 0.0)? < 60.0 && h::num(f, "ps", 100.0)? < 15.0),
        // insider_not_selling
        |f| Ok(h::is_false(f, "insider_selling_recent", false)),
        // debt_not_explosive
        |f| Ok(h::num(f, "debt_ratio", 100.0)? < 70.0),
        // not_retail_mania
        |f| Ok(h::num(f, "retail_holding_pct", 0.0)? < 50.0),
        // fcf_real_not_eps
        |f| Ok(h::known_fcf(f)? && h::num(f, "fcf_margin", 0.0)? >= 5.0),
    ]
}

fn checks_chanos() -> Vec<Check> {
    vec![
        // not_promotional_ceo
        |f| Ok(h::num(f, "ceo_promotional_score", 0.0)? < 7.0),
        // audited_clean
        |f| Ok(h::is_false(f, "audit_qualified", false)),
        // cash_matches_eps
        |f| Ok((h::num(f, "ocf_to_net_income_ratio", 1.0)? - 1.0).abs() < 0.4),
        // not_china_concept
        |f| {
            if h::eq_str(f, "market", "US") {
                let s = h::concat2(f, "industry", "", "country", "")?;
                Ok(!s.contains("China"))
            } else {
                Ok(true)
            }
        },
        // debt_disclosure_clean
        |f| Ok(h::num(f, "off_balance_debt_ratio", 0.0)? < 0.2),
    ]
}

// ────────────────────────────────────────────────────────────────
// D 组 · 技术趋势派
// ────────────────────────────────────────────────────────────────

fn checks_livermore() -> Vec<Check> {
    vec![
        // stage_2
        |f| Ok(h::eq_num(f, "stage_num", 2.0)),
        // ma_bull
        |f| Ok(h::truth(f, "ma_bull_aligned", false)),
        // volume_confirm
        |f| {
            let p = h::num(f, "pct_from_60d_high", -100.0)?;
            Ok(-15.0 < p && p < -2.0 && h::num(f, "rsi", 50.0)? < 75.0)
        },
    ]
}

fn checks_minervini() -> Vec<Check> {
    vec![
        // stage_2_only
        |f| Ok(h::eq_num(f, "stage_num", 2.0)),
        // ma_stack
        |f| Ok(h::truth(f, "ma_bull_aligned", false)),
        // near_high
        |f| Ok(h::num(f, "pct_from_60d_high", -100.0)? > -25.0),
        // ytd_strong
        |f| Ok(h::num(f, "ytd_return", 0.0)? > 0.0),
        // not_overbought
        |f| Ok(h::num(f, "rsi", 50.0)? < 80.0),
    ]
}

fn checks_darvas() -> Vec<Check> {
    vec![
        // box_breakout
        |f| Ok(h::eq_num(f, "stage_num", 2.0)),
        // ma_support
        |f| Ok(h::truth(f, "ma_bull_aligned", false)),
    ]
}

fn checks_gann() -> Vec<Check> {
    vec![
        // trend_up
        |f| Ok(h::eq_num(f, "stage_num", 2.0)),
        // volatility_normal
        |f| {
            let x = h::num(f, "volatility_1y", 100.0)?;
            Ok(0.0 < x && x < 60.0)
        },
    ]
}

// ────────────────────────────────────────────────────────────────
// E 组 · 中国价投/公募派
// ────────────────────────────────────────────────────────────────

fn checks_duan() -> Vec<Check> {
    vec![
        // good_business
        |f| Ok(h::num(f, "net_margin", 0.0)? > 15.0 && h::num(f, "roe_latest", 0.0)? > 10.0),
        // good_people
        |f| {
            Ok(!h::truth(f, "has_pledge_issue", false) && h::truth(f, "no_violations", true))
        },
        // good_price
        |f| Ok(h::num(f, "pe_quantile_5y", 100.0)? < 50.0),
        // pe_not_expensive
        |f| {
            let x = h::num(f, "pe", 999.0)?;
            Ok(0.0 < x && x < 40.0)
        },
        // long_term_clear
        |f| {
            Ok(h::num(f, "moat_total", 0.0)? >= 22.0
                && h::num(f, "consecutive_profit_years", 0.0)? >= 5.0)
        },
    ]
}

fn checks_zhangkun() -> Vec<Check> {
    vec![
        // roe_persistent
        |f| Ok(h::num(f, "roe_5y_above_15", 0.0)? >= 3.0),
        // pricing_power
        |f| Ok(h::num(f, "net_margin", 0.0)? > 18.0),
        // moat_brand
        |f| Ok(h::num(f, "moat_intangible", 0.0)? >= 7.0),
        // pe_discipline
        |f| {
            let x = h::num(f, "pe", 999.0)?;
            Ok(0.0 < x && x < 40.0)
        },
    ]
}

fn checks_zhushaoxing() -> Vec<Check> {
    vec![
        // long_term_growth
        |f| Ok(h::num(f, "revenue_growth_3y_cagr", 0.0)? > 15.0),
        // industry_momentum
        |f| Ok(h::truth(f, "industry_is_growing", false)),
        // low_turnover_fit
        |f| {
            let x = h::num(f, "volatility_1y", 100.0)?;
            Ok(0.0 < x && x < 50.0)
        },
    ]
}

fn checks_xiezhiyu() -> Vec<Check> {
    vec![
        // garp_balance — `0.5 < pe / max(growth, 1) < 2.0`
        |f| {
            let pe = h::num(f, "pe", 0.0)?;
            let g = h::num(f, "revenue_growth_latest", 1.0)?;
            let d = if g >= 1.0 { g } else { 1.0 };
            let v = pe / d;
            Ok(0.5 < v && v < 2.0)
        },
        // growth_minimum
        |f| Ok(h::num(f, "revenue_growth_latest", 0.0)? > 10.0),
    ]
}

fn checks_fengliu() -> Vec<Check> {
    vec![
        // good_odds
        |f| {
            Ok(h::num(f, "pe_quantile_5y", 100.0)? < 40.0
                || h::num(f, "max_drawdown_1y", 0.0)? < -25.0)
        },
        // expectation_gap
        |f| {
            Ok(h::num(f, "upside_to_target", 0.0)? > 15.0
                || h::num(f, "upside_to_target", 0.0)? < -15.0)
        },
        // common_sense
        |f| Ok(h::truth(f, "is_safe", true) && h::known_fcf(f)?),
    ]
}

fn checks_dengxiaofeng() -> Vec<Check> {
    vec![
        // cycle_position
        |f| Ok(!h::truth(f, "industry_in_decline", false)),
        // value_creation
        |f| Ok(h::num(f, "roic", 0.0)? > 10.0 || h::num(f, "roe_latest", 0.0)? > 12.0),
        // pe_reasonable
        |f| {
            let x = h::num(f, "pe", 999.0)?;
            Ok(0.0 < x && x < 35.0)
        },
        // good_price
        |f| Ok(h::num(f, "pe_quantile_5y", 100.0)? < 60.0),
    ]
}

fn checks_zhang_lei() -> Vec<Check> {
    const LONG_RUNWAY: &[&str] = &[
        "互联网", "消费", "医药", "新能源", "半导体", "AI", "白酒", "生物", "创新药",
    ];
    const CYCLICAL: &[&str] = &["钢铁", "煤炭", "化工原料", "航运"];
    vec![
        // long_runway_industry
        |f| any_in_1(f, "industry", LONG_RUNWAY),
        // category_leader_moat
        |f| {
            Ok(h::num(f, "industry_rank", 99.0)? <= 3.0 && h::num(f, "moat_total", 0.0)? >= 26.0)
        },
        // founder_aligned
        |f| {
            Ok(h::num(f, "founder_ownership_pct", 0.0)? >= 3.0
                || h::truth(f, "founder_active", false))
        },
        // compounder_track_record
        |f| {
            Ok(h::num(f, "roe_5y_above_15", 0.0)? >= 3.0
                && h::num(f, "net_profit_growth_3y", 0.0)? > 12.0)
        },
        // not_just_cyclical
        |f| none_in_1(f, "industry", CYCLICAL),
    ]
}

// ────────────────────────────────────────────────────────────────
// F 组 · A 股游资派 (from `_youzi_base_rules` + per-investor extras)
// ────────────────────────────────────────────────────────────────

fn c_stage_2(f: &Value) -> R<bool> {
    Ok(h::eq_num(f, "stage_num", 2.0))
}
fn c_lhb_hot(f: &Value) -> R<bool> {
    Ok(h::num(f, "lhb_30d_count", 0.0)? >= 1.0)
}
fn c_top_of_sector(f: &Value) -> R<bool> {
    let x = h::num(f, "industry_rank", 99.0)?;
    Ok(0.0 < x && x <= 3.0)
}
fn c_sentiment_hot(f: &Value) -> R<bool> {
    Ok(h::num(f, "sentiment_heat", 0.0)? >= 50.0)
}
fn c_fundamentals_ok(f: &Value) -> R<bool> {
    Ok(h::num(f, "roe_latest", 0.0)? > 10.0)
}

// ────────────────────────────────────────────────────────────────
// G 组 · 量化系统派
// ────────────────────────────────────────────────────────────────

fn checks_simons() -> Vec<Check> {
    vec![
        // statistical_edge
        |f| Ok(h::num(f, "ytd_return", -100.0)? > 0.0),
        // volatility_tradeable
        |f| {
            let x = h::num(f, "volatility_1y", 0.0)?;
            Ok(20.0 < x && x < 80.0)
        },
    ]
}

fn checks_thorp() -> Vec<Check> {
    vec![
        // positive_ev
        |f| Ok(h::num(f, "upside_to_target", 0.0)? > 10.0),
        // kelly_ok
        |f| Ok(h::num(f, "volatility_1y", 100.0)? < 50.0),
    ]
}

fn checks_shaw() -> Vec<Check> {
    vec![
        // quality_factor
        |f| Ok(h::num(f, "roe_latest", 0.0)? > 12.0),
        // value_factor
        |f| Ok(h::num(f, "pe_quantile_5y", 100.0)? < 60.0),
        // momentum_factor
        |f| Ok(h::eq_num(f, "stage_num", 2.0)),
        // growth_factor
        |f| Ok(h::num(f, "revenue_growth_3y_cagr", 0.0)? > 15.0),
    ]
}

fn checks_asness() -> Vec<Check> {
    vec![
        // value_factor
        |f| Ok(h::num(f, "pe_ttm", 100.0)? < 20.0 && h::num(f, "pb", 100.0)? < 4.0),
        // quality_factor
        |f| Ok(h::num(f, "roe", 0.0)? > 12.0 && h::num(f, "debt_ratio", 100.0)? < 60.0),
        // momentum_factor
        |f| {
            Ok(h::num(f, "ytd_return", 0.0)? > 0.0 && h::truth(f, "price_above_ma200", false))
        },
        // profitability_consistent
        |f| Ok(h::num(f, "roe_5y_min", 0.0)? > 8.0),
        // not_lottery_ticket
        |f| Ok(h::num(f, "pe_ttm", 0.0)? > 0.0 && h::num(f, "pe_ttm", 1e9)? < 80.0),
    ]
}

// ────────────────────────────────────────────────────────────────
// H 组 · 科技领袖派 / AI CEO
// ────────────────────────────────────────────────────────────────

fn checks_jensen_huang() -> Vec<Check> {
    const KWS: &[&str] = &[
        "AI", "GPU", "CPO", "光模块", "HBM", "半导体", "液冷", "算力", "数据中心",
    ];
    const ECOSYSTEM: &[&str] = &["NVIDIA", "TSMC", "台积电", "SK 海力士", "三星电子"];
    vec![
        // ai_compute_demand
        |f| any_in_2(f, "industry", "name", KWS),
        // cuda_ecosystem_proxy
        |f| {
            Ok(h::num(f, "moat_total", 0.0)? >= 28.0 || any_in_1(f, "name", ECOSYSTEM)?)
        },
        // data_center_capex_beneficiary
        |f| Ok(h::num(f, "rev_growth_yoy", 0.0)? > 30.0),
        // gross_margin_strong
        |f| Ok(h::num(f, "gross_margin", 0.0)? >= 50.0),
        // light_speed_moore_compliant
        |f| Ok(h::num(f, "rd_intensity", 0.0)? >= 8.0),
    ]
}

fn checks_musk() -> Vec<Check> {
    const KWS: &[&str] = &[
        "新能源车", "电池", "航天", "机器人", "AI", "卫星", "Neuralink", "太阳能",
    ];
    const LEGACY: &[&str] = &["传统汽车", "传统制造", "重型机械"];
    vec![
        // first_principles_industry
        |f| any_in_2(f, "industry", "name", KWS),
        // vertical_integration
        |f| {
            Ok(h::num(f, "vertical_integration_score", 0.0)? >= 6.0
                || h::num(f, "gross_margin", 0.0)? >= 20.0)
        },
        // manufacturing_scale
        |f| Ok(h::num(f, "rev", 0.0)? > 10.0 || h::num(f, "revenue_b", 0.0)? > 1.0),
        // not_legacy_oem — generator filter: skip the check when the industry is 新能源
        |f| {
            let s = h::text(f, "industry", "")?;
            if s.contains("新能源") {
                Ok(true)
            } else {
                Ok(!contains_any(&s, LEGACY))
            }
        },
        // ceo_visible
        |f| Ok(h::num(f, "ceo_promotional_score", 0.0)? >= 5.0),
    ]
}

fn checks_altman() -> Vec<Check> {
    const KWS: &[&str] = &[
        "AI", "数据中心", "云", "半导体", "核电", "太阳能", "电网", "SaaS", "机器人",
    ];
    const BOTTLENECK: &[&str] = &["核电", "可控核聚变", "电网", "数据中心", "算力", "HBM", "液冷"];
    const CONSUMER_APP: &[&str] = &["消费电子", "游戏", "社交"];
    vec![
        // agi_supply_chain
        |f| any_in_2(f, "industry", "name", KWS),
        // scaling_laws_compliant
        |f| {
            Ok(h::num(f, "rev_growth_yoy", 0.0)? > 25.0
                || h::num(f, "capex_growth_yoy", 0.0)? > 30.0)
        },
        // platform_or_infra
        |f| Ok(h::num(f, "moat_total", 0.0)? >= 26.0),
        // energy_or_compute_bottleneck
        |f| any_in_1(f, "industry", BOTTLENECK),
        // not_pure_consumer_app
        |f| Ok(!h::str_in(f, "industry", "", CONSUMER_APP)),
    ]
}

fn checks_saylor() -> Vec<Check> {
    const KWS: &[&str] = &[
        "比特币", "BTC", "加密", "Crypto", "Mining", "Coinbase", "区块链", "数字资产",
    ];
    const HARD_MONEY: &[&str] = &["加密", "黄金", "白银", "稀有金属", "比特币"];
    vec![
        // btc_or_digital_asset_exposure
        |f| any_in_2(f, "industry", "name", KWS),
        // treasury_strategy_signal
        |f| {
            Ok(h::num(f, "cash_to_marketcap_ratio", 0.0)? > 0.10
                || h::num(f, "btc_holdings_b", 0.0)? > 0.0)
        },
        // not_just_eps_play
        |f| Ok(h::num(f, "revenue_b", 0.0)? > 0.5),
        // hard_money_thesis
        |f| Ok(h::num(f, "debt_ratio", 100.0)? < 80.0),
        // fiat_devaluation_beneficiary
        |f| any_in_1(f, "industry", HARD_MONEY),
    ]
}

// ────────────────────────────────────────────────────────────────
// I 组 · AI 卡位/瓶颈猎手
// ────────────────────────────────────────────────────────────────

fn checks_serenity() -> Vec<Check> {
    vec![
        // ai_chain_hit
        |f| Ok(h::truth(f, "ai_chain_hit", false)),
        // chokepoint_strong
        |f| Ok(h::num(f, "ai_chokepoint_score", 0.0)? >= 70.0),
        // irreplaceable
        |f| {
            Ok(h::truth(f, "ai_chain_hit", false) && h::truth(f, "ai_irreplaceable", false))
        },
        // smallcap_elastic
        |f| Ok(h::truth(f, "ai_chain_hit", false) && h::truth(f, "ai_smallcap", false)),
        // demand_inflection
        |f| {
            Ok(h::truth(f, "ai_chain_hit", false)
                && (h::truth(f, "policy_supportive", false)
                    || h::truth(f, "has_positive_catalyst", false)
                    || h::num(f, "industry_growth", 0.0)? >= 20.0))
        },
    ]
}

// ────────────────────────────────────────────────────────────────
// 股海贼王 (v3.9.0)
// ────────────────────────────────────────────────────────────────

fn checks_ghzw() -> Vec<Check> {
    const ERA: &[&str] = &[
        "AI", "人工智能", "机器人", "无人驾驶", "智能驾驶", "算力", "半导体", "低空", "固态电池",
    ];
    vec![
        // mainline_theme
        |f| {
            Ok(h::truth(f, "has_positive_catalyst", false)
                && h::num(f, "sentiment_heat", 0.0)? >= 55.0)
        },
        // limit_up_gene
        |f| {
            Ok(h::num(f, "lhb_30d_count", 0.0)? >= 1.0
                || h::truth(f, "has_limit_up_recent", false))
        },
        // strong_tape
        |f| Ok(h::eq_num(f, "stage_num", 2.0) && h::truth(f, "vol_amplified", true)),
        // low_position_logic
        |f| {
            Ok(h::num(f, "pct_from_year_high", 0.0)? < -25.0
                && h::truth(f, "has_positive_catalyst", false))
        },
        // era_carrier — str() on the values, so null renders as "None"
        |f| {
            let s = format!(
                "{}{}",
                h::str_of(f, "industry", ""),
                h::str_of(f, "name", "")
            );
            Ok(contains_any(&s, ERA))
        },
        // liquidity_exit
        |f| {
            Ok(h::num(f, "sentiment_heat", 0.0)? >= 40.0
                || h::num(f, "lhb_30d_count", 0.0)? >= 1.0)
        },
    ]
}

/// `any(k in (f.get(a,"") + f.get(b,"")) for k in KWS)` — no `.lower()`.
fn any_in_2(f: &Value, a: &str, b: &str, kws: &[&str]) -> R<bool> {
    let s = h::concat2(f, a, "", b, "")?;
    Ok(contains_any(&s, kws))
}

/// Per-investor check lists, in the exact order of `INVESTOR_RULES` /
/// `criteria_meta.json`. `None` for an unknown investor.
fn checks_for(investor_id: &str) -> Option<Vec<Check>> {
    Some(match investor_id {
        "buffett" => checks_buffett(),
        "graham" => checks_graham(),
        "fisher" => checks_fisher(),
        "munger" => checks_munger(),
        "templeton" => checks_templeton(),
        "klarman" => checks_klarman(),
        "lynch" => checks_lynch(),
        "oneill" => checks_oneill(),
        "thiel" => checks_thiel(),
        "wood" => checks_wood(),
        "andreessen" => checks_andreessen(),
        "gurley" => checks_gurley(),
        "naval" => checks_naval(),
        "gerstner" => checks_gerstner(),
        "chamath" => checks_chamath(),
        "soros" => checks_soros(),
        "dalio" => checks_dalio(),
        "marks" => checks_marks(),
        "druck" => checks_druck(),
        "robertson" => checks_robertson(),
        "burry" => checks_burry(),
        "chanos" => checks_chanos(),
        "livermore" => checks_livermore(),
        "minervini" => checks_minervini(),
        "darvas" => checks_darvas(),
        "gann" => checks_gann(),
        "duan" => checks_duan(),
        "zhangkun" => checks_zhangkun(),
        "zhushaoxing" => checks_zhushaoxing(),
        "xiezhiyu" => checks_xiezhiyu(),
        "fengliu" => checks_fengliu(),
        "dengxiaofeng" => checks_dengxiaofeng(),
        "zhang_lei" => checks_zhang_lei(),
        // F 组 · 游资 (`_youzi_base_rules` order: stage_2, lhb_hot, top_of_sector,
        // sentiment_hot, then the appended xiao_ey fundamentals rule)
        "zhang_mz" => vec![c_stage_2, c_lhb_hot, c_top_of_sector, c_sentiment_hot],
        "sun_ge" => vec![c_stage_2, c_top_of_sector, c_sentiment_hot],
        "zhao_lg" => vec![c_stage_2, c_lhb_hot, c_top_of_sector, c_sentiment_hot],
        "fs_wyj" => vec![c_lhb_hot, c_sentiment_hot],
        "yangjia" => vec![c_stage_2, c_lhb_hot, c_sentiment_hot],
        "chen_xq" => vec![c_stage_2, c_lhb_hot, c_top_of_sector, c_sentiment_hot],
        "hu_jl" => vec![c_stage_2, c_lhb_hot, c_sentiment_hot],
        "fang_xx" => vec![c_stage_2, c_sentiment_hot],
        "zuoshou" => vec![c_stage_2, c_lhb_hot, c_top_of_sector, c_sentiment_hot],
        "xiao_ey" => vec![c_stage_2, c_sentiment_hot, c_fundamentals_ok],
        "jiao_yy" => vec![c_stage_2, c_top_of_sector, c_sentiment_hot],
        "mao_lb" => vec![c_stage_2, c_sentiment_hot],
        "xiao_xian" => vec![c_stage_2, c_top_of_sector, c_sentiment_hot],
        "lasa" => vec![c_lhb_hot, c_sentiment_hot],
        "chengdu" => vec![c_lhb_hot, c_sentiment_hot],
        "sunan" => vec![c_lhb_hot, c_sentiment_hot],
        "ningbo_st" => vec![c_lhb_hot, c_sentiment_hot],
        "liuyi_zl" => vec![c_stage_2, c_top_of_sector, c_sentiment_hot],
        "liu_sh" => vec![c_lhb_hot, c_sentiment_hot],
        "gu_bl" => vec![c_stage_2, c_top_of_sector, c_sentiment_hot],
        "bj_cj" => vec![c_lhb_hot, c_sentiment_hot],
        "wang_zr" => vec![c_stage_2, c_sentiment_hot],
        "xin_dd" => vec![c_lhb_hot, c_sentiment_hot],
        "ghzw" => checks_ghzw(),
        "simons" => checks_simons(),
        "thorp" => checks_thorp(),
        "shaw" => checks_shaw(),
        "asness" => checks_asness(),
        "jensen_huang" => checks_jensen_huang(),
        "musk" => checks_musk(),
        "altman" => checks_altman(),
        "saylor" => checks_saylor(),
        "serenity" => checks_serenity(),
        _ => return None,
    })
}

/// `investor_criteria.INVESTOR_RULES[investor_id]` resolved to Rust rules.
pub fn rules_for(investor_id: &str) -> Option<&'static Vec<Rule>> {
    static ALL: LazyLock<HashMap<&'static str, Vec<Rule>>> = LazyLock::new(|| {
        let mut map = HashMap::new();
        for id in crate::db::all_ids() {
            if let Some(rules) = build_rules(id) {
                map.insert(id, rules);
            }
        }
        map
    });
    ALL.get(investor_id)
}

fn build_rules(investor_id: &str) -> Option<Vec<Rule>> {
    let checks = checks_for(investor_id)?;
    let list = meta().get(investor_id)?.as_array()?;
    assert_eq!(
        list.len(),
        checks.len(),
        "criteria metadata/check mismatch for {}: {} vs {}",
        investor_id,
        list.len(),
        checks.len()
    );
    let mut out = Vec::with_capacity(list.len());
    for (m, check) in list.iter().zip(checks) {
        out.push(Rule {
            rule_id: m.get("rule_id")?.as_str()?.to_string(),
            name: m.get("name")?.as_str()?.to_string(),
            weight: m.get("weight")?.as_i64()?,
            check,
            pass_msg: m.get("pass_msg")?.as_str()?.to_string(),
            fail_msg: m.get("fail_msg")?.as_str()?.to_string(),
        });
    }
    Some(out)
}

/// `Rule.check` guarded like `investor_evaluator._safe_check`:
/// `Ok(true)` pass, `Ok(false)` fail, `Err` → data missing, skip the rule.
pub fn safe_check(rule: &Rule, features: &Value) -> Option<bool> {
    match (rule.check)(features) {
        Ok(b) => Some(b),
        Err(PyErr) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn rule_table_is_complete_and_ordered() {
        let all = crate::db::investors();
        let mut total = 0;
        for inv in all {
            let id = inv["id"].as_str().unwrap();
            let rules = rules_for(id).unwrap_or_else(|| panic!("no rules for {id}"));
            let meta = meta()[id].as_array().unwrap();
            assert_eq!(rules.len(), meta.len(), "rule count for {id}");
            for (r, m) in rules.iter().zip(meta) {
                assert_eq!(r.rule_id, m["rule_id"]);
                assert_eq!(r.name, m["name"]);
                assert_eq!(r.weight, m["weight"].as_i64().unwrap());
                assert!((1..=5).contains(&r.weight));
            }
            total += rules.len();
        }
        assert_eq!(total, 242);
        assert!(rules_for("no_such_investor").is_none());
    }

    #[test]
    fn missing_data_skips_instead_of_failing() {
        // `fcf_known` absent → ValueError → skip
        let munger = rules_for("munger").unwrap();
        let fcf = munger.iter().find(|r| r.rule_id == "financial_strength").unwrap();
        assert_eq!(safe_check(fcf, &json!({"debt_ratio": 30})), None);
        // present null also raises (bypasses the default)
        assert_eq!(safe_check(fcf, &json!({"debt_ratio": 30, "fcf_known": null})), None);
        assert_eq!(
            safe_check(fcf, &json!({"debt_ratio": 30, "fcf_known": true, "fcf_positive": true})),
            Some(true)
        );
        assert_eq!(
            safe_check(fcf, &json!({"debt_ratio": 30, "fcf_known": true, "fcf_positive": false})),
            Some(false)
        );
        // a null numeric feature skips the rule rather than failing it — but only
        // when Python's `and` actually reaches the operand
        let buffett = rules_for("buffett").unwrap();
        let roe = buffett.iter().find(|r| r.rule_id == "roe_5y_15").unwrap();
        assert_eq!(safe_check(roe, &json!({"roe_5y_above_15": 5, "roe_5y_min": null})), None);
        // `0 >= 4 and …` short-circuits, so the null never raises
        assert_eq!(safe_check(roe, &json!({"roe_5y_min": null})), Some(false));
        assert_eq!(safe_check(roe, &json!({})), Some(false));
    }

    #[test]
    fn boundary_rules_are_inclusive_where_python_is() {
        let buffett = rules_for("buffett").unwrap();
        let roe = buffett.iter().find(|r| r.rule_id == "roe_5y_15").unwrap();
        assert_eq!(safe_check(roe, &json!({"roe_5y_above_15": 4, "roe_5y_min": 12.1})), Some(true));
        assert_eq!(safe_check(roe, &json!({"roe_5y_above_15": 4, "roe_5y_min": 12})), Some(false));
        let debt = buffett.iter().find(|r| r.rule_id == "debt_ratio_50").unwrap();
        assert_eq!(safe_check(debt, &json!({"debt_ratio": 0})), Some(false));
        assert_eq!(safe_check(debt, &json!({"debt_ratio": 49.9})), Some(true));
        assert_eq!(safe_check(debt, &json!({"debt_ratio": 50})), Some(false));
        // lynch peg_ideal: PEG exactly 1 fails, strictly below passes
        let lynch = rules_for("lynch").unwrap();
        let peg = lynch.iter().find(|r| r.rule_id == "peg_ideal").unwrap();
        assert_eq!(safe_check(peg, &json!({"pe": 20, "revenue_growth_latest": 20})), Some(false));
        assert_eq!(safe_check(peg, &json!({"pe": 20, "revenue_growth_latest": 25})), Some(true));
        // wood lowers the haystack, so uppercase keywords only match lowercase text
        let wood = rules_for("wood").unwrap();
        let platform = wood.iter().find(|r| r.rule_id == "innovation_platform").unwrap();
        assert_eq!(safe_check(platform, &json!({"industry": "光模块CPO"})), Some(true));
        assert_eq!(safe_check(platform, &json!({"industry": "AI"})), Some(false));
    }

    #[test]
    fn robertson_conditional_and_chanos_identity_edges() {
        let robertson = rules_for("robertson").unwrap();
        let rank = robertson.iter().find(|r| r.rule_id == "best_in_class").unwrap();
        assert_eq!(safe_check(rank, &json!({})), Some(false)); // default 0 → else False
        assert_eq!(safe_check(rank, &json!({"industry_rank": 1})), Some(true));
        assert_eq!(safe_check(rank, &json!({"industry_rank": 3})), Some(false));
        assert_eq!(safe_check(rank, &json!({"industry_rank": null})), None);

        let chanos = rules_for("chanos").unwrap();
        let audit = chanos.iter().find(|r| r.rule_id == "audited_clean").unwrap();
        assert_eq!(safe_check(audit, &json!({})), Some(true)); // absent → default False
        assert_eq!(safe_check(audit, &json!({"audit_qualified": 0})), Some(false)); // 0 is not False
        assert_eq!(safe_check(audit, &json!({"audit_qualified": null})), Some(false));
        let china = chanos.iter().find(|r| r.rule_id == "not_china_concept").unwrap();
        assert_eq!(safe_check(china, &json!({"market": "A"})), Some(true));
        assert_eq!(safe_check(china, &json!({"market": "US", "industry": "China ADR"})), Some(false));
        assert_eq!(safe_check(china, &json!({"market": "US", "industry": "软件"})), Some(true));
        assert_eq!(safe_check(china, &json!({"market": "US", "industry": null})), None);
    }
}
