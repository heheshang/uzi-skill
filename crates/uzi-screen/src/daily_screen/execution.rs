//! Port of `lib/daily_screen/execution.py` — observable execution
//! prerequisites, not a promise of a future fill.

use chrono::{DateTime, FixedOffset, Utc};
use serde_json::Value;

use super::events::evidence_time;
use super::models::StockSnapshot;
use super::universe::number;

/// `exchange_open(market, now)`.
///
/// Upstream additionally consults `exchange_calendars`; the Rust port keeps the
/// session/ weekday clock implemented by [`uzi_core::cache::market_status`]
/// (which reports `calendar_verified: false` for the same reason).
pub fn exchange_open(market: &str, now: &DateTime<FixedOffset>) -> bool {
    uzi_core::cache::market_status(market, Some(now.with_timezone(&Utc))).is_open
}

fn get_str<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value.get(key).and_then(|v| v.as_str())
}

fn fnum(value: &Value, key: &str) -> Option<f64> {
    value.get(key).and_then(number)
}

fn valid_positive(value: &Value, key: &str) -> bool {
    matches!(fnum(value, key), Some(x) if x > 0.0)
}

fn ms_between(later: &DateTime<FixedOffset>, earlier: &DateTime<FixedOffset>) -> i64 {
    later.signed_duration_since(*earlier).num_milliseconds()
}

/// `execution_gaps(stock, now)`.
pub fn execution_gaps(stock: &StockSnapshot, now: &DateTime<FixedOffset>) -> Vec<String> {
    let bundle = match stock.extra.get("intraday") {
        Some(v) if uzi_core::py::truthy(v) => v.clone(),
        _ => Value::Object(Default::default()),
    };
    let quote = match bundle.get("quote") {
        Some(v) if uzi_core::py::truthy(v) => v.clone(),
        _ => Value::Object(Default::default()),
    };
    let bars: Vec<Value> = bundle
        .get("bars")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    if !uzi_core::py::truthy(&quote) || bars.is_empty() {
        return vec!["intraday_execution_unverified".to_string()];
    }

    let mut gaps: Vec<String> = Vec::new();
    if !uzi_core::py::truthy(uzi_core::py::get(&quote, "source"))
        || !uzi_core::py::truthy(uzi_core::py::get(&bundle, "minute_source"))
    {
        gaps.push("intraday_source_unverified".to_string());
    }
    let at = evidence_time(uzi_core::py::get(&quote, "quote_at"));
    match at {
        Some(at_dt) => {
            let delta = ms_between(now, &at_dt);
            if !(0..=120_000).contains(&delta) {
                gaps.push("quote_stale_or_future".to_string());
            }
        }
        None => gaps.push("quote_stale_or_future".to_string()),
    }
    if !exchange_open(&stock.market, now) {
        gaps.push("exchange_closed_or_unverified".to_string());
    }
    let expected_currency = if stock.market == "H" { "HKD" } else { "CNY" };
    if get_str(&quote, "code") != Some(stock.code.as_str())
        || get_str(&quote, "currency") != Some(expected_currency)
    {
        gaps.push("quote_identity_mismatch".to_string());
    }
    let observed = evidence_time(&Value::String(stock.observed_at.clone()));
    if fnum(&quote, "change_pct").is_none()
        || observed != at
        || stock.source != get_str(&quote, "source").unwrap_or("")
    {
        gaps.push("snapshot_quote_mismatch".to_string());
    }
    let required = [
        "price", "bid", "ask", "bid_size", "ask_size", "amount", "vwap",
    ];
    if required.iter().any(|key| !valid_positive(&quote, key)) {
        gaps.push("book_or_vwap_missing".to_string());
    } else {
        let price = fnum(&quote, "price").unwrap_or(0.0);
        let bid = fnum(&quote, "bid").unwrap_or(0.0);
        let ask = fnum(&quote, "ask").unwrap_or(0.0);
        let vwap = fnum(&quote, "vwap").unwrap_or(0.0);
        if ask < bid || (ask - bid) / price > 0.005 {
            gaps.push("book_spread_excessive".to_string());
        }
        if (ask / price - 1.0).abs() > 0.01 || (bid / price - 1.0).abs() > 0.01 {
            gaps.push("book_quote_price_mismatch".to_string());
        }
        let quote_amount = fnum(&quote, "amount").unwrap_or(0.0);
        if (stock.price / price - 1.0).abs() > 0.001
            || stock.amount < 2e8
            || (stock.amount / quote_amount - 1.0).abs() > 0.001
        {
            gaps.push("snapshot_quote_mismatch".to_string());
        }
        if !(vwap <= price && price <= vwap * 1.03) {
            gaps.push("vwap_support_unconfirmed".to_string());
        }
    }
    let last = bars.last().cloned().unwrap_or(Value::Null);
    let last_at = evidence_time(uzi_core::py::get(&last, "at"));
    match (at, last_at) {
        (Some(at_dt), Some(last_dt)) => {
            let delta = ms_between(&at_dt, &last_dt);
            if !(0..=120_000).contains(&delta) {
                gaps.push("minute_quote_time_mismatch".to_string());
            }
        }
        _ => gaps.push("minute_quote_time_mismatch".to_string()),
    }
    let recent: Vec<Value> = bars.iter().rev().take(3).rev().cloned().collect();
    let stamps: Vec<Option<DateTime<FixedOffset>>> = recent
        .iter()
        .map(|row| evidence_time(uzi_core::py::get(row, "at")))
        .collect();
    let amounts: Vec<Option<f64>> = recent
        .iter()
        .map(|row| fnum(row, "amount_local"))
        .collect();
    if recent.len() < 3
        || stamps.iter().any(Option::is_none)
        || amounts.iter().any(|a| !matches!(a, Some(x) if *x > 0.0))
    {
        gaps.push("minute_liquidity_unverified".to_string());
    } else if stamps
        .windows(2)
        .any(|pair| ms_between(&pair[1].unwrap(), &pair[0].unwrap()) != 60_000)
    {
        gaps.push("minute_sequence_unverified".to_string());
    }
    let price = fnum(&quote, "price");
    let close = fnum(&last, "close");
    if !matches!(price, Some(p) if p != 0.0)
        || !matches!(close, Some(c) if c != 0.0)
        || (close.unwrap() / price.unwrap() - 1.0).abs() > 0.01
    {
        gaps.push("minute_quote_price_mismatch".to_string());
    }
    gaps.sort();
    gaps.dedup();
    gaps
}
