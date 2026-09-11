//! Port of `fetch_valuation.py`.
//!
//! Dimension 10 · 估值 (PE/PB/PEG/历史分位 + 简化 DCF + EV/EBITDA stub).
//!
//! The EastMoney/basic paths are real; the 百度股市通 PE/PB history and the
//! cninfo industry-PE table are AkShare-only and degrade exactly like upstream's
//! `except: pass` / empty branches. `main_safe` is the mini-racer-free entry the
//! pipeline uses when `UZI_DISABLE_MINI_RACER` is set.

use serde_json::{json, Map, Value};

use uzi_core::py::{num_str, py_str, round, truthy};
use uzi_core::ticker::parse_ticker;

fn as_f64(v: &Value) -> Option<f64> {
    match v {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.trim().replace(',', "").parse::<f64>().ok(),
        _ => None,
    }
}

fn nf(x: f64) -> Value {
    serde_json::Number::from_f64(x).map(Value::Number).unwrap_or(Value::Null)
}

fn is_missing(v: Option<&Value>) -> bool {
    match v {
        None | Some(Value::Null) => true,
        Some(Value::String(s)) => s.is_empty() || s == "—",
        _ => false,
    }
}

// ─────────────────────────────────────────────────────────────
// Simple DCF
// ─────────────────────────────────────────────────────────────

/// `simple_dcf(fcf_latest, growth_5y=0.10, growth_terminal=0.03, wacc=0.10,
/// years=10)` — 5+5 阶段永续增长 DCF.
pub fn simple_dcf(fcf_latest: f64, growth_5y: f64, growth_terminal: f64, wacc: f64, years: usize) -> Value {
    if fcf_latest <= 0.0 {
        return json!({"intrinsic_value": Value::Null, "_note": "negative FCF, DCF not applicable"});
    }
    let mut fcfs: Vec<f64> = Vec::with_capacity(years);
    let mut fcf = fcf_latest;
    for y in 1..=years {
        let g = if y <= 5 {
            growth_5y
        } else {
            (growth_5y + growth_terminal) / 2.0
        };
        fcf *= 1.0 + g;
        fcfs.push(fcf);
    }
    // Python `(1 + wacc) ** k` calls libm `pow`; `powf` reproduces it bit-for-bit.
    let pv_fcfs: f64 = fcfs
        .iter()
        .enumerate()
        .map(|(i, f)| f / (1.0 + wacc).powf(i as f64 + 1.0))
        .sum();
    let terminal_value = fcfs[fcfs.len() - 1] * (1.0 + growth_terminal) / (wacc - growth_terminal);
    let pv_terminal = terminal_value / (1.0 + wacc).powf(years as f64);
    json!({
        "intrinsic_value_total": nf(pv_fcfs + pv_terminal),
        "pv_fcfs": nf(pv_fcfs),
        "pv_terminal": nf(pv_terminal),
        "assumptions": {
            "fcf_latest": nf(fcf_latest),
            "growth_5y": nf(growth_5y),
            "growth_terminal": nf(growth_terminal),
            "wacc": nf(wacc),
        },
    })
}

/// `dcf_sensitivity_matrix(...) — intrinsic price across WACC × growth`.
pub fn dcf_sensitivity_matrix(
    fcf_latest: f64,
    waccs: &[i64],
    growths: &[i64],
    current_price: f64,
    shares_out: f64,
    years: usize,
) -> Value {
    let mut values: Vec<Value> = Vec::with_capacity(waccs.len());
    for &w in waccs {
        let mut row: Vec<Value> = Vec::with_capacity(growths.len());
        for &g in growths {
            let result = simple_dcf(fcf_latest, g as f64 / 100.0, 0.03, w as f64 / 100.0, years);
            let per_share = if shares_out != 0.0 {
                let iv = result.get("intrinsic_value_total").and_then(as_f64).unwrap_or(0.0);
                nf(round(iv / shares_out, 2))
            } else {
                // Python `round(0, 2)` on the int fallback stays an int.
                Value::from(0)
            };
            row.push(per_share);
        }
        values.push(Value::Array(row));
    }
    json!({
        "waccs": waccs,
        "growths": growths,
        "values": values,
        "current_price": nf(current_price),
    })
}

// ─────────────────────────────────────────────────────────────
// MX fallback
// ─────────────────────────────────────────────────────────────

/// `float(str(raw).replace("%","").replace("倍","").replace(",","").strip())`.
fn parse_pct_stripped(raw: &Value) -> Option<f64> {
    let s = py_str(raw);
    let cleaned: String = s.chars().filter(|c| *c != '%' && *c != '倍' && *c != ',').collect();
    let t = cleaned.trim();
    if t.is_empty() {
        return None;
    }
    t.parse::<f64>().ok()
}

/// `_mx_latest_pct(result, label_substr)` → `(value, window)`.
fn mx_latest_pct(result: &Value, label_substr: &str) -> (Option<f64>, Option<String>) {
    let Some(obj) = result.as_object() else {
        return (None, None);
    };
    if obj.contains_key("error") {
        return (None, None);
    }
    let data = result.get("data").cloned().unwrap_or_else(|| json!({}));
    let inner = match data.get("data") {
        Some(d) if d.is_object() => d.clone(),
        _ => data.clone(),
    };
    let search = inner.get("searchDataResultDTO").cloned().unwrap_or_else(|| json!({}));
    let dto_list = search
        .get("dataTableDTOList")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    for dto in dto_list {
        let Some(dto) = dto.as_object() else { continue };
        let table = dto
            .get("table")
            .filter(|v| truthy(v))
            .or_else(|| dto.get("rawTable").filter(|v| truthy(v)))
            .cloned()
            .unwrap_or_else(|| json!({}));
        let name_map = match dto.get("nameMap").cloned().unwrap_or_else(|| json!({})) {
            Value::Array(a) => Value::Object(
                a.iter()
                    .enumerate()
                    .map(|(i, v)| (i.to_string(), v.clone()))
                    .collect(),
            ),
            other => other,
        };
        let Some(table) = table.as_object() else { continue };
        for (key, values) in table {
            if key == "headName" {
                continue;
            }
            let Some(values) = values.as_array() else { continue };
            let label = name_map
                .get(key)
                .filter(|v| truthy(v))
                .map(py_str)
                .unwrap_or_else(|| key.clone());
            if !label.contains(label_substr) {
                continue;
            }
            for raw in values {
                let Some(value) = parse_pct_stripped(raw) else { continue };
                if value < 0.0 {
                    continue;
                }
                let window = if label.contains("5年") || label.contains("5 年") {
                    "5 年"
                } else if label.contains("3年") || label.contains("3 年") {
                    "3 年"
                } else if label.contains("上市以来") {
                    "上市以来"
                } else {
                    "历史"
                };
                return (Some(value), Some(window.to_string()));
            }
        }
    }
    (None, None)
}

/// `_fetch_valuation_via_mx(code, name_hint, basic)`.
pub fn fetch_valuation_via_mx(code: &str, name_hint: &str, basic: &Map<String, Value>) -> Value {
    let mut pe = basic.get("pe_ttm").filter(|v| !v.is_null()).cloned();
    let mut pb = basic.get("pb").filter(|v| !v.is_null()).cloned();
    let used_basic = pe.is_some() || pb.is_some();
    let mut used_mx = false;
    let mut pe_quantile: Option<f64> = None;
    let mut pe_window: Option<String> = None;
    let mut pb_quantile: Option<f64> = None;

    let trimmed = name_hint.trim();
    let label = if trimmed.is_empty() { code.to_string() } else { trimmed.to_string() };

    let client = crate::mx::MXClient::default();
    if client.available {
        if pe.is_none() || pb.is_none() {
            let snapshot = client.fetch_snapshot(&label);
            if let Some(snap) = snapshot.as_object() {
                if pe.is_none() {
                    for (key, raw) in snap {
                        if key.contains("市盈率") || key.to_uppercase().contains("PE") {
                            let s = py_str(raw);
                            let cleaned: String = s.chars().filter(|c| *c != '%' && *c != '倍').collect();
                            if let Ok(x) = cleaned.trim().parse::<f64>() {
                                pe = Some(json!(x));
                                used_mx = true;
                                break;
                            }
                        }
                    }
                }
                if pb.is_none() {
                    for (key, raw) in snap {
                        if key.contains("市净率") || key.to_uppercase() == "PB" {
                            let s = py_str(raw);
                            let cleaned: String = s.chars().filter(|c| *c != '%' && *c != '倍').collect();
                            if let Ok(x) = cleaned.trim().parse::<f64>() {
                                pb = Some(json!(x));
                                used_mx = true;
                                break;
                            }
                        }
                    }
                }
            }
        }

        let (mut q, mut w) = mx_latest_pct(&client.query(&format!("{label} 市盈率近五年分位数")), "市盈率");
        if q.is_none() {
            let (q2, w2) = mx_latest_pct(&client.query(&format!("{code} 市盈率历史百分位")), "市盈率");
            q = q2;
            w = w2;
        }
        pe_quantile = q;
        pe_window = w;

        let (mut q, _) = mx_latest_pct(&client.query(&format!("{label} 市净率近五年分位数")), "市净率");
        if q.is_none() {
            let (q2, _) = mx_latest_pct(&client.query(&format!("{code} 市净率PB历史百分位")), "市净率");
            q = q2;
        }
        pb_quantile = q;

        used_mx = used_mx || pe_quantile.is_some() || pb_quantile.is_some();
    }

    let source = if used_basic && used_mx {
        "basic+mx_api"
    } else if used_mx {
        "mx_api"
    } else {
        "basic"
    };
    let mut out = Map::new();
    out.insert("_valuation_source".into(), json!(source));
    if let Some(v) = &pe {
        out.insert("pe".into(), json!(num_str(v)));
    }
    if let Some(v) = &pb {
        out.insert("pb".into(), json!(num_str(v)));
    }
    if let Some(q) = pe_quantile {
        out.insert(
            "pe_quantile".into(),
            json!(format!("{} {:.0} 分位", pe_window.as_deref().unwrap_or("历史"), q)),
        );
    }
    if let Some(q) = pb_quantile {
        out.insert("pb_quantile".into(), json!(format!("{:.0}%", q)));
    }
    Value::Object(out)
}

/// Resolve the `name_hint` argument the way upstream does:
/// `getattr(ti, "raw", "") or basic.get("name") or ""`.
fn name_hint(ti: &uzi_core::ticker::TickerInfo, basic: &Map<String, Value>) -> String {
    if !ti.raw.trim().is_empty() {
        return ti.raw.clone();
    }
    basic
        .get("name")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .unwrap_or("")
        .to_string()
}

// ─────────────────────────────────────────────────────────────
// main_safe
// ─────────────────────────────────────────────────────────────

/// `main_safe(ticker)` — valuation without mini-racer-prone AkShare endpoints.
pub fn main_safe(ticker: &str) -> Result<Value, String> {
    let ti = parse_ticker(ticker);
    let basic = crate::sources::fetch_basic(&ti);
    let basic_obj = basic.as_object().cloned().unwrap_or_default();
    let hint = name_hint(&ti, &basic_obj);
    let mut data = fetch_valuation_via_mx(&ti.code, &hint, &basic_obj);
    let obj = match data.as_object_mut() {
        Some(o) => o,
        None => return Ok(json!({})),
    };
    for field in ["pe", "pb", "pe_quantile", "pb_quantile"] {
        if !obj.contains_key(field) {
            obj.insert(field.to_string(), json!("—"));
        }
    }
    let filled = ["pe", "pb", "pe_quantile", "pb_quantile"]
        .iter()
        .any(|f| !is_missing(obj.get(*f)));
    let source = obj
        .get("_valuation_source")
        .and_then(|v| v.as_str())
        .unwrap_or("basic")
        .to_string();
    Ok(json!({
        "ticker": ti.full,
        "data": data,
        "source": format!("{source} (mini_racer-safe)"),
        "fallback": !filled,
    }))
}

// ─────────────────────────────────────────────────────────────
// main
// ─────────────────────────────────────────────────────────────

/// `main(ticker)`.
pub fn main(ticker: &str) -> Result<Value, String> {
    let ti = parse_ticker(ticker);
    let basic = crate::sources::fetch_basic(&ti);
    let basic_obj = basic.as_object().cloned().unwrap_or_default();
    let pe_history: Vec<Value> = Vec::new();
    let pe_quantile_val: Option<f64> = None;
    let pb_quantile_val: Option<f64> = None;
    let industry_pe_avg: Option<f64> = None;
    let industry_pe_fallback_reason = String::new();
    // 百度股市通 PE/PB 历史 (stock_zh_valuation_baidu) and cninfo
    // 证监会行业 PE table (stock_industry_pe_ratio_cninfo) are AkShare-only; both
    // upstream branches are `except: pass`, so nothing is emitted for them.

    // 3. DCF 敏感度矩阵 — uses fetch_financials.main's net_profit_history.
    let mut dcf_result: Value = json!({});
    let mut dcf_sensitivity: Value = json!({});
    let fin_result = super::financials::main(&ti.full).unwrap_or(Value::Null);
    let net_profit_hist: Vec<Value> = fin_result
        .get("data")
        .and_then(|d| d.get("net_profit_history"))
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let net_profit_latest_yi = net_profit_hist.last().and_then(as_f64).unwrap_or(0.0);
    if net_profit_latest_yi > 0.0 {
        let net_profit_yuan = net_profit_latest_yi * 1e8;
        dcf_result = simple_dcf(net_profit_yuan * 0.8, 0.10, 0.03, 0.10, 10);
        let current_price = basic_obj
            .get("price")
            .filter(|v| truthy(v))
            .and_then(as_f64)
            .unwrap_or(0.0);
        let mut total_shares = basic_obj
            .get("total_shares")
            .filter(|v| truthy(v))
            .and_then(as_f64)
            .unwrap_or(0.0);
        if total_shares == 0.0 {
            let mcap_raw = basic_obj
                .get("market_cap_raw")
                .filter(|v| truthy(v))
                .and_then(as_f64)
                .unwrap_or(0.0);
            if current_price != 0.0 && mcap_raw != 0.0 {
                total_shares = mcap_raw / current_price;
            }
        }
        if total_shares != 0.0 {
            dcf_sensitivity = dcf_sensitivity_matrix(
                net_profit_yuan * 0.8,
                &[8, 9, 10, 11, 12],
                &[6, 8, 10, 12],
                current_price,
                total_shares,
                10,
            );
        } else {
            dcf_sensitivity = json!({"_note": "股本数据缺失 · 无法计算每股敏感性", "values": []});
        }
    }

    let cur_pe = basic_obj.get("pe_ttm").filter(|v| !v.is_null()).cloned();
    let iv_total = dcf_result.get("intrinsic_value_total").and_then(as_f64);
    let dcf_display = match iv_total {
        Some(v) if v != 0.0 => format!("¥{:.1}亿", v / 1e8),
        _ => "—".to_string(),
    };

    let mut data = Map::new();
    data.insert(
        "pe".into(),
        match &cur_pe {
            Some(v) => json!(num_str(v)),
            None => json!("—"),
        },
    );
    data.insert(
        "pb".into(),
        match basic_obj.get("pb").filter(|v| !v.is_null()) {
            Some(v) => json!(num_str(v)),
            None => json!("—"),
        },
    );
    data.insert(
        "pe_quantile".into(),
        match pe_quantile_val {
            Some(v) => json!(format!("5 年 {:.0} 分位", v)),
            None => json!("—"),
        },
    );
    data.insert(
        "pb_quantile".into(),
        match pb_quantile_val {
            Some(v) => json!(format!("{:.0}%", v)),
            None => json!("—"),
        },
    );
    data.insert(
        "industry_pe".into(),
        match industry_pe_avg {
            Some(v) if v != 0.0 => json!(num_str(&nf(v))),
            _ => json!("—"),
        },
    );
    data.insert("industry_pe_fallback_reason".into(), json!(industry_pe_fallback_reason));
    data.insert("dcf".into(), json!(dcf_display));
    data.insert("pe_history".into(), Value::Array(pe_history));
    data.insert("dcf_simple".into(), dcf_result);
    data.insert("dcf_sensitivity".into(), dcf_sensitivity);

    if ["pe", "pe_quantile", "pb_quantile"]
        .iter()
        .any(|f| is_missing(data.get(*f)))
    {
        let hint = name_hint(&ti, &basic_obj);
        let mx_data = fetch_valuation_via_mx(&ti.code, &hint, &basic_obj);
        for field in ["pe", "pb", "pe_quantile", "pb_quantile"] {
            if is_missing(data.get(field)) && !is_missing(mx_data.get(field)) {
                if let Some(v) = mx_data.get(field) {
                    data.insert(field.to_string(), v.clone());
                }
            }
        }
        if let Some(src) = mx_data.get("_valuation_source") {
            data.insert("_valuation_source".into(), src.clone());
        }
    }

    let mut source = "baidu:valuation + cninfo:industry_pe_ratio + simple_dcf".to_string();
    if data
        .get("_valuation_source")
        .and_then(|v| v.as_str())
        .map(|s| s.contains("mx_api"))
        .unwrap_or(false)
    {
        source.push_str("+mx_api");
    }

    Ok(json!({
        "ticker": ti.full,
        "data": Value::Object(data),
        "source": source,
        "fallback": false,
    }))
}
