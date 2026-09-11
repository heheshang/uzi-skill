//! Port of `lib/global_peers.py` (network half) — global peer discovery,
//! ranking and Yahoo fundamentals/FX normalization.
//!
//! Upstream's `YahooGlobalPeerProvider` uses `yfinance` for profile and screener
//! discovery and raw HTTP for the fundamentals time-series; the Rust port
//! implements the HTTP half verbatim (same fixed hosts, same symbol allowlist)
//! and mirrors upstream's `self.yf is None` degradation for the yfinance-backed
//! half: `profile` returns the symbol/market/exchange/currency stub, `discover`
//! returns `[]`, and `build_global_peer_comparison` therefore reports
//! `conclusion_status == "insufficient_peers"` exactly as upstream does without
//! the package installed.

use serde_json::{json, Map, Value};
use std::collections::BTreeMap;
use std::sync::LazyLock;

use chrono::Datelike;

use uzi_core::cache::{cached, TTL_QUARTERLY};
use uzi_core::ticker::TickerInfo;

use crate::http;

pub const YAHOO_FACTS: &[(&str, &str)] = &[
    ("annualTotalRevenue", "revenue"),
    ("annualGrossProfit", "gross_profit"),
    ("annualOperatingIncome", "operating_income"),
    ("annualNetIncome", "net_income"),
    ("annualStockholdersEquity", "equity"),
    ("annualOperatingCashFlow", "operating_cash_flow"),
    ("annualCapitalExpenditure", "capital_expenditure"),
];

/// `_LEGAL_SUFFIXES`.
static LEGAL_SUFFIXES: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(
        r"(?i)\b(incorporated|inc|corporation|corp|company|co|limited|ltd|plc|group|holdings?)\b",
    )
    .unwrap()
});

/// `to_yahoo_symbol(ticker_info)`.
pub fn to_yahoo_symbol(ti: &TickerInfo) -> String {
    match ti.market.as_str() {
        "A" => {
            let suffix = ti
                .full
                .rsplit('.')
                .next()
                .unwrap_or("")
                .to_uppercase();
            let yahoo_suffix = if suffix == "SH" { "SS" } else { suffix.as_str() };
            format!("{}.{}", ti.code, yahoo_suffix)
        }
        "H" => {
            let code = format!("{:0>4}", ti.code);
            format!("{code}.HK")
        }
        _ => ti.full.to_uppercase(),
    }
}

/// `_number(value)` — None for non-finite / bool / unparsable.
pub fn number(value: &Value) -> Option<f64> {
    if value.is_null() || value.is_boolean() {
        return None;
    }
    let n = match value {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.parse::<f64>().ok(),
        _ => None,
    }?;
    if n.is_finite() {
        Some(n)
    } else {
        None
    }
}

fn number_or(value: &Value, key: &str) -> Option<f64> {
    value.get(key).and_then(number)
}

/// `_ratio(numerator, denominator)` — percent, 2dp.
pub fn ratio(numerator: &Value, denominator: &Value) -> Value {
    let num = number(numerator);
    let den = number(denominator);
    match (num, den) {
        (Some(n), Some(d)) if d != 0.0 => json!(uzi_core::py::round(n / d * 100.0, 2)),
        _ => Value::Null,
    }
}

/// `normalize_yahoo_timeseries(symbol, payload)`.
pub fn normalize_yahoo_timeseries(symbol: &str, payload: &Value) -> Value {
    let mut periods: BTreeMap<String, Map<String, Value>> = BTreeMap::new();
    let mut currency: Option<String> = None;

    let result = payload
        .get("timeseries")
        .and_then(|t| t.get("result"))
        .and_then(|r| r.as_array())
        .cloned()
        .unwrap_or_default();
    for series in &result {
        let Some(series) = series.as_object() else {
            continue;
        };
        let meta_types = series
            .get("meta")
            .and_then(|m| m.get("type"))
            .and_then(|t| t.as_array())
            .cloned()
            .unwrap_or_default();
        let mut yahoo_key = meta_types
            .iter()
            .filter_map(|v| v.as_str())
            .find(|k| YAHOO_FACTS.iter().any(|(y, _)| y == k))
            .map(|s| s.to_string());
        if yahoo_key.is_none() {
            yahoo_key = YAHOO_FACTS
                .iter()
                .find(|(k, _)| series.contains_key(*k))
                .map(|(k, _)| k.to_string());
        }
        let Some(yahoo_key) = yahoo_key else { continue };
        let canonical = YAHOO_FACTS
            .iter()
            .find(|(y, _)| *y == yahoo_key)
            .map(|(_, c)| *c)
            .unwrap_or("");
        let Some(points) = series.get(&yahoo_key).and_then(|p| p.as_array()) else {
            continue;
        };
        for point in points {
            let Some(point) = point.as_object() else {
                continue;
            };
            match point.get("periodType") {
                None | Some(Value::Null) => {}
                Some(Value::String(s)) if s == "12M" => {}
                _ => continue,
            }
            let period: String = point
                .get("asOfDate")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .chars()
                .take(10)
                .collect();
            let value = point
                .get("reportedValue")
                .and_then(|r| r.get("raw"))
                .and_then(number);
            let (Some(value), false) = (value, period.is_empty()) else {
                continue;
            };
            if currency.is_none() {
                currency = point
                    .get("currencyCode")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
            }
            periods
                .entry(period)
                .or_default()
                .insert(canonical.to_string(), json!(value));
        }
    }

    let mut out_periods = Map::new();
    for (period, mut facts) in periods {
        facts.insert(
            "gross_margin".into(),
            ratio(
                facts.get("gross_profit").unwrap_or(&Value::Null),
                facts.get("revenue").unwrap_or(&Value::Null),
            ),
        );
        facts.insert(
            "operating_margin".into(),
            ratio(
                facts.get("operating_income").unwrap_or(&Value::Null),
                facts.get("revenue").unwrap_or(&Value::Null),
            ),
        );
        facts.insert(
            "net_margin".into(),
            ratio(
                facts.get("net_income").unwrap_or(&Value::Null),
                facts.get("revenue").unwrap_or(&Value::Null),
            ),
        );
        facts.insert(
            "roe".into(),
            ratio(
                facts.get("net_income").unwrap_or(&Value::Null),
                facts.get("equity").unwrap_or(&Value::Null),
            ),
        );
        let ocf = facts.get("operating_cash_flow").and_then(number);
        let capex = facts.get("capital_expenditure").and_then(number);
        if let (Some(ocf), Some(capex)) = (ocf, capex) {
            let fcf = if capex < 0.0 { ocf + capex } else { ocf - capex };
            facts.insert("free_cash_flow".into(), json!(fcf));
        }
        out_periods.insert(period, Value::Object(facts));
    }

    json!({
        "symbol": symbol,
        "basis": "annual",
        "currency": currency,
        "periods": Value::Object(out_periods),
        "source": "yahoo_fundamentals_timeseries",
    })
}

const MONETARY_METRICS: &[&str] = &[
    "revenue",
    "gross_profit",
    "operating_income",
    "net_income",
    "equity",
    "operating_cash_flow",
    "capital_expenditure",
    "free_cash_flow",
];

/// `apply_yearly_fx(financials, yearly_rates, base_currency)`.
pub fn apply_yearly_fx(financials: &Value, yearly_rates: &Value, base_currency: &str) -> Value {
    let mut result = financials.as_object().cloned().unwrap_or_default();
    result.insert("base_currency".into(), json!(base_currency));
    let mut periods = Map::new();
    if let Some(raw_periods) = financials.get("periods").and_then(|p| p.as_object()) {
        for (period, raw_facts) in raw_periods {
            let mut facts = raw_facts.as_object().cloned().unwrap_or_default();
            let year: String = period.chars().take(4).collect();
            let rate = yearly_rates.get(&year).and_then(number);
            if let Some(rate) = rate.filter(|r| *r > 0.0) {
                facts.insert("fx_rate_to_base".into(), json!(rate));
                for metric in MONETARY_METRICS {
                    if let Some(v) = facts.get(*metric).and_then(number) {
                        facts.insert(
                            format!("{metric}_base"),
                            json!(uzi_core::py::round(v * rate, 4)),
                        );
                    }
                }
            }
            periods.insert(period.clone(), Value::Object(facts));
        }
    }
    result.insert("periods".into(), Value::Object(periods));
    Value::Object(result)
}

/// `issuer_key(name)`.
pub fn issuer_key(name: &str) -> String {
    let normalized = LEGAL_SUFFIXES.replace_all(name, " ");
    let lower = normalized.to_lowercase();
    lower
        .chars()
        .filter(|c| {
            c.is_ascii_lowercase()
                || c.is_ascii_digit()
                || ('\u{4e00}'..='\u{9fff}').contains(c)
        })
        .collect()
}

/// `_candidate_score(target, candidate)` → (score, reasons).
pub fn candidate_score(target: &Value, candidate: &Value) -> (f64, Vec<String>) {
    let mut score = 0.0f64;
    let mut reasons: Vec<String> = Vec::new();

    let same = |k: &str| -> bool {
        target
            .get(k)
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .map(|t| Some(t) == candidate.get(k).and_then(|v| v.as_str()))
            .unwrap_or(false)
    };
    if same("industry") {
        score += 40.0;
        reasons.push("细分行业一致".into());
    }
    if same("sector") {
        score += 15.0;
        reasons.push("行业板块一致".into());
    }

    let target_has_base = target.get("market_cap_base").map(|v| !v.is_null()).unwrap_or(false);
    let candidate_has_base = candidate
        .get("market_cap_base")
        .map(|v| !v.is_null())
        .unwrap_or(false);
    let target_cap = number_or(
        target,
        if target_has_base { "market_cap_base" } else { "market_cap" },
    );
    let candidate_cap = number_or(
        candidate,
        if candidate_has_base {
            "market_cap_base"
        } else {
            "market_cap"
        },
    );
    let same_currency = target
        .get("currency")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(|t| Some(t) == candidate.get("currency").and_then(|v| v.as_str()))
        .unwrap_or(false);
    let comparable_scale = (target_has_base && candidate_has_base) || same_currency;
    if comparable_scale {
        if let (Some(tc), Some(cc)) = (target_cap, candidate_cap) {
            if tc > 0.0 && cc > 0.0 {
                let distance = (cc / tc).log10().abs();
                let scale_score = (20.0 * (1.0 - distance / 3.0)).max(0.0);
                score += scale_score;
                if distance <= 1.0 {
                    reasons.push("规模可比".into());
                }
            }
        }
    }

    if let Some(coverage) = number_or(candidate, "data_coverage") {
        score += coverage.clamp(0.0, 1.0) * 15.0;
        if coverage >= 0.7 {
            reasons.push("财务数据完整".into());
        }
    }
    if let Some(ps) = number_or(candidate, "provider_score") {
        score += ps.clamp(0.0, 1.0) * 10.0;
    }
    if candidate.get("is_secondary").and_then(|v| v.as_bool()) == Some(true) {
        score -= 25.0;
    }
    if reasons.is_empty() {
        reasons.push("候选来源关联".into());
    }
    (uzi_core::py::round(score.max(0.0), 2), reasons)
}

/// `rank_global_candidates(target, candidates, limit)`.
pub fn rank_global_candidates(target: &Value, candidates: &[Value], limit: usize) -> Vec<Value> {
    let target_symbol = target
        .get("symbol")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_uppercase();
    let target_issuer = issuer_key(target.get("name").and_then(|v| v.as_str()).unwrap_or(""));
    let mut best: Vec<(String, Value)> = Vec::new();

    for raw in candidates {
        let Some(raw_obj) = raw.as_object() else { continue };
        let symbol = raw_obj
            .get("symbol")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_uppercase();
        let key = issuer_key(
            raw_obj
                .get("name")
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
                .unwrap_or(&symbol),
        );
        if symbol.is_empty()
            || symbol == target_symbol
            || (!target_issuer.is_empty() && key == target_issuer)
        {
            continue;
        }
        let (score, reasons) = candidate_score(target, raw);
        let mut candidate = raw_obj.clone();
        candidate.insert("symbol".into(), json!(symbol));
        candidate.insert("relevance_score".into(), json!(score));
        candidate.insert("selection_reasons".into(), json!(reasons));
        let candidate = Value::Object(candidate);
        match best.iter_mut().find(|(k, _)| *k == key) {
            Some((_, current)) => {
                let cur = current
                    .get("relevance_score")
                    .and_then(|v| v.as_f64())
                    .unwrap_or(0.0);
                if score > cur {
                    *current = candidate;
                }
            }
            None => best.push((key, candidate)),
        }
    }

    best.sort_by(|a, b| {
        let sa = a.1.get("relevance_score").and_then(|v| v.as_f64()).unwrap_or(0.0);
        let sb = b.1.get("relevance_score").and_then(|v| v.as_f64()).unwrap_or(0.0);
        sb.partial_cmp(&sa)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| {
                let ca = a.1.get("data_coverage").and_then(|v| v.as_f64()).unwrap_or(0.0);
                let cb = b.1.get("data_coverage").and_then(|v| v.as_f64()).unwrap_or(0.0);
                cb.partial_cmp(&ca).unwrap_or(std::cmp::Ordering::Equal)
            })
    });
    best.into_iter().take(limit).map(|(_, v)| v).collect()
}

// ─────────────────────────────────────────────────────────────
// YahooGlobalPeerProvider
// ─────────────────────────────────────────────────────────────

pub const YAHOO_PEER_BASE: &str = "https://query2.finance.yahoo.com";

static SYMBOL_RE: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r"^[A-Za-z0-9.\-^=]{1,32}$").unwrap());

/// `YahooGlobalPeerProvider._validate_symbol`.
pub fn validate_symbol(symbol: &str) -> Result<String, String> {
    let normalized = symbol.trim().to_uppercase();
    if !SYMBOL_RE.is_match(&normalized) {
        return Err(format!("unsupported Yahoo symbol: {symbol:?}"));
    }
    Ok(normalized)
}

fn fact_types() -> String {
    YAHOO_FACTS
        .iter()
        .map(|(k, _)| *k)
        .collect::<Vec<_>>()
        .join(",")
}

/// `YahooGlobalPeerProvider.profile` — the yfinance-free branch (upstream
/// returns exactly this stub when `yf` is `None`).
pub fn profile(symbol: &str) -> Result<Value, String> {
    let symbol = validate_symbol(symbol)?;
    let ti = uzi_core::ticker::parse_ticker(&symbol);
    Ok(json!({
        "symbol": symbol,
        "market": ti.market,
        "country": ti.country,
        "exchange": ti.exchange,
        "currency": ti.currency,
    }))
}

/// `YahooGlobalPeerProvider.discover` — requires yfinance's screener; returns
/// `[]` without it (upstream's documented degradation).
pub fn discover(_target_symbol: &str, _limit: usize) -> Vec<Value> {
    Vec::new()
}

/// `YahooGlobalPeerProvider.financials` — Yahoo fundamentals time-series HTTP.
pub fn financials(symbol: &str, years: usize, timeout: u64) -> Result<Value, String> {
    let symbol = validate_symbol(symbol)?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let span = years.max(2).min(10) as i64 * 366 * 86400;
    let url = format!(
        "{YAHOO_PEER_BASE}/ws/fundamentals-timeseries/v1/finance/timeseries/{symbol}"
    );
    let v = http::get_json_q(
        &url,
        &[
            ("symbol", &symbol),
            ("type", &fact_types()),
            ("period1", &(now - span).to_string()),
            ("period2", &now.to_string()),
        ],
        &[("User-Agent", "UZI-Skill/3 global-peer-comparison")],
        timeout,
    )?;
    Ok(normalize_yahoo_timeseries(&symbol, &v))
}

/// `YahooFxProvider.rates` — calendar-year average FX from Yahoo chart v8.
pub fn fx_rates(
    source_currency: &str,
    base_currency: &str,
    years: usize,
    timeout: u64,
) -> Result<Value, String> {
    let source = source_currency.to_uppercase();
    let base = base_currency.to_uppercase();
    let iso = |s: &str| s.len() == 3 && s.chars().all(|c| c.is_ascii_uppercase());
    if !iso(&source) || !iso(&base) {
        return Err("currency must be a three-letter ISO-like code".into());
    }
    if source == base {
        let current_year = chrono::Utc::now().year();
        let mut out = Map::new();
        for year in (current_year - years as i32)..=(current_year) {
            out.insert(year.to_string(), json!(1.0));
        }
        return Ok(Value::Object(out));
    }
    let direct = chart_year_averages(&format!("{source}{base}=X"), years, timeout);
    if !direct.is_empty() {
        return Ok(Value::Object(direct));
    }
    let inverse = chart_year_averages(&format!("{base}{source}=X"), years, timeout);
    let mut out = Map::new();
    for (year, value) in inverse {
        if let Some(v) = number(&value).filter(|v| *v > 0.0) {
            out.insert(year, json!(uzi_core::py::round(1.0 / v, 8)));
        }
    }
    Ok(Value::Object(out))
}

fn chart_year_averages(symbol: &str, years: usize, timeout: u64) -> Map<String, Value> {
    use chrono::Datelike;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let span = years.max(2).min(10) as i64 * 366 * 86400;
    let url = format!("https://query1.finance.yahoo.com/v8/finance/chart/{symbol}");
    let Ok(v) = http::get_json_q(
        &url,
        &[
            ("period1", &(now - span).to_string()),
            ("period2", &now.to_string()),
            ("interval", "1d"),
            ("events", "history"),
        ],
        &[("User-Agent", "UZI-Skill/3 global-peer-comparison")],
        timeout,
    ) else {
        return Map::new();
    };
    let Some(series) = v
        .get("chart")
        .and_then(|c| c.get("result"))
        .and_then(|r| r.as_array())
        .and_then(|r| r.first())
    else {
        return Map::new();
    };
    let timestamps = series
        .get("timestamp")
        .and_then(|t| t.as_array())
        .cloned()
        .unwrap_or_default();
    let closes = series
        .get("indicators")
        .and_then(|i| i.get("quote"))
        .and_then(|q| q.as_array())
        .and_then(|q| q.first())
        .and_then(|q| q.get("close"))
        .and_then(|c| c.as_array())
        .cloned()
        .unwrap_or_default();
    let mut grouped: BTreeMap<String, Vec<f64>> = BTreeMap::new();
    for (ts, close) in timestamps.iter().zip(closes.iter()) {
        let Some(value) = number(close).filter(|v| *v > 0.0) else {
            continue;
        };
        let Some(year) = ts
            .as_i64()
            .and_then(|t| chrono::DateTime::from_timestamp(t, 0))
            .map(|d| d.year().to_string())
        else {
            continue;
        };
        grouped.entry(year).or_default().push(value);
    }
    let mut out = Map::new();
    for (year, values) in grouped {
        let avg = values.iter().sum::<f64>() / values.len() as f64;
        out.insert(year, json!(uzi_core::py::round(avg, 8)));
    }
    out
}

// ─────────────────────────────────────────────────────────────
// Comparison builders
// ─────────────────────────────────────────────────────────────

pub const COMPARABLE_METRICS: &[&str] = &[
    "revenue_base",
    "net_income_base",
    "gross_margin",
    "operating_margin",
    "net_margin",
    "roe",
    "free_cash_flow_base",
];

/// Python `statistics.quantiles(data, n=4)` (exclusive method).
fn quartiles(ordered: &[f64]) -> [f64; 3] {
    let ld = ordered.len();
    let m = ld as f64 + 1.0;
    let mut out = [0.0; 3];
    for i in 1..4usize {
        // exact integer math: j = i*m // n with n=4
        let mut j = ((i as f64 * m) as i64 / 4) as usize;
        if j < 1 {
            j = 1;
        }
        if j > ld - 1 {
            j = ld - 1;
        }
        let delta = i as f64 * m - j as f64 * 4.0;
        out[i - 1] = (ordered[j - 1] * (4.0 - delta) + ordered[j] * delta) / 4.0;
    }
    out
}

fn median(ordered: &[f64]) -> f64 {
    let n = ordered.len();
    if n == 0 {
        return 0.0;
    }
    if n % 2 == 1 {
        ordered[n / 2]
    } else {
        (ordered[n / 2 - 1] + ordered[n / 2]) / 2.0
    }
}

/// `_benchmarks(peers)`.
pub fn benchmarks(peers: &[Value]) -> Value {
    let mut grouped: Vec<(String, BTreeMap<String, Vec<f64>>)> = Vec::new();
    for peer in peers {
        let Some(periods) = peer
            .get("financials")
            .and_then(|f| f.get("periods"))
            .and_then(|p| p.as_object())
        else {
            continue;
        };
        for (period, facts) in periods {
            let year: String = period.chars().take(4).collect();
            if year.len() != 4 || !facts.is_object() {
                continue;
            }
            for metric in COMPARABLE_METRICS {
                let Some(value) = facts.get(*metric).and_then(number) else {
                    continue;
                };
                match grouped.iter_mut().find(|(m, _)| m == metric) {
                    Some((_, years)) => years.entry(year.clone()).or_default().push(value),
                    None => {
                        let mut years = BTreeMap::new();
                        years.insert(year.clone(), vec![value]);
                        grouped.push((metric.to_string(), years));
                    }
                }
            }
        }
    }
    let mut result = Map::new();
    for (metric, years) in grouped {
        let mut metric_out = Map::new();
        for (year, values) in years {
            let mut ordered = values.clone();
            ordered.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
            let (p25, p75) = if ordered.len() >= 2 {
                let q = quartiles(&ordered);
                (q[0], q[2])
            } else {
                (ordered[0], ordered[0])
            };
            metric_out.insert(
                year,
                json!({
                    "min": uzi_core::py::round(ordered[0], 2),
                    "p25": uzi_core::py::round(p25, 2),
                    "median": uzi_core::py::round(median(&ordered), 2),
                    "p75": uzi_core::py::round(p75, 2),
                    "max": uzi_core::py::round(ordered[ordered.len() - 1], 2),
                    "n": ordered.len(),
                }),
            );
        }
        result.insert(metric, Value::Object(metric_out));
    }
    Value::Object(result)
}

/// `_latest_facts(financials)`.
pub fn latest_facts(financials: &Value) -> (Option<String>, Value) {
    let Some(periods) = financials.get("periods").and_then(|p| p.as_object()) else {
        return (None, json!({}));
    };
    if periods.is_empty() {
        return (None, json!({}));
    }
    let period = periods.keys().max().cloned();
    let facts = period
        .as_ref()
        .and_then(|p| periods.get(p))
        .cloned()
        .unwrap_or_else(|| json!({}));
    (period, facts)
}

/// `_target_percentile(target_financials, peers)`.
pub fn target_percentile(target_financials: &Value, peers: &[Value]) -> Value {
    if peers.len() < 3 {
        return json!({});
    }
    let (Some(target_period), target_facts) = latest_facts(target_financials) else {
        return json!({});
    };
    let year: String = target_period.chars().take(4).collect();
    let mut result = Map::new();
    for metric in COMPARABLE_METRICS {
        let target_value = target_facts.get(*metric).and_then(number);
        let mut values: Vec<f64> = Vec::new();
        for peer in peers {
            let Some(periods) = peer
                .get("financials")
                .and_then(|f| f.get("periods"))
                .and_then(|p| p.as_object())
            else {
                continue;
            };
            let matching: Vec<&Value> = periods
                .iter()
                .filter(|(period, _)| period.starts_with(&year))
                .map(|(_, facts)| facts)
                .collect();
            if let Some(facts) = matching.last() {
                if let Some(v) = facts.get(*metric).and_then(number) {
                    values.push(v);
                }
            }
        }
        if let Some(tv) = target_value {
            if values.len() >= 3 {
                let below = values.iter().filter(|v| **v < tv).count();
                result.insert(
                    (*metric).to_string(),
                    json!(uzi_core::py::round(
                        below as f64 / values.len() as f64 * 100.0,
                        1
                    )),
                );
            }
        }
    }
    Value::Object(result)
}

/// `build_global_peer_comparison(target_symbol, provider, limit)`.
pub fn build_global_peer_comparison(target_symbol: &str, limit: usize, timeout: u64) -> Value {
    let target_symbol = match validate_symbol(target_symbol) {
        Ok(s) => s,
        Err(e) => return json!({"error": e}),
    };
    let target_profile = profile(&target_symbol).unwrap_or_else(|_| json!({}));
    let mut failures = Map::new();
    let target_financials = match financials(&target_symbol, 6, timeout) {
        Ok(v) => v,
        Err(e) => {
            failures.insert(target_symbol.clone(), json!(first160(&e)));
            json!({"symbol": target_symbol, "basis": "annual", "periods": {}})
        }
    };

    let candidate_limit = (limit.max(1) * 2).min(24);
    let mut peers: Vec<Value> = Vec::new();
    for candidate in discover(&target_symbol, candidate_limit) {
        if peers.len() >= limit {
            break;
        }
        let symbol = candidate
            .get("symbol")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        if symbol.is_empty() {
            continue;
        }
        match financials(&symbol, 6, timeout) {
            Ok(fin) if fin.get("periods").and_then(|p| p.as_object()).map(|p| !p.is_empty()).unwrap_or(false) => {
                let mut peer = candidate.as_object().cloned().unwrap_or_default();
                peer.insert("financials".into(), fin);
                peers.push(Value::Object(peer));
            }
            Ok(_) => {
                failures.insert(symbol.clone(), json!("ValueError: empty annual financial series"));
            }
            Err(e) => {
                failures.insert(symbol, json!(first160(&e)));
            }
        }
    }

    let percentiles = target_percentile(&target_financials, &peers);
    let mut target = target_profile.as_object().cloned().unwrap_or_default();
    target.insert("financials".into(), target_financials);
    let mut failed: Vec<String> = failures.keys().cloned().collect();
    failed.sort();
    json!({
        "target": Value::Object(target),
        "peers": peers,
        "peer_count": peers.len(),
        "benchmarks": benchmarks(&peers),
        "target_percentile": percentiles,
        "conclusion_status": if peers.len() >= 3 { "ready" } else { "insufficient_peers" },
        "failed_symbols": failed,
        "failures": Value::Object(failures),
        "source": "global_peer_provider_registry",
    })
}

/// `apply_comparison_fx(result, fx_provider, base_currency)`.
pub fn apply_comparison_fx(result: &Value, base_currency: &str, timeout: u64) -> Value {
    let mut out = result.as_object().cloned().unwrap_or_default();
    let mut rates_by_currency: Vec<(String, Value)> = Vec::new();

    let mut convert = |financials: Value| -> Value {
        let currency = financials
            .get("currency")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_uppercase();
        if currency.is_empty() {
            return financials;
        }
        if !rates_by_currency.iter().any(|(c, _)| *c == currency) {
            let rates = fx_rates(&currency, base_currency, 6, timeout)
                .unwrap_or_else(|_| json!({}));
            rates_by_currency.push((currency.clone(), rates));
        }
        let rates = rates_by_currency
            .iter()
            .find(|(c, _)| *c == currency)
            .map(|(_, r)| r.clone())
            .unwrap_or_else(|| json!({}));
        let has_rates = rates
            .as_object()
            .map(|o| !o.is_empty())
            .unwrap_or(false);
        if has_rates {
            apply_yearly_fx(&financials, &rates, base_currency)
        } else {
            financials
        }
    };

    let mut target = out
        .get("target")
        .and_then(|t| t.as_object().cloned())
        .unwrap_or_default();
    let target_fin = target.get("financials").cloned().unwrap_or_else(|| json!({}));
    target.insert("financials".into(), convert(target_fin));
    out.insert("target".into(), Value::Object(target));

    let mut peers: Vec<Value> = Vec::new();
    for raw_peer in out.get("peers").and_then(|p| p.as_array()).cloned().unwrap_or_default() {
        let mut peer = raw_peer.as_object().cloned().unwrap_or_default();
        let fin = peer.get("financials").cloned().unwrap_or_else(|| json!({}));
        peer.insert("financials".into(), convert(fin));
        peers.push(Value::Object(peer));
    }
    out.insert("peers".into(), json!(peers));
    out.insert("base_currency".into(), json!(base_currency));
    out.insert("benchmarks".into(), benchmarks(&peers));
    let target_fin = out
        .get("target")
        .and_then(|t| t.get("financials"))
        .cloned()
        .unwrap_or_else(|| json!({}));
    out.insert("target_percentile".into(), target_percentile(&target_fin, &peers));
    let mut fx_currencies: Vec<String> = rates_by_currency
        .iter()
        .filter(|(_, rates)| rates.as_object().map(|o| !o.is_empty()).unwrap_or(false))
        .map(|(c, _)| c.clone())
        .collect();
    fx_currencies.sort();
    out.insert("fx_currencies".into(), json!(fx_currencies));
    Value::Object(out)
}

/// `global_peers_to_comps(comparison)`.
pub fn global_peers_to_comps(comparison: &Value) -> Value {
    let mut result = Vec::new();
    for peer in comparison
        .get("peers")
        .and_then(|p| p.as_array())
        .cloned()
        .unwrap_or_default()
    {
        let Some(periods) = peer
            .get("financials")
            .and_then(|f| f.get("periods"))
            .and_then(|p| p.as_object())
        else {
            continue;
        };
        if periods.is_empty() {
            continue;
        }
        let mut ordered: Vec<&String> = periods.keys().collect();
        ordered.sort();
        let latest = periods.get(ordered[ordered.len() - 1]).cloned().unwrap_or_else(|| json!({}));
        let growth = if ordered.len() >= 2 {
            let previous = periods
                .get(ordered[ordered.len() - 2])
                .and_then(|f| f.get("revenue_base"))
                .and_then(number);
            let current = latest.get("revenue_base").and_then(number);
            match (previous, current) {
                (Some(p), Some(c)) if p != 0.0 => {
                    Some(uzi_core::py::round((c / p - 1.0) * 100.0, 2))
                }
                _ => None,
            }
        } else {
            None
        };
        result.push(json!({
            "name": peer.get("name").cloned().unwrap_or_else(|| peer.get("symbol").cloned().unwrap_or(Value::Null)),
            "ticker": peer.get("symbol").cloned().unwrap_or(Value::Null),
            "pe": peer.get("pe").cloned().unwrap_or(Value::Null),
            "pb": peer.get("pb").cloned().unwrap_or(Value::Null),
            "ps": peer.get("ps").cloned().unwrap_or(Value::Null),
            "roe": latest.get("roe").cloned().unwrap_or(Value::Null),
            "net_margin": latest.get("net_margin").cloned().unwrap_or(Value::Null),
            "revenue_growth": growth,
            "market_cap_yi": 0,
        }));
    }
    Value::Array(result)
}

/// `fetch_global_peer_comparison(ticker_info, basic, limit, provider, use_cache)`.
pub fn fetch_global_peer_comparison(ti: &TickerInfo, basic: Option<&Value>, limit: usize) -> Value {
    let symbol = to_yahoo_symbol(ti);
    let ti2 = ti.clone();
    let basic = basic.cloned();
    cached::<_, anyhow::Error>(
        &ti.full,
        &format!("global_peer_comparison_v2__{symbol}__n{limit}"),
        TTL_QUARTERLY,
        move || {
            let mut result = build_global_peer_comparison(&symbol, limit, 12);
            result = apply_comparison_fx(&result, "USD", 12);
            if let Some(basic) = basic.as_ref() {
                if let Some(target) = result.get_mut("target").and_then(|t| t.as_object_mut()) {
                    target.insert("uzi_symbol".into(), json!(ti2.full));
                    if let Some(name) = basic.get("name").filter(|n| !n.is_null()) {
                        target.insert("name".into(), name.clone());
                    }
                    if let Some(ind) = basic.get("industry").filter(|n| !n.is_null()) {
                        target.insert("local_industry".into(), ind.clone());
                    }
                }
            }
            Ok(result)
        },
    )
    .unwrap_or_else(|_| json!({}))
}

fn first160(s: &str) -> String {
    s.chars().take(160).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn yahoo_symbols() {
        assert_eq!(to_yahoo_symbol(&uzi_core::ticker::parse_ticker("600519.SH")), "600519.SS");
        assert_eq!(to_yahoo_symbol(&uzi_core::ticker::parse_ticker("002273.SZ")), "002273.SZ");
        assert_eq!(to_yahoo_symbol(&uzi_core::ticker::parse_ticker("00700.HK")), "0700.HK");
    }

    #[test]
    fn issuer_key_strips_legal_suffixes() {
        assert_eq!(issuer_key("Apple Inc."), "apple");
        assert_eq!(issuer_key("Tencent Holdings Limited"), "tencent");
        assert_eq!(issuer_key("贵州茅台"), "贵州茅台");
    }

    #[test]
    fn timeseries_normalization_computes_margins() {
        let payload = json!({"timeseries": {"result": [
            {"meta": {"type": ["annualTotalRevenue"]}, "annualTotalRevenue": [
                {"asOfDate": "2024-12-31", "periodType": "12M", "currencyCode": "USD",
                 "reportedValue": {"raw": 1000.0}}]},
            {"meta": {"type": ["annualNetIncome"]}, "annualNetIncome": [
                {"asOfDate": "2024-12-31", "periodType": "12M",
                 "reportedValue": {"raw": 250.0}}]},
            {"meta": {"type": ["annualStockholdersEquity"]}, "annualStockholdersEquity": [
                {"asOfDate": "2024-12-31", "periodType": "12M",
                 "reportedValue": {"raw": 500.0}}]},
            {"meta": {"type": ["annualOperatingCashFlow"]}, "annualOperatingCashFlow": [
                {"asOfDate": "2024-12-31", "periodType": "12M",
                 "reportedValue": {"raw": 300.0}}]},
            {"meta": {"type": ["annualCapitalExpenditure"]}, "annualCapitalExpenditure": [
                {"asOfDate": "2024-12-31", "periodType": "12M",
                 "reportedValue": {"raw": -50.0}}]}
        ]}});
        let out = normalize_yahoo_timeseries("TEST", &payload);
        let facts = &out["periods"]["2024-12-31"];
        assert_eq!(facts["net_margin"], json!(25.0));
        assert_eq!(facts["roe"], json!(50.0));
        assert_eq!(facts["free_cash_flow"], json!(250.0));
        assert_eq!(out["currency"], json!("USD"));
    }

    #[test]
    fn quartiles_match_python_exclusive_method() {
        assert_eq!(quartiles(&[1.0, 2.0]), [0.75, 1.5, 2.25]);
        assert_eq!(quartiles(&[1.0, 2.0, 3.0]), [1.0, 2.0, 3.0]);
        assert_eq!(quartiles(&[1.0, 2.0, 3.0, 4.0]), [1.25, 2.5, 3.75]);
        assert_eq!(quartiles(&[1.0, 5.0, 9.0, 12.0, 15.0]), [3.0, 9.0, 13.5]);
    }

    #[test]
    fn candidate_score_prefers_same_industry_and_scale() {
        let target = json!({"symbol": "T", "industry": "Software", "market_cap": 1e11, "currency": "USD"});
        let good = json!({"symbol": "G", "industry": "Software", "market_cap": 1.2e11, "currency": "USD"});
        let bad = json!({"symbol": "B", "industry": "Banks", "market_cap": 1e9, "currency": "USD", "is_secondary": true});
        let (gs, _) = candidate_score(&target, &good);
        let (bs, _) = candidate_score(&target, &bad);
        assert!(gs > bs);
        assert!(bs >= 0.0);
    }

    #[test]
    fn rank_dedupes_by_issuer_and_excludes_target() {
        let target = json!({"symbol": "AAPL", "name": "Apple Inc.", "industry": "Consumer Electronics"});
        let cands = vec![
            json!({"symbol": "AAPL", "name": "Apple Inc."}),
            json!({"symbol": "APLE", "name": "Apple Inc.", "data_coverage": 1.0}),
            json!({"symbol": "MSFT", "name": "Microsoft Corp", "data_coverage": 0.8}),
        ];
        let ranked = rank_global_candidates(&target, &cands, 8);
        assert_eq!(ranked.len(), 1);
        assert_eq!(ranked[0]["symbol"], json!("MSFT"));
    }

    #[test]
    fn validate_symbol_rejects_injection() {
        assert!(validate_symbol("AAPL").is_ok());
        assert!(validate_symbol("0700.HK").is_ok());
        assert!(validate_symbol("AA PL").is_err());
        assert!(validate_symbol("../../etc").is_err());
        assert!(validate_symbol("").is_err());
    }
}
