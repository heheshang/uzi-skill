//! Crypto venue (market `"C"`) data layer.
//!
//! Every dimension is produced from keyless public APIs, with a fallback chain
//! per concern:
//!
//! | concern | primary | fallbacks |
//! |---|---|---|
//! | market cap / supply / ATH / detail | CoinGecko `/coins/markets`, `/coins/{id}` | CoinGecko `/search` → retry |
//! | price / 24h change | CoinGecko markets | OKX `/market/ticker` |
//! | daily OHLCV | OKX `/market/candles` | Binance `/api/v3/klines`, CoinGecko `/market_chart` |
//! | global market + BTC dominance | CoinGecko `/global` | — |
//! | fear & greed | alternative.me `/fng/` | — |
//! | perpetual funding / OI | OKX `/public/funding-rate`, `/public/open-interest` | — |
//!
//! `coin_id` lookups that miss the registry self-heal through CoinGecko search,
//! so an unlisted coin still works via `FOO-USD` / `FOO.CRYPTO`.
//!
//! Ownership: [`dim`] is the single entry point used by
//! [`crate::collect::run_fetcher_job`]; it returns `None` for dims with no
//! crypto-specific payload so the generic (empty) path still runs.

use std::sync::LazyLock;

use serde_json::{json, Map, Value};

use uzi_core::cache::{cached, TTL_DAILY, TTL_HOURLY, TTL_INTRADAY};
use uzi_core::crypto::{self, CryptoCoin};
use uzi_core::py::round;
use uzi_core::ticker::{TickerInfo, CRYPTO_MARKET};

use crate::fetch::kline;
use crate::http;

/// One crypto dimension payload: the `data` object plus its provenance.
pub struct CryptoDim {
    pub data: Value,
    pub source: String,
}

impl CryptoDim {
    fn new(data: Value, source: impl Into<String>) -> Self {
        CryptoDim {
            data,
            source: source.into(),
        }
    }
}

/// `ti.market == "C"`.
pub fn is_crypto(ti: &TickerInfo) -> bool {
    ti.market == CRYPTO_MARKET
}

// ─────────────────────────────────────────────────────────────
// HTTP / cache plumbing
// ─────────────────────────────────────────────────────────────

fn timeout() -> u64 {
    http::timeout_default()
}

/// CoinGecko base URL — the Pro host when `UZI_COINGECKO_KEY` /
/// `COINGECKO_API_KEY` is configured, else the public API.
fn cg_base() -> &'static str {
    if cg_key().is_some() {
        "https://pro-api.coingecko.com/api/v3"
    } else {
        "https://api.coingecko.com/api/v3"
    }
}

fn cg_key() -> Option<String> {
    ["UZI_COINGECKO_KEY", "COINGECKO_API_KEY"]
        .iter()
        .find_map(|k| std::env::var(k).ok().filter(|v| !v.is_empty()))
}

fn cg_headers() -> Vec<(&'static str, String)> {
    let mut h: Vec<(&'static str, String)> = vec![("User-Agent", http::UA.to_string())];
    if let Some(key) = cg_key() {
        h.push(("x-cg-pro-api-key", key));
    }
    h
}

/// Cached JSON GET. `bucket` is the cache directory (a ticker or `_global`).
///
/// A 429 from the free CoinGecko tier is retried once after a short backoff —
/// the parallel dim wave would otherwise turn one rate-limit response into a
/// dozen permanently empty dimensions.
fn cached_json(
    bucket: &str,
    key: &str,
    url: &str,
    headers: &[(&str, String)],
    ttl: u64,
) -> Option<Value> {
    let hdrs: Vec<(&str, &str)> = headers.iter().map(|(k, v)| (*k, v.as_str())).collect();
    cached::<_, anyhow::Error>(bucket, key, ttl, || {
        let mut attempt = 0u32;
        loop {
            let resp = http::get(url, &hdrs, timeout()).map_err(|e| anyhow::anyhow!(e))?;
            if resp.status == 429 && attempt < 1 {
                attempt += 1;
                std::thread::sleep(std::time::Duration::from_millis(2000));
                continue;
            }
            if !resp.is_ok() {
                anyhow::bail!("HTTP {}", resp.status);
            }
            return resp
                .json()
                .ok_or_else(|| anyhow::anyhow!("invalid JSON from {url}"));
        }
    })
    .ok()
}

/// Process-wide memo for the shared crypto fetches.
///
/// `dim()` runs once per dimension on parallel worker threads; without this,
/// every dim would re-issue the same 6 requests (20× the network traffic, and an
/// instant rate-limit on CoinGecko's free tier). Failures are memoized too —
/// re-hammering a 429 during the same run never helps.
fn shared<T: Clone + Send + 'static>(key: &str, f: impl FnOnce() -> T) -> T {
    use std::any::Any;
    use std::collections::HashMap;
    use std::sync::{LazyLock, Mutex};

    static MEMO: LazyLock<Mutex<HashMap<String, Box<dyn Any + Send>>>> =
        LazyLock::new(|| Mutex::new(HashMap::new()));

    if let Ok(guard) = MEMO.lock() {
        if let Some(hit) = guard.get(key).and_then(|v| v.downcast_ref::<T>()) {
            return hit.clone();
        }
    }
    let value = f();
    if let Ok(mut guard) = MEMO.lock() {
        guard.insert(key.to_string(), Box::new(value.clone()));
    }
    value
}

fn cg_get(bucket: &str, path: &str, params: &[(&str, &str)], ttl: u64) -> Option<Value> {
    let url = http::with_query(&format!("{}{}", cg_base(), path), params);
    let key = format!("cg__{}", path.trim_start_matches('/').replace('/', "_"));
    cached_json(bucket, &key, &url, &cg_headers(), ttl)
}

// ─────────────────────────────────────────────────────────────
// Small helpers
// ─────────────────────────────────────────────────────────────

fn as_f64(v: &Value) -> Option<f64> {
    match v {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.trim().parse::<f64>().ok(),
        _ => None,
    }
}

fn num(v: &Value) -> Option<f64> {
    as_f64(v).filter(|x| x.is_finite())
}

fn num_at(v: &Value, path: &[&str]) -> Option<f64> {
    let mut cur = v;
    for k in path {
        cur = cur.get(*k)?;
    }
    num(cur)
}

fn obj<'a>(v: &'a Value, key: &str) -> &'a Value {
    v.get(key).unwrap_or(&Value::Null)
}

/// Compact Chinese money formatting (`1.18万亿` / `4500亿` / `12.3亿`).
fn fmt_usd(v: f64) -> String {
    let a = v.abs();
    if a >= 1e12 {
        format!("{:.2}万亿", v / 1e12)
    } else if a >= 1e8 {
        format!("{:.0}亿", v / 1e8)
    } else if a >= 1e4 {
        format!("{:.0}万", v / 1e4)
    } else {
        format!("{v:.2}")
    }
}

fn jnum(x: f64) -> Value {
    serde_json::Number::from_f64(x)
        .map(Value::Number)
        .unwrap_or(Value::Null)
}

fn round2(x: f64) -> Value {
    jnum(round(x, 2))
}

/// The quote currency mapped to a CoinGecko `vs_currency`.
fn vs_currency(quote: &str) -> &'static str {
    match quote.to_ascii_uppercase().as_str() {
        "EUR" => "eur",
        _ => "usd",
    }
}

/// Whether the quote is a USD-equivalent (so USD market data is a valid proxy).
fn usd_like(quote: &str) -> bool {
    matches!(
        quote.to_ascii_uppercase().as_str(),
        "USD" | "USDT" | "USDC" | "BUSD" | "DAI" | "TUSD" | "FDUSD"
    )
}

/// Registry entry for the ticker, else a synthetic one derived from the code.
fn coin_of(ti: &TickerInfo) -> Option<&'static CryptoCoin> {
    crypto::find(&ti.code)
}

// ─────────────────────────────────────────────────────────────
// CoinGecko data access
// ─────────────────────────────────────────────────────────────

/// Resolve a CoinGecko id, falling back to `/search` for unregistered symbols.
///
/// Only an exact symbol match is accepted — guessing the first search hit would
/// silently analyse the wrong asset, which is worse than reporting a gap.
fn resolve_id(ti: &TickerInfo) -> Option<String> {
    if let Some(c) = coin_of(ti) {
        return Some(c.coingecko_id.to_string());
    }
    let hits = cg_get("_global", "/search", &[("query", ti.code.as_str())], TTL_DAILY)?;
    hits.get("coins")
        .and_then(|v| v.as_array())?
        .iter()
        .find(|c| {
            c.get("symbol")
                .and_then(|s| s.as_str())
                .map(|s| s.eq_ignore_ascii_case(&ti.code))
                .unwrap_or(false)
        })
        .and_then(|c| c.get("id").and_then(|v| v.as_str()))
        .map(str::to_string)
}

/// `/coins/markets` row for this coin (USD-denominated).
fn markets_row(ti: &TickerInfo) -> Option<Value> {
    let id = resolve_id(ti)?;
    let vs = vs_currency(&ti.currency);
    let row = cg_get(
        &ti.full,
        "/coins/markets",
        &[
            ("vs_currency", vs),
            ("ids", id.as_str()),
            ("price_change_percentage", "1h,24h,7d,30d,1y"),
        ],
        TTL_INTRADAY,
    )?;
    row.as_array()?.first().cloned()
}

/// `/coins/{id}` detail row (description, links, developer/community data).
fn coin_detail(ti: &TickerInfo) -> Option<Value> {
    let id = resolve_id(ti)?;
    cg_get(
        &ti.full,
        &format!("/coins/{id}"),
        &[
            ("localization", "false"),
            ("tickers", "false"),
            ("market_data", "true"),
            ("community_data", "true"),
            ("developer_data", "true"),
            ("sparkline", "false"),
        ],
        TTL_DAILY,
    )
}

/// `/global` snapshot.
fn global_snapshot() -> Option<Value> {
    cg_get("_global", "/global", &[], TTL_INTRADAY)
        .and_then(|v| v.get("data").cloned())
}

/// alternative.me fear & greed history (`limit` days, newest first).
fn fear_greed(limit: usize) -> Option<Vec<Value>> {
    let url = http::with_query(
        "https://api.alternative.me/fng/",
        &[("limit", &limit.to_string()), ("format", "json")],
    );
    let v = cached_json("_global", "fng__history", &url, &[], TTL_HOURLY)?;
    v.get("data").and_then(|d| d.as_array()).cloned()
}

/// CoinGecko `/coins/markets` for an explicit id list (stablecoins, sectors).
fn markets_for(ids: &str, per_page: usize) -> Vec<Value> {
    cg_get(
        "_global",
        "/coins/markets",
        &[
            ("vs_currency", "usd"),
            ("ids", ids),
            ("per_page", &per_page.to_string()),
        ],
        TTL_INTRADAY,
    )
    .and_then(|v| v.as_array().cloned())
    .unwrap_or_default()
}

/// Top coins by market cap (excluding `self_id` when given).
fn top_markets(per_page: usize) -> Vec<Value> {
    cg_get(
        "_global",
        "/coins/markets",
        &[
            ("vs_currency", "usd"),
            ("order", "market_cap_desc"),
            ("per_page", &per_page.to_string()),
            ("page", "1"),
            ("price_change_percentage", "24h,7d,30d"),
        ],
        TTL_INTRADAY,
    )
    .and_then(|v| v.as_array().cloned())
    .unwrap_or_default()
}

// ─────────────────────────────────────────────────────────────
// Price / kline fallback chain
// ─────────────────────────────────────────────────────────────

/// Counter currency for CEX pair lookups: USD-like quotes collapse to USDT,
/// and a stablecoin base gets the *other* stablecoin so pairs never become
/// self-referential (`USDT-USDT` / `USDTUSDT` do not exist).
fn counter_ccy(ti: &TickerInfo) -> String {
    let base = ti.code.to_ascii_uppercase();
    if usd_like(&ti.currency) {
        if base == "USDT" {
            "USDC".to_string()
        } else {
            "USDT".to_string()
        }
    } else {
        ti.currency.to_ascii_uppercase()
    }
}

/// OKX spot instrument id (`BTC-USDT`).
fn okx_inst(ti: &TickerInfo) -> String {
    format!("{}-{}", ti.code, counter_ccy(ti))
}

/// OKX `/market/ticker` → `{last, open24h, high24h, low24h, volCcy24h}`.
fn okx_ticker(ti: &TickerInfo) -> Option<Value> {
    let inst = okx_inst(ti);
    let url = http::with_query(
        "https://www.okx.com/api/v5/market/ticker",
        &[("instId", inst.as_str())],
    );
    cached_json(
        &ti.full,
        &format!("okx_ticker__{inst}"),
        &url,
        &[("User-Agent", http::UA.to_string())],
        TTL_INTRADAY,
    )
    .and_then(|v| v.get("data").and_then(|d| d.as_array()).and_then(|a| a.first()).cloned())
}

/// OKX daily candles → A-share-shaped OHLCV rows (oldest first).
fn okx_klines(ti: &TickerInfo) -> Vec<Value> {
    let inst = okx_inst(ti);
    let url = http::with_query(
        "https://www.okx.com/api/v5/market/candles",
        &[("instId", inst.as_str()), ("bar", "1D"), ("limit", "400")],
    );
    let rows = cached_json(
        &ti.full,
        &format!("okx_klines__{inst}"),
        &url,
        &[("User-Agent", http::UA.to_string())],
        TTL_INTRADAY,
    )
    .and_then(|v| v.get("data").and_then(|d| d.as_array()).cloned())
    .unwrap_or_default();

    let mut out: Vec<Value> = rows
        .iter()
        .filter_map(|r| r.as_array())
        .filter(|r| r.len() >= 6)
        .map(|r| {
            // [ts, o, h, l, c, vol, volCcy, volCcyQuote, confirm]
            let date = chrono::DateTime::from_timestamp_millis(as_f64(&r[0])? as i64)
                .map(|d| d.format("%Y-%m-%d").to_string())
                .unwrap_or_default();
            Some(json!({
                "日期": date,
                "开盘": num(&r[1]),
                "最高": num(&r[2]),
                "最低": num(&r[3]),
                "收盘": num(&r[4]),
                "成交量": num(&r[5]),
            }))
        })
        .flatten()
        .collect();
    out.reverse();
    out
}

/// Binance daily klines → A-share-shaped OHLCV rows (oldest first).
fn binance_klines(ti: &TickerInfo) -> Vec<Value> {
    let symbol = format!("{}{}", ti.code, counter_ccy(ti));
    let url = http::with_query(
        "https://api.binance.com/api/v3/klines",
        &[("symbol", symbol.as_str()), ("interval", "1d"), ("limit", "400")],
    );
    let rows = cached_json(
        &ti.full,
        &format!("binance_klines__{symbol}"),
        &url,
        &[("User-Agent", http::UA.to_string())],
        TTL_INTRADAY,
    )
    .and_then(|v| v.as_array().cloned())
    .unwrap_or_default();

    rows.iter()
        .filter_map(|r| r.as_array())
        .filter(|r| r.len() >= 6)
        .map(|r| {
            let date = chrono::DateTime::from_timestamp_millis(as_f64(&r[0])? as i64)
                .map(|d| d.format("%Y-%m-%d").to_string())
                .unwrap_or_default();
            Some(json!({
                "日期": date,
                "开盘": num(&r[1]),
                "最高": num(&r[2]),
                "最低": num(&r[3]),
                "收盘": num(&r[4]),
                "成交量": num(&r[5]),
            }))
        })
        .flatten()
        .collect()
}

/// CoinGecko `/market_chart` (close-only) — last-resort kline source.
///
/// `days=365` is the free-tier ceiling (the Demo plan rejects >365 with error
/// 10012); auto granularity returns daily closes for that range.
fn cg_klines(ti: &TickerInfo) -> Vec<Value> {
    let id = match resolve_id(ti) {
        Some(id) => id,
        None => return Vec::new(),
    };
    let Some(v) = cg_get(
        &ti.full,
        &format!("/coins/{id}/market_chart"),
        &[("vs_currency", "usd"), ("days", "365")],
        TTL_INTRADAY,
    ) else {
        return Vec::new();
    };
    let prices = v
        .get("prices")
        .and_then(|p| p.as_array())
        .cloned()
        .unwrap_or_default();
    let volumes = v
        .get("total_volumes")
        .and_then(|p| p.as_array())
        .cloned()
        .unwrap_or_default();
    prices
        .iter()
        .enumerate()
        .filter_map(|(i, p)| {
            let arr = p.as_array()?;
            let close = as_f64(arr.get(1)?)?;
            let date = chrono::DateTime::from_timestamp_millis(as_f64(arr.first()?)? as i64)
                .map(|d| d.format("%Y-%m-%d").to_string())
                .unwrap_or_default();
            let vol = volumes
                .get(i)
                .and_then(|v| v.as_array())
                .and_then(|a| a.get(1))
                .and_then(num);
            Some(json!({
                "日期": date,
                "开盘": close,
                "最高": close,
                "最低": close,
                "收盘": close,
                "成交量": vol,
            }))
        })
        .collect()
}

/// Daily OHLCV with the fallback chain, plus the source label.
fn klines(ti: &TickerInfo) -> (Vec<Value>, &'static str) {
    let o = okx_klines(ti);
    if o.len() >= 30 {
        return (o, "okx:/market/candles");
    }
    let b = binance_klines(ti);
    if b.len() >= 30 {
        return (b, "binance:/api/v3/klines");
    }
    let c = cg_klines(ti);
    if !c.is_empty() {
        return (c, "coingecko:/coins/{id}/market_chart");
    }
    (o, "okx:/market/candles")
}

/// Best available price + 24h change + high/low + volume.
#[derive(Clone)]
struct Quote {
    price: f64,
    change_24h: Option<f64>,
    high_24h: Option<f64>,
    low_24h: Option<f64>,
    volume_24h: Option<f64>,
    source: &'static str,
}

fn quote(ti: &TickerInfo, row: Option<&Value>) -> Option<Quote> {
    if let Some(r) = row {
        if let Some(price) = num_at(r, &["current_price"]).filter(|p| *p > 0.0) {
            return Some(Quote {
                price,
                change_24h: num_at(r, &["price_change_percentage_24h_in_currency"])
                    .or_else(|| num_at(r, &["price_change_percentage_24h"])),
                high_24h: num_at(r, &["high_24h"]),
                low_24h: num_at(r, &["low_24h"]),
                volume_24h: num_at(r, &["total_volume"]),
                source: "coingecko:/coins/markets",
            });
        }
    }
    let t = okx_ticker(ti)?;
    let last = num_at(&t, &["last"]).filter(|p| *p > 0.0)?;
    let open = num_at(&t, &["open24h"]).filter(|p| *p > 0.0);
    Some(Quote {
        price: last,
        change_24h: open.map(|o| (last - o) / o * 100.0),
        high_24h: num_at(&t, &["high24h"]),
        low_24h: num_at(&t, &["low24h"]),
        volume_24h: num_at(&t, &["volCcy24h"]),
        source: "okx:/market/ticker",
    })
}

// ─────────────────────────────────────────────────────────────
// News
// ─────────────────────────────────────────────────────────────

/// Chinese-market news filtered for this coin / crypto keywords. Global news
/// feeds carry the crypto headline flow; the filter keeps the payload relevant.
fn crypto_news(ti: &TickerInfo, limit: usize) -> Vec<Value> {
    const KEYWORDS: &[&str] = &[
        "加密", "比特币", "以太坊", "稳定币", "区块链", "数字资产", "代币", "链上",
        "BTC", "ETH", "USDT", "crypto", "bitcoin", "ethereum", "stablecoin", "blockchain",
        "token", "ETF",
    ];
    let mut terms: Vec<String> = KEYWORDS.iter().map(|s| s.to_string()).collect();
    if let Some(c) = coin_of(ti) {
        terms.push(c.name.to_string());
        terms.push(c.name_cn.to_string());
    }
    terms.push(ti.code.clone());

    let mut items: Vec<Value> = Vec::new();
    for item in crate::news::fetch_jin10(60) {
        items.push(item.to_dict());
    }
    for item in crate::news::fetch_ths_news_today(60) {
        items.push(item.to_dict());
    }
    items.retain(|it| {
        let title = it.get("title").and_then(|t| t.as_str()).unwrap_or("");
        terms.iter().any(|t| title.contains(t.as_str()))
    });
    items.truncate(limit);
    items
}

// ─────────────────────────────────────────────────────────────
// Trap heuristics
// ─────────────────────────────────────────────────────────────

/// Pump/dump + liquidity risk scored from the fetched market data.
fn trap_data(row: Option<&Value>, kline_stats: &Value, volume: Option<f64>, mcap: Option<f64>) -> Value {
    let mut signals: Vec<String> = Vec::new();
    let mut flags: Vec<String> = Vec::new();
    let mut risk = 0i64;

    let ch_1h = row.and_then(|r| num_at(r, &["price_change_percentage_1h_in_currency"]));
    let ch_24h = row.and_then(|r| num_at(r, &["price_change_percentage_24h_in_currency"]));
    let ch_7d = row.and_then(|r| num_at(r, &["price_change_percentage_7d_in_currency"]));
    let ath_dd = row.and_then(|r| num_at(r, &["ath_change_percentage"]));

    if ch_24h.unwrap_or(0.0) >= 30.0 {
        risk += 25;
        signals.push(format!("24h 涨幅 {:.1}% · 短线过热", ch_24h.unwrap_or(0.0)));
    }
    if ch_7d.unwrap_or(0.0) >= 100.0 {
        risk += 25;
        signals.push(format!("7d 涨幅 {:.0}% · 存在拉盘特征", ch_7d.unwrap_or(0.0)));
    }
    if ch_1h.unwrap_or(0.0).abs() >= 15.0 {
        risk += 10;
        signals.push(format!("1h 波动 {:.1}% · 剧烈异动", ch_1h.unwrap_or(0.0)));
    }
    match (volume, mcap) {
        (Some(v), Some(m)) if m > 0.0 && v > 0.0 => {
            let turnover = v / m;
            if turnover > 1.0 {
                risk += 15;
                signals.push(format!("24h 换手 {:.0}% · 投机资金主导", turnover * 100.0));
            } else if turnover < 0.005 {
                risk += 20;
                signals.push(format!("24h 换手 {:.2}% · 流动性稀薄", turnover * 100.0));
            }
        }
        _ => {
            // "Unknown" must not read as "safe": without turnover/market cap the
            // liquidity check could not run at all.
            risk += 30;
            flags.push("缺少成交额/市值数据 · 无法验证流动性".to_string());
        }
    }
    if let Some(v) = volume {
        if v < 1_000_000.0 {
            risk += 20;
            signals.push(format!("24h 成交额仅 {} · 深度不足", fmt_usd(v)));
        }
    }
    if let Some(dd) = ath_dd {
        if dd <= -90.0 {
            risk += 10;
            flags.push(format!("距 ATH 回撤 {:.0}%", dd));
        }
    }
    let max_dd = kline_stats
        .get("max_drawdown")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let max_dd_val = max_dd.trim_end_matches('%').parse::<f64>().unwrap_or(0.0);
    if max_dd_val <= -70.0 {
        risk += 10;
        flags.push(format!("近一年最大回撤 {}", max_dd));
    }

    let risk = risk.clamp(0, 100);
    let (level, band) = if risk >= 60 {
        ("🔴 高风险", "high")
    } else if risk >= 30 {
        ("🟡 中风险", "mid")
    } else {
        ("🟢 安全", "low")
    };
    json!({
        "risk_score": risk,
        "trap_likelihood": level,
        "trap_level": level,
        "risk_band": band,
        "pump_dump_signals": signals,
        "warning_flags": flags,
        "scam_grade": if risk >= 60 { "D" } else if risk >= 30 { "C" } else { "A" },
    })
}

// ─────────────────────────────────────────────────────────────
// Per-dimension payloads
// ─────────────────────────────────────────────────────────────

fn dim_basic(ti: &TickerInfo, row: Option<&Value>, q: Option<&Quote>) -> Value {
    let coin = coin_of(ti);
    let detail = coin_detail(ti);
    let md = detail.as_ref().map(|d| obj(d, "market_data")).cloned();

    let name = detail
        .as_ref()
        .and_then(|d| d.get("name").and_then(|v| v.as_str()))
        .map(str::to_string)
        .or_else(|| row.and_then(|r| r.get("name").and_then(|v| v.as_str()).map(str::to_string)))
        .or_else(|| coin.map(|c| c.name.to_string()))
        .unwrap_or_else(|| ti.code.clone());
    let name_cn = coin.map(|c| c.name_cn.to_string());
    let sector = coin.map(|c| c.sector.to_string());

    let price = q
        .map(|q| q.price)
        .or_else(|| row.and_then(|r| num_at(r, &["current_price"])))
        .unwrap_or(0.0);
    let change_pct = q
        .and_then(|q| q.change_24h)
        .or_else(|| row.and_then(|r| num_at(r, &["price_change_percentage_24h"])))
        .unwrap_or(0.0);
    let mcap = md
        .as_ref()
        .and_then(|m| num_at(m, &["market_cap", "usd"]))
        .or_else(|| row.and_then(|r| num_at(r, &["market_cap"])));
    let mcap_rank = row
        .and_then(|r| num_at(r, &["market_cap_rank"]))
        .map(|v| v as i64);
    let volume = q
        .and_then(|q| q.volume_24h)
        .or_else(|| row.and_then(|r| num_at(r, &["total_volume"])));
    let circulating = md
        .as_ref()
        .and_then(|m| num_at(m, &["circulating_supply"]))
        .or_else(|| row.and_then(|r| num_at(r, &["circulating_supply"])));
    let total_supply = md
        .as_ref()
        .and_then(|m| num_at(m, &["total_supply"]))
        .or_else(|| row.and_then(|r| num_at(r, &["total_supply"])));
    let max_supply = md
        .as_ref()
        .and_then(|m| num_at(m, &["max_supply"]))
        .or_else(|| row.and_then(|r| num_at(r, &["max_supply"])));
    let ath = md
        .as_ref()
        .and_then(|m| num_at(m, &["ath", "usd"]))
        .or_else(|| row.and_then(|r| num_at(r, &["ath"])));
    let ath_change = md
        .as_ref()
        .and_then(|m| num_at(m, &["ath_change_percentage", "usd"]))
        .or_else(|| row.and_then(|r| num_at(r, &["ath_change_percentage"])));
    let genesis = detail
        .as_ref()
        .and_then(|d| d.get("genesis_date").and_then(|v| v.as_str()))
        .map(str::to_string)
        .or_else(|| coin.map(|c| c.genesis.to_string()))
        .or_else(|| {
            detail
                .as_ref()
                .and_then(|d| d.get("asset_platform_id"))
                .filter(|v| !v.is_null())
                .map(|_| String::new())
        });

    let mut out = Map::new();
    out.insert("code".into(), json!(ti.full));
    out.insert("name".into(), json!(name));
    out.insert(
        "full_name".into(),
        json!(detail
            .as_ref()
            .and_then(|d| d.get("name").and_then(|v| v.as_str()))
            .unwrap_or(name.as_str())),
    );
    out.insert("industry".into(), json!(sector.clone().unwrap_or_else(|| "加密货币".into())));
    out.insert(
        "name_cn".into(),
        name_cn.map(Value::from).unwrap_or(Value::Null),
    );
    out.insert("price".into(), jnum(price));
    out.insert("change_pct".into(), jnum(change_pct));
    out.insert(
        "change_7d_pct".into(),
        row.and_then(|r| num_at(r, &["price_change_percentage_7d_in_currency"]))
            .or_else(|| row.and_then(|r| num_at(r, &["price_change_percentage_7d"])))
            .map(round2)
            .unwrap_or(Value::Null),
    );
    out.insert(
        "change_30d_pct".into(),
        row.and_then(|r| num_at(r, &["price_change_percentage_30d_in_currency"]))
            .or_else(|| row.and_then(|r| num_at(r, &["price_change_percentage_30d"])))
            .map(round2)
            .unwrap_or(Value::Null),
    );
    out.insert(
        "market_cap".into(),
        mcap.map(|v| json!(fmt_usd(v))).unwrap_or(Value::Null),
    );
    out.insert("market_cap_raw".into(), mcap.map(jnum).unwrap_or(Value::Null));
    out.insert(
        "market_cap_yi".into(),
        mcap.map(|v| jnum(v / 1e8)).unwrap_or(Value::Null),
    );
    out.insert(
        "market_cap_rank".into(),
        mcap_rank.map(|v| json!(v)).unwrap_or(Value::Null),
    );
    out.insert("volume_24h".into(), volume.map(jnum).unwrap_or(Value::Null));
    out.insert("high_24h".into(), q.and_then(|q| q.high_24h).map(jnum).unwrap_or(Value::Null));
    out.insert("low_24h".into(), q.and_then(|q| q.low_24h).map(jnum).unwrap_or(Value::Null));
    out.insert("circulating_supply".into(), circulating.map(jnum).unwrap_or(Value::Null));
    out.insert(
        "circulating_cap_yi".into(),
        mcap.map(|v| jnum(v / 1e8)).unwrap_or(Value::Null),
    );
    out.insert("total_supply".into(), total_supply.map(jnum).unwrap_or(Value::Null));
    out.insert("max_supply".into(), max_supply.map(jnum).unwrap_or(Value::Null));
    out.insert("ath".into(), ath.map(jnum).unwrap_or(Value::Null));
    out.insert("ath_change_pct".into(), ath_change.map(round2).unwrap_or(Value::Null));
    out.insert("listed_date".into(), genesis.map(Value::from).unwrap_or(Value::Null));
    out.insert("currency".into(), json!(ti.currency));
    out.insert("asset_class".into(), json!("crypto"));
    out.insert("market".into(), json!(CRYPTO_MARKET));
    out.insert(
        "consensus".into(),
        json!(coin.map(|c| c.consensus).unwrap_or("—")),
    );
    // No corporate fundamentals — keep the keys explicit so the renderers and
    // the scorer can tell "not applicable" from "fetch failed".
    out.insert("pe_ttm".into(), Value::Null);
    out.insert("pb".into(), Value::Null);
    out.insert("eps".into(), Value::Null);
    out.insert("actual_controller".into(), Value::Null);
    Value::Object(out)
}

fn dim_tokenomics(_ti: &TickerInfo, row: Option<&Value>, detail: Option<&Value>) -> Value {
    let md = detail.map(|d| obj(d, "market_data"));
    let circulating = md
        .and_then(|m| num_at(m, &["circulating_supply"]))
        .or_else(|| row.and_then(|r| num_at(r, &["circulating_supply"])));
    let total = md
        .and_then(|m| num_at(m, &["total_supply"]))
        .or_else(|| row.and_then(|r| num_at(r, &["total_supply"])));
    let max = md
        .and_then(|m| num_at(m, &["max_supply"]))
        .or_else(|| row.and_then(|r| num_at(r, &["max_supply"])));
    let fdv = md
        .and_then(|m| num_at(m, &["fully_diluted_valuation", "usd"]))
        .or_else(|| row.and_then(|r| num_at(r, &["fully_diluted_valuation"])));
    let mcap = md
        .and_then(|m| num_at(m, &["market_cap", "usd"]))
        .or_else(|| row.and_then(|r| num_at(r, &["market_cap"])));

    let circulating_ratio = match (circulating, total.or(max)) {
        (Some(c), Some(t)) if t > 0.0 => Some(round(c / t * 100.0, 2)),
        _ => None,
    };
    let supply_model = match max {
        Some(_) => "固定上限（通缩/减半发行）",
        None if total.is_some() => "无硬顶（总量动态）",
        None => "供应模型未知",
    };

    json!({
        "roe": Value::Null,
        "net_margin": Value::Null,
        "gross_margin": Value::Null,
        "roe_history": Value::Null,
        "revenue_history": Value::Null,
        "net_profit_history": Value::Null,
        "dividend_years": Value::Null,
        "financial_health": {
            "debt_ratio": Value::Null,
            "note": "链上资产无资产负债表 · 用代币经济与解锁结构替代财务健康度",
        },
        "circulating_supply": circulating.map(jnum).unwrap_or(Value::Null),
        "total_supply": total.map(jnum).unwrap_or(Value::Null),
        "max_supply": max.map(jnum).unwrap_or(Value::Null),
        "fdv": fdv.map(jnum).unwrap_or(Value::Null),
        "market_cap": mcap.map(jnum).unwrap_or(Value::Null),
        "fdv_to_mcap": match (fdv, mcap) {
            (Some(f), Some(m)) if m > 0.0 => round2(f / m),
            _ => Value::Null,
        },
        "circulating_ratio_pct": circulating_ratio.map(jnum).unwrap_or(Value::Null),
        "supply_model": supply_model,
        "asset_class": "crypto",
    })
}

/// True for pegged assets (stablecoins / wrapped tokens) whose price series is
/// not a market signal — Weinstein stage, NVT and trend scoring do not apply.
fn is_pegged(ti: &TickerInfo) -> bool {
    coin_of(ti)
        .map(|c| c.sector.contains("稳定币") || c.sector.contains("封装"))
        .unwrap_or(false)
}

fn dim_kline(ti: &TickerInfo, kl: &[Value], ksource: &str) -> Value {
    if kl.is_empty() && is_pegged(ti) {
        return json!({
            "kline_count": 0,
            "stage": "不适用（锚定资产）",
            "ma_align": "不适用（锚定资产）",
            "macd": "不适用（锚定资产）",
            "rsi": Value::Null,
            "chip_distribution": {},
            "note": "稳定币/封装资产价格锚定，趋势指标无意义",
        });
    }
    let assembled = kline::assemble_dim(&ti.full, kl, json!({}), ksource);
    assembled.get("data").cloned().unwrap_or_else(|| json!({}))
}

fn dim_macro(global: Option<&Value>, fng: &[Value]) -> Value {
    let fng_latest = fng.first();
    json!({
        "rate_cycle": Value::Null, "fx_trend": Value::Null,
        "geo_risk": Value::Null, "commodity": Value::Null,
        "crypto_total_mcap": global.and_then(|g| num_at(g, &["total_market_cap", "usd"])).map(jnum).unwrap_or(Value::Null),
        "crypto_total_mcap_str": global.and_then(|g| num_at(g, &["total_market_cap", "usd"])).map(|v| json!(fmt_usd(v))).unwrap_or(Value::Null),
        "crypto_total_volume": global.and_then(|g| num_at(g, &["total_volume", "usd"])).map(jnum).unwrap_or(Value::Null),
        "mcap_change_24h_pct": global.and_then(|g| num_at(g, &["market_cap_change_percentage_24h_usd"])).map(round2).unwrap_or(Value::Null),
        "btc_dominance_pct": global.and_then(|g| num_at(g, &["market_cap_percentage", "btc"])).map(round2).unwrap_or(Value::Null),
        "eth_dominance_pct": global.and_then(|g| num_at(g, &["market_cap_percentage", "eth"])).map(round2).unwrap_or(Value::Null),
        "active_cryptocurrencies": global.and_then(|g| num_at(g, &["active_cryptocurrencies"])).map(|v| json!(v as i64)).unwrap_or(Value::Null),
        "fear_greed": fng_latest.and_then(|f| f.get("value").and_then(|v| v.as_str())).and_then(|s| s.parse::<f64>().ok()).map(jnum).unwrap_or(Value::Null),
        "fear_greed_label": fng_latest.and_then(|f| f.get("value_classification").cloned()).unwrap_or(Value::Null),
    })
}

fn dim_peers(ti: &TickerInfo, tops: &[Value], mcap_rank: Option<i64>) -> Value {
    let self_id = resolve_id(ti);
    let mut table: Vec<Value> = Vec::new();
    let mut others: Vec<Value> = Vec::new();
    for c in tops.iter().take(12) {
        let id = c.get("id").and_then(|v| v.as_str()).unwrap_or("");
        let is_self = Some(id.to_string()) == self_id;
        let row = json!({
            "name": c.get("name").cloned().unwrap_or(Value::Null),
            "ticker": c.get("symbol").and_then(|s| s.as_str()).map(|s| s.to_uppercase()),
            "market_cap": c.get("market_cap").cloned().unwrap_or(Value::Null),
            "price": c.get("current_price").cloned().unwrap_or(Value::Null),
            "change_pct": c.get("price_change_percentage_24h").cloned().unwrap_or(Value::Null),
            "volume": c.get("total_volume").cloned().unwrap_or(Value::Null),
            "pe": Value::Null,
            "is_self": is_self,
        });
        if !is_self {
            others.push(row.clone());
        }
        table.push(row);
    }
    json!({
        "peer_table": table,
        "peer_comparison": {
            "peer_count": others.len(),
            "basis": "市值前 12 币种 · 无 PE 口径",
        },
        "rank": mcap_rank.map(|r| json!(r)).unwrap_or(Value::Null),
        "industry": coin_of(ti).map(|c| c.sector).unwrap_or("加密货币"),
    })
}

fn dim_chain(_ti: &TickerInfo, detail: Option<&Value>) -> Value {
    let categories: Vec<String> = detail
        .and_then(|d| d.get("categories"))
        .and_then(|c| c.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str())
                .filter(|s| !s.is_empty())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();
    let desc = detail
        .and_then(|d| d.get("description"))
        .and_then(|d| d.get("zh").or_else(|| d.get("en")))
        .and_then(|v| v.as_str())
        .map(|s| s.chars().take(400).collect::<String>())
        .unwrap_or_default();
    let links = detail.map(|d| obj(d, "links").clone()).unwrap_or(Value::Null);

    let breakdown: Vec<Value> = if categories.is_empty() {
        Vec::new()
    } else {
        categories
            .iter()
            .take(6)
            .map(|c| json!({"segment": c}))
            .collect()
    };
    json!({
        "main_business_breakdown": breakdown,
        "upstream": Value::Null,
        "downstream": Value::Null,
        "client_concentration": Value::Null,
        "categories": categories,
        "description": desc,
        "links": links,
        "note": "公链/协议无传统产业链 · 用生态分类与官方链接替代",
    })
}

/// Crypto has no quarterly fund-disclosure dataset; the dim is kept present (and
/// explicitly marked) so coverage checks never read it as a failed fetch.
fn dim_fund_holders() -> Value {
    json!({
        "total_funds_holding": Value::Null,
        "active_funds_count": Value::Null,
        "full_stats_count": Value::Null,
        "note": "加密资产无公募基金持仓披露 · 机构敞口体现为现货 ETF 与财库公司，见 12_capital_flow",
    })
}

fn dim_research(detail: Option<&Value>, row: Option<&Value>) -> Value {
    let dev = detail.map(|d| obj(d, "developer_data").clone()).unwrap_or(Value::Null);
    let community = detail.map(|d| obj(d, "community_data").clone()).unwrap_or(Value::Null);
    let sentiment_up = detail
        .and_then(|d| num_at(d, &["sentiment_votes_up_percentage"]))
        .or_else(|| detail.and_then(|d| num_at(d, &["sentiment_votes_up_percentage"])));
    json!({
        "report_count": 0,
        "rating_distribution": Value::Null,
        "target_price_avg": Value::Null,
        "buy_rating_pct": Value::Null,
        "developer": dev,
        "community": community,
        "sentiment_votes_up_pct": sentiment_up.map(round2).unwrap_or(Value::Null),
        "market_cap_rank": row.and_then(|r| num_at(r, &["market_cap_rank"])).map(|v| json!(v as i64)).unwrap_or(Value::Null),
        "note": "加密资产无券商研报覆盖 · 以开发者活跃度与社区规模替代",
    })
}

fn dim_industry(ti: &TickerInfo, detail: Option<&Value>, tops: &[Value], mcap: Option<f64>) -> Value {
    let coin = coin_of(ti);
    let sector = coin.map(|c| c.sector).unwrap_or("加密货币");
    let sector_cap: f64 = tops
        .iter()
        .filter(|c| {
            c.get("symbol")
                .and_then(|s| s.as_str())
                .map(|s| {
                    coin.map(|cc| s.eq_ignore_ascii_case(cc.symbol))
                        .unwrap_or(false)
                })
                .unwrap_or(false)
        })
        .filter_map(|c| num_at(c, &["market_cap"]))
        .sum();
    let total: f64 = tops.iter().filter_map(|c| num_at(c, &["market_cap"])).sum();
    let dominance = if total > 0.0 {
        match mcap {
            Some(m) => Some(round(m / total * 100.0, 2)),
            None => None,
        }
    } else {
        None
    };
    json!({
        "industry": sector,
        "growth": Value::Null,
        "tam": Value::Null,
        "penetration": Value::Null,
        "industry_pe": Value::Null,
        "industry_pb": Value::Null,
        "sector_market_cap": if sector_cap > 0.0 { jnum(sector_cap) } else { Value::Null },
        "market_share_pct": dominance.map(jnum).unwrap_or(Value::Null),
        "top_sector_coins": Value::Array(
            tops.iter()
                .filter(|c| c.get("symbol").and_then(|s| s.as_str())
                    .map(|s| coin.map(|cc| s.eq_ignore_ascii_case(cc.symbol)).unwrap_or(false))
                    .unwrap_or(false))
                .take(5)
                .cloned()
                .collect()
        ),
        "categories": detail.and_then(|d| d.get("categories").cloned()).unwrap_or(Value::Null),
        "note": "加密货币行业口径 = 板块市值与赛道地位",
    })
}

fn dim_materials(ti: &TickerInfo, detail: Option<&Value>) -> Value {
    let algo = detail
        .and_then(|d| d.get("hashing_algorithm"))
        .and_then(|v| v.as_str())
        .map(str::to_string);
    let coin = coin_of(ti);
    let consensus = coin.map(|c| c.consensus).unwrap_or("—");
    let algo_present = algo.is_some();
    json!({
        "core_material": Value::Null,
        "price_trend": Value::Null,
        "price_history_12m": Value::Null,
        "materials_detail": Value::Null,
        "cost_share": Value::Null,
        "consensus": consensus,
        "hashing_algorithm": algo.map(Value::from).unwrap_or(Value::Null),
        "note": if algo_present {
            "PoW 网络 · 算力/电力为核心成本项（公开 API 无逐日算力数据）"
        } else {
            "非 PoW 网络 · 无挖矿成本项"
        },
    })
}

fn dim_futures(ti: &TickerInfo) -> Value {
    let swap = format!("{}-{}-SWAP", ti.code, counter_ccy(ti));
    let fr_url = http::with_query(
        "https://www.okx.com/api/v5/public/funding-rate",
        &[("instId", swap.as_str())],
    );
    let fr = cached_json(
        &ti.full,
        &format!("okx_funding__{swap}"),
        &fr_url,
        &[("User-Agent", http::UA.to_string())],
        TTL_INTRADAY,
    )
    .and_then(|v| {
        v.get("data")
            .and_then(|d| d.as_array())
            .and_then(|a| a.first())
            .cloned()
    });
    let oi_url = http::with_query(
        "https://www.okx.com/api/v5/public/open-interest",
        &[("instType", "SWAP"), ("instId", swap.as_str())],
    );
    let oi = cached_json(
        &ti.full,
        &format!("okx_oi__{swap}"),
        &oi_url,
        &[("User-Agent", http::UA.to_string())],
        TTL_INTRADAY,
    )
    .and_then(|v| {
        v.get("data")
            .and_then(|d| d.as_array())
            .and_then(|a| a.first())
            .cloned()
    });
    let funding = fr
        .as_ref()
        .and_then(|f| num_at(f, &["fundingRate"]))
        .map(|r| round(r * 100.0, 4));
    json!({
        "linked_contract": if fr.is_some() || oi.is_some() { json!(swap) } else { Value::Null },
        "funding_rate_pct": funding.map(jnum).unwrap_or(Value::Null),
        "next_funding_time": fr.as_ref().and_then(|f| f.get("fundingTime").cloned()).unwrap_or(Value::Null),
        "open_interest_contracts": oi.as_ref().and_then(|o| num_at(o, &["oi"])).map(jnum).unwrap_or(Value::Null),
        "open_interest_usd": oi.as_ref().and_then(|o| num_at(o, &["oiCcy"])).map(jnum).unwrap_or(Value::Null),
        "price_trend": match funding {
            Some(f) if f > 0.01 => "多头拥挤（正费率偏高）",
            Some(f) if f < -0.01 => "空头拥挤（负费率）",
            Some(_) => "多空均衡",
            None => "无合约数据",
        },
        "inventory": Value::Null,
    })
}

fn dim_valuation(
    _ti: &TickerInfo,
    row: Option<&Value>,
    mcap: Option<f64>,
    volume: Option<f64>,
    fdv: Option<f64>,
    kl: &[Value],
    kstats: &Value,
) -> Value {
    // Classic NVT: market cap ÷ 24h traded volume (liquid majors sit in a
    // 20–60× band). `turnover_ratio` is its inverse — how much of the float
    // actually changes hands per day.
    let nvt = match (mcap, volume) {
        (Some(m), Some(v)) if v > 0.0 => Some(round(m / v, 2)),
        _ => None,
    };
    let turnover_ratio = match (mcap, volume) {
        (Some(m), Some(v)) if m > 0.0 => Some(round(v / m, 4)),
        _ => None,
    };
    let mcap_to_fdv = match (fdv, mcap) {
        (Some(f), Some(m)) if f > 0.0 => Some(round(m / f, 4)),
        _ => None,
    };
    let ath_dd = row
        .and_then(|r| num_at(r, &["ath_change_percentage"]))
        .map(round2);
    // Position inside the traded range (0 = at the low, 100 = at the high).
    let closes: Vec<f64> = kl.iter().filter_map(|r| num(r.get("收盘").unwrap_or(&Value::Null))).collect();
    let range_pos = if closes.len() >= 30 {
        let lo = closes.iter().cloned().fold(f64::INFINITY, f64::min);
        let hi = closes.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
        if hi > lo {
            let last = closes[closes.len() - 1];
            Some(round((last - lo) / (hi - lo) * 100.0, 1))
        } else {
            None
        }
    } else {
        None
    };
    json!({
        "pe": Value::Null, "pb": Value::Null,
        "pe_quantile": Value::Null, "pb_quantile": Value::Null,
        "industry_pe": Value::Null, "dcf": Value::Null,
        "nvt_ratio": nvt.map(jnum).unwrap_or(Value::Null),
        "turnover_ratio": turnover_ratio.map(jnum).unwrap_or(Value::Null),
        "mcap_to_fdv": mcap_to_fdv.map(jnum).unwrap_or(Value::Null),
        "ath_drawdown_pct": ath_dd.unwrap_or(Value::Null),
        "price_range_position_pct": range_pos.map(jnum).unwrap_or(Value::Null),
        "volatility_1y_pct": kstats.get("volatility").cloned().unwrap_or(Value::Null),
        "max_drawdown_1y": kstats.get("max_drawdown").cloned().unwrap_or(Value::Null),
        "method": "NVT / 市值-成交额 / FDV 折价 / 区间位置",
    })
}

fn dim_governance(_ti: &TickerInfo, detail: Option<&Value>, row: Option<&Value>) -> Value {
    let links = detail.map(|d| obj(d, "links").clone()).unwrap_or(Value::Null);
    let unlock_pct = row
        .and_then(|r| num_at(r, &["total_supply"]))
        .zip(row.and_then(|r| num_at(r, &["circulating_supply"])))
        .and_then(|(t, c)| if t > 0.0 { Some(round((t - c) / t * 100.0, 2)) } else { None });
    json!({
        "pledge": [],
        "insider_trades_1y": Value::Null,
        "chairman_turnover": Value::Null,
        "links": links,
        "unvested_supply_pct": unlock_pct.map(jnum).unwrap_or(Value::Null),
        "note": "无股权治理结构 · 关注代币解锁/团队持仓/基金会地址（公开 API 不提供逐笔解锁表）",
    })
}

fn dim_capital_flow(_ti: &TickerInfo, row: Option<&Value>, kl: &[Value]) -> Value {
    let volume = row.and_then(|r| num_at(r, &["total_volume"]));
    let vols: Vec<f64> = kl.iter().filter_map(|r| num(r.get("成交量").unwrap_or(&Value::Null))).collect();
    let vol_change_7d = if vols.len() >= 14 {
        let recent: f64 = vols[vols.len() - 7..].iter().sum::<f64>() / 7.0;
        let prev: f64 = vols[vols.len() - 14..vols.len() - 7].iter().sum::<f64>() / 7.0;
        if prev > 0.0 {
            Some(round((recent - prev) / prev * 100.0, 1))
        } else {
            None
        }
    } else {
        None
    };
    // Stablecoin float as the sector's dry-powder proxy.
    let stables = markets_for("tether,usd-coin,dai,first-digital-usd", 4);
    let stable_cap: f64 = stables.iter().filter_map(|s| num_at(s, &["market_cap"])).sum();
    let stable_chg: f64 = stables
        .iter()
        .filter_map(|s| num_at(s, &["market_cap_change_percentage_24h"]))
        .sum::<f64>()
        / stables.len().max(1) as f64;
    json!({
        "northbound": Value::Null, "margin_recent": Value::Null,
        "holder_count_history": Value::Null, "main_fund_flow_20d": Value::Null,
        "unlock_schedule": Value::Null,
        "volume_24h": volume.map(jnum).unwrap_or(Value::Null),
        "volume_change_7d_pct": vol_change_7d.map(jnum).unwrap_or(Value::Null),
        "stablecoin_market_cap": if stable_cap > 0.0 { jnum(stable_cap) } else { Value::Null },
        "stablecoin_mcap_change_24h_pct": if stables.is_empty() { Value::Null } else { round2(stable_chg) },
        "note": "加密无北向/两融 · 以成交额趋势 + 稳定币总量作为资金面代理",
    })
}

fn dim_policy(news: &[Value]) -> Value {
    let items: Vec<Value> = news
        .iter()
        .filter(|n| {
            let t = n.get("title").and_then(|v| v.as_str()).unwrap_or("");
            ["监管", "政策", "SEC", "合规", "立法", "央行", "禁止", "税收", "ETF"]
                .iter()
                .any(|k| t.contains(k))
        })
        .take(8)
        .cloned()
        .collect();
    json!({
        "policy_dir": Value::Null,
        "subsidy": Value::Null,
        "monitoring": Value::Null,
        "anti_trust": Value::Null,
        "snippets": {
            "policy_dir": Value::Null,
            "subsidy": Value::Null,
            "monitoring": Value::Null,
            "anti_trust": Value::Null,
        },
        "news": items,
        "note": "加密监管以新闻流为准 · 无官方产业政策维度",
    })
}

fn dim_moat(ti: &TickerInfo, detail: Option<&Value>, tops: &[Value], mcap: Option<f64>) -> Value {
    let dev = detail.map(|d| obj(d, "developer_data"));
    let community = detail.map(|d| obj(d, "community_data"));
    let commits = dev.and_then(|d| num_at(d, &["commit_count_4_weeks"])).unwrap_or(0.0);
    let stars = dev.and_then(|d| num_at(d, &["stars"])).unwrap_or(0.0);
    let forks = dev.and_then(|d| num_at(d, &["forks"])).unwrap_or(0.0);
    let followers = community
        .and_then(|c| num_at(c, &["twitter_followers"]))
        .unwrap_or(0.0);
    let reddit = community
        .and_then(|c| num_at(c, &["reddit_subscribers"]))
        .unwrap_or(0.0);
    let total: f64 = tops.iter().filter_map(|c| num_at(c, &["market_cap"])).sum();
    let share = match (mcap, total > 0.0) {
        (Some(m), true) => m / total * 100.0,
        _ => 0.0,
    };

    let scale = (share * 4.0).min(10.0);
    let network = ((followers / 200_000.0).min(6.0) + (reddit / 200_000.0).min(4.0)).min(10.0);
    let rd = ((commits / 20.0).min(6.0) + (stars / 2000.0).min(2.0) + (forks / 500.0).min(2.0)).min(10.0);
    let intangible = ((10.0 - share * 0.4).max(0.0) * 0.0 + rank_brand(ti, tops)).min(10.0);
    let total_score = (scale + network + rd + intangible) / 4.0;

    json!({
        "intangible": jnum(round(intangible, 1)),
        "switching": jnum(round(share.min(10.0), 1)),
        "network": jnum(round(network, 1)),
        "scale": jnum(round(scale, 1)),
        "rd_summary": format!("4 周提交 {commits:.0} · Stars {stars:.0} · Forks {forks:.0}"),
        "scores": {
            "total": jnum(round(total_score, 1)),
            "intangible": jnum(round(intangible, 1)),
            "switching": jnum(round(share.min(10.0), 1)),
            "network": jnum(round(network, 1)),
            "scale": jnum(round(scale, 1)),
        },
        "market_share_pct": round2(share),
        "community": {
            "twitter_followers": jnum(followers),
            "reddit_subscribers": jnum(reddit),
        },
        "web_search_snippets": [],
        "note": "护城河 = 网络效应（社区/开发者）+ 市值份额，非专利/品牌",
    })
}

/// Brand strength proxy: market-cap rank inside the tracked top list.
fn rank_brand(ti: &TickerInfo, tops: &[Value]) -> f64 {
    let self_sym = ti.code.to_ascii_uppercase();
    tops.iter()
        .position(|c| {
            c.get("symbol")
                .and_then(|s| s.as_str())
                .map(|s| s.eq_ignore_ascii_case(&self_sym))
                .unwrap_or(false)
        })
        .map(|i| (10.0 - i as f64 * 0.8).max(1.0))
        .unwrap_or(3.0)
}

fn dim_events(news: &[Value]) -> Value {
    let recent: Vec<Value> = news.iter().take(15).cloned().collect();
    json!({
        "event_timeline": recent.iter().map(|n| json!({
            "date": n.get("time").or_else(|| n.get("date")).cloned().unwrap_or(Value::Null),
            "event": n.get("title").cloned().unwrap_or(Value::Null),
            "source": n.get("source").cloned().unwrap_or(Value::Null),
        })).collect::<Vec<_>>(),
        "recent_news": recent.clone(),
        "news": recent,
        "catalyst": Value::Null,
        "warnings": Value::Null,
        "disclosures_count": news.len(),
        "note": "事件来自中文财经快讯流 · 按币种/加密关键词过滤",
    })
}

fn dim_sentiment(ti: &TickerInfo, detail: Option<&Value>, fng: &[Value], trending: &[Value]) -> Value {
    let latest = fng.first();
    let history: Vec<Value> = fng
        .iter()
        .take(30)
        .filter_map(|f| {
            let v = f.get("value").and_then(|v| v.as_str())?.parse::<f64>().ok()?;
            Some(json!({
                "ts": f.get("timestamp").cloned().unwrap_or(Value::Null),
                "value": v,
                "label": f.get("value_classification").cloned().unwrap_or(Value::Null),
            }))
        })
        .collect();
    let rank = trending.iter().position(|c| {
        c.get("symbol")
            .and_then(|s| s.as_str())
            .map(|s| s.eq_ignore_ascii_case(&ti.code))
            .unwrap_or(false)
    });
    let sentiment_up = detail.and_then(|d| num_at(d, &["sentiment_votes_up_percentage"]));
    json!({
        "xueqiu_heat": Value::Null,
        "thermometer_value": latest
            .and_then(|f| f.get("value").and_then(|v| v.as_str()))
            .and_then(|s| s.parse::<f64>().ok())
            .map(jnum)
            .unwrap_or(Value::Null),
        "sentiment_label": latest.and_then(|f| f.get("value_classification").cloned()).unwrap_or(Value::Null),
        "positive_pct": sentiment_up.map(round2).unwrap_or(Value::Null),
        "platform_snippets": [],
        "hot_trend_mentions": trending.iter().take(10).cloned().collect::<Vec<_>>(),
        "hot_rank": Value::Null,
        "trending_rank": rank.map(|r| json!(r + 1)).unwrap_or(Value::Null),
        "fear_greed_history": history,
        "note": "情绪 = Alternative.me 恐慌贪婪指数 + CoinGecko 热搜/看多投票",
    })
}

fn dim_similar(ti: &TickerInfo, detail: Option<&Value>, tops: &[Value]) -> Value {
    let self_id = resolve_id(ti);
    let category = detail
        .and_then(|d| d.get("categories"))
        .and_then(|c| c.as_array())
        .and_then(|a| a.iter().find_map(|v| v.as_str()))
        .map(str::to_string);
    let mut similar: Vec<Value> = Vec::new();
    if let Some(cat) = &category {
        let rows = cg_get(
            "_global",
            "/coins/markets",
            &[
                ("vs_currency", "usd"),
                ("category", cat.as_str()),
                ("order", "market_cap_desc"),
                ("per_page", "8"),
            ],
            TTL_INTRADAY,
        )
        .and_then(|v| v.as_array().cloned())
        .unwrap_or_default();
        for c in rows {
            let id = c.get("id").and_then(|v| v.as_str()).unwrap_or("");
            if Some(id.to_string()) == self_id {
                continue;
            }
            similar.push(json!({
                "name": c.get("name").cloned().unwrap_or(Value::Null),
                "code": c.get("symbol").and_then(|s| s.as_str()).map(|s| s.to_uppercase()),
                "market_cap": c.get("market_cap").cloned().unwrap_or(Value::Null),
                "change_pct": c.get("price_change_percentage_24h").cloned().unwrap_or(Value::Null),
                "reason": cat,
            }));
        }
    }
    if similar.is_empty() {
        // Degrade to the same sector from the tracked top list.
        for c in tops.iter().take(6) {
            let id = c.get("id").and_then(|v| v.as_str()).unwrap_or("");
            if Some(id.to_string()) == self_id {
                continue;
            }
            similar.push(json!({
                "name": c.get("name").cloned().unwrap_or(Value::Null),
                "code": c.get("symbol").and_then(|s| s.as_str()).map(|s| s.to_uppercase()),
                "market_cap": c.get("market_cap").cloned().unwrap_or(Value::Null),
                "change_pct": c.get("price_change_percentage_24h").cloned().unwrap_or(Value::Null),
                "reason": "市值前列",
            }));
        }
    }
    similar.truncate(5);
    json!({ "similar_stocks": similar, "category": category })
}

// ─────────────────────────────────────────────────────────────
// Entry point
// ─────────────────────────────────────────────────────────────

/// Registry dims the crypto venue handles. Every entry has a crypto payload;
/// the equity `FetcherSpec` fields are bypassed for these (see `collect`).
pub const CRYPTO_DIMS: &[&str] = &[
    "0_basic",
    "1_financials",
    "2_kline",
    "3_macro",
    "4_peers",
    "5_chain",
    "6_fund_holders",
    "6_research",
    "7_industry",
    "8_materials",
    "9_futures",
    "10_valuation",
    "11_governance",
    "12_capital_flow",
    "13_policy",
    "14_moat",
    "15_events",
    "16_lhb",
    "17_sentiment",
    "18_trap",
    "19_contests",
    "similar_stocks",
];

/// Build the crypto payload for `dim_key`, or `None` to fall through to the
/// generic path.
pub fn dim(dim_key: &str, ti: &TickerInfo) -> Option<CryptoDim> {
    if !CRYPTO_DIMS.contains(&dim_key) {
        return None;
    }
    // Shared per-coin context. `shared` memoizes process-wide so the parallel
    // dim wave issues these requests once instead of once per dimension.
    let row = shared(&format!("row|{}", ti.full), || markets_row(ti));
    let detail = shared(&format!("detail|{}", ti.full), || coin_detail(ti));
    let global = shared(&format!("global|{}", ti.full), || global_snapshot());
    let fng = shared(&format!("fng|{}", ti.full), || {
        fear_greed(30).unwrap_or_default()
    });
    let tops = shared(&format!("tops|{}", ti.full), || top_markets(15));
    let (kl, ksource) = shared(&format!("kl|{}|{}", ti.full, ti.currency), || klines(ti));
    let q = shared(&format!("quote|{}", ti.full), || {
        quote(ti, row.as_ref())
    });
    let kline_stats = shared(&format!("kstats|{}", ti.full), || {
        kline::extract_for_viz(&kl)
            .get("kline_stats")
            .cloned()
            .unwrap_or_else(|| json!({}))
    });

    let mcap = row
        .as_ref()
        .and_then(|r| num_at(r, &["market_cap"]))
        .or_else(|| {
            detail
                .as_ref()
                .and_then(|d| num_at(d, &["market_data", "market_cap", "usd"]))
        });
    let volume = q
        .as_ref()
        .and_then(|q| q.volume_24h)
        .or_else(|| row.as_ref().and_then(|r| num_at(r, &["total_volume"])));
    let fdv = row
        .as_ref()
        .and_then(|r| num_at(r, &["fully_diluted_valuation"]))
        .or_else(|| {
            detail
                .as_ref()
                .and_then(|d| num_at(d, &["market_data", "fully_diluted_valuation", "usd"]))
        });
    let mcap_rank = row
        .as_ref()
        .and_then(|r| num_at(r, &["market_cap_rank"]))
        .map(|v| v as i64);

    let source = |s: &str| -> String { s.to_string() };

    let out = match dim_key {
        "0_basic" => CryptoDim::new(
            dim_basic(ti, row.as_ref(), q.as_ref()),
            match q.as_ref().map(|q| q.source) {
                Some(src) => source(src),
                None => "coingecko:/coins/markets".to_string(),
            },
        ),
        "1_financials" => CryptoDim::new(
            dim_tokenomics(ti, row.as_ref(), detail.as_ref()),
            "coingecko:/coins/{id} · tokenomics",
        ),
        "2_kline" => CryptoDim::new(
            dim_kline(ti, &kl, ksource),
            format!("{ksource} + local indicators"),
        ),
        "3_macro" => CryptoDim::new(
            dim_macro(global.as_ref(), &fng),
            "coingecko:/global + alternative.me/fng",
        ),
        "4_peers" => CryptoDim::new(
            dim_peers(ti, &tops, mcap_rank),
            "coingecko:/coins/markets (top 12)",
        ),
        "5_chain" => CryptoDim::new(
            dim_chain(ti, detail.as_ref()),
            "coingecko:/coins/{id}",
        ),
        "6_fund_holders" => CryptoDim::new(dim_fund_holders(), "n/a:crypto"),
        "6_research" => CryptoDim::new(
            dim_research(detail.as_ref(), row.as_ref()),
            "coingecko:/coins/{id} developer+community",
        ),
        "7_industry" => CryptoDim::new(
            dim_industry(ti, detail.as_ref(), &tops, mcap),
            "coingecko:/coins/markets + categories",
        ),
        "8_materials" => CryptoDim::new(
            dim_materials(ti, detail.as_ref()),
            "coingecko:/coins/{id}",
        ),
        "9_futures" => CryptoDim::new(dim_futures(ti), "okx:/public/funding-rate + open-interest"),
        "10_valuation" => CryptoDim::new(
            dim_valuation(ti, row.as_ref(), mcap, volume, fdv, &kl, &kline_stats),
            "coingecko + local NVT/区间位置",
        ),
        "11_governance" => CryptoDim::new(
            dim_governance(ti, detail.as_ref(), row.as_ref()),
            "coingecko:/coins/{id}",
        ),
        "12_capital_flow" => CryptoDim::new(
            dim_capital_flow(ti, row.as_ref(), &kl),
            "coingecko:volumes + stablecoin caps",
        ),
        "13_policy" => CryptoDim::new(dim_policy(&crypto_news(ti, 30)), "news:jin10+ths (crypto filter)"),
        "14_moat" => CryptoDim::new(
            dim_moat(ti, detail.as_ref(), &tops, mcap),
            "coingecko:/coins/{id} dev+community",
        ),
        "15_events" => CryptoDim::new(dim_events(&crypto_news(ti, 30)), "news:jin10+ths (crypto filter)"),
        "16_lhb" => CryptoDim::new(
            json!({
                "lhb_count_30d": Value::Null,
                "lhb_records": [],
                "matched_youzi": [],
                "inst_vs_youzi": Value::Null,
                "note": "加密市场无龙虎榜/席位制度 · 该维度不适用",
            }),
            "n/a:crypto",
        ),
        "17_sentiment" => CryptoDim::new(
            dim_sentiment(ti, detail.as_ref(), &fng, &trending()),
            "alternative.me/fng + coingecko trending",
        ),
        "18_trap" => CryptoDim::new(
            trap_data(row.as_ref(), &kline_stats, volume, mcap),
            "local heuristic: 涨幅/换手/流动性/回撤",
        ),
        "19_contests" => CryptoDim::new(
            json!({
                "xueqiu_cubes": [], "tgb_mentions": [], "ths_simu": [],
                "dpswang": Value::Null, "summary": Value::Null,
                "note": "加密资产无 A 股实盘赛数据 · 该维度不适用",
            }),
            "n/a:crypto",
        ),
        "similar_stocks" => CryptoDim::new(
            dim_similar(ti, detail.as_ref(), &tops),
            "coingecko:/coins/markets by category",
        ),
        _ => return None,
    };
    let mut out = out;
    if let Some(obj) = out.data.as_object_mut() {
        obj.insert("asset_class".into(), json!("crypto"));
    }
    Some(out)
}

/// CoinGecko `/search/trending` → the trending coin list.
fn trending() -> Vec<Value> {
    static EMPTY: LazyLock<Vec<Value>> = LazyLock::new(Vec::new);
    let v = cg_get("_global", "/search/trending", &[], TTL_HOURLY);
    let Some(items) = v.and_then(|v| v.get("coins").and_then(|c| c.as_array().cloned())) else {
        return EMPTY.clone();
    };
    items
        .iter()
        .filter_map(|entry| entry.get("item").cloned())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use uzi_core::ticker::parse_ticker;

    #[test]
    fn crypto_market_is_detected() {
        assert!(is_crypto(&parse_ticker("BTC")));
        assert!(is_crypto(&parse_ticker("SOL-USD")));
        assert!(!is_crypto(&parse_ticker("600519.SH")));
    }

    #[test]
    fn money_formatting_is_compact() {
        assert_eq!(fmt_usd(1.18e12), "1.18万亿");
        assert_eq!(fmt_usd(4.5e11), "4500亿");
        assert_eq!(fmt_usd(1.23e10), "123亿");
        assert_eq!(fmt_usd(12_345.0), "1万");
    }

    #[test]
    fn dim_table_covers_every_registry_dim() {
        // Offline structural check: every registry dim has a crypto payload.
        for key in crate::fetchers::dim_keys() {
            assert!(CRYPTO_DIMS.contains(&key), "crypto dim missing: {key}");
        }
        for key in CRYPTO_DIMS {
            assert!(crate::fetchers::dim_keys().contains(key), "stale crypto dim: {key}");
        }
    }

    #[test]
    fn stablecoin_quotes_map_to_usd() {
        assert_eq!(vs_currency("USDT"), "usd");
        assert_eq!(vs_currency("EUR"), "eur");
        assert!(usd_like("USDC"));
        assert!(!usd_like("BTC"));
    }
}
