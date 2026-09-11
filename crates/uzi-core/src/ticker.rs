//! Port of `lib/market_router.py` — listing-market identification and code
//! normalisation for A / HK / US / global venues.

use serde::{Deserialize, Serialize};
use std::sync::LazyLock;

pub type Market = String;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SecurityType {
    Stock,
    Etf,
    Lof,
    ConvertibleBond,
    MutualFund,
    Unknown,
}

impl SecurityType {
    pub fn as_str(&self) -> &'static str {
        match self {
            SecurityType::Stock => "stock",
            SecurityType::Etf => "etf",
            SecurityType::Lof => "lof",
            SecurityType::ConvertibleBond => "convertible_bond",
            SecurityType::MutualFund => "mutual_fund",
            SecurityType::Unknown => "unknown",
        }
    }
}

/// Upstream `TickerInfo` dataclass.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TickerInfo {
    /// original user input
    pub raw: String,
    /// numeric/letter code without exchange suffix
    pub code: String,
    /// canonical: 002273.SZ / 00700.HK / AAPL
    pub full: String,
    /// legacy A/H/U or ISO-like country code for global venues
    pub market: Market,
    #[serde(default)]
    pub exchange: String,
    #[serde(default)]
    pub currency: String,
    #[serde(default)]
    pub country: String,
}

impl TickerInfo {
    fn bare(raw: &str, code: &str, full: &str, market: &str) -> Self {
        TickerInfo {
            raw: raw.to_string(),
            code: code.to_string(),
            full: full.to_string(),
            market: market.to_string(),
            exchange: String::new(),
            currency: String::new(),
            country: String::new(),
        }
    }

    fn with_venue(
        raw: &str,
        code: &str,
        full: &str,
        market: &str,
        exchange: &str,
        currency: &str,
        country: &str,
    ) -> Self {
        TickerInfo {
            raw: raw.to_string(),
            code: code.to_string(),
            full: full.to_string(),
            market: market.to_string(),
            exchange: exchange.to_string(),
            currency: currency.to_string(),
            country: country.to_string(),
        }
    }
}

const GLOBAL_SUFFIXES: &[(&str, &str, &str, &str)] = &[
    (".TWO", "TW", "TPEX", "TWD"),
    (".KS", "KR", "KRX", "KRW"),
    (".KQ", "KR", "KOSDAQ", "KRW"),
    (".TW", "TW", "TWSE", "TWD"),
    (".SI", "SG", "SGX", "SGD"),
    (".TO", "CA", "TSX", "CAD"),
    (".AX", "AU", "ASX", "AUD"),
    (".DE", "DE", "XETRA", "EUR"),
    (".PA", "FR", "EURONEXT_PARIS", "EUR"),
    (".AS", "NL", "EURONEXT_AMSTERDAM", "EUR"),
    (".SW", "CH", "SIX", "CHF"),
    (".MC", "ES", "BME", "EUR"),
    (".MI", "IT", "BORSA_ITALIANA", "EUR"),
    (".ST", "SE", "OMX_STOCKHOLM", "SEK"),
    (".OL", "NO", "OSLO", "NOK"),
    (".CO", "DK", "OMX_COPENHAGEN", "DKK"),
    (".HE", "FI", "OMX_HELSINKI", "EUR"),
    (".BR", "BE", "EURONEXT_BRUSSELS", "EUR"),
    (".LS", "PT", "EURONEXT_LISBON", "EUR"),
    (".SA", "BR", "B3", "BRL"),
    (".MX", "MX", "BMV", "MXN"),
    (".BK", "TH", "SET", "THB"),
    (".JK", "ID", "IDX", "IDR"),
    (".KL", "MY", "BURSA_MALAYSIA", "MYR"),
    (".NZ", "NZ", "NZX", "NZD"),
    (".JO", "ZA", "JSE", "ZAR"),
    (".TA", "IL", "TASE", "ILS"),
    (".VI", "AT", "VIENNA", "EUR"),
    (".WA", "PL", "WSE", "PLN"),
    (".PR", "CZ", "PRAGUE", "CZK"),
    (".BD", "HU", "BUDAPEST", "HUF"),
    (".NS", "IN", "NSE", "INR"),
    (".BO", "IN", "BSE", "INR"),
    (".T", "JP", "TSE", "JPY"),
    (".V", "CA", "TSXV", "CAD"),
    (".L", "GB", "LSE", "GBP"),
    (".F", "DE", "FRANKFURT", "EUR"),
];

const SH_STOCK_PREFIXES_3: &[&str] = &["688"];
const SH_B_SHARE: &[&str] = &["900"];
const SH_FUND_PREFIXES_2: &[&str] = &["50", "51", "52", "56", "58"];
const SH_BOND_PREFIXES_2: &[&str] = &["10", "11"];

const SZ_STOCK_PREFIXES_3: &[&str] = &["000", "001", "002", "003", "300", "301"];
const SZ_FUND_PREFIXES_3: &[&str] = &["159"];
const SZ_LOF_PREFIXES_2: &[&str] = &["16"];
const SZ_BOND_PREFIXES_2: &[&str] = &["12"];

const BJ_PREFIXES_2: &[&str] = &["83", "87", "88", "92"];

fn starts_with_any(code: &str, prefixes: &[&str]) -> bool {
    prefixes.iter().any(|p| code.starts_with(p))
}

/// Decide SZ/SH/BJ for a 6-digit A-share code (upstream `_a_share_suffix`).
pub fn a_share_suffix(code6: &str) -> &'static str {
    if starts_with_any(code6, BJ_PREFIXES_2) {
        return "BJ";
    }
    if starts_with_any(code6, SH_STOCK_PREFIXES_3) {
        return "SH";
    }
    if starts_with_any(code6, SH_B_SHARE) {
        return "SH";
    }
    if code6.starts_with("60") {
        return "SH";
    }
    if starts_with_any(code6, SH_FUND_PREFIXES_2) {
        return "SH";
    }
    if starts_with_any(code6, SH_BOND_PREFIXES_2) {
        return "SH";
    }
    "SZ"
}

/// Registry of open-end fund codes.
///
/// Upstream asks akshare (`fund_name_em`) once and treats any failure as "not a
/// fund". The Rust port reads the equivalent code list (fetched by `uzi-data`
/// from the Eastmoney fund list API) from `.cache/fund_codes.json`; when the
/// file is absent behaviour matches upstream's failure path.
mod fund_registry {
    use std::collections::HashSet;
    use std::path::{Path, PathBuf};
    use std::sync::OnceLock;

    static CODES: OnceLock<HashSet<String>> = OnceLock::new();

    pub fn registry_path() -> PathBuf {
        crate::cache::cache_root().join("fund_codes.json")
    }

    /// Install a code list, overriding the file-backed default.
    pub fn install(codes: HashSet<String>) {
        let _ = CODES.set(codes);
    }

    fn load() -> &'static HashSet<String> {
        CODES.get_or_init(|| load_from(&registry_path()).unwrap_or_default())
    }

    pub fn load_from(path: &Path) -> Option<HashSet<String>> {
        let text = std::fs::read_to_string(path).ok()?;
        let value: serde_json::Value = serde_json::from_str(&text).ok()?;
        let arr = value
            .get("data")
            .and_then(|v| v.as_array())
            .or_else(|| value.as_array())?;
        Some(
            arr.iter()
                .filter_map(|v| v.as_str())
                .map(|s| s.to_string())
                .collect(),
        )
    }

    pub fn is_mutual_fund(code6: &str) -> bool {
        load().contains(code6)
    }
}

pub use fund_registry::is_mutual_fund;

/// Install the open-end fund code list (equivalent of akshare `fund_name_em`).
pub fn install_fund_codes(codes: std::collections::HashSet<String>) {
    fund_registry::install(codes)
}

/// `classify_security_type` — without the external fund lookup this matches the
/// upstream fallback path (`_is_mutual_fund_code` returning False).
pub fn classify_security_type(code6: &str) -> SecurityType {
    classify_security_type_with(code6, is_mutual_fund)
}

/// `classify_security_type` with an injectable fund lookup.
pub fn classify_security_type_with<F: Fn(&str) -> bool>(code6: &str, is_fund: F) -> SecurityType {
    if code6.is_empty() || !code6.chars().all(|c| c.is_ascii_digit()) || code6.len() != 6 {
        return SecurityType::Unknown;
    }
    if starts_with_any(code6, SZ_FUND_PREFIXES_3) || starts_with_any(code6, SH_FUND_PREFIXES_2) {
        if code6.starts_with("501") || code6.starts_with("502") || code6.starts_with("506") {
            return SecurityType::Lof;
        }
        return SecurityType::Etf;
    }
    if starts_with_any(code6, SZ_LOF_PREFIXES_2) {
        return SecurityType::Lof;
    }
    if starts_with_any(code6, SH_BOND_PREFIXES_2) || starts_with_any(code6, SZ_BOND_PREFIXES_2) {
        if is_fund(code6) {
            return SecurityType::MutualFund;
        }
        return SecurityType::ConvertibleBond;
    }
    let looks_like_stock = starts_with_any(code6, SH_STOCK_PREFIXES_3)
        || starts_with_any(code6, SH_B_SHARE)
        || code6.starts_with("60")
        || starts_with_any(code6, SZ_STOCK_PREFIXES_3)
        || starts_with_any(code6, BJ_PREFIXES_2);
    if !looks_like_stock && is_fund(code6) {
        return SecurityType::MutualFund;
    }
    if looks_like_stock {
        return SecurityType::Stock;
    }
    SecurityType::Unknown
}

/// Best-effort parse. Chinese names need a network resolve via `fetch_basic`.
pub fn parse_ticker(raw: &str) -> TickerInfo {
    static A_NUMERIC: LazyLock<regex::Regex> = LazyLock::new(|| regex::Regex::new(r"^\d{6}$").unwrap());
    static A_FULL: LazyLock<regex::Regex> =
        LazyLock::new(|| regex::Regex::new(r"^(\d{6})\.(SZ|SH|BJ)$").unwrap());
    static HK: LazyLock<regex::Regex> =
        LazyLock::new(|| regex::Regex::new(r"^(\d{4,5})(?:\.HK)?$").unwrap());
    static US: LazyLock<regex::Regex> =
        LazyLock::new(|| regex::Regex::new(r"^[A-Z][A-Z.\-]{0,5}$").unwrap());
    static GENERIC: LazyLock<regex::Regex> = LazyLock::new(|| {
        regex::Regex::new(r"^[A-Z0-9][A-Z0-9.\-]{0,30}\.[A-Z0-9]{1,5}$").unwrap()
    });

    let a_numeric = &*A_NUMERIC;
    let a_full = &*A_FULL;
    let hk = &*HK;
    let us = &*US;
    let generic = &*GENERIC;

    let s: String = raw
        .trim()
        .to_uppercase()
        .chars()
        .filter(|c| *c != ' ')
        .collect();

    if let Some(rest) = s.strip_prefix("SEHK.") {
        if rest.chars().all(|c| c.is_ascii_digit()) && !rest.is_empty() {
            let code = rest.trim_start_matches('0');
            let code = if code.is_empty() { "0" } else { code };
            return TickerInfo::with_venue(
                raw,
                code,
                &format!("{:0>5}.HK", code),
                "H",
                "HKEX",
                "HKD",
                "HK",
            );
        }
    }

    if let Some(caps) = a_full.captures(&s) {
        let code = caps.get(1).unwrap().as_str();
        let suffix = caps.get(2).unwrap().as_str();
        return TickerInfo::with_venue(
            raw,
            code,
            &format!("{}.{}", code, suffix),
            "A",
            suffix,
            "CNY",
            "CN",
        );
    }

    if a_numeric.is_match(&s) {
        let suffix = a_share_suffix(&s);
        return TickerInfo::with_venue(
            raw,
            &s,
            &format!("{}.{}", s, suffix),
            "A",
            suffix,
            "CNY",
            "CN",
        );
    }

    if let Some(stripped) = s.strip_suffix(".HK") {
        if !stripped.is_empty() {
            let code = stripped.trim_start_matches('0');
            let code = if code.is_empty() { "0" } else { code };
            return TickerInfo::with_venue(
                raw,
                code,
                &format!("{:0>5}.HK", code),
                "H",
                "HKEX",
                "HKD",
                "HK",
            );
        }
    }

    for (suffix, market, exchange, currency) in GLOBAL_SUFFIXES {
        if s.len() > suffix.len() && s.ends_with(suffix) {
            let code = &s[..s.len() - suffix.len()];
            return TickerInfo::with_venue(raw, code, &s, market, exchange, currency, market);
        }
    }

    if s.chars().all(|c| c.is_ascii_digit()) && (3..=5).contains(&s.len()) {
        let padded = format!("{:0>5}", s);
        let code = s.trim_start_matches('0');
        let code = if code.is_empty() { "0" } else { code };
        return TickerInfo::with_venue(
            raw,
            code,
            &format!("{}.HK", padded),
            "H",
            "HKEX",
            "HKD",
            "HK",
        );
    }

    if hk.is_match(&s) && !us.is_match(&s) {
        let code = s.trim_start_matches('0');
        let code = if code.is_empty() { "0" } else { code };
        return TickerInfo::with_venue(
            raw,
            code,
            &format!("{:0>5}.HK", s),
            "H",
            "HKEX",
            "HKD",
            "HK",
        );
    }

    if us.is_match(&s) {
        return TickerInfo::with_venue(raw, &s, &s, "U", "US", "USD", "US");
    }

    if generic.is_match(&s) {
        if let Some(idx) = s.rfind('.') {
            let code = &s[..idx];
            let exchange = &s[idx + 1..];
            return TickerInfo::with_venue(raw, code, &s, "G", exchange, "", "");
        }
    }

    // Unrecognized — likely a Chinese name. Caller must resolve.
    TickerInfo::bare(raw, raw, raw, "A")
}

/// True if input contains CJK chars (needs name→code resolution).
pub fn is_chinese_name(raw: &str) -> bool {
    raw.chars().any(|ch| ('\u{4e00}'..='\u{9fff}').contains(&ch))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_share_forms() {
        assert_eq!(parse_ticker("002273").full, "002273.SZ");
        assert_eq!(parse_ticker("600519").full, "600519.SH");
        assert_eq!(parse_ticker("002273.sz").full, "002273.SZ");
        assert_eq!(parse_ticker("688981").full, "688981.SH");
        assert_eq!(parse_ticker("830799").full, "830799.BJ");
        assert_eq!(parse_ticker("512400").full, "512400.SH");
        assert_eq!(parse_ticker("159915").full, "159915.SZ");
        let a = parse_ticker("600519");
        assert_eq!(
            (a.market.as_str(), a.exchange.as_str(), a.currency.as_str()),
            ("A", "SH", "CNY")
        );
    }

    #[test]
    fn parses_hk_us_and_global() {
        assert_eq!(parse_ticker("00700.HK").full, "00700.HK");
        assert_eq!(parse_ticker("00700.HK").code, "700");
        assert_eq!(parse_ticker("700").full, "00700.HK");
        assert_eq!(parse_ticker("700").market, "H");
        assert_eq!(parse_ticker("AAPL").market, "U");
        assert_eq!(parse_ticker("BRK.B").full, "BRK.B");
        assert_eq!(parse_ticker("7203.T").market, "JP");
        assert_eq!(parse_ticker("7203.T").exchange, "TSE");
        assert_eq!(parse_ticker("005930.KS").market, "KR");
        assert_eq!(parse_ticker("SEHK.700").full, "00700.HK");
    }

    #[test]
    fn unresolved_chinese_name_falls_back_to_a() {
        let t = parse_ticker("水晶光电");
        assert_eq!(t.full, "水晶光电");
        assert!(is_chinese_name("水晶光电"));
        assert!(!is_chinese_name("AAPL"));
    }

    #[test]
    fn classifies_security_types() {
        assert_eq!(classify_security_type("600519"), SecurityType::Stock);
        assert_eq!(classify_security_type("300750"), SecurityType::Stock);
        assert_eq!(classify_security_type("512400"), SecurityType::Etf);
        assert_eq!(classify_security_type("501050"), SecurityType::Lof);
        assert_eq!(classify_security_type("162411"), SecurityType::Lof);
        assert_eq!(
            classify_security_type("113050"),
            SecurityType::ConvertibleBond
        );
        // fund codes come from the optional akshare-equivalent registry
        assert_eq!(
            classify_security_type_with("110011", |_| true),
            SecurityType::MutualFund
        );
        assert_eq!(
            classify_security_type_with("005827", |_| true),
            SecurityType::MutualFund
        );
        assert_eq!(classify_security_type("005827"), SecurityType::Unknown);
    }
}
