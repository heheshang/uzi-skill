//! Port of `compute_deep_methods.py` — dimensions 20-22, machine-computed
//! institutional analysis (pure compute, no network).
//!
//! `raw["dimensions"]["20_valuation_models"]` etc. must receive the full
//! `{data, source, fallback}` dict that upstream `compute_dim_*` returns.

use crate::{crypto_models, deep_methods, fin_models, global_peers, research_workflow};
use crate::{dim_data, get_or, num_value, py_str_py};
use serde_json::{json, Map, Value};
use uzi_core::features::sanitize_features;

/// `_normalize_peer` — translate a heterogeneous peer record to the comps schema.
fn normalize_peer(p: &Value) -> Value {
    if !p.is_object() {
        return json!({});
    }
    let mc_raw = {
        let a = get_or(p, "market_cap", Value::Null);
        if uzi_core::py::truthy(&a) {
            a
        } else {
            let b = get_or(p, "market_cap_yi", Value::Null);
            if uzi_core::py::truthy(&b) {
                b
            } else {
                json!(0)
            }
        }
    };
    let mc = if uzi_core::py::truthy(&mc_raw) {
        let cleaned: String = py_str_py(&mc_raw)
            .replace('亿', "")
            .replace(',', "");
        match cleaned.trim().parse::<f64>() {
            Ok(v) => num_value(v),
            Err(_) => json!(0),
        }
    } else {
        json!(0)
    };
    let px = {
        let raw_px = {
            let pv = get_or(p, "price", Value::Null);
            if uzi_core::py::truthy(&pv) {
                pv
            } else {
                json!(0)
            }
        };
        float_or_int_zero(&raw_px)
    };

    let name = {
        let n = get_or(p, "name", Value::Null);
        if uzi_core::py::truthy(&n) {
            n
        } else {
            let t = get_or(p, "ticker", Value::Null);
            if uzi_core::py::truthy(&t) {
                t
            } else {
                get_or(p, "code", json!(""))
            }
        }
    };
    let ticker = {
        let t = get_or(p, "ticker", Value::Null);
        if uzi_core::py::truthy(&t) {
            t
        } else {
            get_or(p, "code", json!(""))
        }
    };
    let pe = {
        let a = get_or(p, "pe", Value::Null);
        if uzi_core::py::truthy(&a) {
            a
        } else {
            get_or(p, "pe_ttm", Value::Null)
        }
    };
    let revenue_growth = {
        let a = get_or(p, "revenue_growth", Value::Null);
        if uzi_core::py::truthy(&a) {
            a
        } else {
            get_or(p, "rev_growth", Value::Null)
        }
    };

    json!({
        "name": name,
        "ticker": ticker,
        "pe": pe,
        "pb": get_or(p, "pb", Value::Null),
        "ps": get_or(p, "ps", Value::Null),
        "ev_ebitda": get_or(p, "ev_ebitda", Value::Null),
        "ev_sales": get_or(p, "ev_sales", Value::Null),
        "roe": get_or(p, "roe", Value::Null),
        "net_margin": get_or(p, "net_margin", Value::Null),
        "revenue_growth": revenue_growth,
        "market_cap_yi": mc,
        "price": px,
    })
}

/// Python `float(x)` with a `0` fallback (returns int 0 on failure).
fn float_or_int_zero(x: &Value) -> Value {
    match x {
        Value::Number(n) => n.as_f64().map(num_value).unwrap_or_else(|| json!(0)),
        Value::String(s) => s
            .trim()
            .parse::<f64>()
            .ok()
            .map(num_value)
            .unwrap_or_else(|| json!(0)),
        Value::Bool(b) => num_value(if *b { 1.0 } else { 0.0 }),
        _ => json!(0),
    }
}

/// `compute_dim_20` — DCF + Comps + 3-stmt + LBO packaged as dim 20.
pub fn compute_dim_20(features: &Value, raw: &Value) -> Value {
    if crypto_models::is_crypto(features, raw) {
        return crypto_models::dim_20(features, raw);
    }
    let features = sanitize_features(features);
    let features = &features;
    let dcf = fin_models::compute_dcf(features, None);
    let three_stmt = fin_models::project_three_stmt(features, None);
    let lbo = fin_models::quick_lbo(features);

    // Comps needs peer data.
    let peers_dim = dim_data(raw, "4_peers");
    let peer_table = {
        let a = get_or(peers_dim, "peer_table", Value::Null);
        if uzi_core::py::truthy(&a) {
            a
        } else {
            let b = get_or(peers_dim, "peer_comparison", Value::Null);
            if uzi_core::py::truthy(&b) {
                b
            } else {
                json!([])
            }
        }
    };
    let global_comparison = {
        let g = get_or(peers_dim, "global_peer_comparison", Value::Null);
        if uzi_core::py::truthy(&g) {
            g
        } else {
            json!({})
        }
    };
    let global_peer_table = Value::Array(global_peers::global_peers_to_comps(&global_comparison));
    let similar_stocks = {
        let s = get_or(raw, "similar_stocks", Value::Null);
        if uzi_core::py::truthy(&s) {
            s
        } else {
            json!([])
        }
    };

    let target_for_comps = json!({
        "name": get_or(features, "name", json!("目标公司")),
        "ticker": get_or(features, "ticker", Value::Null),
        "pe": get_or(features, "pe", Value::Null),
        "pb": get_or(features, "pb", Value::Null),
        "ps": get_or(features, "ps", Value::Null),
        "roe": get_or(features, "roe_last", Value::Null),
        "net_margin": get_or(features, "net_margin", Value::Null),
        "revenue_growth": get_or(features, "rev_growth_3y", Value::Null),
        "market_cap_yi": get_or(features, "market_cap_yi", Value::Null),
        "price": get_or(features, "price", Value::Null),
        "eps": get_or(features, "eps", Value::Null),
        "bvps": get_or(features, "bvps", Value::Null),
    });

    let mut peer_list_for_comps: Vec<Value> = Vec::new();
    let mut seen: Vec<String> = Vec::new();
    for (source, cap) in [
        (&peer_table, 10usize),
        (&global_peer_table, 12),
        (&similar_stocks, 10),
    ] {
        if let Some(arr) = source.as_array() {
            for p in arr.iter().take(cap) {
                let np = normalize_peer(p);
                let name = py_str_py(uzi_core::py::get(&np, "name"));
                if np.is_object() && !name.is_empty() && !seen.contains(&name) {
                    peer_list_for_comps.push(np);
                    seen.push(name);
                }
            }
        }
    }

    let comps = fin_models::build_comps_table(&target_for_comps, &peer_list_for_comps);

    let comps_verdict = if comps.get("valuation_verdict").is_some() {
        comps["valuation_verdict"].clone()
    } else {
        json!("—")
    };

    json!({
        "data": {
            "dcf": dcf.clone(),
            "comps": comps.clone(),
            "three_statement": three_stmt,
            "lbo": lbo.clone(),
            "summary": {
                "dcf_intrinsic": get_or(&dcf, "intrinsic_per_share", Value::Null),
                "dcf_safety_margin_pct": get_or(&dcf, "safety_margin_pct", Value::Null),
                "dcf_verdict": get_or(&dcf, "verdict", Value::Null),
                "lbo_irr_pct": get_or(&lbo, "irr_pct", Value::Null),
                "lbo_verdict": get_or(&lbo, "verdict", Value::Null),
                "comps_verdict": comps_verdict,
            },
        },
        "source": "compute:fin_models (DCF/Comps/3-stmt/LBO)",
        "fallback": false,
    })
}

/// `compute_dim_21` — Initiating + Earnings + Catalyst + Thesis + Morning + Screen + Sector.
pub fn compute_dim_21(features: &Value, raw: &Value, d20: &Value) -> Value {
    if crypto_models::is_crypto(features, raw) {
        return crypto_models::dim_21(features, raw, d20);
    }
    let features = sanitize_features(features);
    let features = &features;
    // `d20` is upstream's `dim_20_data`: callers pass `compute_dim_20(...)["data"]`
    // (`run_real_test._run_modeling_and_scoring`). Treated verbatim, so passing the
    // whole `{data, source, fallback}` wrapper yields no DCF wiring — matching
    // upstream `(dim_20_data or {}).get("dcf")`.
    let dcf_r = d20.get("dcf");
    let comps_r = d20.get("comps");

    let initiating = research_workflow::build_initiating_coverage(features, raw, dcf_r, comps_r);
    let earnings = research_workflow::build_earnings_analysis(features, raw);
    let catalysts = research_workflow::build_catalyst_calendar(features, raw);
    let thesis = research_workflow::build_thesis_tracker(features, raw);
    let morning = research_workflow::build_morning_note(features, raw);
    let mut screens = Map::new();
    for style in ["value", "growth", "quality", "gulp"] {
        screens.insert(
            style.to_string(),
            research_workflow::run_idea_screen(features, style),
        );
    }
    let sector = research_workflow::build_sector_overview(features, raw);

    let screens_passed = screens
        .values()
        .filter(|s| uzi_core::py::truthy(uzi_core::py::get(s, "fits_screen")))
        .count();

    let next_event = {
        let arr = uzi_core::py::get(&catalysts, "next_30d");
        match arr.as_array() {
            Some(a) if !a.is_empty() => {
                get_or(a.first().unwrap(), "event", Value::Null)
            }
            _ => json!("—"),
        }
    };

    let headline = get_or(&initiating, "headline", json!({}));
    let headline = if uzi_core::py::truthy(&headline) {
        headline
    } else {
        json!({})
    };

    json!({
        "data": {
            "initiating_coverage": initiating.clone(),
            "earnings_analysis": earnings.clone(),
            "catalyst_calendar": catalysts.clone(),
            "thesis_tracker": thesis.clone(),
            "morning_note": morning,
            "idea_screens": Value::Object(screens),
            "sector_overview": sector,
            "summary": {
                "rec_rating": get_or(&headline, "rating", Value::Null),
                "target_price": get_or(&headline, "target_price", Value::Null),
                "upside_pct": get_or(&headline, "upside_pct", Value::Null),
                "thesis_intact_pct": get_or(&thesis, "thesis_intact_pct", Value::Null),
                "next_high_impact_event": next_event,
                "earnings_headline": get_or(&earnings, "headline", Value::Null),
                "screens_passed": screens_passed,
            },
        },
        "source": "compute:research_workflow (7 research products)",
        "fallback": false,
    })
}

/// `compute_dim_22` — IC Memo + Unit Econ + VCP + DD + Porter/BCG + Rebalance.
pub fn compute_dim_22(features: &Value, raw: &Value, d20: &Value, d21: &Value) -> Value {
    if crypto_models::is_crypto(features, raw) {
        return crypto_models::dim_22(features, raw, d20, d21);
    }
    // `d21` is part of the frozen signature; dim 22 does not consume it upstream.
    let _ = d21;
    let features = sanitize_features(features);
    let features = &features;
    // Treated verbatim as upstream's `dim_20_data` (see `compute_dim_21`).
    let dcf_r = d20.get("dcf");
    let comps_r = d20.get("comps");

    let ic_memo = deep_methods::build_ic_memo(features, raw, dcf_r, comps_r);
    let unit_econ = deep_methods::build_unit_economics(features, raw);
    let vcp = deep_methods::build_value_creation_plan(features, raw);
    let dd = deep_methods::build_dd_checklist(features, raw);
    let competitive = deep_methods::build_competitive_analysis(features, raw);

    let sample_positions = json!([{
        "ticker": get_or(features, "ticker", json!("—")),
        "name": get_or(features, "name", json!("—")),
        "market_value_yuan": 10000,
        "asset_class": "A股成长",
        "cost_basis": 9500,
    }]);
    let rebalance = deep_methods::build_portfolio_rebalance(
        sample_positions.as_array().unwrap(),
        None,
    );

    let ic_headline = get_or(
        &get_or(&get_or(&ic_memo, "sections", json!({})), "I_exec_summary", json!({})),
        "headline",
        Value::Null,
    );
    let bcg = get_or(&competitive, "bcg_position", json!({}));
    let bcg = if uzi_core::py::truthy(&bcg) {
        bcg
    } else {
        json!({})
    };
    let unit_verdict = if unit_econ.get("verdict").is_some() {
        unit_econ["verdict"].clone()
    } else {
        json!("—")
    };

    json!({
        "data": {
            "ic_memo": ic_memo.clone(),
            "unit_economics": unit_econ.clone(),
            "value_creation_plan": vcp.clone(),
            "dd_checklist": dd.clone(),
            "competitive_analysis": competitive.clone(),
            "portfolio_rebalance": rebalance,
            "summary": {
                "ic_recommendation": ic_headline,
                "bcg_position": get_or(&bcg, "category", Value::Null),
                "industry_attractiveness": get_or(&competitive, "industry_attractiveness_pct", Value::Null),
                "dd_completion_pct": get_or(&dd, "completion_pct", Value::Null),
                "value_creation_uplift_yi": get_or(&vcp, "total_uplift_yi", Value::Null),
                "unit_economics_verdict": unit_verdict,
            },
        },
        "source": "compute:deep_analysis_methods (6 PE/IB/WM methods)",
        "fallback": false,
    })
}
