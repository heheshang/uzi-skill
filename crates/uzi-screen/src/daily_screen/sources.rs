//! Port of `lib/daily_screen/sources.py` — bounded public-data adapters; source
//! timestamps are never retrieval times.
//!
//! The A/H cross-section and industry batch are read through `uzi-data` (the
//! crate that owns those providers); the intraday quote/minute adapters below
//! are the ones upstream keeps inside the screening workflow.

use anyhow::{bail, Result};
use chrono::{DateTime, Utc};
use serde_json::{json, Map, Value};

use super::events::{evidence_time, iso_seconds, shanghai_offset};
use super::models::StockSnapshot;
use super::universe::number;
use crate::csvlite::parse_line;

const QUOTE: &str = "https://push2.eastmoney.com/api/qt/stock/get";
const MINUTES: &str = "https://push2his.eastmoney.com/api/qt/stock/trends2/get";
const UA: &str = "Mozilla/5.0";
/// Upstream `timeout=(3, 8)`; the read timeout is the effective bound.
const TIMEOUT_SECS: u64 = 8;
const MAX_RESPONSE_BYTES: usize = 2_000_000;

/// `CODE = re.compile(r"^(\d{6})\.(SH|SZ|BJ)$|^(\d{5})\.HK$")`.
fn is_code(code: &str) -> bool {
    let (raw, exchange) = match code.split_once('.') {
        Some(parts) => parts,
        None => return false,
    };
    if raw.contains('.') {
        return false;
    }
    let digits = raw.chars().all(|c| c.is_ascii_digit());
    match exchange {
        "SH" | "SZ" | "BJ" => digits && raw.chars().count() == 6,
        "HK" => digits && raw.chars().count() == 5,
        _ => false,
    }
}

/// `_code_parts(code)`.
fn code_parts(code: &str) -> Result<(String, String)> {
    if !is_code(code) {
        bail!("invalid A/H security code");
    }
    let (raw, exchange) = code.split_once('.').expect("validated above");
    Ok((raw.to_string(), exchange.to_string()))
}

fn num_str(text: &str) -> Option<f64> {
    number(&Value::String(text.to_string()))
}

/// `_response(url, params)`.
fn response(url: &str, params: &[(&str, &str)]) -> Result<Vec<u8>> {
    let full = uzi_data::http::with_query(url, params);
    let resp = uzi_data::http::get(&full, &[("User-Agent", UA)], TIMEOUT_SECS)
        .map_err(|e| anyhow::anyhow!("public-data request failed: {e}"))?;
    if resp.status != 200 || resp.body.len() > MAX_RESPONSE_BYTES {
        bail!("unexpected public-data response");
    }
    Ok(resp.body)
}

/// `_json(url, params)`.
fn json_response(url: &str, params: &[(&str, &str)]) -> Result<Value> {
    let body = response(url, params)?;
    let payload: Value = serde_json::from_slice(&body)
        .map_err(|_| anyhow::anyhow!("expected object response"))?;
    if !payload.is_object() {
        bail!("expected object response");
    }
    Ok(payload)
}

/// `fetch_industries(stocks)` — batch current-universe identifiers, cached for
/// one day.
///
/// The datacenter request itself lives in `uzi-data`; this keeps upstream's
/// coverage/health bookkeeping and one-day cache around it.
pub fn fetch_industries(stocks: &mut [StockSnapshot]) -> Result<Value> {
    let mut health = Map::new();
    let index: std::collections::HashMap<String, usize> = stocks
        .iter()
        .enumerate()
        .map(|(i, s)| (s.code.clone(), i))
        .collect();
    for (market, field) in [("A", "EM2016"), ("H", "BELONG_INDUSTRY")] {
        let mut codes: Vec<String> = stocks
            .iter()
            .filter(|s| s.market == market)
            .map(|s| s.code.clone())
            .collect();
        codes.sort();
        if codes.is_empty() {
            continue;
        }
        for code in &codes {
            code_parts(code)?;
        }
        let mut covered: std::collections::HashSet<String> = std::collections::HashSet::new();
        let mut errors: Vec<String> = Vec::new();
        for batch in codes.chunks(200) {
            let key = format!("industry:{}{}", market, batch.join(","));
            let codes_raw: Vec<String> = batch
                .iter()
                .map(|code| code.split('.').next().unwrap_or("").to_string())
                .collect();
            let fetched = uzi_core::cache::cached::<_, anyhow::Error>(
                "_daily_screen",
                &key,
                uzi_core::cache::TTL_QUARTERLY,
                || {
                    let map = uzi_data::sources::fetch_industry_batch(&codes_raw);
                    let rows: Vec<Value> = match map.as_object() {
                        Some(entries) if !entries.is_empty() => entries
                            .iter()
                            .map(|(code, industry)| {
                                let mut row = Map::new();
                                row.insert("SECUCODE".into(), Value::String(code.clone()));
                                row.insert(field.to_string(), industry.clone());
                                Value::Object(row)
                            })
                            .collect(),
                        _ => bail!("empty industry batch"),
                    };
                    let observed_at = chrono::Local::now()
                        .to_rfc3339_opts(chrono::SecondsFormat::Secs, false);
                    Ok(json!({ "rows": rows, "observed_at": observed_at }))
                },
            );
            match fetched {
                Err(err) => {
                    errors.push(error_name(&err.to_string()));
                    // Do not issue dozens of identical requests during a source outage.
                    break;
                }
                Ok(data) => {
                    let observed_at = data
                        .get("observed_at")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    if let Some(rows) = data.get("rows").and_then(|v| v.as_array()) {
                        for row in rows {
                            let code = row.get("SECUCODE").and_then(|v| v.as_str()).unwrap_or("");
                            let industry = uzi_core::py::py_str(uzi_core::py::get(row, field));
                            let industry = industry.trim().to_string();
                            let in_batch = batch.iter().any(|c| c == code);
                            if !in_batch
                                || industry.is_empty()
                                || matches!(industry.as_str(), "-" | "--" | "未分类")
                            {
                                continue;
                            }
                            if let Some(&idx) = index.get(code) {
                                stocks[idx].industry = industry;
                                let mut source = Map::new();
                                source.insert(
                                    "source".into(),
                                    Value::String("eastmoney:RPT_F10_BASIC_ORGINFO".into()),
                                );
                                source.insert("field".into(), Value::String(field.to_string()));
                                source.insert(
                                    "observed_at".into(),
                                    Value::String(observed_at.clone()),
                                );
                                source.insert(
                                    "classification".into(),
                                    Value::String("current_not_historical".into()),
                                );
                                stocks[idx]
                                    .extra
                                    .insert("industry_source".into(), Value::Object(source));
                                covered.insert(code.to_string());
                            }
                        }
                    }
                }
            }
        }
        let coverage = if codes.is_empty() {
            0.0
        } else {
            covered.len() as f64 / codes.len() as f64
        };
        for code in &codes {
            if let Some(&idx) = index.get(code) {
                stocks[idx]
                    .extra
                    .insert("industry_coverage".into(), Value::from(coverage));
            }
        }
        health.insert(
            market.to_string(),
            json!({
                "requested": codes.len(),
                "covered": covered.len(),
                "coverage": coverage,
                "errors": errors,
            }),
        );
    }
    Ok(Value::Object(health))
}

fn error_name(err: &str) -> String {
    let head = err.split(':').next().unwrap_or("").trim();
    if !head.is_empty() && head.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        head.to_string()
    } else {
        "Exception".to_string()
    }
}

/// `parse_tencent(text, code)`.
pub fn parse_tencent(text: &str, code: &str) -> Result<Value> {
    let (raw, exchange) = code_parts(code)?;
    let symbol = format!("{}{}", exchange.to_lowercase(), raw);
    let pattern = format!("v_{symbol}=\"([^\"\\r\\n]+)\";");
    let re = regex::Regex::new(&pattern)?;
    let Some(caps) = re.captures(text) else {
        bail!("Tencent security mismatch");
    };
    let payload = &caps[1];
    let fields: Vec<&str> = payload.split('~').collect();
    if fields.len() < 38 || fields.get(2) != Some(&raw.as_str()) {
        bail!("Tencent quote schema mismatch");
    }
    let stamp = fields[30];
    let fmt = if exchange != "HK" {
        "%Y%m%d%H%M%S"
    } else {
        "%Y/%m/%d %H:%M:%S"
    };
    let naive = chrono::NaiveDateTime::parse_from_str(stamp, fmt)
        .map_err(|_| anyhow::anyhow!("Tencent quote timestamp missing"))?;
    let at = super::events::naive_to_shanghai(naive);
    let get = |index: usize| -> Option<f64> {
        fields.get(index).and_then(|v| num_str(v))
    };
    let lot_multiplier = if exchange == "HK" { 1.0 } else { 100.0 };
    let amount = get(37).map(|a| a * if exchange == "HK" { 1.0 } else { 10000.0 });
    let volume = get(6).map(|v| v * lot_multiplier);
    let vwap = match (amount, volume) {
        (Some(a), Some(v)) if v > 0.0 => Some(a / v),
        _ => None,
    };
    let mut quote = Map::new();
    quote.insert("code".into(), Value::String(code.to_string()));
    quote.insert("source".into(), Value::String("tencent:qt".into()));
    quote.insert("quote_at".into(), Value::String(iso_seconds(&at)));
    quote.insert("price".into(), opt(get(3)));
    quote.insert("prev_close".into(), opt(get(4)));
    quote.insert("open_price".into(), opt(get(5)));
    quote.insert("change_pct".into(), opt(get(32)));
    quote.insert("high".into(), opt(get(33)));
    quote.insert("low".into(), opt(get(34)));
    quote.insert("amount".into(), opt(amount));
    quote.insert("volume_shares".into(), opt(volume));
    quote.insert("bid".into(), opt(get(9)));
    quote.insert("ask".into(), opt(get(19)));
    quote.insert("bid_size".into(), opt(get(10)));
    quote.insert("ask_size".into(), opt(get(20)));
    quote.insert(
        "book_size_unit".into(),
        Value::String(if exchange != "HK" {
            "provider_lots".into()
        } else {
            "provider_units".into()
        }),
    );
    quote.insert(
        "currency".into(),
        Value::String(if exchange == "HK" { "HKD".into() } else { "CNY".into() }),
    );
    quote.insert("vwap".into(), opt(vwap));
    Ok(Value::Object(quote))
}

fn opt(value: Option<f64>) -> Value {
    match value {
        Some(x) => Value::from(x),
        None => Value::Null,
    }
}

/// `fetch_quote(stock)`.
pub fn fetch_quote(stock: &StockSnapshot) -> (Value, Vec<String>) {
    let mut errors: Vec<String> = Vec::new();
    let mut partial = Value::Object(Map::new());
    let parts = match code_parts(&stock.code) {
        Ok(parts) => parts,
        Err(err) => return (partial, vec![format!("ValueError: {err}")]),
    };
    let (raw, exchange) = parts;
    let tencent_url = format!("https://qt.gtimg.cn/q={}{}", exchange.to_lowercase(), raw);
    match uzi_data::http::get(&tencent_url, &[("User-Agent", UA)], TIMEOUT_SECS) {
        Ok(resp) if resp.status == 200 && resp.body.len() <= MAX_RESPONSE_BYTES => {
            match parse_tencent(&resp.gbk_text(), &stock.code) {
                Ok(quote) => {
                    let complete = ["price", "bid", "ask", "ask_size", "bid_size"].iter().all(|f| {
                        matches!(quote.get(*f).and_then(|v| v.as_f64()), Some(x) if x > 0.0)
                    });
                    partial = quote.clone();
                    if complete {
                        return (quote, errors);
                    }
                    errors.push("tencent:book_missing".to_string());
                }
                Err(_) => errors.push("tencent:ValueError".to_string()),
            }
        }
        Ok(_) => errors.push("tencent:ValueError".to_string()),
        Err(_) => errors.push("tencent:ConnectionError".to_string()),
    }
    let secid = format!(
        "{}.{}",
        if exchange == "HK" {
            "116"
        } else if exchange == "SH" {
            "1"
        } else {
            "0"
        },
        raw
    );
    let fields = "f57,f58,f43,f44,f45,f46,f48,f60,f71,f86,f170,f19,f20,f39,f40";
    match json_response(QUOTE, &[("secid", &secid), ("fltt", "2"), ("invt", "2"), ("fields", fields)])
    {
        Ok(payload) => {
            let data = match payload.get("data") {
                Some(v) if v.is_object() => v.clone(),
                _ => Value::Object(Map::new()),
            };
            let identity_ok = uzi_core::py::py_str(uzi_core::py::get(&data, "f57")) == raw
                && matches!(
                    num_str(&uzi_core::py::py_str(uzi_core::py::get(&data, "f86"))),
                    Some(x) if x != 0.0
                );
            if !identity_ok {
                errors.push("eastmoney_quote:ValueError".to_string());
                return (partial, errors);
            }
            let mapped = [
                ("price", "f43"),
                ("prev_close", "f60"),
                ("open_price", "f46"),
                ("high", "f44"),
                ("low", "f45"),
                ("amount", "f48"),
                ("change_pct", "f170"),
                ("vwap", "f71"),
                ("bid", "f19"),
                ("bid_size", "f20"),
                ("ask", "f39"),
                ("ask_size", "f40"),
            ];
            let mut quote = Map::new();
            for (key, field_name) in mapped {
                quote.insert(key.into(), opt(data.get(field_name).and_then(number)));
            }
            let epoch = data
                .get("f86")
                .and_then(number)
                .unwrap_or(0.0);
            let quote_at = DateTime::<Utc>::from_timestamp(
                epoch.trunc() as i64,
                (epoch.fract() * 1e9) as u32,
            )
            .map(|dt| iso_seconds(&dt.with_timezone(&shanghai_offset())))
            .unwrap_or_default();
            quote.insert("code".into(), Value::String(stock.code.clone()));
            quote.insert("source".into(), Value::String("eastmoney:quote".into()));
            quote.insert(
                "currency".into(),
                Value::String(if exchange == "HK" { "HKD".into() } else { "CNY".into() }),
            );
            quote.insert("book_size_unit".into(), Value::String("provider_units".into()));
            quote.insert("quote_at".into(), Value::String(quote_at));
            (Value::Object(quote), errors)
        }
        Err(_) => {
            errors.push("eastmoney_quote:ValueError".to_string());
            (partial, errors)
        }
    }
}

/// `parse_minutes(payload, code, cutoff)`.
pub fn parse_minutes(payload: &Value, code: &str, cutoff: &str) -> Result<Vec<Value>> {
    let (raw, _) = code_parts(code)?;
    let data = match payload.get("data") {
        Some(v) if v.is_object() => v.clone(),
        _ => Value::Object(Map::new()),
    };
    if uzi_core::py::py_str(uzi_core::py::get(&data, "code")) != raw {
        bail!("minute security mismatch");
    }
    let boundary = evidence_time(&Value::String(cutoff.to_string()));
    let Some(boundary) = boundary else {
        bail!("minute cutoff missing");
    };
    let mut bars: Vec<(chrono::DateTime<chrono::FixedOffset>, Value)> = Vec::new();
    let trends = match data.get("trends").and_then(|v| v.as_array()) {
        Some(list) => list.clone(),
        None => Vec::new(),
    };
    for row in trends {
        let line = uzi_core::py::py_str(&row);
        let fields = parse_line(&line);
        if fields.len() != 8 {
            continue;
        }
        let stamp = evidence_time(&Value::String(fields[0].clone()));
        let close = num_str(&fields[2]);
        let amount = num_str(&fields[6]);
        let (Some(stamp), Some(close), Some(amount)) = (stamp, close, amount) else {
            continue;
        };
        if stamp.date_naive() != boundary.date_naive()
            || stamp > boundary
            || close <= 0.0
            || amount < 0.0
        {
            continue;
        }
        let mut bar = Map::new();
        bar.insert("at".into(), Value::String(iso_seconds(&stamp)));
        bar.insert("close".into(), Value::from(close));
        bar.insert("amount_local".into(), Value::from(amount));
        // Upstream keys a dict by timestamp: later duplicates overwrite, order is
        // by sorted timestamp.
        match bars.iter_mut().find(|(existing, _)| *existing == stamp) {
            Some((_, existing)) => *existing = Value::Object(bar),
            None => bars.push((stamp, Value::Object(bar))),
        }
    }
    bars.sort_by_key(|(stamp, _)| *stamp);
    Ok(bars.into_iter().map(|(_, bar)| bar).collect())
}

/// `parse_tencent_minutes(payload, code, cutoff)`.
pub fn parse_tencent_minutes(payload: &Value, code: &str, cutoff: &str) -> Result<Vec<Value>> {
    let (raw, exchange) = code_parts(code)?;
    let data = match payload
        .get("data")
        .and_then(|d| d.get(format!("{}{}", exchange.to_lowercase(), raw)))
        .and_then(|d| d.get("data"))
    {
        Some(v) if v.is_object() => v.clone(),
        _ => Value::Object(Map::new()),
    };
    let day_text = uzi_core::py::py_str(uzi_core::py::get(&data, "date"));
    let day = chrono::NaiveDate::parse_from_str(&day_text, "%Y%m%d")
        .map_err(|_| anyhow::anyhow!("Tencent minute date missing"))?;
    let mut rows: Vec<String> = Vec::new();
    let mut previous_amount = 0.0_f64;
    if let Some(items) = data.get("data").and_then(|v| v.as_array()) {
        for item in items {
            let text = uzi_core::py::py_str(item);
            let fields: Vec<&str> = text.split_whitespace().collect();
            if fields.len() != 4 {
                bail!("Tencent minute schema mismatch");
            }
            let clock = chrono::NaiveTime::parse_from_str(fields[0], "%H%M")
                .map_err(|_| anyhow::anyhow!("Tencent minute schema mismatch"))?;
            let Some(amount) = num_str(fields[3]) else {
                bail!("Tencent cumulative amount invalid");
            };
            if amount < previous_amount {
                bail!("Tencent cumulative amount invalid");
            }
            let delta = amount - previous_amount;
            previous_amount = amount;
            let stamp = day
                .and_time(clock)
                .format("%Y-%m-%d %H:%M")
                .to_string();
            rows.push(format!(
                "{stamp},0,{},0,0,0,{},0",
                fields[1],
                uzi_core::py::float_str(delta)
            ));
        }
    }
    parse_minutes(&json!({"data": {"code": raw, "trends": rows}}), code, cutoff)
}

/// `enrich_intraday(stock)` — keep the latest quote atomic: no old spot price
/// mixed with new book/amount.
pub fn enrich_intraday(stock: &mut StockSnapshot) {
    let (quote, mut errors) = fetch_quote(stock);
    let mut intraday = Map::new();
    intraday.insert("quote".into(), quote.clone());
    intraday.insert("bars".into(), Value::Array(Vec::new()));
    intraday.insert(
        "source_errors".into(),
        Value::Array(errors.iter().map(|e| Value::String(e.clone())).collect()),
    );
    stock
        .extra
        .insert("intraday".into(), Value::Object(intraday));
    if !uzi_core::py::truthy(&quote) {
        return;
    }
    let quote_price = quote.get("price").and_then(|v| v.as_f64());
    if quote_price.is_some()
        && quote.get("amount").map(|v| !v.is_null()).unwrap_or(false)
        && quote.get("change_pct").map(|v| !v.is_null()).unwrap_or(false)
        && quote_price.unwrap_or(0.0) > 0.0
    {
        stock.price = quote_price.unwrap_or(0.0);
        stock.amount = quote.get("amount").and_then(|v| v.as_f64()).unwrap_or(0.0);
        stock.change_pct = quote
            .get("change_pct")
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0);
        stock.prev_close = quote.get("prev_close").and_then(|v| v.as_f64());
        stock.open_price = quote.get("open_price").and_then(|v| v.as_f64());
        stock.high = quote.get("high").and_then(|v| v.as_f64());
        stock.low = quote.get("low").and_then(|v| v.as_f64());
        stock.observed_at = quote
            .get("quote_at")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        stock.source = quote
            .get("source")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
    }
    let Ok((raw, exchange)) = code_parts(&stock.code) else {
        return;
    };
    let secid = format!(
        "{}.{}",
        if exchange == "HK" {
            "116"
        } else if exchange == "SH" {
            "1"
        } else {
            "0"
        },
        raw
    );
    match json_response(
        MINUTES,
        &[
            ("secid", &secid),
            ("fields1", "f1,f2,f3"),
            ("fields2", "f51,f52,f53,f54,f55,f56,f57,f58"),
            ("ndays", "1"),
            ("iscr", "0"),
        ],
    ) {
        Ok(payload) => {
            let cutoff = quote
                .get("quote_at")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            if let Ok(bars) = parse_minutes(&payload, &stock.code, &cutoff) {
                set_intraday_bars(stock, bars, Some("eastmoney:trends2"));
            }
        }
        Err(_) => errors.push("eastmoney_minutes:ValueError".to_string()),
    }
    let bar_count = intraday_bar_count(stock);
    if bar_count < 3 {
        let url = "https://web.ifzq.gtimg.cn/appstock/app/minute/query";
        let params = [("code", format!("{}{}", exchange.to_lowercase(), raw))];
        let borrowed: Vec<(&str, &str)> = params.iter().map(|(k, v)| (*k, v.as_str())).collect();
        if let Ok(payload) = json_response(url, &borrowed) {
            let cutoff = quote
                .get("quote_at")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            if let Ok(bars) = parse_tencent_minutes(&payload, &stock.code, &cutoff) {
                if bars.len() > bar_count {
                    set_intraday_bars(stock, bars, Some("tencent:minute"));
                }
            }
        }
    }
    // Persist the accumulated source errors.
    if let Some(Value::Object(intraday)) = stock.extra.get_mut("intraday") {
        intraday.insert(
            "source_errors".into(),
            Value::Array(errors.drain(..).map(Value::String).collect()),
        );
    }
}

fn intraday_bar_count(stock: &StockSnapshot) -> usize {
    stock
        .extra
        .get("intraday")
        .and_then(|v| v.get("bars"))
        .and_then(|v| v.as_array())
        .map(|a| a.len())
        .unwrap_or(0)
}

fn set_intraday_bars(stock: &mut StockSnapshot, bars: Vec<Value>, minute_source: Option<&str>) {
    if let Some(Value::Object(intraday)) = stock.extra.get_mut("intraday") {
        intraday.insert("bars".into(), Value::Array(bars));
        if let Some(source) = minute_source {
            intraday.insert("minute_source".into(), Value::String(source.to_string()));
        }
    }
}

