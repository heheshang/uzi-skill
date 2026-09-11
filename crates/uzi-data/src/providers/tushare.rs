//! Port of `lib/providers/tushare_provider.py`.
//!
//! Tushare Pro is a token-authenticated REST API; both `pip install tushare` and
//! a `TUSHARE_TOKEN` are required. The Rust port keeps the same availability
//! contract: [`is_available`] is true only when `TUSHARE_TOKEN` is set *and* a
//! transport exists — upstream additionally requires the Python package, which
//! Rust cannot have, so the provider always reports unavailable and raises the
//! upstream's exact "未启用" error. `_ts_code` is ported because callers use it
//! for symbol formatting regardless of transport.

use serde_json::Value;

use super::ProviderError;

pub const NAME: &str = "tushare";
pub const REQUIRES_KEY: bool = true;
pub const MARKETS: &[&str] = &["A"];

/// Upstream: `_TS_OK and bool(TUSHARE_TOKEN)`. The Python package can never be
/// present in Rust, so this is always false.
pub fn is_available() -> bool {
    false
}

pub fn token_present() -> bool {
    std::env::var("TUSHARE_TOKEN")
        .map(|v| !v.trim().is_empty())
        .unwrap_or(false)
}

/// `600519 → 600519.SH` / `000001 → 000001.SZ` / `430047 → 430047.BJ`.
pub fn ts_code(code: &str) -> String {
    let code6 = code.split('.').next().unwrap_or(code);
    let code6 = format!("{code6:0>6}");
    let prefix = |arr: &[&str]| arr.iter().any(|p| code6.starts_with(p));
    if prefix(&[
        "60", "68", "90", "50", "51", "52", "56", "58", "10", "11",
    ]) {
        return format!("{code6}.SH");
    }
    if prefix(&["83", "87", "88", "92"]) {
        return format!("{code6}.BJ");
    }
    format!("{code6}.SZ")
}

fn unavailable() -> ProviderError {
    ProviderError("Tushare 未启用（pip install tushare + TUSHARE_TOKEN）".into())
}

pub fn fetch_basic_a(_code: &str) -> Result<Value, ProviderError> {
    Err(unavailable())
}

pub fn fetch_financials_a(_code: &str, _years: usize) -> Result<Value, ProviderError> {
    Err(unavailable())
}

pub fn fetch_kline_a(
    _code: &str,
    _period: &str,
    _start: &str,
    _adjust: &str,
) -> Result<Value, ProviderError> {
    Err(unavailable())
}

pub fn fetch_top10_holders(_code: &str) -> Result<Value, ProviderError> {
    Err(unavailable())
}

pub fn fetch_top_list(_code: &str, _start: &str, _end: &str) -> Result<Value, ProviderError> {
    Err(unavailable())
}

pub fn fetch_hsgt_flow(_date: &str) -> Result<Value, ProviderError> {
    Err(unavailable())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ts_code_matches_upstream() {
        assert_eq!(ts_code("600519"), "600519.SH");
        assert_eq!(ts_code("000001"), "000001.SZ");
        assert_eq!(ts_code("430047"), "430047.SZ");
        assert_eq!(ts_code("832000"), "832000.BJ");
        assert_eq!(ts_code("600519.SH"), "600519.SH");
    }
}
