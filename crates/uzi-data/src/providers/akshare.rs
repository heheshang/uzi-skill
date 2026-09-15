//! Port of `lib/providers/akshare_provider.py`.
//!
//! Upstream wraps the AkShare Python package. The Rust port cannot import
//! AkShare, so it calls the *same documented public HTTP endpoints* AkShare's
//! `stock_individual_info_em` / `stock_zh_a_hist` / `stock_financial_abstract`
//! wrappers use (EastMoney push2 / push2his / datacenter). Parsing lives in
//! [`crate::em`] and is shared with `data_sources.rs`.
//!
//! If the EastMoney endpoint is unreachable the method raises
//! `ProviderError`, exactly like upstream's `except Exception as e: raise
//! ProviderError(...)`, so the failover chain behaves identically.

use serde_json::{json, Value};

use super::ProviderError;
use crate::em;

pub const NAME: &str = "akshare";
pub const REQUIRES_KEY: bool = false;
pub const MARKETS: &[&str] = &["A", "H", "U"];

/// AkShare is a Python package; its two call patterns that the chain needs are
/// HTTP-backed, so the Rust provider is always available.
pub fn is_available() -> bool {
    true
}

/// `fetch_basic_a` — `ak.stock_individual_info_em` endpoint (push2 spot).
pub fn fetch_basic_a(code: &str) -> Result<Value, ProviderError> {
    let full = format!("{code}.{}", uzi_core::ticker::a_share_suffix(code));
    let quote = em::push2_quote(code, &full, 8)
        .map_err(|e| ProviderError(format!("akshare.stock_individual_info_em: {e}")))?;
    if quote.as_object().map(|o| o.is_empty()).unwrap_or(true) {
        return Err(ProviderError("akshare stock_individual_info_em empty".into()));
    }
    Ok(json!({"ok": true, "raw": quote}))
}

/// `fetch_financials_a` — `ak.stock_financial_abstract` endpoint.
pub fn fetch_financials_a(code: &str) -> Result<Value, ProviderError> {
    let full = format!("{code}.{}", uzi_core::ticker::a_share_suffix(code));
    let rows = em::financial_abstract(code, &full, 12)
        .map_err(|e| ProviderError(format!("akshare.stock_financial_abstract: {e}")))?;
    Ok(json!({"ok": true, "raw": rows}))
}

/// `fetch_cash_flow_a` — `ak.stock_cash_flow_sheet_by_report_em` endpoint.
pub fn fetch_cash_flow_a(code: &str, dates: &str) -> Result<Value, ProviderError> {
    let full = format!("{code}.{}", uzi_core::ticker::a_share_suffix(code));
    let rows = em::cash_flow_report(code, &full, dates, 12)
        .map_err(|e| ProviderError(format!("akshare.stock_cash_flow_sheet_by_report_em: {e}")))?;
    Ok(json!({"ok": true, "raw": rows}))
}

/// `fetch_dividend_a` — `ak.stock_history_dividend_detail` endpoint.
pub fn fetch_dividend_a(code: &str) -> Result<Value, ProviderError> {
    let rows = em::dividend_history(code, 12)
        .map_err(|e| ProviderError(format!("akshare.stock_history_dividend_detail: {e}")))?;
    Ok(json!({"ok": true, "raw": rows}))
}

/// `fetch_kline_a` — `ak.stock_zh_a_hist` endpoint (EastMoney push2his).
pub fn fetch_kline_a(
    code: &str,
    period: &str,
    _start: &str,
    adjust: &str,
) -> Result<Value, ProviderError> {
    let full = format!("{code}.{}", uzi_core::ticker::a_share_suffix(code));
    let klt = match period {
        "weekly" => "102",
        "monthly" => "103",
        _ => "101",
    };
    let fqt = if adjust == "qfq" { "1" } else { "0" };
    let rows = em::kline(code, &full, klt, fqt, "500", 12)
        .map_err(|e| ProviderError(format!("akshare.stock_zh_a_hist: {e}")))?;
    Ok(Value::Array(rows))
}
