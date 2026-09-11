//! Port of `lib/pipeline/fetchers/registry.py` — the 23-fetcher adapter catalog.
//!
//! Upstream builds a `BaseFetcher` subclass per dim through `_make_adapter`,
//! wrapping the legacy `fetch_X.main(*args)` scripts. The Rust port registers an
//! equivalent [`FnFetcher`] per dim whose `raw_fn` calls the ported module in
//! [`crate::fetch`] with the same `args_fn`-derived arguments, so `collect`
//! keeps the identical wave/dependency structure and `FetcherSpec` fields.

use std::sync::LazyLock;

use serde_json::Value;

use uzi_core::cache::{TTL_DAILY, TTL_HOURLY, TTL_INTRADAY, TTL_QUARTERLY, TTL_REALTIME, TTL_STATIC};
use uzi_core::dim::FetcherSpec;

use crate::base_fetcher::FnFetcher;
use crate::fetch;

/// `_TTL_BY_DIM`.
pub fn ttl_by_dim(dim_key: &str) -> u64 {
    match dim_key {
        "0_basic" => TTL_REALTIME,
        "1_financials" => TTL_QUARTERLY,
        "2_kline" => TTL_INTRADAY,
        "3_macro" => TTL_DAILY,
        "4_peers" => TTL_QUARTERLY,
        "5_chain" => TTL_STATIC,
        "6_fund_holders" => TTL_QUARTERLY,
        "6_research" => TTL_HOURLY,
        "7_industry" => TTL_DAILY,
        "8_materials" => TTL_DAILY,
        "9_futures" => TTL_INTRADAY,
        "10_valuation" => TTL_INTRADAY,
        "11_governance" => TTL_QUARTERLY,
        "12_capital_flow" => TTL_INTRADAY,
        "13_policy" => TTL_HOURLY,
        "14_moat" => TTL_QUARTERLY,
        "15_events" => TTL_HOURLY,
        "16_lhb" => TTL_DAILY,
        "17_sentiment" => TTL_INTRADAY,
        "18_trap" => TTL_INTRADAY,
        "19_contests" => TTL_HOURLY,
        "similar_stocks" => TTL_QUARTERLY,
        _ => TTL_INTRADAY,
    }
}

/// `_MINI_RACER_LEGACY_MODULES` — mini_racer (V8) is not thread-safe upstream;
/// these legacy modules run in a serial group.
pub const MINI_RACER_LEGACY_MODULES: &[&str] = &["fetch_industry", "fetch_capital_flow", "fetch_valuation"];

/// `DEPENDENT_DIMS` — dims that need `0_basic.industry` and run in wave 3.
pub const DEPENDENT_DIMS: &[&str] = &["3_macro", "7_industry", "9_futures", "13_policy"];

struct SpecDef {
    dim_key: &'static str,
    legacy_module: &'static str,
    required: &'static [&'static str],
    optional: &'static [&'static str],
    top_level: &'static [&'static str],
    depends_on: &'static [&'static str],
    sources: &'static [&'static str],
    markets: &'static [&'static str],
    args: ArgsFn,
}

#[derive(Clone, Copy)]
enum ArgsFn {
    Ticker,
    TickerLimit4,
    Industry,
    IndustryAndMaterials,
}

/// Registry declaration order mirrors upstream (`FETCHER_REGISTRY` literal).
static DEFS: &[SpecDef] = &[
    SpecDef {
        dim_key: "0_basic",
        legacy_module: "fetch_basic",
        required: &["name", "price"],
        optional: &[
            "industry", "market_cap", "pe_ttm", "pb", "eps", "actual_controller", "listed_date",
            "full_name",
        ],
        top_level: &[],
        depends_on: &[],
        sources: &["legacy:fetch_basic"],
        markets: &["A", "H", "U"],
        args: ArgsFn::Ticker,
    },
    SpecDef {
        dim_key: "1_financials",
        legacy_module: "fetch_financials",
        required: &["roe", "net_margin"],
        optional: &[
            "gross_margin", "revenue_growth", "financial_health", "ocf", "ocf_history",
            "ocf_to_net_income_ratio",
        ],
        top_level: &[],
        depends_on: &[],
        sources: &["legacy:fetch_financials"],
        markets: &["A", "H", "U"],
        args: ArgsFn::Ticker,
    },
    SpecDef {
        dim_key: "2_kline",
        legacy_module: "fetch_kline",
        required: &[],
        optional: &[
            "kline_daily", "ma5", "ma20", "ma60", "rsi", "price_change_1m", "price_change_3m",
        ],
        top_level: &[],
        depends_on: &[],
        sources: &["legacy:fetch_kline"],
        markets: &["A", "H", "U"],
        args: ArgsFn::Ticker,
    },
    SpecDef {
        dim_key: "3_macro",
        legacy_module: "fetch_macro",
        required: &[],
        optional: &[
            "rate_cycle", "fx_trend", "geo_risk", "commodity", "growth_momentum",
        ],
        top_level: &[],
        depends_on: &["0_basic"],
        sources: &["legacy:fetch_macro"],
        markets: &["A", "H", "U"],
        args: ArgsFn::Industry,
    },
    SpecDef {
        dim_key: "4_peers",
        legacy_module: "fetch_peers",
        required: &[],
        optional: &["peer_table", "peer_comparison", "rank", "industry"],
        top_level: &[],
        depends_on: &[],
        sources: &["legacy:fetch_peers"],
        markets: &["A", "H", "U"],
        args: ArgsFn::Ticker,
    },
    SpecDef {
        dim_key: "5_chain",
        legacy_module: "fetch_chain",
        required: &[],
        optional: &[
            "main_business_breakdown", "upstream", "downstream", "client_concentration",
        ],
        top_level: &[],
        depends_on: &[],
        sources: &["legacy:fetch_chain"],
        markets: &["A", "H", "U"],
        args: ArgsFn::Ticker,
    },
    SpecDef {
        dim_key: "6_fund_holders",
        legacy_module: "fetch_fund_holders",
        required: &[],
        optional: &["total_funds_holding", "active_funds_count", "full_stats_count"],
        top_level: &["fund_managers"],
        depends_on: &[],
        sources: &["legacy:fetch_fund_holders", "akshare:fund_portfolio_hold_em"],
        markets: &["A", "H", "U"],
        args: ArgsFn::Ticker,
    },
    SpecDef {
        dim_key: "6_research",
        legacy_module: "fetch_research",
        required: &[],
        optional: &[
            "coverage", "rating_distribution", "target_price_avg", "buy_rating_pct",
        ],
        top_level: &[],
        depends_on: &[],
        sources: &["legacy:fetch_research"],
        markets: &["A", "H", "U"],
        args: ArgsFn::Ticker,
    },
    SpecDef {
        dim_key: "7_industry",
        legacy_module: "fetch_industry",
        required: &[],
        optional: &[
            "industry", "growth", "tam", "penetration", "industry_pe", "industry_pb",
            "cninfo_metrics",
        ],
        top_level: &[],
        depends_on: &["0_basic"],
        sources: &["legacy:fetch_industry"],
        markets: &["A", "H", "U"],
        args: ArgsFn::Industry,
    },
    SpecDef {
        dim_key: "8_materials",
        legacy_module: "fetch_materials",
        required: &[],
        optional: &[
            "core_material", "price_trend", "price_history_12m", "materials_detail", "cost_share",
        ],
        top_level: &[],
        depends_on: &[],
        sources: &["legacy:fetch_materials"],
        markets: &["A", "H", "U"],
        args: ArgsFn::Ticker,
    },
    SpecDef {
        dim_key: "9_futures",
        legacy_module: "fetch_futures",
        required: &[],
        optional: &["linked_contract", "price_trend", "inventory"],
        top_level: &[],
        depends_on: &["0_basic", "8_materials"],
        sources: &["legacy:fetch_futures"],
        markets: &["A", "H", "U"],
        args: ArgsFn::IndustryAndMaterials,
    },
    SpecDef {
        dim_key: "10_valuation",
        legacy_module: "fetch_valuation",
        required: &[],
        optional: &["pe", "pb", "pe_quantile", "pb_quantile", "industry_pe", "dcf"],
        top_level: &[],
        depends_on: &[],
        sources: &["legacy:fetch_valuation"],
        markets: &["A", "H", "U"],
        args: ArgsFn::Ticker,
    },
    SpecDef {
        dim_key: "11_governance",
        legacy_module: "fetch_governance",
        required: &[],
        optional: &["pledge", "insider_trades_1y", "chairman_turnover"],
        top_level: &[],
        depends_on: &[],
        sources: &["legacy:fetch_governance"],
        markets: &["A", "H", "U"],
        args: ArgsFn::Ticker,
    },
    SpecDef {
        dim_key: "12_capital_flow",
        legacy_module: "fetch_capital_flow",
        required: &[],
        optional: &[
            "northbound", "margin_recent", "holder_count_history", "main_fund_flow_20d",
            "institutional_history",
        ],
        top_level: &[],
        depends_on: &[],
        sources: &["legacy:fetch_capital_flow"],
        markets: &["A", "H", "U"],
        args: ArgsFn::Ticker,
    },
    SpecDef {
        dim_key: "13_policy",
        legacy_module: "fetch_policy",
        required: &[],
        optional: &["policy_dir", "subsidy", "monitoring", "anti_trust", "snippets"],
        top_level: &[],
        depends_on: &["0_basic"],
        sources: &["legacy:fetch_policy"],
        markets: &["A", "H", "U"],
        args: ArgsFn::Industry,
    },
    SpecDef {
        dim_key: "14_moat",
        legacy_module: "fetch_moat",
        required: &[],
        optional: &[
            "intangible", "switching", "network", "scale", "rd_summary", "scores",
            "web_search_snippets",
        ],
        top_level: &[],
        depends_on: &[],
        sources: &["legacy:fetch_moat"],
        markets: &["A", "H", "U"],
        args: ArgsFn::Ticker,
    },
    SpecDef {
        dim_key: "15_events",
        legacy_module: "fetch_events",
        required: &[],
        optional: &[
            "event_timeline", "recent_news", "catalyst", "warnings", "disclosures_count",
        ],
        top_level: &[],
        depends_on: &[],
        sources: &["legacy:fetch_events"],
        markets: &["A", "H", "U"],
        args: ArgsFn::Ticker,
    },
    SpecDef {
        dim_key: "16_lhb",
        legacy_module: "fetch_lhb",
        required: &[],
        optional: &["lhb_count_30d", "lhb_records", "matched_youzi", "inst_vs_youzi"],
        top_level: &[],
        depends_on: &[],
        sources: &["legacy:fetch_lhb"],
        markets: &["A", "H", "U"],
        args: ArgsFn::Ticker,
    },
    SpecDef {
        dim_key: "17_sentiment",
        legacy_module: "fetch_sentiment",
        required: &[],
        optional: &[
            "xueqiu_heat", "thermometer_value", "positive_pct", "sentiment_label",
            "platform_snippets", "hot_trend_mentions",
        ],
        top_level: &[],
        depends_on: &[],
        sources: &["legacy:fetch_sentiment"],
        markets: &["A", "H", "U"],
        args: ArgsFn::Ticker,
    },
    SpecDef {
        dim_key: "18_trap",
        legacy_module: "fetch_trap_signals",
        required: &[],
        optional: &["risk_score", "pump_dump_signals", "warning_flags", "trap_likelihood"],
        top_level: &[],
        depends_on: &[],
        sources: &["legacy:fetch_trap_signals"],
        markets: &["A", "H", "U"],
        args: ArgsFn::Ticker,
    },
    SpecDef {
        dim_key: "19_contests",
        legacy_module: "fetch_contests",
        required: &[],
        optional: &["xueqiu_cubes", "tgb_mentions", "ths_simu", "dpswang", "summary"],
        top_level: &[],
        depends_on: &[],
        sources: &["legacy:fetch_contests"],
        markets: &["A", "H", "U"],
        args: ArgsFn::Ticker,
    },
    SpecDef {
        dim_key: "similar_stocks",
        legacy_module: "fetch_similar_stocks",
        required: &[],
        optional: &["similar_stocks"],
        top_level: &["similar_stocks"],
        depends_on: &[],
        sources: &["legacy:fetch_similar_stocks"],
        markets: &["A"],
        args: ArgsFn::TickerLimit4,
    },
];

/// `FETCHER_REGISTRY` order (declaration order, `0_basic` first).
pub fn dim_keys() -> Vec<&'static str> {
    DEFS.iter().map(|d| d.dim_key).collect()
}

/// `list_fetchers()` — sorted by key string.
pub fn list_fetchers() -> Vec<&'static str> {
    let mut keys = dim_keys();
    keys.sort_unstable();
    keys
}

fn def_for(dim_key: &str) -> Option<&'static SpecDef> {
    DEFS.iter().find(|d| d.dim_key == dim_key)
}

fn raw_fn_for(legacy_module: &'static str, args: ArgsFn) -> fn(&Value, &Value) -> Result<Value, String> {
    match (legacy_module, args) {
        ("fetch_basic", _) => |t, _r| fetch::basic::main(t.as_str().unwrap_or("")),
        ("fetch_financials", _) => |t, _r| fetch::financials::main(t.as_str().unwrap_or("")),
        ("fetch_kline", _) => |t, _r| fetch::kline::main(t.as_str().unwrap_or("")),
        ("fetch_macro", _) => |_t, r| fetch::macro_::main(&industry_of(r)),
        ("fetch_peers", _) => |t, _r| fetch::peers::main(t.as_str().unwrap_or("")),
        ("fetch_chain", _) => |t, _r| fetch::chain::main(t.as_str().unwrap_or("")),
        ("fetch_fund_holders", _) => |t, _r| fetch::fund_holders::main(t.as_str().unwrap_or("")),
        ("fetch_research", _) => |t, _r| fetch::research::main(t.as_str().unwrap_or("")),
        ("fetch_industry", _) => |_t, r| fetch::industry::main(&industry_of(r)),
        ("fetch_materials", _) => |t, _r| fetch::materials::main(t.as_str().unwrap_or("")),
        ("fetch_futures", _) => |_t, r| fetch::futures::main(&industry_of(r), &materials_of(r)),
        ("fetch_valuation", _) => |t, _r| fetch::valuation::main(t.as_str().unwrap_or("")),
        ("fetch_governance", _) => |t, _r| fetch::governance::main(t.as_str().unwrap_or("")),
        ("fetch_capital_flow", _) => |t, _r| fetch::capital_flow::main(t.as_str().unwrap_or("")),
        ("fetch_policy", _) => |_t, r| fetch::policy::main(&industry_of(r)),
        ("fetch_moat", _) => |t, _r| fetch::moat::main(t.as_str().unwrap_or("")),
        ("fetch_events", _) => |t, _r| fetch::events::main(t.as_str().unwrap_or("")),
        ("fetch_lhb", _) => |t, _r| fetch::lhb::main(t.as_str().unwrap_or("")),
        ("fetch_sentiment", _) => |t, _r| fetch::sentiment::main(t.as_str().unwrap_or("")),
        ("fetch_trap_signals", _) => |t, _r| fetch::trap_signals::main(t.as_str().unwrap_or("")),
        ("fetch_contests", _) => |t, _r| fetch::contests::main(t.as_str().unwrap_or("")),
        ("fetch_similar_stocks", _) => |t, _r| fetch::similar_stocks::main(t.as_str().unwrap_or("")),
        _ => |_t, _r| Err(format!("unregistered legacy module")),
    }
}

/// `(raw.get(dim_key) or {}).get("data") or {}` — the upstream `_get` helper.
pub fn dim_data<'a>(raw: &'a Value, dim_key: &str) -> Option<&'a Value> {
    raw.get(dim_key)
        .and_then(|d| d.get("data"))
        .filter(|d| d.is_object())
}

/// `r.get("0_basic", {}).get("data", {}).get("industry", "") or "综合"`.
pub fn industry_of(raw: &Value) -> String {
    let v = dim_data(raw, "0_basic")
        .and_then(|d| d.get("industry").cloned())
        .unwrap_or(Value::Null);
    let s = v.as_str().unwrap_or("");
    if s.is_empty() {
        "综合".to_string()
    } else {
        s.to_string()
    }
}

/// `r.get("8_materials", {}).get("data", {}).get("materials_detail") or None`.
pub fn materials_of(raw: &Value) -> Value {
    let v = dim_data(raw, "8_materials")
        .and_then(|d| d.get("materials_detail").cloned())
        .unwrap_or(Value::Null);
    if v.is_null() {
        Value::Null
    } else {
        v
    }
}

/// `_make_adapter` — build the [`Fetcher`] for a dim.
pub fn make_adapter(dim_key: &str) -> Option<FnFetcher> {
    let def = def_for(dim_key)?;
    let spec = FetcherSpec {
        dim_key: def.dim_key.to_string(),
        required_fields: def.required.iter().map(|s| s.to_string()).collect(),
        optional_fields: def.optional.iter().map(|s| s.to_string()).collect(),
        top_level_fields: def.top_level.iter().map(|s| s.to_string()).collect(),
        sources: def.sources.iter().map(|s| s.to_string()).collect(),
        markets: def.markets.iter().map(|s| s.to_string()).collect(),
        cache_ttl_sec: ttl_by_dim(def.dim_key),
        depends_on: def.depends_on.iter().map(|s| s.to_string()).collect(),
    };
    Some(FnFetcher {
        spec,
        legacy_module: def.legacy_module.to_string(),
        keep_zero: &[],
        raw_fn: raw_fn_for(def.legacy_module, def.args),
    })
}

/// `get_fetcher(dim_key)`.
pub fn get_fetcher(dim_key: &str) -> Option<FnFetcher> {
    make_adapter(dim_key)
}

/// True when the dim's legacy module belongs to the mini-racer serial group.
pub fn is_mini_racer(dim_key: &str) -> bool {
    def_for(dim_key)
        .map(|d| MINI_RACER_LEGACY_MODULES.contains(&d.legacy_module))
        .unwrap_or(false)
}

/// Test hook: resolve the spec of a registered dim.
pub fn spec_of(dim_key: &str) -> Option<&'static FetcherSpec> {
    static SPECS: LazyLock<Vec<(&'static str, FetcherSpec)>> = LazyLock::new(|| {
        DEFS.iter()
            .map(|d| {
                (
                    d.dim_key,
                    FetcherSpec {
                        dim_key: d.dim_key.to_string(),
                        required_fields: d.required.iter().map(|s| s.to_string()).collect(),
                        optional_fields: d.optional.iter().map(|s| s.to_string()).collect(),
                        top_level_fields: d.top_level.iter().map(|s| s.to_string()).collect(),
                        sources: d.sources.iter().map(|s| s.to_string()).collect(),
                        markets: d.markets.iter().map(|s| s.to_string()).collect(),
                        cache_ttl_sec: ttl_by_dim(d.dim_key),
                        depends_on: d.depends_on.iter().map(|s| s.to_string()).collect(),
                    },
                )
            })
            .collect()
    });
    SPECS.iter().find(|(k, _)| *k == dim_key).map(|(_, s)| s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_has_22_dims_including_similar_stocks() {
        let keys = dim_keys();
        assert_eq!(keys.len(), 22);
        assert_eq!(keys[0], "0_basic");
        assert!(keys.contains(&"similar_stocks"));
    }

    #[test]
    fn fund_holders_and_similar_stocks_declare_top_level_fields() {
        assert_eq!(
            spec_of("6_fund_holders").unwrap().top_level_fields,
            vec!["fund_managers".to_string()]
        );
        assert_eq!(
            spec_of("similar_stocks").unwrap().top_level_fields,
            vec!["similar_stocks".to_string()]
        );
    }

    #[test]
    fn dependency_dims_match_collect_constant() {
        for dim in DEPENDENT_DIMS {
            let spec = spec_of(dim).unwrap();
            assert!(spec.depends_on.contains(&"0_basic".to_string()), "{dim}");
        }
        assert_eq!(spec_of("9_futures").unwrap().depends_on, vec!["0_basic", "8_materials"]);
    }

    #[test]
    fn ttl_table_matches_upstream() {
        assert_eq!(ttl_by_dim("0_basic"), TTL_REALTIME);
        assert_eq!(ttl_by_dim("1_financials"), TTL_QUARTERLY);
        assert_eq!(ttl_by_dim("2_kline"), TTL_INTRADAY);
        assert_eq!(ttl_by_dim("16_lhb"), TTL_DAILY);
        assert_eq!(ttl_by_dim("unknown_dim"), TTL_INTRADAY);
    }

    #[test]
    fn mini_racer_group_matches_upstream_modules() {
        assert!(is_mini_racer("7_industry"));
        assert!(is_mini_racer("12_capital_flow"));
        assert!(is_mini_racer("10_valuation"));
        assert!(!is_mini_racer("0_basic"));
    }
}
