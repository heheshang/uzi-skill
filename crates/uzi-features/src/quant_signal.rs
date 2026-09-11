//! Port of `lib/quant_signal.py` — quant-fund holdings signal.
//!
//! Upstream asks akshare (`fund_portfolio_hold_em`) for each fund's top-10
//! holdings. The Rust port is **network-free**: it reads the same
//! `.cache/_quant/<fund_code>/api_cache/top10_holdings*.json` payloads the
//! upstream run had already cached (via `uzi_core::cache`), and reconstructs the
//! fund universe by enumerating the cached `_quant` fund directories whose
//! top-10 contains the ticker (upstream's `_fetch_all_holding_funds` asks
//! akshare for exactly those funds, ordered by 持仓市值 desc).
//!
//! Structural rule is unchanged: **top-1 holding < 2% of NAV → quant-like**;
//! ≥ 3 quant funds holding the stock in their top-10 → `quant_factor` style.

use serde_json::{Map, Value};
use uzi_core::cache::{cache_path, cache_root, read_json};

/// `QUANT_TOP1_THRESHOLD`.
pub const QUANT_TOP1_THRESHOLD: f64 = 2.0;
/// `QUANT_FACTOR_MIN_COUNT`.
pub const QUANT_FACTOR_MIN_COUNT: usize = 3;
/// `_fetch_all_holding_funds(max_funds)`.
pub const MAX_FUNDS: usize = 80;

/// `float(v)` over the JSON values akshare could hand back.
fn float_like(v: &Value) -> Option<f64> {
    match v {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => uzi_core::py::parse_float(s),
        Value::Bool(b) => Some(if *b { 1.0 } else { 0.0 }),
        _ => None,
    }
}

/// Cache root for the quant holdings cache.
///
/// `uzi_core::cache::cache_root()` is cwd-relative like upstream's `.cache`.
/// Because this crate can be driven from any working directory (cargo test runs
/// each crate from its own directory), fall back to the workspace-root `.cache`
/// when the cwd-relative one is absent — the cache never leaves the repo.
fn quant_cache_root() -> std::path::PathBuf {
    if std::env::var("UZI_CACHE_ROOT")
        .map(|v| !v.is_empty())
        .unwrap_or(false)
    {
        return cache_root();
    }
    let root = cache_root();
    if root.exists() {
        return root;
    }
    let alt = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(".cache");
    if alt.exists() {
        alt
    } else {
        root
    }
}

/// `_fetch_top_holdings(fund_code, top_n=10)` against the on-disk cache.
///
/// Upstream caches `[]` on failure; a missing cache file behaves the same.
fn cached_top_holdings(fund_code: &str, top_n: usize) -> Vec<Value> {
    let rel = cache_path(&format!("_quant/{}", fund_code), "top10_holdings");
    let path = match rel.strip_prefix(cache_root()) {
        Ok(suffix) => quant_cache_root().join(suffix),
        Err(_) => rel,
    };
    let Some(payload) = read_json(&path) else {
        return Vec::new();
    };
    match payload.get("data") {
        Some(Value::Array(a)) => a.iter().take(top_n).cloned().collect(),
        _ => Vec::new(),
    }
}

/// `_is_quant_like(top_holdings)`.
fn is_quant_like(top_holdings: &[Value]) -> (bool, f64) {
    let Some(first) = top_holdings.first() else {
        return (false, 0.0);
    };
    let top1 = match first.get("占净值比例") {
        None => 0.0,
        Some(v) => match float_like(v) {
            Some(x) => x,
            None => return (false, 0.0),
        },
    };
    (top1 < QUANT_TOP1_THRESHOLD, top1)
}

/// `str(h.get("股票代码", "")).strip()` — the akshare holding row's stock code.
fn holding_code(h: &Value) -> String {
    h.get("股票代码")
        .map(uzi_core::py::py_str)
        .unwrap_or_default()
        .trim()
        .to_string()
}

/// Network-free stand-in for `_fetch_all_holding_funds(ticker_code, max_funds)`.
///
/// Upstream asks akshare's `fetch_holding_funds(ticker_code)` for the funds
/// **holding that stock**, ordered by `持仓市值` desc, then keeps the first
/// `max_funds`. The Rust port reconstructs that universe from the on-disk cache:
/// a `_quant/<fund_code>` directory exists only because that fund's top-10 was
/// fetched, so a fund belongs to the universe iff its cached top-10 contains the
/// ticker. Survivors are ordered by the target holding's `持仓市值` desc (code
/// asc as the deterministic tie-break) so the `max_funds` cap drops the same
/// funds upstream would.
fn cached_holding_funds(ticker_code: &str, max_funds: usize) -> Vec<Value> {
    let dir = quant_cache_root().join("_quant");
    let mut codes: Vec<String> = match std::fs::read_dir(&dir) {
        Ok(entries) => entries
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
            .filter_map(|e| e.file_name().into_string().ok())
            .collect(),
        Err(_) => Vec::new(),
    };
    codes.sort();

    let mut held: Vec<(f64, String)> = Vec::new();
    for code in codes {
        let top10 = cached_top_holdings(&code, 10);
        let Some(h) = top10.iter().find(|h| holding_code(h) == ticker_code) else {
            continue;
        };
        let market_value = h.get("持仓市值").and_then(float_like).unwrap_or(0.0);
        held.push((market_value, code));
    }
    held.sort_by(|a, b| {
        b.0.partial_cmp(&a.0)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.1.cmp(&b.1))
    });

    held.into_iter()
        .take(max_funds)
        .map(|(_, code)| {
            let mut m = Map::new();
            m.insert("fund_code".into(), Value::from(code));
            m.insert("fund_name".into(), Value::from(""));
            Value::Object(m)
        })
        .collect()
}

/// Port of `quant_signal.detect_quant_signal(stock_code, fund_managers)`.
///
/// `raw["fund_managers"]` is consulted only when it already carries `fund_code`
/// values (upstream keeps a large caller-supplied sample); otherwise the cached
/// fund universe is used, mirroring upstream's auto-fetch of a bigger sample.
pub fn detect_quant_signal(stock_code: &str, raw: &Value) -> Value {
    let code5 = stock_code.split('.').next().unwrap_or("").trim().to_string();

    let mut fund_managers: Vec<Value> = match raw.get("fund_managers") {
        Some(Value::Array(a)) if a.iter().any(|m| m.get("fund_code").is_some()) => a.clone(),
        _ => Vec::new(),
    };

    if fund_managers.len() < 20 {
        let bigger = cached_holding_funds(&code5, MAX_FUNDS);
        if !bigger.is_empty() {
            fund_managers = bigger;
        }
    }

    if fund_managers.is_empty() {
        let mut out = Map::new();
        out.insert("count".into(), Value::from(0));
        out.insert("quant_funds".into(), Value::Array(Vec::new()));
        out.insert("active_funds_total".into(), Value::from(0));
        out.insert("quant_funds_total".into(), Value::from(0));
        out.insert("is_quant_factor_style".into(), Value::Bool(false));
        return Value::Object(out);
    }

    let mut quant_holders: Vec<Value> = Vec::new();
    let mut quant_total = 0i64;

    for m in &fund_managers {
        let Some(fund_code) = m.get("fund_code").and_then(|c| c.as_str()) else {
            continue;
        };
        if fund_code.is_empty() {
            continue;
        }
        let top10 = cached_top_holdings(fund_code, 10);
        let (is_q, top1_pct) = is_quant_like(&top10);
        if !is_q {
            continue;
        }
        quant_total += 1;
        for (rank, h) in top10.iter().enumerate() {
            if holding_code(h) == code5 {
                let weight_pct = match h.get("占净值比例") {
                    None => 0.0,
                    Some(v) => float_like(v).unwrap_or(0.0),
                };
                let mut holder = Map::new();
                holder.insert(
                    "name".into(),
                    Value::from(py_nonempty_str(m.get("fund_name"))),
                );
                holder.insert("fund_code".into(), Value::from(fund_code));
                holder.insert("rank".into(), Value::from(rank as i64 + 1));
                holder.insert("weight_pct".into(), Value::from(weight_pct));
                holder.insert("top1_pct".into(), Value::from(top1_pct));
                holder.insert("manager".into(), Value::from(py_nonempty_str(m.get("name"))));
                quant_holders.push(Value::Object(holder));
                break;
            }
        }
    }

    quant_holders.sort_by(|a, b| {
        let wa = a.get("weight_pct").and_then(|v| v.as_f64()).unwrap_or(0.0);
        let wb = b.get("weight_pct").and_then(|v| v.as_f64()).unwrap_or(0.0);
        wb.partial_cmp(&wa).unwrap_or(std::cmp::Ordering::Equal)
    });

    let count = quant_holders.len();
    let mut out = Map::new();
    out.insert("count".into(), Value::from(count as i64));
    out.insert(
        "quant_funds".into(),
        Value::Array(quant_holders.into_iter().take(10).collect()),
    );
    out.insert(
        "active_funds_total".into(),
        Value::from(fund_managers.len() as i64),
    );
    out.insert("quant_funds_total".into(), Value::from(quant_total));
    out.insert(
        "is_quant_factor_style".into(),
        Value::Bool(count >= QUANT_FACTOR_MIN_COUNT),
    );
    Value::Object(out)
}

/// `str(m.get(k, "") or "")`.
fn py_nonempty_str(v: Option<&Value>) -> String {
    match v {
        None => String::new(),
        Some(x) => {
            let s = uzi_core::py::py_str(x);
            if s.is_empty() || s == "None" {
                String::new()
            } else {
                s
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quant_like_threshold_uses_top1_share() {
        assert!(is_quant_like(&[serde_json::json!({"占净值比例": 1.99})]).0);
        assert!(!is_quant_like(&[serde_json::json!({"占净值比例": 2.0})]).0);
        assert!(!is_quant_like(&[]).0);
    }

    #[test]
    fn missing_top1_share_treated_as_zero_like_akshare_default() {
        // float(h.get("占净值比例", 0)) → 0.0 → quant-like
        assert!(is_quant_like(&[serde_json::json!({"股票代码": "600000"})]).0);
    }
}
