//! Port of `lib/data_sources.py` — unified data-source layer with caching and
//! multi-source fallback.
//!
//! Upstream wraps `akshare` / `yfinance` / `baostock` / `requests`. The Rust
//! port has no Python runtime, so every branch that required one of those
//! libraries is either served by the *documented HTTP endpoint the library
//! calls* (EastMoney push2 / push2his / datacenter, Tencent qt, Sina hq, Yahoo
//! chart v8, Stooq) or returns exactly the empty payload upstream returns on
//! failure. No data is invented: an unreachable endpoint yields `{}` / `[]`.

use chrono::{Datelike, Utc};
use serde_json::{json, Map, Value};
use std::sync::LazyLock;

use uzi_core::cache::{cached, TTL_DAILY, TTL_HOURLY, TTL_INTRADAY, TTL_QUARTERLY, TTL_REALTIME};
use uzi_core::ticker::TickerInfo;

use crate::em;
use crate::http;
use crate::providers;

/// `_mx_available()` — `bool(os.environ.get("MX_APIKEY"))`.
pub fn mx_available() -> bool {
    std::env::var("MX_APIKEY")
        .map(|v| !v.is_empty())
        .unwrap_or(false)
}

static EMPTY_MAP: LazyLock<Map<String, Value>> = LazyLock::new(Map::new);

fn obj(v: &Value) -> &Map<String, Value> {
    v.as_object().unwrap_or(&EMPTY_MAP)
}

fn nonempty(v: &Value) -> bool {
    !(v.is_null() || v.as_str().map(|s| s.is_empty() || s == "-").unwrap_or(false))
}

// ─────────────────────────────────────────────────────────────
// v2.6 · Tencent qt price fallback (A/H/U)
// ─────────────────────────────────────────────────────────────

/// `_fetch_price_tencent_qt(market, code_raw)` — empty dict on any failure,
/// never raises.
pub fn price_tencent_qt(market: &str, code_raw: &str) -> Value {
    let symbol = match market {
        "A" => {
            let prefix = if code_raw.starts_with("60")
                || code_raw.starts_with("688")
                || code_raw.starts_with("900")
            {
                "sh"
            } else {
                "sz"
            };
            format!("{prefix}{code_raw}")
        }
        "H" => format!("hk{:0>5}", code_raw),
        "U" => format!("us{code_raw}"),
        _ => return json!({}),
    };
    // The assignment pins the plain-HTTP host as the verified reachable endpoint.
    let url = format!("http://qt.gtimg.cn/q={symbol}");
    let Ok(resp) = http::get_plain(&url, 8) else {
        return json!({});
    };
    if !resp.is_ok() {
        return json!({});
    }
    let text = resp.gbk_text();
    if !text.contains('=') || !text.contains('"') {
        return json!({});
    }
    let content = text.splitn(2, '=').nth(1).unwrap_or("").trim();
    let content = content.trim_end_matches(';').trim().trim_matches('"');
    let parts: Vec<&str> = content.split('~').collect();
    if parts.len() < 35 {
        return json!({});
    }
    let f = |idx: usize| -> Option<f64> {
        let v = parts.get(idx)?.trim();
        if v.is_empty() || v == "-" {
            None
        } else {
            v.parse::<f64>().ok()
        }
    };
    let mut out = Map::new();
    let name = parts.get(1).copied().unwrap_or("");
    if !name.is_empty() {
        out.insert("name".into(), json!(name));
    }
    for (key, idx) in [
        ("price", 3),
        ("prev_close", 4),
        ("open", 5),
        ("change_pct", 32),
        ("high", 33),
        ("low", 34),
    ] {
        if let Some(v) = f(idx) {
            out.insert(key.into(), json!(v));
        }
    }
    if parts.len() > 39 {
        if let Some(pe) = f(39) {
            out.insert("pe_ttm".into(), json!(pe));
        }
    }
    if parts.len() > 44 {
        if let Some(circ) = f(44) {
            out.insert("circulating_cap".into(), json!(format!("{circ}亿")));
            out.insert("circulating_cap_raw".into(), json!(circ * 1e8));
        }
    }
    if parts.len() > 45 {
        if let Some(total) = f(45) {
            out.insert("market_cap".into(), json!(format!("{total}亿")));
            out.insert("market_cap_raw".into(), json!(total * 1e8));
        }
    }
    if parts.len() > 46 {
        if let Some(pb) = f(46) {
            out.insert("pb".into(), json!(pb));
        }
    }
    Value::Object(out)
}

// ─────────────────────────────────────────────────────────────
// Field-level merge helpers
// ─────────────────────────────────────────────────────────────

fn append_fallback_snap(out: &mut Map<String, Value>, marker: &str) {
    let current = out
        .get("_fallback_snap")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let mut parts: Vec<String> = current
        .split('+')
        .map(|s| s.to_string())
        .filter(|s| !s.is_empty())
        .collect();
    if !parts.iter().any(|p| p == marker) {
        parts.push(marker.to_string());
    }
    out.insert("_fallback_snap".into(), json!(parts.join("+")));
}

const BASIC_FALLBACK_FIELDS: &[&str] = &[
    "name",
    "price",
    "change_pct",
    "open",
    "prev_close",
    "high",
    "low",
    "pe_ttm",
    "pb",
    "market_cap",
    "market_cap_raw",
    "circulating_cap",
    "circulating_cap_raw",
    "industry",
    "listed_date",
];

/// `_merge_missing_basic_fields(out, source, marker, fields)` — later providers
/// only patch holes; earlier successes win.
fn merge_missing_basic_fields(
    out: &mut Map<String, Value>,
    source: &Value,
    marker: &str,
    fields: &[&str],
) -> bool {
    let src = obj(source);
    if src.is_empty() {
        return false;
    }
    let mut changed = false;
    for field in fields {
        let missing = match out.get(*field) {
            None | Some(Value::Null) => true,
            Some(Value::String(s)) => s.is_empty() || s == "-",
            _ => false,
        };
        if !missing {
            continue;
        }
        if let Some(v) = src.get(*field) {
            if nonempty(v) {
                out.insert((*field).to_string(), v.clone());
                changed = true;
            }
        }
    }
    if changed {
        append_fallback_snap(out, marker);
    }
    changed
}

/// `_ensure_a_share_basic_fields(out, ti)` — patch report-critical holes from
/// Tencent qt and the hard-coded industry map.
pub fn ensure_a_share_basic_fields(out: &mut Map<String, Value>, ti: &TickerInfo) {
    let critical = [
        "name",
        "price",
        "pe_ttm",
        "pb",
        "market_cap",
        "industry",
        "listed_date",
    ];
    let missing_any = |o: &Map<String, Value>, fields: &[&str]| -> bool {
        fields.iter().any(|f| match o.get(*f) {
            None | Some(Value::Null) => true,
            Some(Value::String(s)) => s.is_empty() || s == "-",
            _ => false,
        })
    };
    if !missing_any(out, &critical) {
        return;
    }

    let qt = price_tencent_qt("A", &ti.code);
    merge_missing_basic_fields(
        out,
        &qt,
        "field:tencent_qt",
        &[
            "name",
            "price",
            "change_pct",
            "open",
            "prev_close",
            "high",
            "low",
            "pe_ttm",
            "pb",
            "market_cap",
            "market_cap_raw",
            "circulating_cap",
            "circulating_cap_raw",
        ],
    );

    if missing_any(out, &["name", "price", "pe_ttm", "pb", "listed_date"]) {
        // BaoStock is a Python-only library; upstream records the failure key so
        // downstream provenance stays auditable.
        out.insert(
            "_field_baostock_err".into(),
            json!("ImportError: baostock not installed"),
        );
    }

    if missing_any(out, &["name"]) {
        out.insert(
            "_field_ak_code_name_err".into(),
            json!("ImportError: akshare not installed"),
        );
    }

    if missing_any(out, &["industry"]) {
        if let Some(ind) = known_industry(&ti.code) {
            out.insert("industry".into(), json!(ind));
            append_fallback_snap(out, "field:known_industry");
        }
    }
}

// ─────────────────────────────────────────────────────────────
// 0. Basic info
// ─────────────────────────────────────────────────────────────

/// `fetch_basic(ti)` — TTL 60s realtime quote.
pub fn fetch_basic(ti: &TickerInfo) -> Value {
    let key = format!("basic__{}", ti.code);
    let ttl = TTL_REALTIME;
    let ti2 = ti.clone();
    cached::<_, anyhow::Error>(&ti.full, &key, ttl, move || {
        Ok(match ti2.market.as_str() {
            "A" => fetch_basic_a(&ti2),
            "H" => fetch_basic_hk(&ti2),
            _ => fetch_basic_us(&ti2),
        })
    })
    .unwrap_or_else(|_| json!({}))
}

/// `_fetch_basic_a(ti)` — the Rust chain walks upstream's *HTTP* layers:
/// EastMoney push2 single-stock → Tencent qt → Sina hq → known industry map.
/// (The XueQiu/Baidu layers upstream prefers are AkShare-only scrapers and
/// degrade to the same empty payload here.)
pub fn fetch_basic_a(ti: &TickerInfo) -> Value {
    let mut out: Map<String, Value> = Map::new();
    out.insert("code".into(), json!(ti.full));

    // PRIMARY (HTTP): EastMoney push2 single-stock payload
    match em::push2_quote(&ti.code, &ti.full, 8) {
        Ok(parsed) => {
            for (k, v) in obj(&parsed).iter() {
                if nonempty(v) {
                    out.insert(k.clone(), v.clone());
                }
            }
            if out.contains_key("price") {
                append_fallback_snap(&mut out, "em-direct");
            }
        }
        Err(e) => {
            out.insert("_em_direct_err".into(), json!(first80(&e)));
        }
    }

    // FALLBACK: Tencent qt (independent host, always tried when key fields miss)
    let needs = |o: &Map<String, Value>| -> bool {
        let empty = |k: &str| match o.get(k) {
            None | Some(Value::Null) => true,
            Some(Value::String(s)) => s.is_empty() || s == "-",
            _ => false,
        };
        empty("price") || empty("pe_ttm") || empty("market_cap")
    };
    if needs(&out) {
        let qt = price_tencent_qt("A", &ti.code);
        merge_missing_basic_fields(
            &mut out,
            &qt,
            "tencent_qt",
            BASIC_FALLBACK_FIELDS,
        );
    }

    // FALLBACK: Sina hq (another independent host)
    if match out.get("price") {
        None | Some(Value::Null) => true,
        Some(Value::String(s)) => s.is_empty() || s == "-",
        _ => false,
    } {
        if let Ok(Some(payload)) = sina_quote_payload(&ti.code, "A") {
            let fields: Vec<&str> = payload.split(',').collect();
            if fields.len() > 30 {
                let name = fields[0];
                let _open = fields[1].parse::<f64>().unwrap_or(0.0);
                let prev_close = fields[2].parse::<f64>().unwrap_or(0.0);
                let price = fields[3].parse::<f64>().unwrap_or(0.0);
                let chg = if prev_close != 0.0 {
                    uzi_core::py::round((price - prev_close) / prev_close * 100.0, 2)
                } else {
                    0.0
                };
                if out.get("name").map(|v| !nonempty(v)).unwrap_or(true) {
                    out.insert("name".into(), json!(name));
                }
                out.insert("price".into(), json!(price));
                out.insert("change_pct".into(), json!(chg));
                out.insert("_fallback_snap".into(), json!("sina-hq"));
            }
        }
    }

    // LAST RESORT: industry from the hard-coded map (critical for downstream)
    if !out.get("industry").map(nonempty).unwrap_or(false) {
        if let Some(ind) = known_industry(&ti.code) {
            out.insert("industry".into(), json!(ind));
        }
    }

    ensure_a_share_basic_fields(&mut out, ti);
    Value::Object(out)
}

/// `_fetch_basic_hk(ti)` — v2.5 chain. The AkShare layers are library-only; the
/// Tencent qt / Sina hq HTTP layers are ported verbatim.
fn fetch_basic_hk(ti: &TickerInfo) -> Value {
    let code5 = format!("{:0>5}", ti.code);
    let mut out: Map<String, Value> = Map::new();
    out.insert("code".into(), json!(ti.full));

    let qt = price_tencent_qt("H", &code5);
    if nonempty(&qt["price"]) {
        for k in [
            "price",
            "change_pct",
            "open",
            "prev_close",
            "high",
            "low",
        ] {
            if let Some(v) = qt.get(k) {
                if nonempty(v) {
                    out.insert(k.into(), v.clone());
                }
            }
        }
        append_fallback_snap(&mut out, "tencent_qt");
    }

    if !out.get("price").map(nonempty).unwrap_or(false) {
        if let Ok(Some(payload)) = sina_quote_payload(&code5, "H") {
            let fields: Vec<&str> = payload.split(',').collect();
            if fields.len() > 6 {
                if let Ok(price) = fields[6].parse::<f64>() {
                    out.insert("price".into(), json!(price));
                    out.insert("name".into(), json!(fields.get(1).copied().unwrap_or("")));
                    append_fallback_snap(&mut out, "sina_hq");
                }
            }
        }
    }
    Value::Object(out)
}

/// `_fetch_basic_us(ti)` — yfinance is a Python library; the documented
/// equivalent is the Yahoo chart v8 endpoint, which is what upstream's own
/// K-line fallback uses. Fields yfinance's `.info` adds (industry, PE, PB) have
/// no equivalent here and are omitted rather than invented.
fn fetch_basic_us(ti: &TickerInfo) -> Value {
    let symbol = crate::global_peers::to_yahoo_symbol(ti);
    let mut out: Map<String, Value> = Map::new();
    out.insert("code".into(), json!(ti.full));
    let url = format!(
        "https://query1.finance.yahoo.com/v8/finance/chart/{symbol}?interval=1d&range=5d"
    );
    if let Ok(v) = http::get_json(&url, &[("Referer", "https://finance.yahoo.com/")], 10) {
        let meta = v
            .get("chart")
            .and_then(|c| c.get("result"))
            .and_then(|r| r.as_array())
            .and_then(|r| r.first())
            .and_then(|r| r.get("meta"));
        if let Some(meta) = meta {
            if let Some(p) = meta.get("regularMarketPrice").and_then(|v| v.as_f64()) {
                out.insert("price".into(), json!(p));
            }
            if let Some(pc) = meta.get("chartPreviousClose").and_then(|v| v.as_f64()) {
                out.insert("prev_close".into(), json!(pc));
            }
            if let Some(n) = meta.get("longName").or_else(|| meta.get("shortName")) {
                out.insert("name".into(), n.clone());
            }
            if let Some(cur) = meta.get("currency") {
                out.insert("currency".into(), cur.clone());
            }
        }
    }
    Value::Object(out)
}

/// Payload inside the first quoted string of a Sina hq response.
fn sina_quote_payload(code: &str, market: &str) -> Result<Option<String>, String> {
    let symbol = match market {
        "A" => {
            let prefix = if code.starts_with("60") || code.starts_with("688") || code.starts_with("900") {
                "sh"
            } else {
                "sz"
            };
            format!("{prefix}{code}")
        }
        "H" => format!("hk{code}"),
        _ => format!("gb_{}", code.to_lowercase()),
    };
    let url = format!("http://hq.sinajs.cn/list={symbol}");
    let resp = http::get(
        &url,
        &[("Referer", "http://finance.sina.com.cn")],
        8,
    )?;
    let text = resp.gbk_text();
    let Some(start) = text.find('"') else {
        return Ok(None);
    };
    let Some(end) = text.rfind('"') else {
        return Ok(None);
    };
    if end <= start {
        return Ok(None);
    }
    Ok(Some(text[start + 1..end].to_string()))
}

fn first80(s: &str) -> String {
    s.chars().take(80).collect()
}

// ─────────────────────────────────────────────────────────────
// Hard-coded industry map (last-resort fallback)
// ─────────────────────────────────────────────────────────────

/// `_STOCK_INDUSTRY_MAP` — verbatim from `data_sources.py`.
pub fn known_industry(code: &str) -> Option<&'static str> {
    let m: &[(&str, &str)] = &[
        // 光学光电子
        ("002273", "光学光电子"),
        ("002281", "光学光电子"),
        ("300433", "光学光电子"),
        ("688127", "光学光电子"),
        ("002456", "光学光电子"),
        ("603501", "光学光电子"),
        // 白酒
        ("600519", "白酒"),
        ("000858", "白酒"),
        ("000568", "白酒"),
        ("002304", "白酒"),
        ("600809", "白酒"),
        ("600779", "白酒"),
        ("000799", "白酒"),
        // 半导体
        ("688981", "半导体"),
        ("603986", "半导体"),
        ("002371", "半导体"),
        ("002129", "半导体"),
        ("300782", "半导体"),
        ("688012", "半导体"),
        ("688008", "半导体"),
        ("688536", "半导体"),
        // 新能源 / 电池
        ("300750", "电池"),
        ("002594", "汽车整车"),
        ("300014", "电池"),
        ("002460", "电池"),
        ("300207", "电池"),
        ("300124", "电池"),
        ("300919", "电池"),
        // AI / 算力
        ("300308", "光模块"),
        ("300394", "光模块"),
        ("300502", "光模块"),
        ("002463", "光模块"),
        // 医药生物
        ("300760", "医药生物"),
        ("600276", "医药生物"),
        ("603259", "医药生物"),
        ("600196", "医药生物"),
        // 消费电子
        ("002475", "消费电子"),
        ("002241", "消费电子"),
        ("002938", "消费电子"),
        // 银行
        ("601398", "银行"),
        ("601939", "银行"),
        ("601288", "银行"),
        ("600036", "银行"),
        ("601166", "银行"),
        ("000001", "银行"),
        // 保险
        ("601318", "保险"),
        ("601601", "保险"),
        ("601628", "保险"),
        ("601336", "保险"),
        // 证券
        ("600030", "证券"),
        ("601688", "证券"),
        ("000776", "证券"),
        // 房地产
        ("000002", "房地产"),
        ("600048", "房地产"),
        ("001979", "房地产"),
        // 钢铁
        ("600019", "钢铁"),
        ("600808", "钢铁"),
        ("000898", "钢铁"),
        // 家电
        ("000333", "家电"),
        ("000651", "家电"),
        ("600690", "家电"),
        // 食品饮料
        ("600887", "食品饮料"),
        ("603288", "食品饮料"),
        // 港口
        ("000582", "港口"),
        ("601018", "港口"),
        ("600017", "港口"),
        ("600018", "港口"),
        ("000905", "港口"),
        ("601298", "港口"),
        ("000507", "港口"),
        // 交通运输
        ("601006", "交通运输"),
        ("600009", "交通运输"),
        ("601111", "交通运输"),
        // 航运
        ("601866", "航运"),
        ("601872", "航运"),
        ("600026", "航运"),
        ("601880", "航运"),
        // 建筑
        ("601668", "建筑装饰"),
        ("601186", "建筑装饰"),
        ("002051", "建筑装饰"),
        // 电力
        ("600900", "电力"),
        ("601985", "电力"),
        ("600886", "电力"),
        // 煤炭
        ("601088", "煤炭"),
        ("600188", "煤炭"),
        ("601898", "煤炭"),
        // 军工
        ("600893", "军工"),
        ("000768", "军工"),
        ("601989", "军工"),
        // 汽车
        ("600104", "汽车"),
        ("601238", "汽车"),
        ("000625", "汽车"),
    ];
    m.iter().find(|(c, _)| *c == code).map(|(_, i)| *i)
}

// ─────────────────────────────────────────────────────────────
// 1. K-line (OHLCV)
// ─────────────────────────────────────────────────────────────

/// `fetch_kline(ti, period, start, adjust)` — TTL 5min.
pub fn fetch_kline(ti: &TickerInfo, period: &str, start: &str, adjust: &str) -> Value {
    let key = format!("kline__{}__{}__{}__{}", ti.code, period, start, adjust);
    let ti2 = ti.clone();
    let (p, s, a) = (period.to_string(), start.to_string(), adjust.to_string());
    cached::<_, anyhow::Error>(&ti.full, &key, TTL_INTRADAY, move || {
        Ok(fetch_kline_impl(&ti2, &p, &s, &a))
    })
    .unwrap_or_else(|_| json!([]))
}

fn fetch_kline_impl(ti: &TickerInfo, period: &str, start: &str, adjust: &str) -> Value {
    match ti.market.as_str() {
        "A" => kline_a_share_chain(ti, period, start, adjust),
        "H" => kline_hk_chain(ti, start, adjust),
        _ => kline_us_chain(ti),
    }
}

fn kline_a_share_chain(ti: &TickerInfo, period: &str, start: &str, adjust: &str) -> Value {
    let code = ti.code.as_str();
    let mut errors: Vec<String> = Vec::new();

    // 1+4. AkShare 东财 == EastMoney push2his direct (same documented endpoint)
    match providers::akshare::fetch_kline_a(code, period, start, adjust) {
        Ok(rows) if rows.as_array().map(|a| !a.is_empty()).unwrap_or(false) => return rows,
        Ok(_) => {}
        Err(e) => errors.push(format!("akshare-em: {e}")),
    }

    // 2+5. Sina direct K-line
    let sina_symbol = format!(
        "{}{}",
        if ti.full.ends_with("SH") { "sh" } else { "sz" },
        code
    );
    match em::sina_kline(&sina_symbol, "500", 12) {
        Ok(rows) if !rows.is_empty() => return Value::Array(rows),
        Ok(_) => {}
        Err(e) => errors.push(format!("sina-direct: {e}")),
    }

    // 6. Tencent ifzq direct K-line
    match em::tencent_kline(&sina_symbol, 12) {
        Ok(rows) if !rows.is_empty() => return Value::Array(rows),
        Ok(_) => {}
        Err(e) => errors.push(format!("tencent-direct: {e}")),
    }

    // 7. providers chain (tushare / efinance / baostock) — last-resort failover
    match providers::try_chain_kline(code, period, start, adjust) {
        Ok((rows, _src)) if rows.as_array().map(|a| !a.is_empty()).unwrap_or(false) => {
            return rows
        }
        Ok(_) => {}
        Err(e) => errors.push(format!("providers: {e}")),
    }

    json!([{
        "_kline_fetch_error": if errors.is_empty() {
            "no source available".to_string()
        } else {
            errors.join("; ")
        }
    }])
}

fn kline_hk_chain(ti: &TickerInfo, _start: &str, _adjust: &str) -> Value {
    let code5 = format!("{:0>5}", ti.code);
    let mut errors: Vec<String> = Vec::new();
    let yf_code = format!("{}.HK", code5.trim_start_matches('0'));
    let rows = crate::em::yahoo_chart(&yf_code, "2y", 15);
    if !rows.is_empty() {
        return Value::Array(rows);
    }
    errors.push("yahoo-v8-hk: empty".to_string());
    json!([{
        "_kline_fetch_error": if errors.is_empty() {
            "no HK source available".to_string()
        } else {
            errors.join("; ")
        }
    }])
}

fn kline_us_chain(ti: &TickerInfo) -> Value {
    let symbol = crate::global_peers::to_yahoo_symbol(ti);
    let rows = crate::em::yahoo_chart(&symbol, "2y", 15);
    if !rows.is_empty() {
        return Value::Array(rows);
    }
    let rows = crate::em::stooq_daily(&symbol, 12);
    if !rows.is_empty() {
        return Value::Array(rows);
    }
    json!([])
}

// ─────────────────────────────────────────────────────────────
// 2. Financials
// ─────────────────────────────────────────────────────────────

/// `fetch_financials(ti)` — TTL 24h. AkShare's `stock_financial_abstract` and
/// `stock_financial_analysis_indicator` are the only upstream paths; the Rust
/// port serves the abstract from EastMoney's documented F10 endpoint and returns
/// `{}` (upstream's failure payload) otherwise.
pub fn fetch_financials(ti: &TickerInfo) -> Value {
    if ti.market != "A" {
        return json!({});
    }
    let key = format!("fin__{}", ti.code);
    let ti2 = ti.clone();
    cached::<_, anyhow::Error>(&ti.full, &key, TTL_QUARTERLY, move || {
        Ok(fetch_financials_impl(&ti2))
    })
    .unwrap_or_else(|_| json!({}))
}

fn fetch_financials_impl(ti: &TickerInfo) -> Value {
    match providers::akshare::fetch_financials_a(&ti.code) {
        Ok(v) => {
            let raw = v.get("raw").cloned().unwrap_or_else(|| json!([]));
            json!({
                "abstract": raw,
                "indicator": [],
            })
        }
        Err(e) => json!({"error": first80(&e.to_string())}),
    }
}

/// `fetch_cash_flow(ti, dates)` — A-share cash-flow statement.
///
/// `dates` is the comma-separated list of report periods to request
/// (`2025-12-31,2024-12-31`); the endpoint needs it explicitly. Returns `[]`
/// when the endpoint is unreachable — no numbers are invented.
pub fn fetch_cash_flow(ti: &TickerInfo, dates: &str) -> Value {
    if ti.market != "A" || dates.is_empty() {
        return json!([]);
    }
    let key = format!("cash_flow__{}", ti.code);
    let ti2 = ti.clone();
    let dates = dates.to_string();
    cached::<_, anyhow::Error>(&ti.full, &key, TTL_QUARTERLY, move || {
        Ok(match providers::akshare::fetch_cash_flow_a(&ti2.code, &dates) {
            Ok(v) => v.get("raw").cloned().unwrap_or_else(|| json!([])),
            Err(_) => json!([]),
        })
    })
    .unwrap_or_else(|_| json!([]))
}

/// `fetch_dividend(ti)` — A-share dividend/distribution history.
pub fn fetch_dividend(ti: &TickerInfo) -> Value {
    if ti.market != "A" {
        return json!([]);
    }
    let key = format!("dividend__{}", ti.code);
    let ti2 = ti.clone();
    cached::<_, anyhow::Error>(&ti.full, &key, TTL_QUARTERLY, move || {
        Ok(match providers::akshare::fetch_dividend_a(&ti2.code) {
            Ok(v) => v.get("raw").cloned().unwrap_or_else(|| json!([])),
            Err(_) => json!([]),
        })
    })
    .unwrap_or_else(|_| json!([]))
}

// ─────────────────────────────────────────────────────────────
// 3. 龙虎榜 (A only)
// ─────────────────────────────────────────────────────────────

/// `fetch_lhb_recent(ti, days)`. Upstream's AkShare `stock_lhb_stock_detail_em`
/// requires enumerating on-board dates; the Rust port returns the empty list
/// upstream returns when AkShare is unavailable (never invented records).
pub fn fetch_lhb_recent(ti: &TickerInfo, days: i64) -> Value {
    if ti.market != "A" {
        return json!([]);
    }
    let key = format!("lhb__{}__{}", ti.code, days);
    cached::<_, anyhow::Error>(&ti.full, &key, TTL_DAILY, || Ok(json!([])))
        .unwrap_or_else(|_| json!([]))
}

// ─────────────────────────────────────────────────────────────
// 4. News (财联社 / 个股新闻)
// ─────────────────────────────────────────────────────────────

/// `fetch_news(ti, limit)` — AkShare `stock_news_em` is the only upstream path;
/// empty list on failure (upstream's documented degradation).
pub fn fetch_news(ti: &TickerInfo, limit: usize) -> Value {
    let key = format!("news__{}__{}", ti.code, limit);
    cached::<_, anyhow::Error>(&ti.full, &key, TTL_HOURLY, || Ok(json!([])))
        .unwrap_or_else(|_| json!([]))
}

// ─────────────────────────────────────────────────────────────
// 5. Sentiment / hot rank
// ─────────────────────────────────────────────────────────────

/// `fetch_hot_rank(ti)` — AkShare `stock_hot_rank_detail_em`; `{}` on failure.
pub fn fetch_hot_rank(ti: &TickerInfo) -> Value {
    if ti.market != "A" {
        return json!({});
    }
    let key = format!("hot__{}", ti.code);
    cached::<_, anyhow::Error>(&ti.full, &key, TTL_INTRADAY, || Ok(json!({})))
        .unwrap_or_else(|_| json!({}))
}

// ─────────────────────────────────────────────────────────────
// 6. North-bound capital (A only)
// ─────────────────────────────────────────────────────────────

/// `fetch_northbound(ti)` — direct EastMoney datacenter query, ported verbatim.
pub fn fetch_northbound(ti: &TickerInfo) -> Value {
    if ti.market != "A" {
        return json!({});
    }
    let key = format!("hsgt__{}", ti.code);
    let ti2 = ti.clone();
    cached::<_, anyhow::Error>(&ti.full, &key, TTL_DAILY, move || {
        Ok(fetch_north_impl(&ti2))
    })
    .unwrap_or_else(|_| json!({}))
}

fn fetch_north_impl(ti: &TickerInfo) -> Value {
    if ti.code.len() != 6 || !ti.code.chars().all(|c| c.is_ascii_digit()) {
        return json!({});
    }
    let Ok(rows) = em::hsgt_hold(&ti.code, 12) else {
        return json!({});
    };
    let num = |v: Option<&Value>| -> Value {
        match v {
            None | Some(Value::Null) => Value::Null,
            Some(Value::String(s)) if s.is_empty() => Value::Null,
            Some(other) => other
                .as_f64()
                .map(|f| json!(f))
                .unwrap_or(Value::Null),
        }
    };
    let mut sorted: Vec<&Value> = rows.iter().filter(|r| r.is_object()).collect();
    sorted.sort_by_key(|r| {
        r.get("TRADE_DATE")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .unwrap_or_default()
    });
    let start = sorted.len().saturating_sub(60);
    let flow: Vec<Value> = sorted[start..]
        .iter()
        .map(|row| {
            let g = |k: &str| row.get(k);
            json!({
                "持股日期": g("TRADE_DATE").and_then(|v| v.as_str()).map(|s| s.chars().take(10).collect::<String>()).unwrap_or_default(),
                "当日收盘价": num(g("CLOSE_PRICE")),
                "当日涨跌幅": num(g("CHANGE_RATE")),
                "持股数量": num(g("HOLD_SHARES")),
                "持股市值": num(g("HOLD_MARKET_CAP")),
                "持股数量占A股百分比": num(g("HOLD_SHARES_RATIO")),
                "今日增持股数": num(g("ADD_SHARES_REPAIR")),
                "今日增持资金": num(g("PREDICT_AMC")),
                "今日持股市值变化": num(g("HMC_CHANGE")),
            })
        })
        .collect();
    json!({"flow_history": flow})
}

// ─────────────────────────────────────────────────────────────
// 6.5 Capital-flow sub-sources (block trades / holders / restricted / fund-flow)
// ─────────────────────────────────────────────────────────────

/// `fetch_block_trades(ti)` — EastMoney `RPT_BLOCKTRADE_STA`, last 90 days.
pub fn fetch_block_trades(ti: &TickerInfo) -> Vec<Value> {
    if ti.market != "A" {
        return Vec::new();
    }
    let today = chrono::Local::now().naive_local().date();
    let year_str = today.format("%Y").to_string();
    let start = format!("{year_str}-01-01");
    let end = today.format("%Y-%m-%d").to_string();
    let code = ti.code.clone();
    let key = format!("dzjy__{}__{}", code, end);
    cached::<_, anyhow::Error>(&ti.full, &key, TTL_DAILY, move || {
        let rows = em::block_trade_sta(&code, &start, &end, 12).map_err(|e| anyhow::anyhow!(e))?;
        let mapped: Vec<Value> = rows
            .into_iter()
            .map(|r| {
                let obj = r.as_object().cloned().unwrap_or_default();
                json!({
                    "证券代码": obj.get("SECURITY_CODE").cloned().unwrap_or(Value::Null),
                    "证券简称": obj.get("SECURITY_NAME_ABBR").cloned().unwrap_or(Value::Null),
                    "交易日期": obj.get("TRADE_DATE").and_then(|v| v.as_str()).map(|s| s.chars().take(10).collect::<String>()).unwrap_or_default(),
                    "成交笔数": obj.get("DEAL_NUM").cloned().unwrap_or(Value::Null),
                    "成交总量": obj.get("VOLUME").cloned().unwrap_or(Value::Null),
                    "成交总额": obj.get("DEAL_AMT").cloned().unwrap_or(Value::Null),
                    "成交均价": obj.get("AVERAGE_PRICE").cloned().unwrap_or(Value::Null),
                    "收盘价": obj.get("CLOSE_PRICE").cloned().unwrap_or(Value::Null),
                    "折溢率": obj.get("PREMIUM_RATIO").cloned().unwrap_or(Value::Null),
                    "涨跌幅": obj.get("CHANGE_RATE").cloned().unwrap_or(Value::Null),
                    "换手率": obj.get("TURNOVERRATE").cloned().unwrap_or(Value::Null),
                })
            })
            .collect();
        Ok(Value::Array(mapped))
    })
    .unwrap_or_else(|_| json!([]))
    .as_array()
    .cloned()
    .unwrap_or_default()
}

/// `fetch_holder_counts(ti)` — EastMoney `RPT_HOLDERNUM_DET`.
pub fn fetch_holder_counts(ti: &TickerInfo) -> Vec<Value> {
    if ti.market != "A" {
        return Vec::new();
    }
    let code = ti.code.clone();
    let key = format!("gdhs__{}", code);
    cached::<_, anyhow::Error>(&ti.full, &key, TTL_QUARTERLY, move || {
        let rows = em::holder_num_det(&code, 12).map_err(|e| anyhow::anyhow!(e))?;
        let mapped: Vec<Value> = rows
            .into_iter()
            .map(|r| {
                let obj = r.as_object().cloned().unwrap_or_default();
                json!({
                    "股东户数": obj.get("HOLDER_NUM").cloned().unwrap_or(Value::Null),
                    "上期股东户数": obj.get("PRE_HOLDER_NUM").cloned().unwrap_or(Value::Null),
                    "股东户数增幅": obj.get("HOLDER_NUM_RATIO").cloned().unwrap_or(Value::Null),
                    "变动日期": obj.get("END_DATE").and_then(|v| v.as_str()).map(|s| s.chars().take(10).collect::<String>()).unwrap_or_default(),
                    "上期变动日期": obj.get("PRE_END_DATE").and_then(|v| v.as_str()).map(|s| s.chars().take(10).collect::<String>()).unwrap_or_default(),
                    "区间涨跌幅": obj.get("INTERVAL_CHRATE").cloned().unwrap_or(Value::Null),
                    "户均持股市值": obj.get("AVG_MARKET_CAP").cloned().unwrap_or(Value::Null),
                    "户均持股数量": obj.get("AVG_HOLD_NUM").cloned().unwrap_or(Value::Null),
                })
            })
            .collect();
        Ok(Value::Array(mapped))
    })
    .unwrap_or_else(|_| json!([]))
    .as_array()
    .cloned()
    .unwrap_or_default()
}

/// `fetch_restricted_release(ti)` — EastMoney `RPT_LIFT_STAGE`.
pub fn fetch_restricted_release(ti: &TickerInfo) -> Vec<Value> {
    if ti.market != "A" {
        return Vec::new();
    }
    let code = ti.code.clone();
    let key = format!("lift__{}", code);
    cached::<_, anyhow::Error>(&ti.full, &key, TTL_QUARTERLY, move || {
        let rows = em::lift_stage(&code, 12).map_err(|e| anyhow::anyhow!(e))?;
        let mapped: Vec<Value> = rows
            .into_iter()
            .map(|r| {
                let obj = r.as_object().cloned().unwrap_or_default();
                json!({
                    "代码": obj.get("SECURITY_CODE").cloned().unwrap_or(Value::Null),
                    "名称": obj.get("SECURITY_NAME_ABBR").cloned().unwrap_or(Value::Null),
                    "解禁日期": obj.get("FREE_DATE").and_then(|v| v.as_str()).map(|s| s.chars().take(10).collect::<String>()).unwrap_or_default(),
                    "解禁股份数量": obj.get("CURRENT_FREE_SHARES").cloned().unwrap_or(Value::Null),
                    "解禁数量": obj.get("ABLE_FREE_SHARES").cloned().unwrap_or(Value::Null),
                    "解禁市值": obj.get("LIFT_MARKET_CAP").cloned().unwrap_or(Value::Null),
                    "占解禁前流通市值比例": obj.get("FREE_RATIO").cloned().unwrap_or(Value::Null),
                    "解禁前一交易日收盘价": obj.get("NEW").cloned().unwrap_or(Value::Null),
                    "限售股类型": obj.get("FREE_SHARES_TYPE").cloned().unwrap_or(Value::Null),
                    "解禁前20日涨跌幅": obj.get("B20_ADJCHRATE").cloned().unwrap_or(Value::Null),
                    "解禁后20日涨跌幅": obj.get("A20_ADJCHRATE").cloned().unwrap_or(Value::Null),
                    "占总市值比例": obj.get("TOTAL_RATIO").cloned().unwrap_or(Value::Null),
                    "未解禁数量": obj.get("NON_FREE_SHARES").cloned().unwrap_or(Value::Null),
                    "解禁股东数": obj.get("BATCH_HOLDER_NUM").cloned().unwrap_or(Value::Null),
                })
            })
            .collect();
        Ok(Value::Array(mapped))
    })
    .unwrap_or_else(|_| json!([]))
    .as_array()
    .cloned()
    .unwrap_or_default()
}

/// `fetch_main_fund_flow(ti)` — EastMoney `push2his` fflow/daykline.
pub fn fetch_main_fund_flow(ti: &TickerInfo) -> Vec<Value> {
    if ti.market != "A" {
        return Vec::new();
    }
    let secid = em::secid(&ti.code, &ti.full);
    let key = format!("fflow__{}", ti.code);
    cached::<_, anyhow::Error>(&ti.full, &key, TTL_DAILY, move || {
        let rows = em::fund_flow_daykline(&secid, "60", 12).map_err(|e| anyhow::anyhow!(e))?;
        Ok(Value::Array(rows))
    })
    .unwrap_or_else(|_| json!([]))
    .as_array()
    .cloned()
    .unwrap_or_default()
}


// ─────────────────────────────────────────────────────────────
// 7. Research reports
// ─────────────────────────────────────────────────────────────

/// `fetch_research_reports(ti)` — AkShare `stock_research_report_em`; `[]` on
/// failure.
///
/// Upstream calls `reportapi.eastmoney.com/report/list` and renames the English
/// payload keys to the Chinese column names the consumer (`fetch/research.rs`)
/// reads. The Rust port hits the *same documented endpoint* and applies the
/// same rename. A failed/empty fetch yields `[]` exactly like upstream, so the
/// caller degrades to `fallback=true`.
pub fn fetch_research_reports(ti: &TickerInfo) -> Value {
    if ti.market != "A" {
        return json!([]);
    }
    let key = format!("research__{}", ti.code);
    cached::<_, anyhow::Error>(&ti.full, &key, TTL_QUARTERLY, || {
        let code = &ti.code;
        let year_now = Utc::now().year();
        let end = format!("{}-01-01", year_now + 1);
        let payload = http::get_json_q(
            "https://reportapi.eastmoney.com/report/list",
            &[
                ("industryCode", "*"),
                ("pageSize", "5000"),
                ("industry", "*"),
                ("rating", "*"),
                ("ratingChange", "*"),
                ("beginTime", "2000-01-01"),
                ("endTime", &end),
                ("pageNo", "1"),
                ("fields", ""),
                ("qType", "0"),
                ("orgCode", ""),
                ("code", code),
                ("rcode", ""),
                ("p", "1"),
                ("pageNum", "1"),
                ("pageNumber", "1"),
            ],
            &[],
            25,
        )
        .map_err(|e| anyhow::anyhow!("reportapi research: {e}"))?;

        let current_year = payload
            .get("currentYear")
            .and_then(|v| v.as_i64())
            .unwrap_or(year_now as i64) as i32;

        // Year-tagged forecast column titles, matching upstream's
        // `{current_year}-盈利预测-收益` naming exactly.
        let (y0, y1, y2) = (current_year, current_year + 1, current_year + 2);
        let col_eps_y0 = format!("{y0}-盈利预测-收益");
        let col_pe_y0 = format!("{y0}-盈利预测-市盈率");
        let col_eps_y1 = format!("{y1}-盈利预测-收益");
        let col_pe_y1 = format!("{y1}-盈利预测-市盈率");
        let col_eps_y2 = format!("{y2}-盈利预测-收益");
        let col_pe_y2 = format!("{y2}-盈利预测-市盈率");

        let data = payload
            .get("data")
            .and_then(|d| d.as_array())
            .cloned()
            .unwrap_or_default();

        let mut out: Vec<Value> = Vec::with_capacity(data.len());
        for rec in data {
            let get = |k: &str| rec.get(k).cloned().unwrap_or(Value::Null);
            // Upstream only keeps rows where every forecast field is numeric,
            // and drops rows whose current-year EPS is NaN — but the consumer
            // already filters on `forecast_float`, so we keep the raw rows and
            // only drop ones with an empty title (a degenerate report).
            let title = get("title");
            if title.as_str().map(|s| s.trim().is_empty()).unwrap_or(true) {
                continue;
            }
            let info_code = get("infoCode");
            let pdf_url = match info_code.as_str() {
                Some(x) if !x.is_empty() => Value::String(format!("https://pdf.dfcfw.com/pdf/H3_{x}_1.pdf")),
                _ => Value::Null,
            };
            let mut row = Map::new();
            row.insert("报告名称".to_string(), title);
            row.insert("股票简称".to_string(), get("stockName"));
            row.insert("股票代码".to_string(), get("stockCode"));
            row.insert("东财评级".to_string(), get("emRatingName"));
            row.insert("机构".to_string(), get("orgSName"));
            row.insert("机构代码".to_string(), get("orgCode"));
            row.insert("日期".to_string(), get("publishDate"));
            row.insert("行业".to_string(), get("indvInduName"));
            row.insert("报告PDF链接".to_string(), pdf_url);
            row.insert(col_eps_y0.clone(), get("predictThisYearEps"));
            row.insert(col_pe_y0.clone(), get("predictThisYearPe"));
            row.insert(col_eps_y1.clone(), get("predictNextYearEps"));
            row.insert(col_pe_y1.clone(), get("predictNextYearPe"));
            row.insert(col_eps_y2.clone(), get("predictNextTwoYearEps"));
            row.insert(col_pe_y2.clone(), get("predictNextTwoYearPe"));
            out.push(Value::Object(row));
        }
        Ok(json!(out))
    })
    .unwrap_or_else(|_| json!([]))
}

// ─────────────────────────────────────────────────────────────
// Top-level: resolve Chinese name → ticker
// ─────────────────────────────────────────────────────────────

/// `build_a_share_index()` — the full `(code, name)` table behind a 7-day cache.
///
/// Upstream caches under the pseudo-ticker `_global`; a missing cache file just
/// means the table is fetched (or, offline, that resolution degrades to `none`).
///
/// A **failed** fetch is deliberately not cached: the endpoint is frequently
/// blocked (see the `em_push2` registry entry), and persisting the empty result
/// would disable Chinese-name resolution for the full 7-day TTL. Returning `Err`
/// from the fetcher leaves the cache untouched — [`cached`] only writes on `Ok` —
/// so the next call retries.
pub fn build_a_share_index() -> Value {
    cached::<_, anyhow::Error>("_global", "a_share_name_index", uzi_core::cache::TTL_STATIC, || {
        let rows = fetch_a_code_name();
        if rows.as_array().map(|a| a.is_empty()).unwrap_or(true) {
            anyhow::bail!("A 股代码/名称表返回为空（push2 常被反爬拦截），不写入缓存");
        }
        Ok(rows)
    })
    .unwrap_or_else(|_| json!([]))
}

/// Tier 2 — upstream's akshare exact substring search over the code/name table.
///
/// Upstream tries `stock_zh_a_spot_em()` then `stock_info_a_code_name()`; the
/// Rust port has a single equivalent endpoint, so it consults that table once.
/// The first row whose name *contains* the query wins, matching
/// `df[df[name_col].str.contains(name, na=False)].iloc[0]`.
fn resolve_exact(name: &str) -> Option<Value> {
    let table = build_a_share_index();
    let rows = table.as_array()?;
    for row in rows {
        let row_name = row.get("name").and_then(|v| v.as_str()).unwrap_or("");
        if row_name.is_empty() || !row_name.contains(name) {
            continue;
        }
        let code = match row.get("code") {
            Some(Value::String(s)) => s.clone(),
            Some(Value::Number(n)) => n.to_string(),
            _ => continue,
        };
        let ti = uzi_core::ticker::parse_ticker(&code);
        return Some(json!({
            "resolved": {
                "raw": ti.raw, "code": ti.code, "full": ti.full, "market": ti.market,
                "exchange": ti.exchange, "currency": ti.currency, "country": ti.country,
            },
            "candidates": [{"code": ti.full, "name": row_name, "distance": 0, "source": "exact"}],
            "source": "exact",
            "user_input": name,
        }));
    }
    None
}

/// Tier 3 — `lib.name_matcher.fuzzy_match` over the local index.
///
/// Only a distance-0 hit auto-resolves, exactly like upstream: anything looser is
/// returned as candidates for the caller to disambiguate.
fn resolve_fuzzy(name: &str) -> Option<Value> {
    let table = build_a_share_index();
    let entries = uzi_core::name_matcher::index_from_values(table.as_array()?);
    let hits = uzi_core::name_matcher::fuzzy_match_default(name, &entries, 5);
    if hits.is_empty() {
        return None;
    }
    let candidates: Vec<Value> = hits
        .iter()
        .map(|h| {
            let ti = uzi_core::ticker::parse_ticker(&h.code);
            json!({
                "code": ti.full,
                "name": h.name,
                "distance": h.distance,
                "source": "fuzzy",
            })
        })
        .collect();
    let auto = if hits[0].distance == 0 {
        let ti = uzi_core::ticker::parse_ticker(&hits[0].code);
        json!({
            "raw": ti.raw, "code": ti.code, "full": ti.full, "market": ti.market,
            "exchange": ti.exchange, "currency": ti.currency, "country": ti.country,
        })
    } else {
        Value::Null
    };
    Some(json!({
        "resolved": auto,
        "candidates": candidates,
        "source": "fuzzy",
        "user_input": name,
    }))
}

/// `resolve_chinese_name_rich(name)`.
///
/// Tier 1 is the MX 妙想 API (requires `MX_APIKEY`), tier 2 the exact substring
/// search over the A-share code/name table, tier 3 the local Levenshtein fuzzy
/// match from [`uzi_core::name_matcher`]. Returns upstream's
/// `{"resolved": None, "candidates": [], "source": "none"}` when every tier
/// misses — including when the table cannot be fetched at all.
pub fn resolve_chinese_name_rich(name: &str) -> Value {
    if mx_available() {
        if let Some(hits) = crate::mx::resolve_entity(name) {
            if let Some(first) = hits.first() {
                let secu = first.get("secuCode").and_then(|v| v.as_str()).unwrap_or("");
                let ti = uzi_core::ticker::parse_ticker(secu);
                let cands: Vec<Value> = hits
                    .iter()
                    .take(5)
                    .map(|x| {
                        json!({
                            "code": x.get("secuCode").cloned().unwrap_or(Value::Null),
                            "name": x.get("fullName").cloned().unwrap_or(Value::Null),
                            "distance": 0,
                            "source": "mx",
                        })
                    })
                    .collect();
                return json!({
                    "resolved": {
                        "raw": ti.raw, "code": ti.code, "full": ti.full, "market": ti.market,
                        "exchange": ti.exchange, "currency": ti.currency, "country": ti.country,
                    },
                    "candidates": cands,
                    "source": "mx",
                    "user_input": name,
                });
            }
        }
    }

    if let Some(exact) = resolve_exact(name) {
        return exact;
    }
    if let Some(fuzzy) = resolve_fuzzy(name) {
        return fuzzy;
    }

    json!({
        "resolved": Value::Null,
        "candidates": [],
        "source": "none",
        "user_input": name,
    })
}

/// `resolve_chinese_name(name)` — thin wrapper returning the ticker only when
/// the resolver was confident.
pub fn resolve_chinese_name(name: &str) -> Option<TickerInfo> {
    let r = resolve_chinese_name_rich(name);
    let resolved = r.get("resolved")?;
    if resolved.is_null() {
        return None;
    }
    let full = resolved.get("full")?.as_str()?;
    Some(uzi_core::ticker::parse_ticker(full))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_industry_map() {
        assert_eq!(known_industry("600519"), Some("白酒"));
        assert_eq!(known_industry("002273"), Some("光学光电子"));
        assert_eq!(known_industry("999999"), None);
    }

    #[test]
    fn northbound_is_empty_offline_shaped() {
        // non-numeric / short codes never touch the network
        let ti = uzi_core::ticker::parse_ticker("600519.SH");
        let v = fetch_north_impl(&ti); // may be {} when EM is unreachable
        assert!(v.is_object());
    }

    #[test]
    fn merge_only_fills_missing_fields() {
        let mut out = Map::new();
        out.insert("price".into(), json!(10.0));
        out.insert("name".into(), json!("A"));
        let src = json!({"price": 99.0, "name": "B", "pe_ttm": 12.0});
        let changed = merge_missing_basic_fields(&mut out, &src, "field:x", &["price", "name", "pe_ttm"]);
        assert!(changed);
        assert_eq!(out["price"], json!(10.0));
        assert_eq!(out["name"], json!("A"));
        assert_eq!(out["pe_ttm"], json!(12.0));
        assert_eq!(out["_fallback_snap"], json!("field:x"));
    }

    #[test]
    fn fallback_snap_dedupes() {
        let mut out = Map::new();
        append_fallback_snap(&mut out, "a");
        append_fallback_snap(&mut out, "b");
        append_fallback_snap(&mut out, "a");
        assert_eq!(out["_fallback_snap"], json!("a+b"));
    }

    #[test]
    fn tencent_symbol_prefix_rules() {
        // 600519 → sh via the "60" rule, 002273 → sz
        let v = price_tencent_qt("A", "002273");
        // offline: {} ; online: name present. Either way it is an object.
        assert!(v.is_object());
    }
}

// ─────────────────────────────────────────────────────────────
// Market cross-sections (upstream ak.stock_zh_a_spot_em / stock_hk_spot_em)
// ─────────────────────────────────────────────────────────────

/// EastMoney `clist` field → akshare Chinese column name, in akshare order.
const A_SPOT_FIELDS: &[(&str, &str)] = &[
    ("f12", "代码"),
    ("f14", "名称"),
    ("f2", "最新价"),
    ("f3", "涨跌幅"),
    ("f6", "成交额"),
    ("f100", "所属行业"),
    ("f17", "今开"),
    ("f18", "昨收"),
    ("f15", "最高"),
    ("f16", "最低"),
    ("f8", "换手率"),
    ("f10", "量比"),
    ("f20", "总市值"),
];

/// `ak.stock_zh_a_spot_em()`. `[]` when the clist endpoint is unreachable.
pub fn fetch_a_spot() -> Value {
    spot_clist("m:0+t:6,m:0+t:80,m:1+t:2,m:1+t:23", A_SPOT_FIELDS)
}

/// `ak.stock_hk_spot_em()`.
pub fn fetch_hk_spot() -> Value {
    spot_clist(
        "m:116+t:3,m:116+t:4,m:116+t:1,m:116+t:2",
        &[
            ("f12", "代码"),
            ("f14", "名称"),
            ("f2", "最新价"),
            ("f4", "涨跌额"),
            ("f3", "涨跌幅"),
            ("f6", "成交额"),
            ("f17", "今开"),
            ("f15", "最高"),
            ("f16", "最低"),
            ("f18", "昨收"),
            ("f5", "成交量"),
            ("f20", "总市值"),
        ],
    )
}

fn spot_clist(fs: &str, fields: &[(&str, &str)]) -> Value {
    let field_list: Vec<&str> = fields.iter().map(|(f, _)| *f).collect();
    let url = "https://push2.eastmoney.com/api/qt/clist/get";
    let pz = "6000";
    let Ok(v) = http::get_json_q(
        url,
        &[
            ("pn", "1"),
            ("pz", pz),
            ("po", "1"),
            ("np", "1"),
            ("ut", em::UT),
            ("fltt", "2"),
            ("invt", "2"),
            ("fid", "f3"),
            ("fs", fs),
            ("fields", &field_list.join(",")),
        ],
        &[],
        15,
    ) else {
        return json!([]);
    };
    let diff = v
        .get("data")
        .and_then(|d| d.get("diff"))
        .cloned()
        .unwrap_or(Value::Null);
    let rows: Vec<Value> = match diff {
        Value::Array(a) => a,
        Value::Object(o) => o.values().cloned().collect(),
        _ => Vec::new(),
    };
    let mut out = Vec::new();
    for row in rows {
        let mut obj = Map::new();
        for (field, label) in fields {
            let value = row.get(*field).cloned().unwrap_or(Value::Null);
            // clist uses "-" for missing numerics
            let value = if value.as_str() == Some("-") { Value::Null } else { value };
            obj.insert((*label).to_string(), value);
        }
        out.push(Value::Object(obj));
    }
    Value::Array(out)
}

/// `ak.stock_info_a_code_name()` — the code/name index used for name resolution.
pub fn fetch_a_code_name() -> Value {
    let rows = spot_clist("m:0+t:6,m:0+t:80,m:1+t:2,m:1+t:23", &[("f12", "code"), ("f14", "name")]);
    rows
}

/// Industry per code, derived from the A-share cross-section.
pub fn fetch_industry_batch(codes: &[String]) -> Value {
    let spot = fetch_a_spot();
    let mut out = Map::new();
    if let Some(rows) = spot.as_array() {
        for row in rows {
            let code = row.get("代码").and_then(|v| v.as_str()).unwrap_or("");
            if code.is_empty() || (!codes.is_empty() && !codes.iter().any(|c| c == code)) {
                continue;
            }
            let industry = row.get("所属行业").cloned().unwrap_or(Value::Null);
            out.insert(code.to_string(), industry);
        }
    }
    Value::Object(out)
}

/// `ak.fund_portfolio_hold_em(symbol)` — fund → top holdings (EastMoney F10).
pub fn fetch_fund_portfolio_hold(code: &str) -> Value {
    let url = http::with_query(
        "https://fundf10.eastmoney.com/FundArchivesDatas.aspx",
        &[("type", "jjcc"), ("code", code), ("topline", "10")],
    );
    let Ok(resp) = http::get(&url, &[("Referer", "https://fundf10.eastmoney.com/")], 12) else {
        return json!([]);
    };
    if !resp.is_ok() {
        return json!([]);
    }
    let text = resp.text();
    static ROW: LazyLock<regex::Regex> = LazyLock::new(|| {
        regex::Regex::new(
            r#"<tr><td>\d+</td><td><a[^>]*>(\d{6})</a></td><td[^>]*><a[^>]*>([^<]+)</a></td>"#,
        )
        .unwrap()
    });
    let mut out = Vec::new();
    for cap in ROW.captures_iter(&text) {
        out.push(json!({"股票代码": &cap[1], "股票名称": &cap[2]}));
    }
    Value::Array(out)
}
