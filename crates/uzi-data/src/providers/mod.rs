//! Port of `lib/providers/__init__.py` — the data-provider framework with
//! automatic failover.
//!
//! Upstream registers five providers: `akshare`, `efinance`, `tushare`,
//! `baostock` (all Python libraries) and `direct_http` (raw HTTP). The Rust port
//! has no Python runtime, so the library-backed providers expose the same
//! public surface but reach the *documented HTTP endpoints those libraries use*
//! (EastMoney push2 / push2his, XueQiu public) where that endpoint is reachable,
//! and report `is_available() == false` when the upstream dependency genuinely
//! cannot exist in Rust (efinance, tushare-without-token, baostock's socket
//! protocol). See each submodule's header.

use serde_json::{json, Value};
use std::fmt;

pub mod akshare;
pub mod baostock;
pub mod cli;
pub mod direct_http;
pub mod efinance;
pub mod tushare;

/// Unified error type so the fetch chain can fail over gracefully.
#[derive(Debug, Clone)]
pub struct ProviderError(pub String);

impl fmt::Display for ProviderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for ProviderError {}

impl From<String> for ProviderError {
    fn from(s: String) -> Self {
        ProviderError(s)
    }
}

/// Metadata for one registered provider.
pub struct Provider {
    pub name: &'static str,
    pub requires_key: bool,
    pub markets: &'static [&'static str],
    pub available: fn() -> bool,
}

/// Built-in registry, in upstream registration order.
pub fn registry() -> &'static [Provider] {
    &[
        Provider {
            name: akshare::NAME,
            requires_key: akshare::REQUIRES_KEY,
            markets: akshare::MARKETS,
            available: akshare::is_available,
        },
        Provider {
            name: efinance::NAME,
            requires_key: efinance::REQUIRES_KEY,
            markets: efinance::MARKETS,
            available: efinance::is_available,
        },
        Provider {
            name: tushare::NAME,
            requires_key: tushare::REQUIRES_KEY,
            markets: tushare::MARKETS,
            available: tushare::is_available,
        },
        Provider {
            name: baostock::NAME,
            requires_key: baostock::REQUIRES_KEY,
            markets: baostock::MARKETS,
            available: baostock::is_available,
        },
        Provider {
            name: direct_http::NAME,
            requires_key: direct_http::REQUIRES_KEY,
            markets: direct_http::MARKETS,
            available: direct_http::is_available,
        },
    ]
}

pub fn get(name: &str) -> Option<&'static Provider> {
    registry().iter().find(|p| p.name == name)
}

/// `list_providers(market, available_only)`.
pub fn list_providers(market: Option<&str>, available_only: bool) -> Vec<&'static Provider> {
    registry()
        .iter()
        .filter(|p| market.map(|m| p.markets.contains(&m)).unwrap_or(true))
        .filter(|p| !available_only || (p.available)())
        .collect()
}

/// `get_provider_chain(dim, market)` — `UZI_PROVIDERS_<DIM>` overrides the
/// built-in default order `akshare → efinance → tushare → baostock`.
pub fn provider_chain(dim: &str, market: &str) -> Vec<&'static Provider> {
    let default_order = ["akshare", "efinance", "tushare", "baostock"];
    let env_key = format!("UZI_PROVIDERS_{}", dim.to_uppercase());
    let order: Vec<String> = match std::env::var(&env_key) {
        Ok(v) if !v.trim().is_empty() => v
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect(),
        _ => default_order.iter().map(|s| s.to_string()).collect(),
    };
    order
        .iter()
        .filter_map(|name| get(name))
        .filter(|p| p.markets.contains(&market) && (p.available)())
        .collect()
}

/// `health_check()` — availability + diagnostics per provider.
pub fn health_check() -> Value {
    let mut out = serde_json::Map::new();
    for p in registry() {
        let avail = (p.available)();
        out.insert(
            p.name.to_string(),
            json!({
                "available": avail,
                "markets": p.markets,
                "requires_key": p.requires_key,
                "status": if avail { "ok" } else { "unavailable" },
            }),
        );
    }
    Value::Object(out)
}

/// `try_chain("fetch_kline_a", ...)` — first provider that returns a non-empty
/// result wins. Returns `(data, provider_name)`.
pub fn try_chain_kline(
    code: &str,
    period: &str,
    start: &str,
    adjust: &str,
) -> Result<(Value, String), ProviderError> {
    let chain = provider_chain("kline", "A");
    chain_kline(chain, code, period, start, adjust)
}

fn chain_kline(
    chain: Vec<&'static Provider>,
    code: &str,
    period: &str,
    start: &str,
    adjust: &str,
) -> Result<(Value, String), ProviderError> {
    if chain.is_empty() {
        return Err(ProviderError(
            "[kline/A] 无可用 provider（检查 TUSHARE_TOKEN / pip install）".to_string(),
        ));
    }
    let mut errors: Vec<String> = Vec::new();
    for p in chain {
        let res = match p.name {
            akshare::NAME => akshare::fetch_kline_a(code, period, start, adjust),
            efinance::NAME => efinance::fetch_kline(code, "A", 500),
            tushare::NAME => tushare::fetch_kline_a(code, period, start, adjust),
            baostock::NAME => baostock::fetch_kline_a(code, start),
            _ => Err(ProviderError(format!("{}: 未实现 fetch_kline_a", p.name))),
        };
        match res {
            Ok(rows) => {
                let empty = rows.as_array().map(|a| a.is_empty()).unwrap_or(true);
                if empty {
                    errors.push(format!("{}: empty kline", p.name));
                } else {
                    return Ok((rows, p.name.to_string()));
                }
            }
            Err(e) => errors.push(format!("{}: {}", p.name, e)),
        }
    }
    Err(ProviderError(format!(
        "[kline/A] 所有 provider 都失败: {}",
        errors
            .iter()
            .take(3)
            .cloned()
            .collect::<Vec<_>>()
            .join(" | ")
    )))
}

/// `try_chain("fetch_financials_a", ...)`.
pub fn try_chain_financials(code: &str, years: usize) -> Result<(Value, String), ProviderError> {
    let chain = provider_chain("financials", "A");
    if chain.is_empty() {
        return Err(ProviderError(
            "[financials/A] 无可用 provider".to_string(),
        ));
    }
    let mut errors: Vec<String> = Vec::new();
    for p in chain {
        let res = match p.name {
            akshare::NAME => akshare::fetch_financials_a(code),
            tushare::NAME => tushare::fetch_financials_a(code, years),
            baostock::NAME => baostock::fetch_financials_a(code, years),
            _ => Err(ProviderError(format!(
                "{}: 未实现 fetch_financials_a",
                p.name
            ))),
        };
        match res {
            Ok(v) => return Ok((v, p.name.to_string())),
            Err(e) => errors.push(format!("{}: {}", p.name, e)),
        }
    }
    Err(ProviderError(format!(
        "[financials/A] 所有 provider 都失败: {}",
        errors
            .iter()
            .take(3)
            .cloned()
            .collect::<Vec<_>>()
            .join(" | ")
    )))
}

/// `try_chain("fetch_basic_a", ...)`.
pub fn try_chain_basic(code: &str) -> Result<(Value, String), ProviderError> {
    let chain = provider_chain("basic", "A");
    if chain.is_empty() {
        return Err(ProviderError("[basic/A] 无可用 provider".to_string()));
    }
    let mut errors: Vec<String> = Vec::new();
    for p in chain {
        let res = match p.name {
            akshare::NAME => akshare::fetch_basic_a(code),
            efinance::NAME => efinance::fetch_basic_a(code),
            tushare::NAME => tushare::fetch_basic_a(code),
            _ => Err(ProviderError(format!("{}: 未实现 fetch_basic_a", p.name))),
        };
        match res {
            Ok(v) => return Ok((v, p.name.to_string())),
            Err(e) => errors.push(format!("{}: {}", p.name, e)),
        }
    }
    Err(ProviderError(format!(
        "[basic/A] 所有 provider 都失败: {}",
        errors
            .iter()
            .take(3)
            .cloned()
            .collect::<Vec<_>>()
            .join(" | ")
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_order_matches_upstream() {
        let names: Vec<&str> = registry().iter().map(|p| p.name).collect();
        assert_eq!(
            names,
            vec!["akshare", "efinance", "tushare", "baostock", "direct_http"]
        );
    }

    #[test]
    fn default_chain_excludes_direct_http() {
        // upstream default_order never lists direct_http
        let chain = provider_chain("kline", "A");
        assert!(chain.iter().all(|p| p.name != "direct_http"));
    }

    #[test]
    fn env_override_reorders_chain() {
        std::env::set_var("UZI_PROVIDERS_KLINE", "direct_http");
        let chain = provider_chain("kline", "A");
        assert_eq!(chain.len(), 1);
        assert_eq!(chain[0].name, "direct_http");
        std::env::remove_var("UZI_PROVIDERS_KLINE");
    }

    #[test]
    fn health_check_marks_library_providers_unavailable() {
        let h = health_check();
        assert_eq!(h["direct_http"]["available"], json!(true));
        assert_eq!(h["direct_http"]["requires_key"], json!(false));
        // efinance/baostock are Python-only
        assert_eq!(h["efinance"]["available"], json!(false));
        assert_eq!(h["baostock"]["available"], json!(false));
    }
}
