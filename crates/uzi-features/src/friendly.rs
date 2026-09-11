//! Port of `compute_friendly.py` — Tier-4 friendly layer (scenarios + exit triggers).

use serde_json::{Map, Value};
use std::sync::LazyLock;
use uzi_core::py::{f_fin, py_str, round, truthy};

use crate::stock_features::extract_features;

/// `compute_friendly._parse_pct` — `float(str(s).replace("%","").replace("+",""))`, else 0.0.
fn parse_pct(v: &Value) -> f64 {
    match v {
        Value::Number(n) => n.as_f64().unwrap_or(0.0),
        Value::String(s) => s
            .replace(['%', '+'], "")
            .trim()
            .parse::<f64>()
            .unwrap_or(0.0),
        _ => 0.0,
    }
}

/// `(raw["dimensions"][dim] or {}).get("data") or {}`.
fn dd<'a>(raw: &'a Value, dim: &str) -> &'a Map<String, Value> {
    static EMPTY: LazyLock<Map<String, Value>> = LazyLock::new(Map::new);
    match raw
        .get("dimensions")
        .and_then(|d| d.get(dim))
        .and_then(|e| e.get("data"))
    {
        Some(Value::Object(o)) => o,
        _ => &EMPTY,
    }
}

/// Port of `compute_friendly.compute_scenarios`.
///
/// `dims_scored` is accepted for signature parity (upstream ignores it).
pub fn compute_scenarios(raw: &Value, dims_scored: &Value) -> Value {
    let _ = dims_scored;
    let basic = dd(raw, "0_basic");
    let kline = dd(raw, "2_kline");
    let research = dd(raw, "6_research");

    let entry_price = match basic.get("price") {
        Some(v) if truthy(v) => v.clone(),
        _ => Value::from(0),
    };

    let stats = match kline.get("kline_stats") {
        Some(Value::Object(o)) => o,
        _ => {
            static EMPTY: LazyLock<Map<String, Value>> = LazyLock::new(Map::new);
            &EMPTY
        }
    };
    let vol_str = stats.get("volatility").cloned().unwrap_or(Value::from("30%"));
    let sigma = {
        let x = parse_pct(&vol_str);
        if x != 0.0 {
            x
        } else {
            30.0
        }
    };

    let upside_str = research
        .get("upside")
        .cloned()
        .unwrap_or(Value::from("+15%"));
    let base_return = {
        let x = parse_pct(&upside_str);
        if x != 0.0 {
            x
        } else {
            15.0
        }
    };

    let case = |name: &str, probability: &str, ret: f64| {
        let mut m = Map::new();
        m.insert("name".into(), Value::from(name));
        m.insert("probability".into(), Value::from(probability));
        m.insert("return".into(), Value::from(ret));
        Value::Object(m)
    };

    let mut out = Map::new();
    out.insert("entry_price".into(), entry_price);
    out.insert(
        "cases".into(),
        Value::Array(vec![
            case("最坏情况", "5%", round(-2.0 * sigma, 1)),
            case("偏差情况", "25%", round(-1.0 * sigma + base_return * 0.2, 1)),
            case("合理情况", "40%", round(base_return, 1)),
            case("乐观情况", "25%", round(1.0 * sigma + base_return * 0.5, 1)),
            case("极致乐观", "5%", round(2.0 * sigma + base_return, 1)),
        ]),
    );
    Value::Object(out)
}

/// Port of `compute_friendly.compute_exit_triggers` (returns a JSON array of strings).
///
/// `synthesis` is accepted for upstream signature parity (unused upstream).
pub fn compute_exit_triggers(raw: &Value, dims_scored: &Value, synthesis: &Value) -> Value {
    let _ = synthesis;
    let mut triggers: Vec<Value> = Vec::new();

    let basic = dd(raw, "0_basic");
    let kline = dd(raw, "2_kline");
    let val = dd(raw, "10_valuation");
    let lhb = dd(raw, "16_lhb");
    let research = dd(raw, "6_research");
    let market = raw.get("market").and_then(|m| m.as_str()).unwrap_or("A");
    let cur = match market {
        "H" => "HK$",
        "U" => "$",
        _ => "¥",
    };

    let price = match basic.get("price") {
        Some(v) if truthy(v) => f_fin(v, 0.0),
        _ => 0.0,
    };

    // 1. 技术止损
    let ma60 = match kline.get("ma60_60d") {
        Some(Value::Array(a)) => a.clone(),
        _ => Vec::new(),
    };
    let ma60_last = ma60
        .iter()
        .rev()
        .find(|v| truthy(v))
        .map(|v| f_fin(v, 0.0));
    match ma60_last {
        Some(m) if price != 0.0 && m < price => triggers.push(Value::from(format!(
            "股价跌破 {}{:.2}（60 日均线支撑位）→ 无条件止损",
            cur, m
        ))),
        _ if price != 0.0 => triggers.push(Value::from(format!(
            "股价跌破 {}{:.2}（当前价 -12%）→ 无条件止损",
            cur,
            price * 0.88
        ))),
        _ => triggers.push(Value::from("股价放量跌破 60 日均线 → 无条件止损")),
    }

    // 2. 基本面恶化 — 用财报增速
    let feat = extract_features(raw, dims_scored);
    let rev_g = feat.get("revenue_growth_latest").cloned().unwrap_or(Value::Null);
    let rev_g = if truthy(&rev_g) { f_fin(&rev_g, 0.0) } else { 0.0 };
    if rev_g < 0.0 {
        triggers.push(Value::from(format!(
            "营收同比已转负（-{:.1}%）→ 基本面反转信号",
            rev_g.abs()
        )));
    } else {
        triggers.push(Value::from("下季度营收同比转负 → 基本面反转信号"));
    }

    // 3. 业绩不达
    let growth_str = research.get("upside").cloned().unwrap_or(Value::from("+15%"));
    let g = parse_pct(&growth_str);
    if g > 0.0 {
        let min_growth = std::cmp::max(10, (g - 15.0) as i64);
        triggers.push(Value::from(format!(
            "下次业绩预告低于 +{}% → 预期管理失守",
            min_growth
        )));
    } else {
        triggers.push(Value::from("连续两期业绩不及券商预期中位数 → 逻辑失效"));
    }

    // 4. 游资撤离
    let matched = lhb.get("matched_youzi").cloned().unwrap_or(Value::Null);
    let matched_str = match &matched {
        Value::Array(a) => a.iter().take(2).map(py_str).collect::<Vec<_>>().join(" / "),
        other => {
            if truthy(other) {
                py_str(other).split('/').next().unwrap_or("").to_string()
            } else {
                "顶级游资".to_string()
            }
        }
    };
    if !matched_str.is_empty() && matched_str != "—" {
        triggers.push(Value::from(format!(
            "{} 席位大额卖出 > 2 亿 → 顶级资金撤离信号",
            matched_str
        )));
    } else if !lhb.is_empty() {
        triggers.push(Value::from("龙虎榜游资席位出现大额净卖出 → 资金撤离信号"));
    }

    // 5. 估值泡沫
    let pe_quantile = val.get("pe_quantile").cloned().unwrap_or(Value::from(""));
    let q_str = py_str(&pe_quantile);
    match regex_quantile(&q_str) {
        Some(cur_q) => {
            if cur_q >= 80 {
                triggers.push(Value::from(format!(
                    "PE 已处于 5 年 {} 分位 → 泡沫区获利了结",
                    cur_q
                )));
            } else {
                let target = std::cmp::min(90, cur_q + 15);
                triggers.push(Value::from(format!(
                    "PE 站上 5 年 {} 分位 → 泡沫区获利了结",
                    target
                )));
            }
        }
        None => triggers.push(Value::from("PE 站上 5 年 90 分位 → 泡沫区获利了结")),
    }

    Value::Array(triggers.into_iter().take(5).collect())
}

/// Port of `compute_friendly.main(ticker)` — reads cached task outputs and builds
/// the `synthesis.friendly` block.
pub fn build_friendly(ticker: &str) -> Value {
    use uzi_core::cache::read_task_output;

    let raw = read_task_output(ticker, "raw_data").unwrap_or(Value::Null);
    let dimensions = read_task_output(ticker, "dimensions").unwrap_or(Value::Null);
    let synthesis = read_task_output(ticker, "synthesis").unwrap_or(Value::Null);

    let scenarios = compute_scenarios(&raw, &dimensions);
    let exit_triggers = compute_exit_triggers(&raw, &dimensions, &synthesis);

    let similar = match raw.get("similar_stocks") {
        Some(Value::Array(a)) => Value::Array(a.iter().take(4).cloned().collect()),
        _ => Value::Array(Vec::new()),
    };

    let mut out = Map::new();
    out.insert("scenarios".into(), scenarios);
    out.insert("exit_triggers".into(), exit_triggers);
    out.insert("similar_stocks".into(), similar);
    Value::Object(out)
}

/// `re.search(r"(\d+)\s*分位", s)` → first capture as int.
fn regex_quantile(s: &str) -> Option<i64> {
    static RE: LazyLock<regex::Regex> =
        LazyLock::new(|| regex::Regex::new(r"(\d+)\s*分位").unwrap());
    RE.captures(s)
        .and_then(|c| c.get(1))
        .and_then(|m| m.as_str().parse::<i64>().ok())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn scenarios_use_volatility_defaults_when_missing() {
        let raw = json!({"dimensions": {"0_basic": {"data": {"price": 10}}}});
        let out = compute_scenarios(&raw, &json!({}));
        assert_eq!(out["entry_price"], json!(10));
        // sigma defaults to 30, base_return to 15
        assert_eq!(out["cases"][0]["return"], json!(-60.0));
        assert_eq!(out["cases"][2]["return"], json!(15.0));
        assert_eq!(out["cases"][4]["return"], json!(75.0));
    }

    #[test]
    fn zero_volatility_falls_back_to_thirty() {
        let raw = json!({"dimensions": {"0_basic": {"data": {"price": 10}}
            , "2_kline": {"data": {"kline_stats": {"volatility": "0%"}}}}});
        let out = compute_scenarios(&raw, &json!({}));
        assert_eq!(out["cases"][0]["return"], json!(-60.0));
    }

    #[test]
    fn exit_triggers_pad_defaults_when_data_absent() {
        let raw = json!({"ticker": "AAPL", "market": "U",
            "dimensions": {"0_basic": {"data": {"price": 5}}}});
        let out = compute_exit_triggers(&raw, &json!({}), &json!({}));
        let arr = out.as_array().unwrap();
        // no lhb data, but matched defaults to 顶级游资 → still 5 triggers
        assert_eq!(arr.len(), 5);
        assert_eq!(arr[0], json!("股价跌破 $4.40（当前价 -12%）→ 无条件止损"));
        assert_eq!(arr[1], json!("下季度营收同比转负 → 基本面反转信号"));
        assert_eq!(
            arr[3],
            json!("顶级游资 席位大额卖出 > 2 亿 → 顶级资金撤离信号")
        );
        assert_eq!(arr[4], json!("PE 站上 5 年 90 分位 → 泡沫区获利了结"));
    }

    /// Differential check against upstream `compute_friendly` for the synthetic
    /// fixture (values captured from
    /// `python3 -c "from compute_friendly import ...` on `raw_data_synthetic.json`).
    #[test]
    fn synthetic_friendly_matches_upstream() {
        let repo_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let raw: Value = serde_json::from_str(
            &std::fs::read_to_string(
                repo_root.join("tools/golden/fixtures/raw_data_synthetic.json"),
            )
            .unwrap(),
        )
        .unwrap();
        let dims = raw["dimensions"].clone();

        let scenarios = compute_scenarios(&raw, &dims);
        assert_eq!(scenarios["entry_price"], json!(23.45));
        assert_eq!(scenarios["cases"][0]["return"], json!(-64.2));
        assert_eq!(scenarios["cases"][1]["return"], json!(-29.1));
        assert_eq!(scenarios["cases"][2]["return"], json!(15.0));
        assert_eq!(scenarios["cases"][3]["return"], json!(39.6));
        assert_eq!(scenarios["cases"][4]["return"], json!(79.2));

        let triggers = compute_exit_triggers(&raw, &dims, &json!({}));
        assert_eq!(
            triggers,
            json!([
                "股价跌破 ¥20.64（当前价 -12%）→ 无条件止损",
                "下季度营收同比转负 → 基本面反转信号",
                "下次业绩预告低于 +10% → 预期管理失守",
                "章盟主 / 赵老哥 席位大额卖出 > 2 亿 → 顶级资金撤离信号",
                "PE 站上 5 年 57 分位 → 泡沫区获利了结"
            ])
        );
    }
}
