//! EastMoney HTTP endpoints used by `data_sources.py` and the akshare provider.
//!
//! These are the documented public endpoints that AkShare's
//! `stock_zh_a_hist` / `stock_individual_info_em` / `stock_financial_abstract` /
//! `stock_hsgt_*` wrappers call, and that `data_sources.py` also calls directly
//! in its own "直连 HTTP" fallback layers. Keeping them in one module lets the
//! provider chain and the data-source chain share the exact parsing.

use serde_json::{json, Map, Value};

use crate::http;

/// EastMoney's public `ut` token (hard-coded upstream).
pub const UT: &str = "fa5fd1943c7b386f172d6893dbfba10b";

/// `1.600519` for SH, `0.000001` for SZ.
pub fn secid(code6: &str, full: &str) -> String {
    if full.ends_with("SH") {
        format!("1.{code6}")
    } else {
        format!("0.{code6}")
    }
}

/// `data_sources._parse_em_direct_payload` — push2 single-stock payload.
pub fn parse_em_direct_payload(data: &Value) -> Value {
    let Some(obj) = data.as_object() else {
        return Value::Object(Map::new());
    };
    // `raw / scale if raw not in (None, "", "-") else None`
    let scaled = |field: &str, scale: f64| -> Option<f64> {
        match obj.get(field) {
            None => None,
            Some(v) => match v {
                Value::Null => None,
                Value::String(s) if s.is_empty() || s == "-" => None,
                _ => v.as_f64().map(|x| x / scale),
            },
        }
    };

    let mut out = Map::new();
    let price = scaled("f43", 100.0);
    let prev_close = scaled("f60", 100.0);
    let mut change_pct = scaled("f170", 100.0);
    if change_pct.is_none() {
        if let (Some(p), Some(pc)) = (price, prev_close) {
            if pc != 0.0 {
                change_pct = Some(uzi_core::py::round((p - pc) / pc * 100.0, 2));
            }
        }
    }
    if let Some(p) = price.filter(|p| *p != 0.0) {
        out.insert("price".into(), json!(p));
    }
    if let Some(pc) = prev_close.filter(|p| *p != 0.0) {
        out.insert("prev_close".into(), json!(pc));
    }
    if let Some(cp) = change_pct {
        out.insert("change_pct".into(), json!(uzi_core::py::round(cp, 2)));
    }
    if let Some(v) = obj.get("f47") {
        let empty = matches!(v, Value::Null)
            || matches!(v, Value::String(s) if s.is_empty() || s == "-");
        if !empty {
            out.insert("volume".into(), v.clone());
        }
    }
    if let Some(v) = obj.get("f162").and_then(|v| v.as_f64()).filter(|v| *v != 0.0) {
        out.insert("pe_ttm".into(), json!(v / 100.0));
    }
    if let Some(v) = obj.get("f167").and_then(|v| v.as_f64()).filter(|v| *v != 0.0) {
        out.insert("pb".into(), json!(v / 100.0));
    }
    if let Some(v) = obj.get("f116").and_then(|v| v.as_f64()).filter(|v| *v != 0.0) {
        out.insert(
            "market_cap".into(),
            json!(format!("{}亿", uzi_core::py::round(v / 1e8, 1))),
        );
        out.insert("market_cap_raw".into(), json!(v));
    }
    if let Some(v) = obj.get("f117").and_then(|v| v.as_f64()).filter(|v| *v != 0.0) {
        out.insert(
            "circulating_cap".into(),
            json!(format!("{}亿", uzi_core::py::round(v / 1e8, 1))),
        );
        out.insert("circulating_cap_raw".into(), json!(v));
    }
    Value::Object(out)
}

/// `requests.get(push2.eastmoney.com/api/qt/stock/get)` for one ticker.
pub fn push2_quote(code6: &str, full: &str, timeout: u64) -> Result<Value, String> {
    let url = "https://push2.eastmoney.com/api/qt/stock/get";
    let v = http::get_json_q(
        url,
        &[
            ("secid", &secid(code6, full)),
            (
                "fields",
                "f43,f44,f45,f46,f47,f48,f50,f57,f58,f60,f116,f117,f162,f164,f167,f170",
            ),
            ("ut", UT),
        ],
        &[],
        timeout,
    )?;
    let data = v.get("data").cloned().unwrap_or(Value::Null);
    if data.is_null() {
        return Err("empty push2 payload".to_string());
    }
    Ok(parse_em_direct_payload(&data))
}

/// `push2his.eastmoney.com/api/qt/stock/kline/get` — 东财 K 线 (中文列名).
pub fn kline(
    code6: &str,
    full: &str,
    klt: &str,
    fqt: &str,
    lmt: &str,
    timeout: u64,
) -> Result<Vec<Value>, String> {
    let url = "https://push2his.eastmoney.com/api/qt/stock/kline/get";
    let v = http::get_json_q(
        url,
        &[
            ("secid", &secid(code6, full)),
            ("ut", UT),
            ("fields1", "f1,f2,f3,f4,f5,f6"),
            (
                "fields2",
                "f51,f52,f53,f54,f55,f56,f57,f58,f59,f60,f61",
            ),
            ("klt", klt),
            ("fqt", fqt),
            ("lmt", lmt),
        ],
        &[],
        timeout,
    )?;
    let klines = v
        .get("data")
        .and_then(|d| d.get("klines"))
        .and_then(|k| k.as_array())
        .cloned()
        .unwrap_or_default();
    let mut rows = Vec::new();
    for line in klines {
        let Some(s) = line.as_str() else { continue };
        let parts: Vec<&str> = s.split(',').collect();
        if parts.len() >= 7 {
            rows.push(json!({
                "日期": parts[0],
                "开盘": parts[1].parse::<f64>().unwrap_or(0.0),
                "收盘": parts[2].parse::<f64>().unwrap_or(0.0),
                "最高": parts[3].parse::<f64>().unwrap_or(0.0),
                "最低": parts[4].parse::<f64>().unwrap_or(0.0),
                "成交量": parts[5].parse::<f64>().unwrap_or(0.0),
                "成交额": parts[6].parse::<f64>().unwrap_or(0.0),
            }));
        }
    }
    if rows.is_empty() {
        return Err("empty kline".to_string());
    }
    Ok(rows)
}

/// `datacenter.eastmoney.com` F10 主要财务指标 — the endpoint behind
/// `ak.stock_financial_abstract`.
pub fn financial_abstract(code6: &str, full: &str, timeout: u64) -> Result<Vec<Value>, String> {
    let secucode = format!("{code6}.{}", if full.ends_with("SH") { "SH" } else { "SZ" });
    let url = "https://datacenter.eastmoney.com/securities/api/data/get";
    let v = http::get_json_q(
        url,
        &[
            ("type", "RPT_F10_FINANCE_MAINFINADATA"),
            ("sty", "APP_F10_MAINFINADATA"),
            ("quoteColumns", ""),
            ("filter", &format!("(SECUCODE=\"{secucode}\")")),
            ("p", "1"),
            ("ps", "20"),
            ("sr", "-1"),
            ("st", "REPORT_DATE"),
            ("source", "HSF10"),
            ("client", "PC"),
        ],
        &[],
        timeout,
    )?;
    let rows = v
        .get("result")
        .and_then(|r| r.get("data"))
        .and_then(|d| d.as_array())
        .cloned()
        .unwrap_or_default();
    if rows.is_empty() {
        return Err("empty financial abstract".to_string());
    }
    Ok(rows)
}

/// Northbound (沪股通/深股通) holdings — `RPT_MUTUAL_HOLDSTOCKNDATE_STA`.
pub fn hsgt_hold(code6: &str, timeout: u64) -> Result<Vec<Value>, String> {
    if code6.len() != 6 || !code6.chars().all(|c| c.is_ascii_digit()) {
        return Err("invalid code".to_string());
    }
    let url = "https://datacenter-web.eastmoney.com/api/data/v1/get";
    let filter = format!("(SECURITY_CODE=\"{code6}\")(INTERVAL_TYPE=\"1\")");
    let v = http::get_json_q(
        url,
        &[
            ("sortColumns", "TRADE_DATE"),
            ("sortTypes", "-1"),
            ("pageSize", "500"),
            ("pageNumber", "1"),
            ("reportName", "RPT_MUTUAL_HOLDSTOCKNDATE_STA"),
            ("columns", "ALL"),
            ("source", "WEB"),
            ("client", "WEB"),
            ("filter", &filter),
        ],
        &[(
            "Referer",
            &format!("https://data.eastmoney.com/hsgt/StockHdStatistics/{code6}.html"),
        )],
        timeout,
    )?;
    Ok(v.get("result")
        .and_then(|r| r.get("data"))
        .and_then(|d| d.as_array())
        .cloned()
        .unwrap_or_default())
}

/// Sina daily K-line JSON (`money.finance.sina.com.cn/.../getKLineData`).
pub fn sina_kline(symbol: &str, datalen: &str, timeout: u64) -> Result<Vec<Value>, String> {
    let url = "https://money.finance.sina.com.cn/quotes_service/api/json_v2.php/CN_MarketData.getKLineData";
    let v = http::get_json_q(
        url,
        &[
            ("symbol", symbol),
            ("scale", "240"),
            ("ma", "no"),
            ("datalen", datalen),
        ],
        &[],
        timeout,
    )?;
    let arr = v.as_array().cloned().unwrap_or_default();
    let mut rows = Vec::new();
    for d in arr {
        rows.push(json!({
            "日期": d.get("day").cloned().unwrap_or(Value::Null),
            "开盘": strnum(d.get("open")),
            "最高": strnum(d.get("high")),
            "最低": strnum(d.get("low")),
            "收盘": strnum(d.get("close")),
            "成交量": strnum(d.get("volume")),
        }));
    }
    if rows.is_empty() {
        return Err("empty sina kline".to_string());
    }
    Ok(rows)
}

/// Tencent ifzq daily K-line (`web.ifzq.gtimg.cn/appstock/app/fqkline/get`).
pub fn tencent_kline(symbol: &str, timeout: u64) -> Result<Vec<Value>, String> {
    let url = "https://web.ifzq.gtimg.cn/appstock/app/fqkline/get";
    let v = http::get_json_q(
        url,
        &[("param", &format!("{symbol},day,,,500,qfq"))],
        &[],
        timeout,
    )?;
    let payload = v.get("data").and_then(|d| d.get(symbol));
    let klines = payload
        .and_then(|p| p.get("qfqday").or_else(|| p.get("day")))
        .and_then(|k| k.as_array())
        .cloned()
        .unwrap_or_default();
    let mut rows = Vec::new();
    for line in klines {
        let Some(arr) = line.as_array() else { continue };
        if arr.len() >= 6 {
            rows.push(json!({
                "日期": arr[0],
                "开盘": strnum(Some(&arr[1])),
                "收盘": strnum(Some(&arr[2])),
                "最高": strnum(Some(&arr[3])),
                "最低": strnum(Some(&arr[4])),
                "成交量": strnum(Some(&arr[5])),
            }));
        }
    }
    if rows.is_empty() {
        return Err("empty tencent kline".to_string());
    }
    Ok(rows)
}

/// Yahoo Chart v8 (`query1.finance.yahoo.com/v8/finance/chart/{sym}`), normalised
/// to EastMoney's Chinese column names.
pub fn yahoo_chart(symbol: &str, range_: &str, timeout: u64) -> Vec<Value> {
    let url = format!(
        "https://query1.finance.yahoo.com/v8/finance/chart/{symbol}?interval=1d&range={range_}"
    );
    let headers: &[(&str, &str)] = &[
        (
            "Accept",
            "application/json,text/plain,*/*",
        ),
        ("Referer", "https://finance.yahoo.com/"),
    ];
    let mut resp = match http::get(&url, headers, timeout) {
        Ok(r) => r,
        Err(_) => return Vec::new(),
    };
    if resp.status == 429 {
        std::thread::sleep(std::time::Duration::from_secs(2));
        resp = match http::get(&url, headers, timeout) {
            Ok(r) => r,
            Err(_) => return Vec::new(),
        };
    }
    if !resp.is_ok() {
        return Vec::new();
    }
    let Some(v) = resp.json() else { return Vec::new() };
    let result = v
        .get("chart")
        .and_then(|c| c.get("result"))
        .and_then(|r| r.as_array())
        .cloned()
        .unwrap_or_default();
    let Some(res0) = result.first() else {
        return Vec::new();
    };
    let ts = res0
        .get("timestamp")
        .and_then(|t| t.as_array())
        .cloned()
        .unwrap_or_default();
    let quote = res0
        .get("indicators")
        .and_then(|i| i.get("quote"))
        .and_then(|q| q.as_array())
        .and_then(|q| q.first())
        .cloned()
        .unwrap_or(Value::Null);
    let col = |name: &str| -> Vec<Value> {
        quote
            .get(name)
            .and_then(|c| c.as_array())
            .cloned()
            .unwrap_or_default()
    };
    let opens = col("open");
    let closes = col("close");
    let highs = col("high");
    let lows = col("low");
    let vols = col("volume");
    let mut rows = Vec::new();
    for (i, ts_v) in ts.iter().enumerate() {
        let Some(close) = closes.get(i).and_then(|c| c.as_f64()) else {
            continue;
        };
        let pick = |arr: &[Value]| -> f64 {
            arr.get(i).and_then(|v| v.as_f64()).unwrap_or(close)
        };
        let vol = vols.get(i).and_then(|v| v.as_f64()).unwrap_or(0.0);
        let date = ts_v
            .as_i64()
            .and_then(|t| chrono::DateTime::from_timestamp(t, 0))
            .map(|d| d.format("%Y-%m-%d").to_string())
            .unwrap_or_default();
        rows.push(json!({
            "日期": date,
            "开盘": pick(&opens),
            "收盘": close,
            "最高": pick(&highs),
            "最低": pick(&lows),
            "成交量": vol,
        }));
    }
    rows
}

/// Stooq CSV fallback for US tickers.
pub fn stooq_daily(symbol: &str, timeout: u64) -> Vec<Value> {
    let url = format!("https://stooq.com/q/d/l/?s={}.us&i=d", symbol.to_lowercase());
    let Ok(resp) = http::get_plain(&url, timeout) else {
        return Vec::new();
    };
    let text = resp.text();
    let mut lines = text.lines();
    let Some(_header) = lines.next() else {
        return Vec::new();
    };
    let mut rows = Vec::new();
    for line in lines {
        let parts: Vec<&str> = line.split(',').collect();
        if parts.len() >= 6 {
            rows.push(json!({
                "Date": parts[0],
                "Open": parts[1].parse::<f64>().unwrap_or(0.0),
                "High": parts[2].parse::<f64>().unwrap_or(0.0),
                "Low": parts[3].parse::<f64>().unwrap_or(0.0),
                "Close": parts[4].parse::<f64>().unwrap_or(0.0),
                "Volume": parts[5].parse::<f64>().unwrap_or(0.0),
            }));
        }
    }
    rows
}

/// `float(value or 0)` on a JSON scalar.
fn strnum(v: Option<&Value>) -> f64 {
    match v {
        None | Some(Value::Null) => 0.0,
        Some(Value::String(s)) => s.parse::<f64>().unwrap_or(0.0),
        Some(other) => other.as_f64().unwrap_or(0.0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secid_prefix_by_exchange() {
        assert_eq!(secid("600519", "600519.SH"), "1.600519");
        assert_eq!(secid("002273", "002273.SZ"), "0.002273");
    }

    #[test]
    fn direct_payload_scaling() {
        // price 1500.00 stored as 150000; pct 1.25 as 125; mcap 210000000000
        let payload = json!({
            "f43": 150000, "f60": 148000, "f170": 135,
            "f162": 3420, "f167": 311, "f116": 210000000000.0, "f117": 200000000000.0
        });
        let out = parse_em_direct_payload(&payload);
        assert_eq!(out["price"], json!(1500.0));
        assert_eq!(out["prev_close"], json!(1480.0));
        assert_eq!(out["change_pct"], json!(1.35));
        assert_eq!(out["pe_ttm"], json!(34.2));
        assert_eq!(out["pb"], json!(3.11));
        assert_eq!(out["market_cap"], json!("2100亿"));
        assert_eq!(out["market_cap_raw"], json!(210000000000.0));
    }

    #[test]
    fn direct_payload_derives_change_pct_when_missing() {
        let payload = json!({"f43": 110.0, "f60": 100.0});
        let out = parse_em_direct_payload(&payload);
        assert_eq!(out["change_pct"], json!(10.0));
    }

    #[test]
    fn empty_payload_is_empty_object() {
        assert_eq!(parse_em_direct_payload(&json!({})), json!({}));
        assert_eq!(parse_em_direct_payload(&Value::Null), json!({}));
    }
}
