//! Port of `lib/providers/efinance_provider.py`.
//!
//! `efinance` is a Python package that aggregates EastMoney / Sina / THS
//! scrapers internally. There is no Rust equivalent and no single documented
//! HTTP endpoint that reproduces its `get_quote_history` contract across
//! A/HK/US, so the Rust provider mirrors upstream's `_EF_OK == False` state:
//! [`is_available`] is `false` and every method raises `ProviderError`, which
//! makes the failover chain skip it exactly as upstream does when the package
//! is not installed.
//!
//! (`efinance.stock.get_quote_history` for A-shares resolves to the same
//! EastMoney push2his endpoint already served by the akshare provider, so no
//! data is lost.)

use serde_json::Value;

use super::ProviderError;

pub const NAME: &str = "efinance";
pub const REQUIRES_KEY: bool = false;
pub const MARKETS: &[&str] = &["A", "H", "U"];

pub fn is_available() -> bool {
    false
}

pub fn fetch_basic_a(_code: &str) -> Result<Value, ProviderError> {
    Err(ProviderError("efinance 未安装".into()))
}

pub fn fetch_kline(_code: &str, _market: &str, _days: usize) -> Result<Value, ProviderError> {
    Err(ProviderError("efinance 未安装".into()))
}

pub fn fetch_realtime_quote(_code: &str) -> Result<Value, ProviderError> {
    Err(ProviderError("efinance 未安装".into()))
}

/// Upstream explicitly raises "efinance 不直接提供股票→基金反查，跳过".
pub fn fetch_fund_holders(_code: &str) -> Result<Value, ProviderError> {
    Err(ProviderError(
        "efinance 不直接提供股票→基金反查，跳过".into(),
    ))
}
