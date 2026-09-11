//! Port of `lib/providers/baostock_provider.py`.
//!
//! BaoStock speaks its own (non-HTTP) protocol through the Python `baostock`
//! package: `login()` / `query_history_k_data_plus()` over a proprietary socket
//! session. There is no documented HTTP endpoint to reimplement, so the Rust
//! provider mirrors upstream's `_BS_OK == False`: unavailable, and every method
//! raises `ProviderError("baostock 未安装")`. `_bs_code` is ported because
//! callers format symbols with it.

use serde_json::Value;

use super::ProviderError;

pub const NAME: &str = "baostock";
pub const REQUIRES_KEY: bool = false;
pub const MARKETS: &[&str] = &["A"];

pub fn is_available() -> bool {
    false
}

/// `600519 → sh.600519` / `000001 → sz.000001`.
pub fn bs_code(code: &str) -> String {
    let code6 = code.split('.').next().unwrap_or(code);
    let code6 = format!("{code6:0>6}");
    let sh = [
        "60", "68", "90", "50", "51", "52", "56", "58", "10", "11",
    ]
    .iter()
    .any(|p| code6.starts_with(p));
    format!("{}{code6}", if sh { "sh." } else { "sz." })
}

fn unavailable() -> ProviderError {
    ProviderError("baostock 未安装".into())
}

pub fn fetch_financials_a(_code: &str, _years: usize) -> Result<Value, ProviderError> {
    Err(unavailable())
}

pub fn fetch_kline_a(_code: &str, _start: &str) -> Result<Value, ProviderError> {
    Err(unavailable())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bs_code_matches_upstream() {
        assert_eq!(bs_code("600519"), "sh.600519");
        assert_eq!(bs_code("000001"), "sz.000001");
        assert_eq!(bs_code("688981"), "sh.688981");
    }
}
