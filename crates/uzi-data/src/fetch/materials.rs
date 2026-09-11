//! Port of `fetch_materials.py`.
//!
//! `ak.futures_main_sina` is a documented Sina endpoint
//! (`stock2.finance.sina.com.cn/futures/api/jsonp.php/.../InnerFuturesNewService
//! .getDailyKLine`); the JSONP payload is parsed directly, so the price trends
//! are real (never fabricated). Missing contracts keep upstream's
//! `数据获取失败` / `无直接期货品种` degradation.

use serde_json::{json, Map, Value};

use uzi_core::py::round;
use uzi_core::ticker::parse_ticker;

/// `INDUSTRY_MATERIALS` — industry → [(name, sina contract)].
const INDUSTRY_MATERIALS: &[(&str, &[(&str, &str)])] = &[
    ("光学光电子", &[("玻璃", "FG0"), ("铜", "CU0")]),
    ("半导体", &[("铜", "CU0"), ("黄金", "AU0"), ("白银", "AG0")]),
    ("医药生物", &[("黄金", "AU0")]),
    ("电池", &[("碳酸锂", "LC0"), ("镍", "NI0"), ("钴", "—")]),
    ("钢铁", &[("铁矿石", "I0"), ("焦炭", "J0"), ("螺纹钢", "RB0")]),
    ("建材", &[("水泥", "—"), ("玻璃", "FG0"), ("沥青", "BU0")]),
    ("化工", &[("原油", "SC0"), ("聚丙烯", "PP0"), ("PVC", "V0"), ("甲醇", "MA0")]),
    ("白酒", &[("玉米", "C0"), ("大豆", "A0")]),
    ("养殖业", &[("豆粕", "M0"), ("玉米", "C0"), ("生猪", "LH0")]),
    ("农业", &[("大豆", "A0"), ("玉米", "C0"), ("棕榈油", "P0")]),
    ("工业金属", &[("沪铝", "AL0"), ("沪铜", "CU0"), ("沪锌", "ZN0")]),
    ("有色金属", &[("沪铝", "AL0"), ("沪铜", "CU0"), ("沪镍", "NI0")]),
    ("贵金属", &[("黄金", "AU0"), ("白银", "AG0")]),
    ("能源金属", &[("碳酸锂", "LC0"), ("镍", "NI0")]),
    ("小金属", &[("沪锡", "SN0"), ("沪铅", "PB0")]),
    ("煤炭开采", &[("焦煤", "JM0"), ("动力煤", "ZC0")]),
    ("焦炭", &[("焦炭", "J0"), ("焦煤", "JM0")]),
    ("油气开采", &[("原油", "SC0")]),
];

/// `ak.futures_main_sina(symbol)` — daily main-contract closes, chronological.
pub(crate) fn sina_main_daily(symbol: &str, timeout: u64) -> Option<Vec<Value>> {
    let trade_date = "20210817";
    let url = format!(
        "https://stock2.finance.sina.com.cn/futures/api/jsonp.php/var%20_{symbol}{trade_date}=/InnerFuturesNewService.getDailyKLine?symbol={symbol}&_={trade_date}"
    );
    let resp = crate::http::get_plain(&url, timeout).ok()?;
    if !resp.is_ok() {
        return None;
    }
    let text = resp.text();
    let start = text.find("([")? + 1;
    let end = text.rfind("])")? + 1;
    let parsed: Value = serde_json::from_str(&text[start..end]).ok()?;
    let rows = parsed.as_array()?;
    let mut out = Vec::with_capacity(rows.len());
    for r in rows {
        let date = r.get("d").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let close = match r.get("c") {
            Some(Value::Number(n)) => n.as_f64(),
            Some(Value::String(s)) => s.parse::<f64>().ok(),
            _ => None,
        };
        if let Some(close) = close {
            out.push(json!({"日期": date, "收盘价": close}));
        }
    }
    if out.is_empty() {
        None
    } else {
        Some(out)
    }
}

/// `_get_material_trend(sina_code)` — 12-month window; `{}` on any failure.
fn get_material_trend(sina_code: &str) -> Value {
    let Some(rows) = sina_main_daily(sina_code, 20) else {
        return json!({});
    };
    let tail = if rows.len() > 250 {
        &rows[rows.len() - 250..]
    } else {
        &rows[..]
    };
    let closes: Vec<f64> = tail
        .iter()
        .filter_map(|r| r.get("收盘价").and_then(|v| v.as_f64()))
        .filter(|v| *v > 0.0)
        .collect();
    if closes.is_empty() {
        return json!({});
    }
    let step = (closes.len() / 12).max(1);
    let downsampled: Vec<Value> = closes
        .iter()
        .step_by(step)
        .take(12)
        .map(|v| json!(round(*v, 2)))
        .collect();
    let first = closes[0];
    let last = closes[closes.len() - 1];
    let trend_pct = if first != 0.0 {
        (last - first) / first * 100.0
    } else {
        0.0
    };
    json!({
        "latest_price": round(last, 2),
        "price_history_12m": downsampled,
        "trend_pct_12m": round(trend_pct, 1),
        "trend_label": format!("12月 {}{:.1}%", if trend_pct >= 0.0 { "+" } else { "" }, trend_pct),
        "data_points": closes.len(),
    })
}

fn find_materials(industry: &str) -> &'static [(&'static str, &'static str)] {
    for entry in INDUSTRY_MATERIALS {
        if entry.0 == industry {
            return entry.1;
        }
    }
    if industry.is_empty() {
        return &[];
    }
    let prefix: String = industry.chars().take(2).collect();
    for entry in INDUSTRY_MATERIALS {
        let k2: String = entry.0.chars().take(2).collect();
        if entry.0.contains(&prefix) || industry.contains(&k2) {
            return entry.1;
        }
    }
    &[]
}

/// Resolve a ticker input through `fetch_basic`'s industry (else "综合").
fn resolve_industry(input: &str) -> String {
    let stripped = input.replace('.', "").replace("SZ", "").replace("SH", "");
    let is_ticker = !stripped.is_empty() && stripped.chars().all(|c| c.is_ascii_digit());
    if !is_ticker {
        return input.to_string();
    }
    let ti = parse_ticker(input);
    crate::sources::fetch_basic(&ti)
        .get("industry")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .unwrap_or("综合")
        .to_string()
}

fn material_entry(name: &str, code: &str, trend: &Value) -> (Map<String, Value>, Option<Vec<Value>>) {
    let mut m = Map::new();
    m.insert("name".into(), json!(name));
    if code == "—" {
        m.insert("note".into(), json!("无直接期货品种"));
        m.insert("trend".into(), json!("—"));
        return (m, None);
    }
    m.insert("code".into(), json!(code));
    if trend.is_object() && trend.get("error").is_none() && !trend.as_object().map(|o| o.is_empty()).unwrap_or(true) {
        if let Some(o) = trend.as_object() {
            for (k, v) in o {
                m.insert(k.clone(), v.clone());
            }
        }
        let hist = trend
            .get("price_history_12m")
            .and_then(|v| v.as_array())
            .cloned();
        (m, hist)
    } else {
        m.insert("note".into(), json!("数据获取失败"));
        (m, None)
    }
}

pub fn main(ticker_or_industry: &str) -> Result<Value, String> {
    let industry = resolve_industry(ticker_or_industry);
    let materials = find_materials(&industry);

    let mut material_data: Vec<Value> = Vec::new();
    let mut combined_history: Vec<Value> = Vec::new();
    for &(name, code) in materials.iter().take(3) {
        let trend = if code == "—" {
            json!({})
        } else {
            get_material_trend(code)
        };
        let (entry, history) = material_entry(name, code, &trend);
        if let Some(history) = history {
            if combined_history.is_empty() {
                combined_history = history;
            }
        }
        material_data.push(Value::Object(entry));
    }

    let core_names = if material_data.is_empty() {
        "—".to_string()
    } else {
        material_data
            .iter()
            .take(3)
            .map(|m| m.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string())
            .collect::<Vec<_>>()
            .join(" / ")
    };

    let valid_trends: Vec<f64> = material_data
        .iter()
        .filter_map(|m| m.get("trend_pct_12m").and_then(|v| v.as_f64()))
        .collect();
    let avg_trend = if valid_trends.is_empty() {
        0.0
    } else {
        valid_trends.iter().sum::<f64>() / valid_trends.len() as f64
    };
    let price_trend = if valid_trends.is_empty() {
        "—".to_string()
    } else {
        format!(
            "12月均 {}{:.1}%",
            if avg_trend >= 0.0 { "+" } else { "" },
            avg_trend
        )
    };

    let cost_share = if matches!(industry.as_str(), "钢铁" | "化工" | "建材") {
        "原材料约占 25-40%"
    } else {
        "—"
    };
    let fallback = material_data.is_empty();

    Ok(json!({
        "data": {
            "core_material": core_names,
            "price_trend": price_trend,
            "price_history_12m": combined_history,
            "materials_detail": material_data,
            "cost_share": cost_share,
            "import_dep": "—",
            "industry_resolved": industry,
        },
        "source": "akshare:futures_main_sina + INDUSTRY_MATERIALS mapping",
        "fallback": fallback,
    }))
}
