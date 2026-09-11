//! Port of `lib/stock_style.py` — dynamic style detection + weighted scoring.

use serde_json::{Map, Value};
use uzi_core::py::{f_fin, round};

pub const WHITE_HORSE: &str = "white_horse";
pub const GROWTH_TECH: &str = "growth_tech";
pub const CYCLE: &str = "cycle";
pub const SMALL_SPECULATIVE: &str = "small_speculative";
pub const DIVIDEND_DEFENSE: &str = "dividend_defense";
pub const DISTRESSED: &str = "distressed";
pub const QUANT_FACTOR: &str = "quant_factor";
pub const BALANCED: &str = "balanced";

/// `ALL_STYLES`, in upstream order.
pub const ALL_STYLES: &[&str] = &[
    WHITE_HORSE,
    GROWTH_TECH,
    CYCLE,
    SMALL_SPECULATIVE,
    DIVIDEND_DEFENSE,
    DISTRESSED,
    QUANT_FACTOR,
    BALANCED,
];

/// `STYLE_LABELS`.
pub const STYLE_LABELS: &[(&str, &str)] = &[
    (WHITE_HORSE, "白马价值"),
    (GROWTH_TECH, "高成长科技"),
    (CYCLE, "周期股"),
    (SMALL_SPECULATIVE, "小盘投机"),
    (DIVIDEND_DEFENSE, "分红防御"),
    (DISTRESSED, "困境反转"),
    (QUANT_FACTOR, "量化因子型"),
    (BALANCED, "中性兜底"),
];

/// `STYLE_EXPLANATIONS`.
pub const STYLE_EXPLANATIONS: &[(&str, &str)] = &[
    (
        WHITE_HORSE,
        "大盘 + 高 ROE + 低 PE · 价值派 (A 组+E 组) 加权 ×1.5、游资降权 ×0.3",
    ),
    (
        GROWTH_TECH,
        "高成长 + 科技/医药/新能源 · 成长派 (B 组) 加权 ×1.5、技术派 ×1.2",
    ),
    (
        CYCLE,
        "周期行业 · 宏观派 (C 组) 加权 ×1.5、原料/期货维度加权 ×1.5",
    ),
    (
        SMALL_SPECULATIVE,
        "A 股小盘 · 游资 (F 组) 加权 ×1.5、龙虎榜/舆情维度加权 ×1.5",
    ),
    (
        DIVIDEND_DEFENSE,
        "高股息 + 银行/电力 · 价值派加权、财务/治理维度加权 ×1.3",
    ),
    (
        DISTRESSED,
        "PB<1 + ROE 低 · 卡拉曼/邓普顿加权 ×1.5、估值/财务维度加权",
    ),
    (
        QUANT_FACTOR,
        "多家量化基金重仓 · 量化派 (G 组) 加权 ×1.5、资金流维度加权",
    ),
    (BALANCED, "无明显风格倾向 · 全派系等权"),
];

/// `STYLE_LABELS[key]` — `""` for unknown keys.
pub fn style_label(key: &str) -> &'static str {
    STYLE_LABELS
        .iter()
        .find(|(k, _)| *k == key)
        .map(|(_, v)| *v)
        .unwrap_or("")
}

/// `STYLE_EXPLANATIONS[key]` — `""` for unknown keys.
pub fn style_explanation(key: &str) -> &'static str {
    STYLE_EXPLANATIONS
        .iter()
        .find(|(k, _)| *k == key)
        .map(|(_, v)| *v)
        .unwrap_or("")
}

const CYCLE_INDUSTRIES: &[&str] = &[
    "煤炭", "钢铁", "有色金属", "化工", "石油石化", "建材", "水泥", "航运", "猪肉", "种植业",
    "造纸", "玻璃", "工程机械", "海运", "原油",
];

const GROWTH_INDUSTRIES: &[&str] = &[
    "半导体", "光学光电子", "电池", "光模块", "汽车整车", "医药生物", "医疗器械", "软件服务",
    "云计算", "AI", "数字经济", "信息技术", "新能源", "新材料", "生物医药", "创新药", "锂电池",
    "电动汽车", "5G",
];

const DEFENSIVE_INDUSTRIES: &[&str] = &[
    "银行", "保险", "电力", "燃气", "水务", "公用事业", "高速公路", "港口", "白酒", "食品饮料",
    "家电",
];

/// `STYLE_GROUP_WEIGHTS` — 8 styles × groups A..I.
fn group_weights(style: &str) -> &'static [(&'static str, f64)] {
    match style {
        WHITE_HORSE => &[
            ("A", 1.5),
            ("B", 0.7),
            ("C", 1.0),
            ("D", 0.8),
            ("E", 1.5),
            ("F", 0.3),
            ("G", 1.0),
            ("H", 0.8),
            ("I", 0.4),
        ],
        GROWTH_TECH => &[
            ("A", 0.7),
            ("B", 1.5),
            ("C", 0.8),
            ("D", 1.2),
            ("E", 0.7),
            ("F", 0.7),
            ("G", 1.0),
            ("H", 1.5),
            ("I", 1.5),
        ],
        CYCLE => &[
            ("A", 1.0),
            ("B", 0.5),
            ("C", 1.5),
            ("D", 1.0),
            ("E", 1.0),
            ("F", 1.0),
            ("G", 0.8),
            ("H", 0.6),
            ("I", 0.7),
        ],
        SMALL_SPECULATIVE => &[
            ("A", 0.4),
            ("B", 0.7),
            ("C", 0.5),
            ("D", 1.3),
            ("E", 0.5),
            ("F", 1.5),
            ("G", 0.7),
            ("H", 0.8),
            ("I", 1.3),
        ],
        DIVIDEND_DEFENSE => &[
            ("A", 1.5),
            ("B", 0.5),
            ("C", 1.0),
            ("D", 0.7),
            ("E", 1.3),
            ("F", 0.3),
            ("G", 1.0),
            ("H", 0.4),
            ("I", 0.2),
        ],
        DISTRESSED => &[
            ("A", 1.5),
            ("B", 0.4),
            ("C", 1.0),
            ("D", 0.7),
            ("E", 1.3),
            ("F", 0.5),
            ("G", 0.5),
            ("H", 0.5),
            ("I", 0.5),
        ],
        QUANT_FACTOR => &[
            ("A", 0.8),
            ("B", 0.8),
            ("C", 0.8),
            ("D", 1.0),
            ("E", 0.8),
            ("F", 0.7),
            ("G", 1.5),
            ("H", 0.8),
            ("I", 0.8),
        ],
        _ => &[
            ("A", 1.0),
            ("B", 1.0),
            ("C", 1.0),
            ("D", 1.0),
            ("E", 1.0),
            ("F", 1.0),
            ("G", 1.0),
            ("H", 1.0),
            ("I", 1.0),
        ],
    }
}

/// `STYLE_DIM_MULTIPLIERS`.
fn dim_multipliers(style: &str) -> &'static [(&'static str, f64)] {
    match style {
        WHITE_HORSE => &[
            ("1_financials", 1.5),
            ("10_valuation", 1.5),
            ("14_moat", 1.5),
            ("16_lhb", 0.3),
            ("17_sentiment", 0.5),
        ],
        GROWTH_TECH => &[
            ("7_industry", 1.5),
            ("14_moat", 1.3),
            ("1_financials", 0.8),
            ("10_valuation", 0.7),
        ],
        CYCLE => &[
            ("3_macro", 1.5),
            ("8_materials", 1.5),
            ("9_futures", 1.5),
        ],
        SMALL_SPECULATIVE => &[
            ("16_lhb", 1.5),
            ("17_sentiment", 1.5),
            ("2_kline", 1.3),
            ("12_capital_flow", 1.3),
            ("1_financials", 0.5),
            ("14_moat", 0.3),
        ],
        DIVIDEND_DEFENSE => &[
            ("1_financials", 1.3),
            ("11_governance", 1.3),
            ("16_lhb", 0.3),
            ("17_sentiment", 0.5),
        ],
        DISTRESSED => &[
            ("1_financials", 1.5),
            ("10_valuation", 1.5),
            ("11_governance", 1.5),
        ],
        QUANT_FACTOR => &[
            ("12_capital_flow", 1.5),
            ("2_kline", 1.3),
            ("16_lhb", 0.7),
        ],
        _ => &[],
    }
}

/// `PERSON_OVERRIDES` — (style, investor_id) → multiplier.
const PERSON_OVERRIDES: &[(&str, &str, f64)] = &[
    (WHITE_HORSE, "buffett", 1.5),
    (WHITE_HORSE, "duan", 1.4),
    (WHITE_HORSE, "munger", 1.3),
    (DISTRESSED, "klarman", 1.5),
    (DISTRESSED, "templeton", 1.4),
    (GROWTH_TECH, "wood", 1.5),
    (GROWTH_TECH, "thiel", 1.3),
    (GROWTH_TECH, "lynch", 1.2),
    (CYCLE, "soros", 1.4),
    (CYCLE, "dalio", 1.3),
    (SMALL_SPECULATIVE, "zhao_lg", 1.5),
    (SMALL_SPECULATIVE, "zhang_mz", 1.3),
    (QUANT_FACTOR, "simons", 1.5),
    (QUANT_FACTOR, "thorp", 1.3),
    (QUANT_FACTOR, "shaw", 1.3),
];

fn fv(v: &Value) -> f64 {
    f_fin(v, 0.0)
}

/// `a or b` for JSON values, defaulting to `Null`.
fn py_or<'a>(a: &'a Value, b: &'a Value) -> &'a Value {
    if uzi_core::py::truthy(a) {
        a
    } else {
        b
    }
}

/// Port of `stock_style.detect_style`.
///
/// The upstream quant branch delegates to `lib.quant_signal.detect_quant_signal`,
/// which asks akshare for fund holdings. This port is network-free, so
/// [`crate::quant_signal::detect_quant_signal`] reads the same
/// `.cache/_quant/<fund_code>/api_cache/top10_holdings*.json` files the upstream
/// run had already fetched.
pub fn detect_style(features: &Value, raw: &Value) -> String {
    let Value::Object(fmap) = features else {
        return BALANCED.to_string();
    };
    let raw = if raw.is_object() {
        raw
    } else {
        &Value::Object(Map::new())
    };
    let g = |k: &str| fmap.get(k).unwrap_or(&Value::Null);

    let pb = fv(g("pb"));
    let pe = fv(py_or(g("pe"), g("pe_ttm")));
    let roe_5y_min = fv(g("roe_5y_min"));
    let roe_5y_avg = fv(g("roe_5y_avg"));
    let mcap_yi = fv(g("market_cap_yi"));
    let rev_g = fv(py_or(g("revenue_growth_3y_cagr"), g("revenue_growth_latest")));
    let div_y = fv(g("dividend_yield"));
    let industry = g("industry").as_str().unwrap_or("").trim().to_string();
    let default_market = Value::from("A");
    let market = py_or(g("market"), &default_market);

    // 1. 困境反转
    if pb > 0.0 && pb < 1.0 && roe_5y_min < 5.0 {
        return DISTRESSED.to_string();
    }

    // 2. 量化因子型
    let default_ticker = Value::from(raw.get("ticker").and_then(|t| t.as_str()).unwrap_or(""));
    let code = py_or(g("code"), &default_ticker);
    let sig = crate::quant_signal::detect_quant_signal(code.as_str().unwrap_or(""), raw);
    if uzi_core::py::truthy(&sig["is_quant_factor_style"]) {
        return QUANT_FACTOR.to_string();
    }

    // 3. A 股小盘投机
    if market.as_str() == Some("A") && mcap_yi > 0.0 && mcap_yi < 100.0 {
        return SMALL_SPECULATIVE.to_string();
    }

    // 4. 周期股
    if CYCLE_INDUSTRIES.iter().any(|kw| industry.contains(kw)) {
        return CYCLE.to_string();
    }

    // 5. 高成长科技
    if rev_g > 20.0 && GROWTH_INDUSTRIES.iter().any(|kw| industry.contains(kw)) {
        return GROWTH_TECH.to_string();
    }

    // 6. 分红防御
    if div_y > 4.0 && DEFENSIVE_INDUSTRIES.iter().any(|kw| industry.contains(kw)) {
        return DIVIDEND_DEFENSE.to_string();
    }

    // 7. 白马价值
    if mcap_yi > 1000.0 && pe > 0.0 && pe < 25.0 && roe_5y_avg > 12.0 {
        return WHITE_HORSE.to_string();
    }

    BALANCED.to_string()
}

fn lookup<'a>(table: &'a [(&'static str, f64)], key: &str, default: f64) -> f64 {
    table
        .iter()
        .find(|(k, _)| *k == key)
        .map(|(_, v)| *v)
        .unwrap_or(default)
}

/// Port of `stock_style.apply_style_weights`.
pub fn apply_style_weights(panel_investors: &Value, dims_scored: &Value, style: &str) -> Value {
    let style = if STYLE_GROUP_WEIGHTS_HAS.contains(&style) {
        style
    } else {
        BALANCED
    };
    let group_w = group_weights(style);
    let dim_mults = dim_multipliers(style);

    // ── Panel weighted consensus ──
    let mut bullish_w = 0.0f64;
    let mut neutral_w = 0.0f64;
    let mut active_w = 0.0f64;
    let mut bullish_n = 0i64;
    let mut neutral_n = 0i64;
    let mut bearish_n = 0i64;
    if let Value::Array(investors) = panel_investors {
        for inv in investors {
            let sig = inv.get("signal").and_then(|s| s.as_str()).unwrap_or("neutral");
            if sig == "skip" {
                continue;
            }
            let gid = inv.get("group").and_then(|g| g.as_str()).unwrap_or("");
            let gw = lookup(group_w, gid, 1.0);
            let iid = inv
                .get("investor_id")
                .and_then(|s| s.as_str())
                .unwrap_or("");
            let pw = PERSON_OVERRIDES
                .iter()
                .find(|(s, i, _)| *s == style && *i == iid)
                .map(|(_, _, v)| *v)
                .unwrap_or(1.0);
            let w = gw * pw;
            active_w += w;
            if sig == "bullish" {
                bullish_w += w;
                bullish_n += 1;
            } else if sig == "neutral" {
                neutral_w += w * 0.6;
                neutral_n += 1;
            } else {
                bearish_n += 1;
            }
        }
    }

    let consensus = (bullish_w + neutral_w) / active_w.max(0.001) * 100.0;
    let active_n = bullish_n + neutral_n + bearish_n;
    let raw_consensus_old = bullish_n as f64 / (active_n.max(1)) as f64 * 100.0;

    // ── Fundamental weighted score ──
    let mut total_weighted = 0.0f64;
    let mut total_weight = 0.0f64;
    let mut total_weighted_old = 0.0f64;
    let mut total_weight_old = 0.0f64;
    if let Some(dims) = dims_scored.get("dimensions").and_then(|d| d.as_object()) {
        for (dim_key, d) in dims {
            if !d.is_object() {
                continue;
            }
            let score = fv(d.get("score").unwrap_or(&Value::Null));
            let base_w = fv(d.get("weight").unwrap_or(&Value::from(1)));
            let mult = lookup(dim_mults, dim_key, 1.0);
            let w = base_w * mult;
            total_weighted += score * w;
            total_weight += w;
            total_weighted_old += score * base_w;
            total_weight_old += base_w;
        }
    }

    let fund_score = if total_weight != 0.0 {
        total_weighted / total_weight * 10.0
    } else {
        0.0
    };
    let raw_fund_old = if total_weight_old != 0.0 {
        total_weighted_old / total_weight_old * 10.0
    } else {
        0.0
    };

    let mut diagnostics = Map::new();
    diagnostics.insert("active_weight".into(), Value::from(round(active_w, 2)));
    diagnostics.insert("bullish_weight".into(), Value::from(round(bullish_w, 2)));
    diagnostics.insert("neutral_weight".into(), Value::from(round(neutral_w, 2)));
    diagnostics.insert("active_count".into(), Value::from(active_n));
    diagnostics.insert("bullish_count".into(), Value::from(bullish_n));
    diagnostics.insert("neutral_count".into(), Value::from(neutral_n));
    diagnostics.insert("bearish_count".into(), Value::from(bearish_n));
    diagnostics.insert(
        "raw_consensus_old".into(),
        Value::from(round(raw_consensus_old, 1)),
    );
    diagnostics.insert("raw_fund_old".into(), Value::from(round(raw_fund_old, 1)));

    let mut out = Map::new();
    out.insert("panel_consensus".into(), Value::from(round(consensus, 1)));
    out.insert("fundamental_score".into(), Value::from(round(fund_score, 1)));
    out.insert("style".into(), Value::from(style));
    out.insert("diagnostics".into(), Value::Object(diagnostics));
    Value::Object(out)
}

/// Membership mirror of `STYLE_GROUP_WEIGHTS` keys.
const STYLE_GROUP_WEIGHTS_HAS: &[&str] = &[
    WHITE_HORSE,
    GROWTH_TECH,
    CYCLE,
    SMALL_SPECULATIVE,
    DIVIDEND_DEFENSE,
    DISTRESSED,
    QUANT_FACTOR,
    BALANCED,
];

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn unknown_style_falls_back_to_balanced_matrix() {
        let investors = json!([{"signal": "bullish", "group": "A", "investor_id": "x"}]);
        let dims = json!({"dimensions": {"1_financials": {"score": 8, "weight": 2}}});
        let a = apply_style_weights(&investors, &dims, "nonsense");
        let b = apply_style_weights(&investors, &dims, BALANCED);
        assert_eq!(a, b);
    }

    #[test]
    fn skip_signals_leave_consensus_at_zero() {
        let investors = json!([{"signal": "skip", "group": "A", "investor_id": "x"}]);
        let dims = json!({"dimensions": {}});
        let out = apply_style_weights(&investors, &dims, QUANT_FACTOR);
        assert_eq!(out["panel_consensus"], json!(0.0));
        assert_eq!(out["diagnostics"]["active_count"], json!(0));
    }

    #[test]
    fn neutral_counts_at_sixty_percent_weight() {
        // one neutral investor in group A, quant_factor → A=0.8, no override
        let investors = json!([{"signal": "neutral", "group": "A", "investor_id": "x"}]);
        let dims = json!({"dimensions": {}});
        let out = apply_style_weights(&investors, &dims, QUANT_FACTOR);
        assert_eq!(out["diagnostics"]["active_weight"], json!(0.8));
        assert_eq!(out["diagnostics"]["neutral_weight"], json!(0.48));
        assert_eq!(out["panel_consensus"], json!(60.0));
    }
}
