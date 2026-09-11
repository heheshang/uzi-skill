//! Port of `fetch_futures.py`.
//!
//! Contract prices come from the same documented Sina daily main-contract
//! endpoint as `fetch_materials` (`sina_main_daily`); when a leg cannot be
//! fetched the payload keeps upstream's `None` / `—` degradation instead of
//! inventing numbers.

use serde_json::{json, Value};

use uzi_core::py::{py_str, round, truthy};

use crate::fetch::materials::sina_main_daily;

/// `INDUSTRY_FUTURES` — industry → (contract label, sina contract).
const INDUSTRY_FUTURES: &[(&str, Option<&str>, Option<&str>)] = &[
    ("钢铁", Some("螺纹钢 RB"), Some("RB0")),
    ("建材", Some("玻璃 FG"), Some("FG0")),
    ("煤炭", Some("焦煤 JM"), Some("JM0")),
    ("有色金属", Some("沪铜 CU"), Some("CU0")),
    ("化工", Some("原油 SC"), Some("SC0")),
    ("农业", Some("豆粕 M"), Some("M0")),
    ("养殖业", Some("生猪 LH"), Some("LH0")),
    ("电池", Some("碳酸锂 LC"), Some("LC0")),
    ("工业金属", Some("沪铝 AL"), Some("AL0")),
    ("贵金属", Some("黄金 AU"), Some("AU0")),
    ("能源金属", Some("碳酸锂 LC"), Some("LC0")),
    ("小金属", Some("沪锡 SN"), Some("SN0")),
    ("煤炭开采", Some("焦煤 JM"), Some("JM0")),
    ("焦炭", Some("焦炭 J"), Some("J0")),
    ("油气开采", Some("原油 SC"), Some("SC0")),
    ("光学光电子", None, None),
    ("半导体", None, None),
    ("医药生物", None, None),
    ("白酒", None, None),
    ("银行", None, None),
    ("保险", None, None),
];

/// `_pull_price(code)` — last 60 daily closes; `{}` on any failure.
fn pull_price(code: &str) -> Value {
    let Some(rows) = sina_main_daily(code, 20) else {
        return json!({});
    };
    let tail = if rows.len() > 60 {
        &rows[rows.len() - 60..]
    } else {
        &rows[..]
    };
    let closes: Vec<f64> = tail
        .iter()
        .filter_map(|r| r.get("收盘价").and_then(|v| v.as_f64()))
        .filter(|v| *v > 0.0)
        .collect();
    if closes.len() < 2 {
        return json!({});
    }
    let first = closes[0];
    let last = closes[closes.len() - 1];
    let trend_pct = if first != 0.0 {
        (last - first) / first * 100.0
    } else {
        0.0
    };
    json!({
        "latest": round(last, 2),
        "trend_60d_pct": round(trend_pct, 1),
        "history_60d": closes.iter().map(|v| json!(round(*v, 2))).collect::<Vec<_>>(),
    })
}

/// `(name, code)` for the industry, exact then fuzzy (upstream `k[:2]` rule).
fn linked_contract(industry: &str) -> (Option<&'static str>, Option<&'static str>) {
    for entry in INDUSTRY_FUTURES {
        if entry.0 == industry {
            return (entry.1, entry.2);
        }
    }
    if industry.is_empty() {
        return (None, None);
    }
    let prefix: String = industry.chars().take(2).collect();
    for entry in INDUSTRY_FUTURES {
        let k2: String = entry.0.chars().take(2).collect();
        if entry.0.contains(&prefix) || industry.contains(&k2) {
            return (entry.1, entry.2);
        }
    }
    (None, None)
}

fn trend_label(price_data: &Value) -> String {
    match price_data.get("trend_60d_pct").and_then(|v| v.as_f64()) {
        Some(pct) => format!("60 日 {}{:.1}%", if pct >= 0.0 { "+" } else { "" }, pct),
        None => "—".to_string(),
    }
}

pub fn main(industry: &str, materials_detail: &Value) -> Result<Value, String> {
    // v3.9.4 · prefer the contracts already identified by 8_materials.
    if let Some(rows) = materials_detail.as_array() {
        if !rows.is_empty() {
            for m in rows {
                let code = m.get("code").map(py_str).unwrap_or_default().trim().to_string();
                let name = m.get("name").map(py_str).unwrap_or_default().trim().to_string();
                if code.is_empty() || name.is_empty() {
                    continue;
                }
                let price_data = pull_price(&code);
                let label = trend_label(&price_data);
                let latest = price_data
                    .get("latest")
                    .filter(|v| truthy(v))
                    .cloned()
                    .unwrap_or_else(|| m.get("latest_price").cloned().unwrap_or(Value::Null));
                return Ok(json!({
                    "data": {
                        "linked_contract": name,
                        "contract_code": code,
                        "latest_price": latest,
                        "contract_trend": label,
                        "price_history_60d": price_data.get("history_60d").cloned().unwrap_or_else(|| json!([])),
                        "source_note": "8_materials 识别的原材料期货",
                    },
                    "source": "akshare:futures_main_sina via materials",
                    "fallback": false,
                }));
            }
        }
    }

    let (name, code) = linked_contract(industry);
    let Some(code) = code else {
        return Ok(json!({
            "data": {
                "linked_contract": "无直接关联品种",
                "contract_trend": "—",
                "note": format!("{industry} 行业与期货市场无强相关品种"),
            },
            "source": "INDUSTRY_FUTURES mapping",
            "fallback": false,
        }));
    };

    let price_data = pull_price(code);
    let label = trend_label(&price_data);
    Ok(json!({
        "data": {
            "linked_contract": name,
            "contract_code": code,
            "latest_price": price_data.get("latest").cloned().unwrap_or(Value::Null),
            "contract_trend": label,
            "price_history_60d": price_data.get("history_60d").cloned().unwrap_or_else(|| json!([])),
        },
        "source": "akshare:futures_main_sina",
        "fallback": false,
    }))
}
