//! Port of the 22 `fetch_*.py` scripts (one module per upstream file).
//!
//! Each module exposes `pub fn main(...) -> Result<Value, String>` returning the
//! legacy dict shapes verbatim:
//!
//! * `{"ticker", "market", "data", "source", "fallback"}` — most fetchers
//! * a bare dict — `fetch_macro` / `fetch_policy` / `fetch_industry`
//!
//! [`crate::base_fetcher::FnFetcher`] applies upstream's unwrapping rules.

pub mod basic;
pub mod financials;
pub mod kline;
pub mod macro_;
pub mod peers;
pub mod chain;
pub mod fund_holders;
pub mod research;
pub mod industry;
pub mod materials;
pub mod futures;
pub mod valuation;
pub mod governance;
pub mod capital_flow;
pub mod policy;
pub mod moat;
pub mod events;
pub mod lhb;
pub mod sentiment;
pub mod trap_signals;
pub mod contests;
pub mod similar_stocks;

/// The ticker string a registry job passes in.
pub fn ticker_str(v: &serde_json::Value) -> &str {
    v.as_str().unwrap_or("")
}
