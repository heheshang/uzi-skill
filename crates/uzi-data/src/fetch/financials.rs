//! Port of `fetch_financials.py`.
//!
//! Dimension 1 · 财报 — 产出 viz 需要的完整 shape.
//!
//! The base data comes from [`crate::sources::fetch_financials`] (EastMoney F10
//! `RPT_F10_FINANCE_MAINFINADATA`, the endpoint behind AkShare's
//! `stock_financial_abstract`). Upstream's second indicator source
//! (`stock_financial_analysis_indicator`, Sina) plus the balance-sheet /
//! cash-flow / dividend / baostock calls are Python-library-only: each degrades
//! to the failure key upstream records, and the metrics those sources carry
//! (加权ROE / 流动比率 / 资产负债率 / 总资产净利率 / 销售净利率 / 总资产周转率) are read
//! from the F10 fields that expose the same values. No number is invented.

use std::collections::BTreeMap;
use std::sync::LazyLock;

use serde_json::{json, Map, Value};

use uzi_core::py::{py_str, round, truthy};
use uzi_core::ticker::TickerInfo;

static YEAR_RE: LazyLock<regex::Regex> = LazyLock::new(|| regex::Regex::new(r"(20\d{2})").unwrap());

/// `_REVENUE_GROWTH_COLUMNS` — provenance label used for the reported YoY.
const REVENUE_GROWTH_COLUMNS: [&str; 4] = [
    "主营业务收入增长率(%)",
    "营业总收入同比增长(%)",
    "营业收入同比增长率(%)",
    "营业收入增长率(%)",
];

fn nf(x: f64) -> Value {
    serde_json::Number::from_f64(x).map(Value::Number).unwrap_or(Value::Null)
}

/// `_to_float(v)` — `float(str(v).replace(",","").replace("%",""))`, 0.0 on
/// missing/unparseable cells.
fn to_float(v: Option<&Value>) -> f64 {
    let Some(v) = v else { return 0.0 };
    match v {
        Value::Null => 0.0,
        Value::String(s) if s.is_empty() || s == "--" || s == "-" => 0.0,
        _ => {
            let s = py_str(v);
            let cleaned: String = s.chars().filter(|c| *c != ',' && *c != '%').collect();
            cleaned.trim().parse::<f64>().unwrap_or(0.0)
        }
    }
}

/// `_to_float_or_none(v)` — like [`to_float`] but preserves a legitimate zero
/// and treats non-finite results as missing.
fn to_float_or_none(v: Option<&Value>) -> Option<f64> {
    let Some(v) = v else { return None };
    match v {
        Value::Null => None,
        Value::String(s) if s.is_empty() || s == "--" || s == "-" => None,
        _ => {
            let s = py_str(v);
            let cleaned: String = s.chars().filter(|c| *c != ',' && *c != '%').collect();
            match cleaned.trim().parse::<f64>() {
                Ok(x) if x.is_finite() => Some(x),
                _ => None,
            }
        }
    }
}

/// `_to_yi(v)` — raw 元 → 亿.
fn to_yi(v: Option<&Value>) -> f64 {
    round(to_float(v) / 1e8, 2)
}

/// `str(row["REPORT_DATE"])`.
fn date_of(row: &Value) -> String {
    row.get("REPORT_DATE").map(py_str).unwrap_or_default()
}

/// `str(...)[:10]`.
fn date10(row: &Value) -> String {
    date_of(row).chars().take(10).collect()
}

/// `str(col).endswith("1231")`.
fn is_annual(row: &Value) -> bool {
    date_of(row).chars().take(10).collect::<String>().ends_with("-12-31")
}

// ─────────────────────────────────────────────────────────────
// Income-history helpers (upstream step 1 + step 5)
// ─────────────────────────────────────────────────────────────

/// `_drop_all_zero_histories(out)`.
fn drop_all_zero_histories(out: &mut Map<String, Value>) {
    for key in ["revenue_history", "net_profit_history"] {
        let values = out
            .get(key)
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        if !values.is_empty() && !values.iter().any(|v| to_float(Some(v)).abs() > 1e-9) {
            out.remove(key);
            let entry = out
                .entry("_zero_history_dropped")
                .or_insert_with(|| json!([]));
            if let Some(arr) = entry.as_array_mut() {
                arr.push(json!(key));
            }
        }
    }
}

/// `_apply_operating_cash_flow(out, df_cf)` — OCF (not true FCF), 亿 units.
///
/// `rows` are the cash-flow report rows; upstream reaches this only when
/// `ak.stock_cash_flow_sheet_by_report_em` responds, which is never in the Rust
/// port, so the empty slice leaves `out` untouched.
fn apply_operating_cash_flow(out: &mut Map<String, Value>, rows: &[Value]) {
    let mut ocf_history: Vec<f64> = rows
        .iter()
        .filter_map(|r| r.get("经营活动产生的现金流量净额"))
        .map(|v| to_yi(Some(v)))
        .filter(|v| *v != 0.0)
        .collect();
    if ocf_history.is_empty() {
        return;
    }

    let ocf_latest = ocf_history[0];
    out.insert("ocf".into(), json!(format!("{:.1}亿", ocf_latest)));
    out.insert("operating_cash_flow".into(), json!(format!("{:.1}亿", ocf_latest)));
    out.insert("operating_cash_flow_yi".into(), nf(round(ocf_latest, 2)));
    ocf_history.truncate(6);
    out.insert("ocf_history".into(), json!(ocf_history));

    let np_latest = out
        .get("net_profit_history")
        .and_then(|v| v.as_array())
        .and_then(|a| a.last())
        .map(|v| to_float(Some(v)))
        .unwrap_or(0.0);
    if np_latest != 0.0 {
        let ratio = round(ocf_latest / np_latest, 2);
        out.insert("ocf_to_net_income_ratio".into(), nf(ratio));
        let fh = out
            .entry("financial_health")
            .or_insert_with(|| json!({}));
        if let Some(obj) = fh.as_object_mut() {
            obj.insert("ocf_to_net_income_ratio".into(), nf(ratio));
            obj.insert("fcf_margin".into(), nf(round(ratio * 100.0, 1)));
        }
    }
}

// ─────────────────────────────────────────────────────────────
// MX fallbacks
// ─────────────────────────────────────────────────────────────

/// `_mx_search_table(result)` → `(table, name_map)`.
fn mx_search_table(result: &Value) -> (Map<String, Value>, Map<String, Value>) {
    let Some(obj) = result.as_object() else {
        return (Map::new(), Map::new());
    };
    if obj.get("error").map(truthy).unwrap_or(false) {
        return (Map::new(), Map::new());
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
    let Some(dto) = dto_list.first().and_then(|d| d.as_object()) else {
        return (Map::new(), Map::new());
    };
    let table = dto
        .get("rawTable")
        .filter(|v| truthy(v))
        .or_else(|| dto.get("table").filter(|v| truthy(v)))
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
    (
        table.as_object().cloned().unwrap_or_default(),
        name_map.as_object().cloned().unwrap_or_default(),
    )
}

/// `_parse_mx_roe_series(result)` — oldest-to-newest annual weighted-ROE series.
fn parse_mx_roe_series(result: &Value) -> Value {
    let (table, name_map) = mx_search_table(result);
    let heads = table
        .get("headName")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    let label_of = |key: &str| -> String {
        name_map
            .get(key)
            .filter(|v| truthy(v))
            .map(py_str)
            .unwrap_or_else(|| key.to_string())
    };

    let series_key = table.keys().find(|k| {
        k.as_str() != "headName" && {
            let label = label_of(k);
            label.to_uppercase().contains("ROE") || label.contains("净资产收益率")
        }
    });
    let series_key = series_key.or_else(|| {
        table
            .iter()
            .find(|(k, v)| k.as_str() != "headName" && v.as_array().map(|a| !a.is_empty()).unwrap_or(false))
            .map(|(k, _)| k)
    });
    let Some(series_key) = series_key else {
        return json!({});
    };

    let mut by_year: BTreeMap<String, f64> = BTreeMap::new();
    if let Some(series) = table.get(series_key).and_then(|v| v.as_array()) {
        for (index, raw) in series.iter().enumerate() {
            let head = heads.get(index).map(py_str).unwrap_or_default();
            if head.contains("季") || head.contains("中报") {
                continue;
            }
            let Some(value) = to_float_or_none(Some(raw)) else { continue };
            if let Some(m) = YEAR_RE.find(&head) {
                by_year.insert(m.as_str().to_string(), round(value, 2));
            }
        }
    }
    if by_year.is_empty() {
        return json!({});
    }
    let years: Vec<String> = by_year.keys().cloned().collect();
    let start = years.len().saturating_sub(6);
    let years: Vec<String> = years[start..].to_vec();
    let history: Vec<f64> = years.iter().map(|y| by_year[y]).collect();
    let roe = history.last().map(|v| format!("{:.1}%", v)).unwrap_or_else(|| "—".to_string());
    json!({
        "roe_history": history,
        "financial_years": years,
        "roe": roe,
    })
}

/// `_fetch_roe_history_via_mx(code, name_hint="")`.
fn fetch_roe_history_via_mx(code: &str, name_hint: &str) -> Value {
    let client = crate::mx::MXClient::default();
    if !client.available {
        return json!({});
    }
    let trimmed = name_hint.trim();
    let label = if trimmed.is_empty() { code.to_string() } else { trimmed.to_string() };
    for query in [
        format!("{label} 近五年加权净资产收益率"),
        format!("{code} 近五年加权净资产收益率ROE"),
        format!("{code} 历年年报净资产收益率ROE(加权)"),
    ] {
        let parsed = parse_mx_roe_series(&client.query(&query));
        if truthy(parsed.get("roe_history").unwrap_or(&Value::Null)) {
            if let Some(mut obj) = parsed.as_object().cloned() {
                obj.insert("_mx_roe_query".into(), json!(query));
                return Value::Object(obj);
            }
        }
    }
    json!({})
}

/// `_fetch_financial_health_via_mx(code, name_hint="")`.
fn fetch_financial_health_via_mx(code: &str, name_hint: &str) -> Value {
    let client = crate::mx::MXClient::default();
    if !client.available {
        return json!({});
    }
    let trimmed = name_hint.trim();
    let label = if trimmed.is_empty() { code.to_string() } else { trimmed.to_string() };
    let result = client.query(&format!("{label} 流动比率 资产负债率 总资产净利率 销售净利率"));
    let (table, name_map) = mx_search_table(&result);
    let heads: Vec<String> = table
        .get("headName")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default()
        .iter()
        .map(py_str)
        .collect();
    let annual_index = heads
        .iter()
        .position(|h| h.contains("年报") && !h.contains("季") && !h.contains("中报"))
        .unwrap_or(0);

    let mut health = Map::new();
    for (key, values) in &table {
        if key == "headName" {
            continue;
        }
        let Some(values) = values.as_array() else { continue };
        if values.is_empty() {
            continue;
        }
        let label_text = name_map
            .get(key)
            .filter(|v| truthy(v))
            .map(py_str)
            .unwrap_or_else(|| key.clone());
        let raw = values.get(annual_index).or_else(|| values.first());
        let Some(value) = to_float_or_none(raw) else { continue };
        if label_text.contains("流动比率") {
            health.insert("current_ratio".into(), nf(value));
        } else if label_text.contains("资产负债率") {
            health.insert("debt_ratio".into(), nf(value));
        } else if label_text.contains("总资产净利率") || label_text.to_uppercase().contains("ROA") {
            health.insert("roic".into(), nf(value));
        } else if label_text.contains("销售净利率")
            || (label_text.contains("净利率") && !label_text.contains("总资产"))
        {
            health.insert("net_margin_pct".into(), nf(value));
        }
    }
    Value::Object(health)
}

// ─────────────────────────────────────────────────────────────
// A-share
// ─────────────────────────────────────────────────────────────

/// `_fetch_a_share(ti)`.
pub fn fetch_a_share(ti: &TickerInfo) -> Value {
    let mut out = Map::new();
    let code = ti.code.clone();
    let name_hint = ti.raw.clone();

    // ─── 1+2. EastMoney F10 abstract carries both the income history upstream's
    // `stock_financial_abstract` provides and the ratios upstream's Sina
    // `stock_financial_analysis_indicator` provides.
    let fin = crate::sources::fetch_financials(ti);
    let rows: Vec<Value> = match fin.get("error").filter(|v| truthy(v)).cloned() {
        Some(err) => {
            out.insert("_abstract_error".into(), err);
            Vec::new()
        }
        None => fin
            .get("abstract")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default(),
    };

    if !rows.is_empty() {
        // ─── 1. 历年关键指标 — recent 6 annual periods, oldest → newest.
        let mut cols: Vec<&Value> = rows.iter().filter(|r| is_annual(r)).take(6).collect();
        cols.sort_by(|a, b| date_of(a).cmp(&date_of(b)));
        let revenue_history: Vec<f64> = cols.iter().map(|r| to_yi(r.get("TOTALOPERATEREVE"))).collect();
        let net_profit_history: Vec<f64> = cols.iter().map(|r| to_yi(r.get("PARENTNETPROFIT"))).collect();
        let financial_years: Vec<String> = cols.iter().map(|r| date_of(r).chars().take(4).collect()).collect();
        out.insert("revenue_history".into(), json!(revenue_history));
        out.insert("net_profit_history".into(), json!(net_profit_history));
        out.insert("financial_years".into(), json!(financial_years));
        drop_all_zero_histories(&mut out);

        // ─── 2. 指标序列 — ascending by report period.
        let mut ind: Vec<&Value> = rows.iter().collect();
        ind.sort_by(|a, b| date_of(a).cmp(&date_of(b)));
        let last = *ind.last().unwrap();
        let mut annual_ind: Vec<&Value> = ind.iter().cloned().filter(|r| is_annual(r)).collect();
        if annual_ind.is_empty() {
            annual_ind = ind.clone();
        }
        let last_annual = *annual_ind.last().unwrap();

        let start = annual_ind.len().saturating_sub(6);
        let roe_hist: Vec<f64> = annual_ind[start..]
            .iter()
            .map(|r| to_float(r.get("ROEJQ")))
            .collect();
        out.insert("roe_history".into(), json!(roe_hist));

        // Newest explicit report-period revenue YoY (annual or quarterly).
        let mut reported: Option<(f64, String)> = None;
        for r in ind.iter().rev() {
            if let Some(v) = to_float_or_none(r.get("TOTALOPERATEREVETZ")) {
                reported = Some((v, date10(r)));
                break;
            }
        }
        if let Some((v, period)) = reported {
            out.insert("revenue_growth_yoy".into(), nf(round(v, 2)));
            out.insert("revenue_growth".into(), json!(format!("{v:+.1}%")));
            out.insert("revenue_growth_period".into(), json!(period));
            out.insert("revenue_growth_basis".into(), json!("reported_yoy"));
            out.insert(
                "revenue_growth_source".into(),
                json!(format!(
                    "akshare.stock_financial_analysis_indicator:{}",
                    REVENUE_GROWTH_COLUMNS[0]
                )),
            );
        }

        // Financial health (current/debt from the latest period, annual ratios).
        let mut health = Map::new();
        for (src, dst, row) in [
            ("LD", "current_ratio", last),
            ("ZCFZL", "debt_ratio", last),
            ("ZZCJLL", "roic", last_annual),
            ("XSJLL", "net_margin_pct", last_annual),
        ] {
            let v = to_float(row.get(src));
            if v != 0.0 {
                health.insert(dst.to_string(), nf(v));
            }
        }
        if !health.is_empty() {
            out.insert("financial_health".into(), Value::Object(health));
        }

        // v3.9.4 · 资产负债表 — `stock_balance_sheet_by_report_em` is AkShare-only.
        out.insert(
            "_balance_sheet_error".into(),
            json!("ImportError: akshare not installed"),
        );

        out.insert("roe".into(), json!(format!("{:.1}%", to_float(last_annual.get("ROEJQ")))));
        out.insert("roe_mrq".into(), json!(format!("{:.1}%", to_float(last.get("ROEJQ")))));
        out.insert(
            "net_margin".into(),
            json!(format!("{:.1}%", to_float(last_annual.get("XSJLL")))),
        );
        out.insert("financial_period".into(), json!(date10(last_annual)));

        // v3.8.0 · DuPont 杜邦分解.
        let dp_nm = to_float(last_annual.get("XSJLL"));
        let dp_to = to_float(last_annual.get("TOAZZL"));
        let dp_dr = to_float(last_annual.get("ZCFZL"));
        let dp_em = if dp_dr != 0.0 && dp_dr < 100.0 {
            Some(100.0 / (100.0 - dp_dr))
        } else {
            None
        };
        if let Some(dp_em) = dp_em {
            let dp_roe = dp_nm * dp_to * dp_em;
            let quality = if dp_em >= 2.5 && dp_nm < 10.0 {
                "leverage_driven"
            } else if dp_nm >= 15.0 {
                "margin_driven"
            } else {
                "balanced"
            };
            out.insert(
                "dupont".into(),
                json!({
                    "net_margin_pct": nf(round(dp_nm, 2)),
                    "asset_turnover": nf(round(dp_to, 3)),
                    "equity_multiplier": nf(round(dp_em, 2)),
                    "roe_reconstructed_pct": nf(round(dp_roe, 2)),
                    "roe_quality": quality,
                }),
            );
        }
    }

    if !truthy(out.get("roe_history").unwrap_or(&Value::Null)) {
        let mx_roe = fetch_roe_history_via_mx(&code, &name_hint);
        if truthy(mx_roe.get("roe_history").unwrap_or(&Value::Null)) {
            if let Some(v) = mx_roe.get("roe_history") {
                out.insert("roe_history".into(), v.clone());
            }
            if truthy(mx_roe.get("financial_years").unwrap_or(&Value::Null))
                && !truthy(out.get("financial_years").unwrap_or(&Value::Null))
            {
                if let Some(v) = mx_roe.get("financial_years") {
                    out.insert("financial_years".into(), v.clone());
                }
            }
            if truthy(mx_roe.get("roe").unwrap_or(&Value::Null))
                && !truthy(out.get("roe").unwrap_or(&Value::Null))
            {
                if let Some(v) = mx_roe.get("roe") {
                    out.insert("roe".into(), v.clone());
                }
            }
            out.insert("_roe_source".into(), json!("mx_api"));
            out.remove("_indicator_error");
        }
    }

    if !truthy(out.get("financial_health").unwrap_or(&Value::Null)) {
        let mx_health = fetch_financial_health_via_mx(&code, &name_hint);
        if truthy(&mx_health) {
            let nm = mx_health.get("net_margin_pct").cloned();
            out.insert("financial_health".into(), mx_health);
            out.insert("_financial_health_source".into(), json!("mx_api"));
            if !truthy(out.get("net_margin").unwrap_or(&Value::Null)) {
                if let Some(nm) = nm {
                    out.insert("net_margin".into(), json!(format!("{:.1}%", to_float(Some(&nm)))));
                }
            }
        }
    }

    // ─── 3. 营收增速 summary (derived from the annual history).
    let rh = out
        .get("revenue_history")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    if !out.contains_key("revenue_growth_yoy") && rh.len() >= 2 {
        let prev = to_float(rh.get(rh.len() - 2));
        if prev != 0.0 {
            let last = to_float(rh.last());
            let growth = (last - prev) / prev * 100.0;
            out.insert("revenue_growth_yoy".into(), nf(round(growth, 2)));
            out.insert("revenue_growth".into(), json!(format!("{growth:+.1}%")));
            let period = out
                .get("financial_years")
                .and_then(|v| v.as_array())
                .and_then(|a| a.last())
                .map(py_str)
                .unwrap_or_else(|| "None".to_string());
            out.insert("revenue_growth_period".into(), json!(period));
            out.insert("revenue_growth_basis".into(), json!("annual_yoy"));
            out.insert("revenue_growth_source".into(), json!("derived:revenue_history"));
        }
    }

    // ─── 4. 现金流 — `stock_cash_flow_sheet_by_report_em` is AkShare-only.
    out.insert(
        "_cash_flow_error".into(),
        json!("ImportError: akshare not installed"),
    );
    apply_operating_cash_flow(&mut out, &[]);

    // ─── 5. 分红历史 — `stock_history_dividend_detail` is AkShare-only.
    out.insert(
        "_dividend_error".into(),
        json!("ImportError: akshare not installed"),
    );

    // v3.4.2 · BaoStock 兜底.
    let needs_fallback = !truthy(out.get("roe").unwrap_or(&Value::Null))
        && !truthy(out.get("revenue_history").unwrap_or(&Value::Null))
        && !truthy(out.get("net_margin").unwrap_or(&Value::Null));
    if needs_fallback {
        out.insert("_baostock_err".into(), json!("ImportError: baostock not installed"));
    }

    Value::Object(out)
}

// ─────────────────────────────────────────────────────────────
// HK / US
// ─────────────────────────────────────────────────────────────

/// `_fetch_hk(ti)` — `stock_financial_hk_analysis_indicator_em` is AkShare-only.
fn fetch_hk(_ti: &TickerInfo) -> Value {
    json!({"_hk_indicator_error": "ImportError: akshare not installed"})
}

/// `_fetch_us(ti)` — yfinance is a Python library; upstream returns `{}` on
/// `ImportError`.
fn fetch_us(_ti: &TickerInfo) -> Value {
    json!({})
}

// ─────────────────────────────────────────────────────────────
// main
// ─────────────────────────────────────────────────────────────

/// `main(ticker)`.
pub fn main(ticker: &str) -> Result<Value, String> {
    let ti = uzi_core::ticker::parse_ticker(ticker);
    let data = match ti.market.as_str() {
        "A" => fetch_a_share(&ti),
        "H" => fetch_hk(&ti),
        _ => fetch_us(&ti),
    };
    let source = match ti.market.as_str() {
        "A" => "akshare:stock_financial_abstract + indicator + cash_flow + dividend_detail",
        "H" => "akshare:stock_financial_hk_report_em",
        "U" => "yfinance:financials + quarterly_financials + balance_sheet + info",
        _ => "unknown",
    };
    let error: Option<String> = None;
    let fallback = !truthy(&data);
    Ok(json!({
        "ticker": ti.full,
        "data": data,
        "source": source,
        "fallback": fallback,
        "error": error,
    }))
}
