//! Per-method entry points — the `uzi <ticker> --method <NAME>` surface.
//!
//! Upstream exposes the institutional methods as CLI slash commands
//! (`commands/dcf.md` …) whose bodies call Python library functions directly.
//! There is no Python runtime here, so an agent cannot call a function: without
//! an entry point the migrated commands would describe work that cannot be done.
//! This module is that entry point.
//!
//! Two kinds of method, both printing the same shape (one JSON object to stdout):
//!
//! * **cached** — already produced by `--stage1` and sitting in
//!   `raw_data.json → dimensions.{20,21,22}.data.<key>`. Re-running them would be
//!   wasted work, so the cached block is printed verbatim.
//! * **computed** — the Tier-1 research products (`ai-readiness`,
//!   `earnings-preview`, `model-update`) plus the portfolio methods
//!   (`rebalance`, `returns`), which no pipeline stage produces. These are
//!   evaluated on demand from the cached inputs.

use anyhow::{bail, Context};
use serde_json::{json, Value};

use uzi_core::cache::read_task_output;
use uzi_models::tier1;

/// A method that can be printed for one ticker.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
    /// Taken from `dimensions["20_valuation_models"].data`.
    Cached(&'static str, &'static str),
    /// Produced by a Tier-1 builder from `(features, raw)`.
    Tier1(Tier1Method),
    /// Needs a portfolio rather than a single ticker.
    Portfolio(&'static str),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tier1Method {
    AiReadiness,
    EarningsPreview,
    ModelUpdate,
}

/// The method registry, in the order `--method` help lists them.
///
/// `--method <name>` values are the command names upstream uses, so a migrated
/// `commands/*.md` can name the flag directly.
pub const METHODS: &[(&str, Method)] = &[
    // ── dim 20 · valuation models ──
    ("dcf", Method::Cached("20_valuation_models", "dcf")),
    ("comps", Method::Cached("20_valuation_models", "comps")),
    ("lbo", Method::Cached("20_valuation_models", "lbo")),
    ("three-statement", Method::Cached("20_valuation_models", "three_statement")),
    // ── dim 21 · research workflow ──
    ("initiate", Method::Cached("21_research_workflow", "initiating_coverage")),
    ("earnings", Method::Cached("21_research_workflow", "earnings_analysis")),
    ("catalysts", Method::Cached("21_research_workflow", "catalyst_calendar")),
    ("thesis", Method::Cached("21_research_workflow", "thesis_tracker")),
    ("morning-note", Method::Cached("21_research_workflow", "morning_note")),
    ("idea-screen", Method::Cached("21_research_workflow", "idea_screens")),
    ("sector-overview", Method::Cached("21_research_workflow", "sector_overview")),
    // ── dim 22 · deep decision methods ──
    ("competitive", Method::Cached("22_deep_methods", "competitive_analysis")),
    ("ic-memo", Method::Cached("22_deep_methods", "ic_memo")),
    ("unit-economics", Method::Cached("22_deep_methods", "unit_economics")),
    ("value-creation", Method::Cached("22_deep_methods", "value_creation_plan")),
    ("dd", Method::Cached("22_deep_methods", "dd_checklist")),
    // ── tier 1 · computed on demand ──
    ("ai-readiness", Method::Tier1(Tier1Method::AiReadiness)),
    ("earnings-preview", Method::Tier1(Tier1Method::EarningsPreview)),
    ("model-update", Method::Tier1(Tier1Method::ModelUpdate)),
    // ── portfolio methods ──
    ("rebalance", Method::Portfolio("rebalance")),
    ("returns", Method::Portfolio("returns")),
];

/// Look up a method by its CLI name.
pub fn lookup(name: &str) -> Option<Method> {
    METHODS
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, m)| *m)
}

/// Every method that needs a portfolio rather than a ticker.
pub fn portfolio_methods() -> Vec<&'static str> {
    METHODS
        .iter()
        .filter_map(|(n, m)| matches!(m, Method::Portfolio(_)).then_some(*n))
        .collect()
}

/// Comma-joined method names, for error messages and `--help`.
pub fn names() -> String {
    METHODS.iter().map(|(n, _)| *n).collect::<Vec<_>>().join(", ")
}

/// Which pipeline dimension each cached method lives in, for error hints.
fn dim_of(dim: &str) -> &'static str {
    match dim {
        "20_valuation_models" => "机构建模（Task 1.5）",
        "21_research_workflow" => "研究工作流（Task 1.5）",
        "22_deep_methods" => "深度决策方法（Task 1.5）",
        _ => "管线",
    }
}

/// `uzi <ticker> --method <NAME>`.
///
/// Reads the cached `raw_data.json`. Methods produced by Task 1.5 are returned
/// verbatim; Tier-1 methods are computed from the same snapshot.
pub fn run_single(ticker: &str, name: &str) -> anyhow::Result<Value> {
    let Some(method) = lookup(name) else {
        bail!("未知方法 {name:?}。可用: {}", names());
    };

    if let Method::Portfolio(_) = method {
        bail!(
            "{name} 需要组合而非单只股票，用法: uzi --portfolio <csv> --method {name}\n可用单票方法: {}",
            METHODS
                .iter()
                .filter(|(_, m)| !matches!(m, Method::Portfolio(_)))
                .map(|(n, _)| *n)
                .collect::<Vec<_>>()
                .join(", ")
        );
    }

    let ti = uzi_cli_target(ticker)?;
    let raw = read_task_output(&ti, "raw_data")
        .with_context(|| format!("缺少 .cache/{ti}/raw_data.json —— 先跑 uzi {ticker} --stage1"))?;

    match method {
        Method::Cached(dim, key) => {
            let block = raw
                .get("dimensions")
                .and_then(|d| d.get(dim))
                .and_then(|d| d.get("data"))
                .and_then(|d| d.get(key));

            match block {
                Some(v) if !v.is_null() => Ok(v.clone()),
                // The dimension exists but this method inside it did not run —
                // distinct from "the pipeline never ran", and worth saying so.
                _ => {
                    if is_crypto(&raw) {
                        bail!(
                            "{ti} 是加密资产，{key} 不适用（加密估值走 NVT 网络价值折现，见 --method ic-memo / unit-economics / competitive）"
                        );
                    }
                    bail!(
                        "{ti} 的 {dim}.{key} 不可用（{}未产出该方法）。重跑: uzi {ticker} --no-resume --stage1",
                        dim_of(dim)
                    )
                }
            }
        }
        Method::Tier1(which) => {
            if is_crypto(&raw) {
                bail!(
                    "{ti} 是加密资产，{name} 是按企业财报/研发口径构建的股票方法，不适用；用 --method ic-memo / unit-economics / competitive 查看加密估值"
                );
            }
            let dims = raw.get("dimensions").cloned().unwrap_or_else(|| json!({}));
            let features = uzi_core::features::sanitize_features(&uzi_features::extract_features(
                &raw, &dims,
            ));
            Ok(match which {
                Tier1Method::AiReadiness => tier1::build_ai_readiness(&features, &raw),
                Tier1Method::EarningsPreview => tier1::build_earnings_preview(&features, &raw),
                // No update payload / prior model is supplied on this path: the
                // command reports the model as computed from current data.
                Tier1Method::ModelUpdate => {
                    tier1::build_model_update(&features, &raw, None, None, None)
                }
            })
        }
        Method::Portfolio(_) => unreachable!("handled above"),
    }
}

/// Resolve a ticker to the cache key holding its artifacts.
fn uzi_cli_target(ticker: &str) -> anyhow::Result<String> {
    let ti = crate::stages::resolve_cached_target(ticker, &["raw_data"])?;
    Ok(ti.full)
}

/// True when a cached snapshot belongs to the crypto venue.
fn is_crypto(raw: &Value) -> bool {
    raw.get("dimensions")
        .and_then(|d| d.get("0_basic"))
        .and_then(|d| d.get("data"))
        .and_then(|d| d.get("market"))
        .and_then(|m| m.as_str())
        == Some("C")
}

/// `uzi --portfolio <csv> --method <rebalance|returns>`.
pub fn run_portfolio(csv_path: &str, name: &str) -> anyhow::Result<Value> {
    let Some(method) = lookup(name) else {
        bail!(
            "未知方法 {name:?}。组合方法: {}",
            portfolio_methods().join(", ")
        );
    };
    let Method::Portfolio(kind) = method else {
        bail!(
            "{name} 是单票方法，用法: uzi <ticker> --method {name}"
        );
    };

    let holdings = uzi_screen::portfolio::parse_csv(std::path::Path::new(csv_path))
        .with_context(|| format!("读取组合 CSV 失败: {csv_path}"))?;
    if holdings.is_empty() {
        bail!("组合 CSV 没有任何持仓: {csv_path}");
    }
    let holdings = enrich_from_cache(holdings);

    Ok(match kind {
        // 5 percentage points is upstream's default drift threshold.
        "rebalance" => tier1::build_rebalance(&holdings, None, 5.0),
        "returns" => tier1::build_returns_attribution(&holdings, None),
        _ => unreachable!("registry only lists known portfolio kinds"),
    })
}

/// Fill the per-holding fields `parse_csv` cannot carry, from cached artifacts.
///
/// `parse_csv` reads only `ticker` / `weight` / `note` (upstream's
/// `_parse_csv` does the same), but the portfolio methods need more:
/// `build_returns_attribution` treats a holding without `return_pct` as
/// "需补价格区间" and contributes 0 to the total, so driving it straight off a CSV
/// would always report a 0% portfolio return.
///
/// These fields are already computed during `--stage1`, so they are read back
/// from `.cache/<ticker>/raw_data.json`:
///
/// | field | source |
/// |---|---|
/// | `return_pct` | `2_kline.kline_stats.ytd_return` (`"-13.8%"` → `-13.8`) |
/// | `price` | `0_basic.data.price` |
/// | `industry` | `0_basic.data.industry` |
/// | `name` | `0_basic.data.name` |
///
/// `market` is not filled here — `build_rebalance` infers it from the ticker
/// suffix via `infer_market`, and the cached `0_basic.market` is frequently
/// `null`. `value` (position market value) cannot be derived: it needs the
/// portfolio's total capital, which is not in the cache.
///
/// Caller-supplied values always win, and a holding whose cache is missing is
/// left untouched — so the method still reports it as needing a price rather
/// than inventing one. No network access.
fn enrich_from_cache(holdings: Vec<Value>) -> Value {
    let enriched: Vec<Value> = holdings
        .into_iter()
        .map(|h| {
            let Some(ticker) = h.get("ticker").and_then(|v| v.as_str()) else {
                return h;
            };
            let Some(raw) = read_task_output(ticker, "raw_data") else {
                return h;
            };
            let Some(mut obj) = h.as_object().cloned() else {
                return h;
            };

            let dim_data = |dim: &str, key: &str| -> Option<Value> {
                raw.get("dimensions")?
                    .get(dim)?
                    .get("data")?
                    .get(key)
                    .filter(|v| !v.is_null())
                    .cloned()
            };

            if !obj.contains_key("return_pct") {
                if let Some(v) = dim_data("2_kline", "kline_stats")
                    .and_then(|s| s.get("ytd_return").cloned())
                {
                    let parsed = match &v {
                        Value::String(s) => uzi_core::py::parse_float(s),
                        other => other.as_f64(),
                    };
                    if let Some(pct) = parsed {
                        obj.insert("return_pct".into(), json!(pct));
                    }
                }
            }
            if !obj.contains_key("industry") {
                if let Some(v) = dim_data("0_basic", "industry") {
                    obj.insert("industry".into(), v);
                }
            }
            if !obj.contains_key("price") {
                if let Some(v) = dim_data("0_basic", "price").filter(|v| v.as_f64().is_some()) {
                    obj.insert("price".into(), v);
                }
            }
            if !obj.contains_key("name") {
                if let Some(v) = dim_data("0_basic", "name") {
                    obj.insert("name".into(), v);
                }
            }
            Value::Object(obj)
        })
        .collect();
    Value::Array(enriched)
}

/// `uzi <ticker> --segmental <discover|validate>`.
///
/// The segmental model is a two-step agent workflow: `discover` writes a
/// skeleton from cached data, the agent fills the segment model, `validate`
/// reconciles it. Exit code is the subcommand's own (non-zero = not validated).
pub fn run_segmental(ticker: &str, action: &str) -> anyhow::Result<i32> {
    match action {
        "discover" => Ok(uzi_features::segmental::cmd_discover(ticker)),
        "validate" => Ok(uzi_features::segmental::cmd_validate(ticker)),
        other => bail!("未知子命令 {other:?}。可用: discover, validate"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_registry_entry_resolves_by_its_own_name() {
        for (name, expected) in METHODS {
            assert_eq!(lookup(name), Some(*expected), "{name}");
        }
        assert_eq!(lookup("nope"), None);
    }

    #[test]
    fn cached_methods_point_at_real_dimension_keys() {
        // The dimension + key pair must match what Task 1.5 actually writes;
        // a typo here would surface only as a runtime "unavailable" error.
        let expected = [
            ("dcf", "20_valuation_models", "dcf"),
            ("comps", "20_valuation_models", "comps"),
            ("lbo", "20_valuation_models", "lbo"),
            ("ic-memo", "22_deep_methods", "ic_memo"),
            ("dd", "22_deep_methods", "dd_checklist"),
            ("initiate", "21_research_workflow", "initiating_coverage"),
            ("earnings", "21_research_workflow", "earnings_analysis"),
            ("catalysts", "21_research_workflow", "catalyst_calendar"),
            ("thesis", "21_research_workflow", "thesis_tracker"),
        ];
        for (name, dim, key) in expected {
            assert_eq!(
                lookup(name),
                Some(Method::Cached(dim, key)),
                "{name} should map to {dim}.{key}"
            );
        }
    }

    #[test]
    fn portfolio_and_single_stock_methods_are_disjoint() {
        let pf = portfolio_methods();
        assert_eq!(pf, vec!["rebalance", "returns"]);
        for name in &pf {
            assert!(matches!(lookup(name), Some(Method::Portfolio(_))));
        }
        // Everything else must not be a portfolio method.
        for (name, m) in METHODS {
            if !pf.contains(name) {
                assert!(!matches!(m, Method::Portfolio(_)), "{name}");
            }
        }
    }

    #[test]
    fn a_portfolio_method_on_a_ticker_is_rejected_with_usage() {
        let err = run_single("600519.SH", "rebalance").unwrap_err().to_string();
        assert!(err.contains("需要组合"), "{err}");
        assert!(err.contains("--portfolio"), "{err}");
    }

    #[test]
    fn a_single_stock_method_on_a_portfolio_is_rejected_with_usage() {
        let err = run_portfolio("/dev/null", "dcf").unwrap_err().to_string();
        assert!(err.contains("单票方法"), "{err}");
        assert!(err.contains("--method dcf"), "{err}");
    }

    #[test]
    fn unknown_methods_list_the_valid_names() {
        let err = run_single("600519.SH", "bogus").unwrap_err().to_string();
        assert!(err.contains("未知方法"), "{err}");
        assert!(err.contains("dcf"), "{err}");

        let err = run_portfolio("/dev/null", "bogus").unwrap_err().to_string();
        assert!(err.contains("rebalance"), "{err}");
    }

    #[test]
    fn segmental_rejects_unknown_actions() {
        let err = run_segmental("600519.SH", "frobnicate")
            .unwrap_err()
            .to_string();
        assert!(err.contains("discover"), "{err}");
        assert!(err.contains("validate"), "{err}");
    }

    #[test]
    fn names_lists_every_method_and_both_portfolio_ones() {
        let all = names();
        for (name, _) in METHODS {
            assert!(all.contains(name), "{name} missing from {all}");
        }
        assert!(all.contains("rebalance") && all.contains("returns"));
    }

    /// Caller-supplied values must survive enrichment untouched.
    #[test]
    fn enrichment_keeps_existing_fields() {
        let holdings = vec![json!({
            "ticker": "NOPE.NOT.CACHED",
            "weight": 1.0,
            "note": "n",
            "return_pct": 12.5,
            "industry": "自定义",
            "name": "自定义名"
        })];
        let out = enrich_from_cache(holdings);
        let h = &out.as_array().unwrap()[0];
        assert_eq!(h["return_pct"], json!(12.5));
        assert_eq!(h["industry"], json!("自定义"));
        assert_eq!(h["name"], json!("自定义名"));
    }

    /// An uncached ticker must be left alone rather than filled with a guess —
    /// the method should keep reporting it as needing a price.
    #[test]
    fn enrichment_leaves_uncached_holdings_untouched() {
        let holdings = vec![json!({"ticker": "0ZZZ.NOT.CACHED", "weight": 1.0, "note": ""})];
        let out = enrich_from_cache(holdings);
        let h = &out.as_array().unwrap()[0];
        assert!(h.get("return_pct").is_none(), "{h}");
        assert!(h.get("industry").is_none(), "{h}");
        assert_eq!(h["ticker"], json!("0ZZZ.NOT.CACHED"));
    }

    /// Enrichment must not create fields for a holding that has no ticker.
    #[test]
    fn enrichment_skips_holdings_without_a_ticker() {
        let holdings = vec![json!({"weight": 1.0, "note": "no ticker"})];
        let out = enrich_from_cache(holdings);
        let h = &out.as_array().unwrap()[0];
        assert_eq!(h["weight"], json!(1.0));
        assert!(h.get("return_pct").is_none());
    }
}
