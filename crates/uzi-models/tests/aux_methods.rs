//! Differential tests for the modeling modules that sit outside the golden
//! `compute_dim_20/21/22` chain: `fin_models` extras (accretion/dilution, proxy
//! DCF), `global_peers` normalization/ranking primitives, and every `tier1`
//! builder.
//!
//! Expected trees were produced by running the upstream Python modules on the
//! same inputs (`lib/fin_models.py`, `lib/global_peers.py`, `lib/tier1/*`). Only
//! `datetime.now()`-derived strings are masked before comparison — every number,
//! key order and verdict is exact.

use serde_json::{json, Value};
use std::path::PathBuf;
use uzi_core::json::to_py_compact;
use uzi_core::testkit::{assert_json_eq, load_json};
use uzi_models::tier1::{
    build_ai_readiness, build_earnings_preview, build_model_update, build_rebalance,
    build_returns_attribution,
};
use uzi_models::{deep_methods, fin_models, global_peers};

fn expected(name: &str) -> Value {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/data")
        .join(name);
    load_json(&path)
}

/// Replace date / quarter strings so `datetime.now()` values do not make the
/// comparison time-dependent.
fn mask_dates(v: &Value) -> Value {
    match v {
        Value::String(s) => Value::String(mask_str(s)),
        Value::Array(a) => Value::Array(a.iter().map(mask_dates).collect()),
        Value::Object(o) => Value::Object(
            o.iter()
                .map(|(k, val)| (k.clone(), mask_dates(val)))
                .collect(),
        ),
        other => other.clone(),
    }
}

fn mask_str(s: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        // YYYY-MM-DD
        let date_at = i + 10 <= chars.len()
            && chars[i].is_ascii_digit()
            && chars.get(i + 1).map(|c| c.is_ascii_digit()).unwrap_or(false)
            && chars.get(i + 2).map(|c| c.is_ascii_digit()).unwrap_or(false)
            && chars.get(i + 3).map(|c| c.is_ascii_digit()).unwrap_or(false)
            && chars.get(i + 4) == Some(&'-')
            && chars.get(i + 5).map(|c| c.is_ascii_digit()).unwrap_or(false)
            && chars.get(i + 6).map(|c| c.is_ascii_digit()).unwrap_or(false)
            && chars.get(i + 7) == Some(&'-')
            && chars.get(i + 8).map(|c| c.is_ascii_digit()).unwrap_or(false)
            && chars.get(i + 9).map(|c| c.is_ascii_digit()).unwrap_or(false);
        // YYYY QN
        let quarter_at = i + 6 <= chars.len()
            && chars[i].is_ascii_digit()
            && chars.get(i + 1).map(|c| c.is_ascii_digit()).unwrap_or(false)
            && chars.get(i + 2).map(|c| c.is_ascii_digit()).unwrap_or(false)
            && chars.get(i + 3).map(|c| c.is_ascii_digit()).unwrap_or(false)
            && chars.get(i + 4) == Some(&' ')
            && chars.get(i + 5) == Some(&'Q')
            && chars.get(i + 6).map(|c| c.is_ascii_digit()).unwrap_or(false);
        if date_at {
            out.push_str("<DATE>");
            i += 10;
        } else if quarter_at {
            out.push_str("<QUARTER>");
            i += 7;
        } else {
            out.push(chars[i]);
            i += 1;
        }
    }
    out
}

fn compare(actual: &Value, name: &str) {
    let want = expected("aux_demos.json");
    let want = mask_dates(&want[name]);
    assert_json_eq(
        &mask_dates(actual),
        &want,
        &format!("aux_demos/{}", name),
    );
}

// ── fin_models extras ───────────────────────────────────────────────────────

fn demo_features() -> Value {
    json!({
        "price": 18.5, "market_cap_yi": 260.0, "shares_outstanding_yi": 14.0,
        "revenue_latest_yi": 52.0, "net_margin": 12.5, "pe": 35.0, "pb": 2.8,
        "total_debt_yi": 10.0, "cash_yi": 40.0, "fcf_latest_yi": 6.5,
        "ebitda_yi": 10.0, "equity_yi": 92.0,
    })
}

#[test]
fn fin_models_demos_match_upstream() {
    let f = demo_features();
    compare(&fin_models::compute_dcf(&f, None), "dcf");
    compare(&fin_models::quick_lbo(&f), "lbo");
    compare(&fin_models::project_three_stmt(&f, None), "stmt");

    let acquirer = json!({"name":"Acq","shares_yi":10.0,"price":20.0,"eps":1.0,"pe":20.0,"net_income_yi":10.0});
    let target = json!({"name":"Tgt","shares_yi":5.0,"price":10.0,"eps":0.5,"pe":20.0,"net_income_yi":2.5});
    compare(
        &fin_models::accretion_dilution(&acquirer, &target),
        "accretion",
    );
    compare(
        &fin_models::accretion_dilution(
            &json!({"name":"A","shares_yi":10.0,"price":0.0}),
            &json!({"name":"T","shares_yi":5.0,"price":10.0}),
        ),
        "accretion_no_eps",
    );

    // FCF proxy branch: no fcf_latest_yi → revenue × net_margin × 0.8
    let proxy = json!({"price":20.0,"revenue_latest_yi":50.0,"net_margin":10.0,
        "shares_outstanding_yi":10.0,"total_debt_yi":5.0,"cash_yi":3.0});
    compare(&fin_models::compute_dcf(&proxy, None), "dcf_proxy");
}

// ── deep_analysis_methods extras ────────────────────────────────────────────

fn deep_features() -> Value {
    json!({
        "price": 18.5, "market_cap_yi": 260, "shares_outstanding_yi": 14.0,
        "revenue_latest_yi": 52, "net_margin": 12.5, "pe": 35, "pb": 2.8,
        "total_debt_yi": 10, "cash_yi": 40, "fcf_latest_yi": 6.5,
        "ebitda_yi": 10, "equity_yi": 92, "name": "测试公司",
        "roe_last": 11.8, "roe_5y_above_15": 0, "fcf_positive": true,
        "moat_total": 27, "stage_num": 2, "rev_growth_3y": 18,
        "eps_growth_3y": 15, "debt_ratio": 30, "gross_margin": 35,
    })
}

#[test]
fn deep_analysis_method_demos_match_upstream() {
    let raw = json!({"dimensions": {}});
    let f = deep_features();
    compare(&deep_methods::build_ic_memo(&f, &raw, None, None), "ic_memo");
    compare(&deep_methods::build_unit_economics(&f, &raw), "unit_econ");
    compare(&deep_methods::build_value_creation_plan(&f, &raw), "vcp");
    compare(&deep_methods::build_dd_checklist(&f, &raw), "dd");
    let mut comp = f.clone();
    comp["market_share"] = json!(12);
    comp["industry_growth"] = json!(14);
    compare(&deep_methods::build_competitive_analysis(&comp, &raw), "competitive");

    // Recurring/SaaS branch (industry read from `0_basic.data.industry`).
    let saas_raw = json!({"dimensions": {"0_basic": {"data": {"industry": "SaaS软件"}}}});
    let mut saas = f.clone();
    saas["customer_count"] = json!(100);
    compare(
        &deep_methods::build_unit_economics(&saas, &saas_raw),
        "unit_econ_recurring",
    );

    // Single-position portfolio rebalance view used by `compute_dim_22`.
    let pos = json!([{"ticker":"X","name":"Y","market_value_yuan":10000,"asset_class":"A股成长","cost_basis":9500}]);
    compare(
        &deep_methods::build_portfolio_rebalance(pos.as_array().unwrap(), None),
        "rebalance_dm",
    );
    compare(
        &deep_methods::build_portfolio_rebalance(&[], None),
        "rebalance_dm_empty",
    );
}

// ── tier1 ───────────────────────────────────────────────────────────────────

#[test]
fn ai_readiness_matches_upstream() {
    let f = json!({
        "name":"AXT科技","code":"TEST.US","industry":"磷化铟InP衬底/光模块","market_cap_yi":80,"moat_total":25,
        "ai_chokepoint_score":88.0,"ai_chain_hit":true,"ai_chain_keywords":["inp","磷化铟","光模块","cpo"],
        "ai_irreplaceable":true,"ai_smallcap":true,"industry_growth":35,
    });
    let raw = json!({"dimensions":{
        "5_chain":{"data":{"desc":"磷化铟 InP 衬底 光模块 上游"}},
        "15_events":{"data":{"event_timeline":["大订单 缺货涨价 扩产"]}},
        "7_industry":{"data":{"growth":"35%"}},
    }});
    compare(&build_ai_readiness(&f, &raw), "ai_readiness");

    let off = json!({
        "name":"钢铁厂","code":"600019.SH","industry":"钢铁","ai_chokepoint_score":3.0,
        "ai_chain_hit":false,"ai_chain_keywords":[],"ai_irreplaceable":false,
        "ai_smallcap":false,"moat_total":10,
    });
    compare(&build_ai_readiness(&off, &json!({})), "ai_readiness_offchain");
}

#[test]
fn earnings_preview_matches_upstream() {
    let f = json!({
        "name":"中际旭创","code":"300308.SZ","industry":"光模块","market":"A","price":120.0,"eps":4.5,
        "consensus_eps_2026":6.2,"revenue_latest_yi":240.0,"revenue_growth_3y_cagr":45.0,
        "revenue_growth_latest":60.0,"gross_margin":33.0,"target_price_avg":150.0,
        "research_coverage":28,"buy_rating_pct":90,"volatility_1y":55.0,"has_positive_catalyst":true,
    });
    compare(&build_earnings_preview(&f, &json!({})), "earnings_preview");
    compare(
        &build_earnings_preview(&json!({"name":"Sparse","market":"US"}), &json!({})),
        "earnings_preview_sparse",
    );
}

#[test]
fn model_update_matches_upstream() {
    let feats = json!({
        "name":"演示科技","code":"000001.SZ","price":18.5,"revenue_growth_latest":22.0,
        "revenue_growth_3y_cagr":15.0,"gross_margin":38.0,"net_margin":14.0,"pe":35.0,
        "target_price_avg":24.0,"eps":0.62,
    });
    compare(
        &build_model_update(&feats, &json!({}), None, None, None),
        "model_update_demo",
    );

    let dcf = json!({
        "intrinsic_per_share":20.0,"current_price":18.5,"safety_margin_pct":8.1,
        "assumptions":{"stage1_growth":0.10,"terminal_g":0.025,"beta":1.0},
        "wacc_breakdown":{"wacc":0.085,"equity_weight":0.7,"inputs":{"rf":0.025,"erp":0.06,"beta":1.0}},
    });
    let comps = json!({
        "implied_price":{"via_median_pe":22.0},"target":{"eps":0.62},
        "peer_stats":{"pe":{"median":35.5}},
    });
    let updates = json!({"rev_growth":26.0,"net_margin":16.0,"capex_pct":7.0});
    compare(
        &build_model_update(&feats, &json!({}), Some(&updates), Some(&dcf), Some(&comps)),
        "model_update_explicit",
    );
    let updates2 = json!({"beta":1.4,"target_pe":40.0});
    compare(
        &build_model_update(&feats, &json!({}), Some(&updates2), None, None),
        "model_update_no_dcf",
    );
}

#[test]
fn rebalance_matches_upstream() {
    let holdings = json!([
        {"ticker":"600519.SH","weight":0.40,"market":"A","industry":"白酒","value":400000,"price":1500},
        {"ticker":"000858.SZ","weight":0.35,"market":"A","industry":"白酒","value":350000,"price":150},
        {"ticker":"00700.HK","weight":0.25,"market":"HK","industry":"互联网","value":250000,"price":400},
    ]);
    compare(&build_rebalance(&holdings, None, 5.0), "rebalance");

    let no_value = json!([
        {"ticker":"AAPL","weight":60,"industry":"Tech"},
        {"ticker":"MSFT","weight":40,"industry":"Tech"},
    ]);
    compare(&build_rebalance(&no_value, None, 5.0), "rebalance_no_value");
    compare(&build_rebalance(&json!([]), None, 5.0), "rebalance_empty");
}

#[test]
fn returns_attribution_matches_upstream() {
    let holdings = json!([
        {"ticker":"600519.SH","weight":0.30,"return_pct":12.0,"industry":"白酒","name":"贵州茅台"},
        {"ticker":"000858.SZ","weight":0.15,"return_pct":-8.0,"industry":"白酒","name":"五粮液"},
        {"ticker":"002594.SZ","weight":0.25,"return_pct":30.0,"industry":"电动车","name":"比亚迪"},
        {"ticker":"300750.SZ","weight":0.20,"return_pct":5.0,"industry":"电动车","name":"宁德时代"},
        {"ticker":"AAPL","weight":0.10,"industry":"科技","name":"Apple"},
    ]);
    compare(&build_returns_attribution(&holdings, Some(6.0)), "returns");
    compare(&build_returns_attribution(&json!([]), None), "returns_empty");
    let no_weights = json!([
        {"ticker":"A","return_pct":5.0,"school":"价值"},
        {"ticker":"B","return_pct":-2.0,"school":"成长"},
    ]);
    compare(
        &build_returns_attribution(&no_weights, None),
        "returns_no_weights",
    );
}

// ── global_peers primitives ─────────────────────────────────────────────────

fn yahoo_payload() -> Value {
    json!({"timeseries":{"result":[
        {"meta":{"type":["annualTotalRevenue"]},"annualTotalRevenue":[
            {"asOfDate":"2023-12-31","periodType":"12M","reportedValue":{"raw":1000.0},"currencyCode":"USD"},
            {"asOfDate":"2024-12-31","periodType":"12M","reportedValue":{"raw":1200.0},"currencyCode":"USD"}]},
        {"meta":{"type":["annualNetIncome"]},"annualNetIncome":[
            {"asOfDate":"2023-12-31","periodType":"12M","reportedValue":{"raw":100.0}},
            {"asOfDate":"2024-12-31","periodType":"12M","reportedValue":{"raw":150.0}}]},
        {"meta":{"type":["annualStockholdersEquity"]},"annualStockholdersEquity":[
            {"asOfDate":"2024-12-31","periodType":"12M","reportedValue":{"raw":800.0}}]},
        {"meta":{"type":["annualOperatingCashFlow"]},"annualOperatingCashFlow":[
            {"asOfDate":"2024-12-31","periodType":"12M","reportedValue":{"raw":300.0}}]},
        {"meta":{"type":["annualCapitalExpenditure"]},"annualCapitalExpenditure":[
            {"asOfDate":"2024-12-31","periodType":"12M","reportedValue":{"raw":-120.0}}]},
        {"meta":{"type":["quarterlyTotalRevenue"]},"quarterlyTotalRevenue":[
            {"asOfDate":"2024-06-30","periodType":"3M","reportedValue":{"raw":999.0}}]},
    ]}})
}

fn gp_peers() -> Value {
    json!([
        {"financials":{"periods":{"2023-12-31":{"revenue_base":100.0,"roe":10.0,"net_margin":5.0},
            "2024-12-31":{"revenue_base":120.0,"roe":12.0,"net_margin":6.0}}}},
        {"financials":{"periods":{"2023-12-31":{"revenue_base":200.0,"roe":20.0,"net_margin":10.0},
            "2024-12-31":{"revenue_base":210.0,"roe":22.0,"net_margin":11.0}}}},
        {"financials":{"periods":{"2023-12-31":{"revenue_base":300.0,"roe":30.0,"net_margin":15.0},
            "2024-12-31":{"revenue_base":330.0,"roe":33.0,"net_margin":16.0}}}},
    ])
}

#[test]
fn global_peer_primitives_match_upstream() {
    let want = expected("aux_global_peers.json");

    let normalized = global_peers::normalize_yahoo_timeseries("P.US", &yahoo_payload());
    assert_json_eq(&normalized, &want["normalize"], "normalize_yahoo_timeseries");
    assert_json_eq(
        &global_peers::normalize_yahoo_timeseries("X.US", &json!({})),
        &want["normalize_empty"],
        "normalize_yahoo_timeseries/empty",
    );
    assert_json_eq(
        &global_peers::apply_yearly_fx(&normalized, &json!({"2023":7.1,"2024":7.2}), "CNY"),
        &want["apply_fx"],
        "apply_yearly_fx",
    );

    let keys: Vec<Value> = ["Apple Inc.", "NVIDIA Corporation", "", "贵州茅台股份有限公司", "ACME Holdings Ltd"]
        .iter()
        .map(|s| Value::String(global_peers::issuer_key(s)))
        .collect();
    assert_json_eq(&Value::Array(keys), &want["issuer"], "issuer_key");

    let target = json!({"symbol":"TGT.US","name":"Target","industry":"Semis","sector":"Tech",
        "currency":"USD","market_cap":1000.0});
    let cands = json!([
        {"symbol":"A.US","name":"Alpha Inc","industry":"Semis","sector":"Tech","currency":"USD","market_cap":900.0,"data_coverage":0.8,"provider_score":1.0},
        {"symbol":"A.US","name":"Alpha Inc","industry":"Semis","sector":"Tech","currency":"USD","market_cap":950.0,"data_coverage":0.9,"provider_score":0.5},
        {"symbol":"B.US","name":"TGT.US","industry":"Semis"},
        {"symbol":"TGT.US","name":"Target"},
        {"symbol":"C.US","name":"Gamma Holdings","industry":"Other","currency":"USD","market_cap":10.0,"data_coverage":0.2,"is_secondary":true},
    ]);
    assert_json_eq(
        &Value::Array(global_peers::rank_global_candidates(
            &target,
            cands.as_array().unwrap(),
            8,
        )),
        &want["rank"],
        "rank_global_candidates",
    );

    let peers = gp_peers();
    assert_json_eq(
        &global_peers::benchmarks(peers.as_array().unwrap()),
        &want["benchmarks"],
        "benchmarks",
    );
    let tgt = json!({"periods":{"2024-12-31":{"revenue_base":150.0,"roe":15.0,"net_margin":7.0}}});
    assert_json_eq(
        &global_peers::target_percentile(&tgt, peers.as_array().unwrap()),
        &want["target_pct"],
        "target_percentile",
    );

    let comparison = json!({"peers":[
        {"name":"Alpha","symbol":"A.US","pe":20.0,"pb":2.0,"ps":3.0,"financials":peers[0]["financials"]},
        {"name":"Beta","symbol":"B.US","pe":30.0,"pb":3.0,"ps":4.0,"financials":peers[1]["financials"]},
    ]});
    assert_json_eq(
        &Value::Array(global_peers::global_peers_to_comps(&comparison)),
        &want["to_comps"],
        "global_peers_to_comps",
    );
    assert_json_eq(
        &Value::Array(global_peers::global_peers_to_comps(&json!({}))),
        &want["to_comps_empty"],
        "global_peers_to_comps/empty",
    );
}

#[test]
fn global_peers_to_comps_output_is_consumable_by_build_comps_table() {
    // Round-trip: the adapter's rows must satisfy the comps schema so the peer
    // table actually produces percentiles (guards against silent schema drift).
    let peers = json!([
        {"name":"Alpha","symbol":"A.US","pe":20.0,"pb":2.0,"financials":{"periods":{"2024-12-31":{"net_margin":6.0,"revenue_base":120.0}}}},
        {"name":"Beta","symbol":"B.US","pe":30.0,"pb":3.0,"financials":{"periods":{"2024-12-31":{"net_margin":11.0,"revenue_base":210.0}}}},
        {"name":"Gamma","symbol":"C.US","pe":40.0,"pb":4.0,"financials":{"periods":{"2024-12-31":{"net_margin":16.0,"revenue_base":330.0}}}},
    ]);
    let rows = global_peers::global_peers_to_comps(&json!({"peers": peers}));
    assert_eq!(rows.len(), 3);
    let target = json!({"name": "T", "price": 10.0, "pe": 25.0, "eps": 0.5});
    let comps = fin_models::build_comps_table(&target, &rows);
    assert_eq!(comps["peer_count"], json!(3));
    assert_eq!(comps["peer_stats"]["pe"]["median"], json!(30.0));
    assert_eq!(comps["valuation_verdict"], json!("🟡 合理偏低"));
    // and the peers survive the round trip in order with their tickers
    let tickers: Vec<&str> = comps["peers"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["ticker"].as_str().unwrap())
        .collect();
    assert_eq!(tickers, vec!["A.US", "B.US", "C.US"]);
    let _ = to_py_compact(&comps);
}
