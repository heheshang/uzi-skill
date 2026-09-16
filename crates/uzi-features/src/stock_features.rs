//! Port of `lib/stock_features.py`.
//!
//! `extract_features` produces the flat, typed feature dict that every investor
//! criterion reads. It is deliberately built as one insertion-ordered
//! [`Map`] so key order (and Python's int-vs-float JSON distinction) matches the
//! upstream dict exactly.

use serde_json::{Map, Value};
use std::sync::LazyLock;
use uzi_core::json::to_py_compact;
use uzi_core::py::{self, round, truthy};

// ───────────────────────── Python-value helpers ─────────────────────────

fn empty_map() -> &'static Map<String, Value> {
    static E: LazyLock<Map<String, Value>> = LazyLock::new(Map::new);
    &E
}

/// `stock_features._f(v, default)` — `None` when the value is missing.
///
/// The numeric parse (strip `, % + ¥ 亿`, reject the `- — None nan N/A`
/// placeholders) is `uzi_core::py::f_fin`; a `NaN` probe distinguishes a parsed
/// value from a missing one. `_f` returns `default` for `None`/bool/list/dict.
fn fin_opt(v: &Value) -> Option<f64> {
    match v {
        Value::Number(n) => n.as_f64(),
        Value::String(_) => {
            let x = py::f_fin(v, f64::NAN);
            if x.is_nan() {
                None
            } else {
                Some(x)
            }
        }
        _ => None,
    }
}

/// `_f(v)` with the upstream default `0.0`.
fn fv(v: &Value) -> f64 {
    fin_opt(v).unwrap_or(0.0)
}

/// `_f(v, default)`.
fn fvd(v: &Value, default: f64) -> f64 {
    fin_opt(v).unwrap_or(default)
}

/// A JSON float (Python `float`).
fn jf(x: f64) -> Value {
    Value::from(x)
}

/// A JSON integer (Python `int`).
fn ji(x: i64) -> Value {
    Value::from(x)
}

fn optv(x: Option<f64>) -> Value {
    match x {
        Some(v) => jf(v),
        None => Value::Null,
    }
}

/// Python `a or b` on two JSON values.
fn py_or<'a>(a: &'a Value, b: &'a Value) -> &'a Value {
    if truthy(a) {
        a
    } else {
        b
    }
}

/// `m.get(key, default)`.
fn mget_or<'a>(m: &'a Map<String, Value>, key: &str, default: &'a Value) -> &'a Value {
    m.get(key).unwrap_or(default)
}

/// `m.get(key)` with `Null` for absent keys.
fn mget<'a>(m: &'a Map<String, Value>, key: &str) -> &'a Value {
    static NULL: Value = Value::Null;
    m.get(key).unwrap_or(&NULL)
}

/// `m.get(key) or {}`.
fn mobj<'a>(m: &'a Map<String, Value>, key: &str) -> &'a Map<String, Value> {
    match m.get(key) {
        Some(Value::Object(o)) => o,
        _ => empty_map(),
    }
}

/// `m.get(key) or []`.
fn marr<'a>(m: &'a Map<String, Value>, key: &str) -> &'a [Value] {
    match m.get(key) {
        Some(Value::Array(a)) => a,
        _ => &[],
    }
}

/// `v.get(key, default)`.
fn vget_or<'a>(v: &'a Value, key: &str, default: &'a Value) -> &'a Value {
    v.get(key).unwrap_or(default)
}

/// `(dim_data.get(key) or {}).get("data") or {}`.
fn dd<'a>(raw: &'a Value, key: &str) -> &'a Map<String, Value> {
    match raw
        .get("dimensions")
        .and_then(|d| d.get(key))
        .and_then(|e| e.get("data"))
    {
        Some(Value::Object(o)) => o,
        _ => empty_map(),
    }
}

// ───────────────────── upstream numeric helpers ─────────────────────

/// `_pct_change(values, n)`.
fn pct_change(values: &[Value], n: usize) -> f64 {
    if values.len() < 2 {
        return 0.0;
    }
    let last = fv(&values[values.len() - 1]);
    let earlier = if values.len() > n {
        fv(&values[values.len() - 1 - n])
    } else {
        fv(&values[0])
    };
    if earlier == 0.0 {
        return 0.0;
    }
    (last - earlier) / earlier.abs() * 100.0
}

/// `_avg` — v3.9.4 keeps zero observations (`!= 0`).
fn avg(values: &[Value], default: f64) -> f64 {
    let vals: Vec<f64> = values.iter().map(fv).filter(|x| *x != 0.0).collect();
    if vals.is_empty() {
        default
    } else {
        vals.iter().sum::<f64>() / vals.len() as f64
    }
}

/// `_min` — v3.9.4 keeps zero observations (`!= 0`).
fn min_nonzero(values: &[Value], default: f64) -> f64 {
    let mut best: Option<f64> = None;
    for v in values {
        let x = fv(v);
        if x != 0.0 {
            best = Some(match best {
                Some(b) if b <= x => b,
                _ => x,
            });
        }
    }
    best.unwrap_or(default)
}

/// `_last(values, default)`.
fn last_val(values: &[Value], default: f64) -> f64 {
    match values.last() {
        Some(v) => fvd(v, default),
        None => default,
    }
}

/// `_market_cap_to_yi`.
fn market_cap_to_yi(v: &Value) -> f64 {
    let n = fv(v);
    if n > 1_000_000.0 {
        n / 1e8
    } else {
        n
    }
}

// ───────────────────────── extract_features ─────────────────────────

/// Port of `stock_features.extract_features`. `dims` is accepted for signature
/// parity; upstream never reads it inside the function.
pub fn extract_features(raw: &Value, dims: &Value) -> Value {
    let _ = dims;
    let mut f: Map<String, Value> = Map::new();

    let basic = dd(raw, "0_basic");
    let fin = dd(raw, "1_financials");
    let kline = dd(raw, "2_kline");
    let macro_ = dd(raw, "3_macro");
    let peers = dd(raw, "4_peers");
    let chain = dd(raw, "5_chain");
    let research = dd(raw, "6_research");
    let industry = dd(raw, "7_industry");
    let valuation = dd(raw, "10_valuation");
    let gov = dd(raw, "11_governance");
    let capital = dd(raw, "12_capital_flow");
    let policy = dd(raw, "13_policy");
    let moat = dd(raw, "14_moat");
    let events = dd(raw, "15_events");
    let lhb = dd(raw, "16_lhb");
    let sentiment = dd(raw, "17_sentiment");
    let trap = dd(raw, "18_trap");
    let contests = dd(raw, "19_contests");

    // ── BASIC / PRICE ──
    f.insert(
        "code".into(),
        py_or(mget(basic, "code"), vget_or(raw, "ticker", &Value::Null)).clone(),
    );
    f.insert("name".into(), py_or(mget(basic, "name"), &Value::from("—")).clone());
    f.insert(
        "industry".into(),
        py_or(mget(basic, "industry"), &Value::from("—")).clone(),
    );
    f.insert("price".into(), jf(fv(mget(basic, "price"))));
    f.insert("change_pct".into(), jf(fv(mget(basic, "change_pct"))));
    f.insert(
        "market_cap_yi".into(),
        jf(market_cap_to_yi(py_or(
            mget(basic, "market_cap_yi"),
            mget(basic, "market_cap"),
        ))),
    );
    f.insert(
        "circulating_cap_yi".into(),
        jf(market_cap_to_yi(py_or(
            mget(basic, "circulating_cap_yi"),
            mget(basic, "circulating_cap"),
        ))),
    );
    f.insert(
        "listed_date".into(),
        Value::from(
            py::py_str(mget_or(basic, "listed_date", &Value::from("")))
                .chars()
                .take(10)
                .collect::<String>(),
        ),
    );
    f.insert(
        "chairman".into(),
        py_or(mget(basic, "chairman"), &Value::from("—")).clone(),
    );
    f.insert(
        "actual_controller".into(),
        py_or(mget(basic, "actual_controller"), &Value::from("—")).clone(),
    );
    f.insert("staff_num".into(), jf(fv(mget(basic, "staff_num"))));

    // ── FINANCIALS ──
    let roe_hist = marr(fin, "roe_history");
    let rev_hist = marr(fin, "revenue_history");
    let np_hist = marr(fin, "net_profit_history");
    let div_years = marr(fin, "dividend_years");
    let div_amounts = marr(fin, "dividend_amounts");

    let roe_tail = &roe_hist[roe_hist.len().saturating_sub(5)..];
    f.insert("roe_latest".into(), jf(last_val(roe_hist, 0.0)));
    if roe_hist.len() >= 2 {
        f.insert("roe_5y_avg".into(), jf(avg(roe_tail, 0.0)));
        f.insert("roe_5y_min".into(), jf(min_nonzero(roe_tail, 0.0)));
    } else {
        f.insert("roe_5y_avg".into(), jf(last_val(roe_hist, 0.0)));
        f.insert("roe_5y_min".into(), jf(last_val(roe_hist, 0.0)));
    }
    f.insert(
        "roe_5y_above_15".into(),
        ji(roe_tail.iter().filter(|v| fv(v) > 15.0).count() as i64),
    );
    f.insert(
        "roe_5y_above_10".into(),
        ji(roe_tail.iter().filter(|v| fv(v) > 10.0).count() as i64),
    );
    f.insert(
        "roe_trend_up".into(),
        Value::Bool(if roe_hist.len() >= 3 {
            last_val(roe_hist, 0.0) > avg(&roe_hist[..roe_hist.len() - 1], 0.0)
        } else {
            false
        }),
    );

    let revenue_ttm = fin_opt(mget(fin, "revenue_ttm")).filter(|x| x.is_finite());
    let profit_ttm = fin_opt(mget(fin, "net_profit_ttm")).filter(|x| x.is_finite());
    f.insert(
        "revenue_latest_yi".into(),
        jf(revenue_ttm.unwrap_or_else(|| last_val(rev_hist, 0.0))),
    );
    f.insert(
        "revenue_latest_basis".into(),
        Value::from(if revenue_ttm.is_some() { "ttm" } else { "annual" }),
    );
    let explicit_rev_yoy = mget(fin, "revenue_growth_yoy");
    f.insert(
        "revenue_growth_latest".into(),
        if !explicit_rev_yoy.is_null() {
            jf(fv(explicit_rev_yoy))
        } else {
            jf(pct_change(rev_hist, 1))
        },
    );
    f.insert(
        "revenue_growth_period".into(),
        mget(fin, "revenue_growth_period").clone(),
    );
    f.insert(
        "revenue_growth_basis".into(),
        mget(fin, "revenue_growth_basis").clone(),
    );
    f.insert(
        "revenue_growth_source".into(),
        mget(fin, "revenue_growth_source").clone(),
    );
    let cagr = if rev_hist.len() >= 4 && fv(&rev_hist[rev_hist.len() - 4]) > 0.0 {
        ((last_val(rev_hist, 0.0) / fv(&rev_hist[rev_hist.len() - 4])).powf(1.0 / 3.0) - 1.0) * 100.0
    } else {
        0.0
    };
    f.insert(
        "revenue_growth_3y_cagr".into(),
        if rev_hist.len() >= 4 && fv(&rev_hist[rev_hist.len() - 4]) > 0.0 {
            jf(cagr)
        } else {
            ji(0)
        },
    );

    f.insert(
        "net_profit_latest_yi".into(),
        jf(profit_ttm.unwrap_or_else(|| last_val(np_hist, 0.0))),
    );
    f.insert(
        "net_profit_latest_basis".into(),
        Value::from(if profit_ttm.is_some() { "ttm" } else { "annual" }),
    );
    let explicit_np_yoy = mget(fin, "net_profit_growth_yoy");
    f.insert(
        "net_profit_growth_latest".into(),
        if !explicit_np_yoy.is_null() {
            jf(fv(explicit_np_yoy))
        } else {
            jf(pct_change(np_hist, 1))
        },
    );
    f.insert(
        "net_profit_growth_period".into(),
        mget(fin, "net_profit_growth_period").clone(),
    );
    f.insert(
        "net_profit_growth_basis".into(),
        mget(fin, "net_profit_growth_basis").clone(),
    );
    f.insert(
        "net_profit_growth_source".into(),
        mget(fin, "net_profit_growth_source").clone(),
    );
    let np_tail = &np_hist[np_hist.len().saturating_sub(5)..];
    f.insert(
        "net_profit_5y_positive".into(),
        ji(np_tail.iter().filter(|v| fv(v) > 0.0).count() as i64),
    );
    f.insert(
        "consecutive_profit_years".into(),
        ji(np_hist.iter().filter(|v| fv(v) > 0.0).count() as i64),
    );

    // Ratios require a matching period; zero and losses are valid observations.
    let (margin_rev, margin_profit, margin_basis) =
        if revenue_ttm.is_some() && profit_ttm.is_some() {
            (revenue_ttm, profit_ttm, "ttm")
        } else if !rev_hist.is_empty() && !np_hist.is_empty() && rev_hist.len() == np_hist.len() {
            (
                fin_opt(rev_hist.last().unwrap()),
                fin_opt(np_hist.last().unwrap()),
                "annual",
            )
        } else {
            (None, None, "unavailable")
        };
    match (margin_rev, margin_profit) {
        (Some(r), Some(p)) if r > 0.0 => {
            f.insert("net_margin".into(), jf(round(p / r * 100.0, 1)));
            f.insert("net_margin_basis".into(), Value::from(margin_basis));
        }
        _ => {
            let reported = fin_opt(mget(fin, "net_margin"));
            f.insert("net_margin".into(), optv(reported));
            f.insert(
                "net_margin_basis".into(),
                Value::from(if reported.is_some() { "reported" } else { "unavailable" }),
            );
        }
    }

    let health = mobj(fin, "financial_health");
    f.insert("current_ratio".into(), jf(fv(mget(health, "current_ratio"))));
    f.insert("debt_ratio".into(), jf(fv(mget(health, "debt_ratio"))));
    f.insert("fcf_margin".into(), jf(fv(mget(health, "fcf_margin"))));
    let ocf_raw = py_or(
        mget(fin, "ocf_to_net_income_ratio"),
        mget(health, "ocf_to_net_income_ratio"),
    );
    // `_f(..., default=0)` — a missing/unparseable value yields the int 0, not null.
    f.insert(
        "ocf_to_net_income_ratio".into(),
        match fin_opt(ocf_raw) {
            Some(x) => jf(x),
            None => ji(0),
        },
    );
    f.insert("roic".into(), jf(fv(mget(health, "roic"))));

    // v3.8.0 · DuPont
    let dupont = mobj(fin, "dupont");
    if !dupont.is_empty() {
        f.insert(
            "dupont_net_margin".into(),
            jf(fv(mget(dupont, "net_margin_pct"))),
        );
        f.insert(
            "dupont_asset_turnover".into(),
            jf(fv(mget(dupont, "asset_turnover"))),
        );
        f.insert(
            "dupont_equity_multiplier".into(),
            jf(fv(mget(dupont, "equity_multiplier"))),
        );
        f.insert(
            "dupont_roe".into(),
            jf(fv(mget(dupont, "roe_reconstructed_pct"))),
        );
        f.insert(
            "roe_quality".into(),
            Value::from(py::py_str(mget_or(dupont, "roe_quality", &Value::from("")))),
        );
    }

    // Dividend
    f.insert(
        "consecutive_dividend_years".into(),
        ji(div_years.len() as i64),
    );
    f.insert(
        "dividend_yield".into(),
        jf(fv(mget(basic, "dividend_yield_ttm"))),
    );
    let div_tail = &div_amounts[div_amounts.len().saturating_sub(5)..];
    f.insert(
        "total_dividend_5y_per_10".into(),
        if div_tail.is_empty() {
            ji(0)
        } else {
            jf(div_tail.iter().map(fv).sum::<f64>())
        },
    );

    // ── K-LINE / TECHNICAL ──
    let stage = Value::from(py::py_str(mget_or(kline, "stage", &Value::from("—"))));
    let stage_num = {
        let s = stage.as_str().unwrap_or("");
        if s.contains("Stage 2") {
            2
        } else if s.contains("Stage 1") {
            1
        } else if s.contains("Stage 3") {
            3
        } else if s.contains("Stage 4") {
            4
        } else {
            0
        }
    };
    f.insert("stage".into(), stage);
    f.insert("stage_num".into(), ji(stage_num));
    let ma_align = Value::from(py::py_str(mget_or(kline, "ma_align", &Value::from("—"))));
    f.insert("ma_align".into(), ma_align.clone());
    f.insert(
        "ma_bull_aligned".into(),
        Value::Bool(ma_align.as_str().unwrap_or("").contains("多头")),
    );
    let macd = Value::from(py::py_str(mget_or(kline, "macd", &Value::from("—"))));
    let macd_s = macd.as_str().unwrap_or("");
    f.insert("macd".into(), macd.clone());
    f.insert(
        "macd_golden_cross".into(),
        Value::Bool(macd_s.contains("金叉") && macd_s.contains("水上")),
    );
    f.insert("rsi".into(), jf(fv(mget(kline, "rsi"))));
    f.insert("rsi_overbought".into(), Value::Bool(fv(mget(kline, "rsi")) > 70.0));
    f.insert("rsi_oversold".into(), Value::Bool(fv(mget(kline, "rsi")) < 30.0));

    let ind = mobj(kline, "indicators");
    f.insert("kdj_k".into(), jf(fv(mget(ind, "kdj_k"))));
    f.insert("kdj_d".into(), jf(fv(mget(ind, "kdj_d"))));
    f.insert("kdj_j".into(), jf(fv(mget(ind, "kdj_j"))));
    let kdj_k = mget(ind, "kdj_k");
    let kdj_d = mget(ind, "kdj_d");
    f.insert(
        "kdj_golden_cross".into(),
        Value::Bool(truthy(kdj_k) && truthy(kdj_d) && fv(kdj_k) > fv(kdj_d)),
    );
    f.insert(
        "obv_trend_up".into(),
        Value::Bool(truthy(mget(ind, "obv_trend_up"))),
    );
    f.insert("williams_r".into(), jf(fv(mget(ind, "williams_r"))));
    let williams_raw = mget(ind, "williams_r");
    let williams_r = fv(williams_raw);
    let williams_present = !williams_raw.is_null();
    f.insert(
        "williams_overbought".into(),
        Value::Bool(williams_present && williams_r > -20.0),
    );
    f.insert(
        "williams_oversold".into(),
        Value::Bool(williams_present && williams_r < -80.0),
    );

    let stats = mobj(kline, "kline_stats");
    f.insert("ytd_return".into(), jf(fv(mget(stats, "ytd_return"))));
    f.insert("volatility_1y".into(), jf(fv(mget(stats, "volatility"))));
    f.insert(
        "max_drawdown_1y".into(),
        jf(fv(mget(stats, "max_drawdown"))),
    );

    let candles = marr(kline, "candles_60d");
    if !candles.is_empty() {
        let closes: Vec<f64> = candles
            .iter()
            .map(|c| fv(c.get("close").unwrap_or(&Value::Null)))
            .collect();
        let highs: Vec<f64> = candles
            .iter()
            .map(|c| fv(c.get("high").unwrap_or(&Value::Null)))
            .collect();
        let lows: Vec<f64> = candles
            .iter()
            .map(|c| fv(c.get("low").unwrap_or(&Value::Null)))
            .collect();
        if !closes.is_empty() && !highs.is_empty() && !lows.is_empty() {
            let hi = highs.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
            let lo = lows.iter().cloned().fold(f64::INFINITY, f64::min);
            f.insert(
                "pct_from_60d_high".into(),
                if hi > 0.0 {
                    jf((closes[closes.len() - 1] - hi) / hi * 100.0)
                } else {
                    ji(0)
                },
            );
            f.insert(
                "pct_from_60d_low".into(),
                if lo > 0.0 {
                    jf((closes[closes.len() - 1] - lo) / lo * 100.0)
                } else {
                    ji(0)
                },
            );
        }
    }

    f.insert("vcp_hint".into(), Value::Bool(false));

    // ── VALUATION ──
    f.insert(
        "pe".into(),
        jf(py_or_f64(fv(mget(basic, "pe_ttm")), fv(mget(valuation, "pe")))),
    );
    f.insert(
        "pb".into(),
        jf(py_or_f64(fv(mget(basic, "pb")), fv(mget(valuation, "pb")))),
    );
    let pe = fv(mget(&f, "pe"));
    let pb = fv(mget(&f, "pb"));
    f.insert("pe_x_pb".into(), jf(pe * pb));
    let q_str = py::py_str(mget_or(valuation, "pe_quantile", &Value::from("")));
    f.insert(
        "pe_quantile_5y".into(),
        ji(regex_first_int(&q_str).unwrap_or(50)),
    );
    f.insert("industry_pe".into(), jf(fv(mget(valuation, "industry_pe"))));
    let industry_pe = fv(mget(valuation, "industry_pe"));
    f.insert(
        "pe_vs_industry".into(),
        if industry_pe > 0.0 {
            jf((pe - industry_pe) / industry_pe * 100.0)
        } else {
            ji(0)
        },
    );
    f.insert("dcf_intrinsic_yi".into(), ji(0));
    let dcf_str = py::py_str(mget_or(valuation, "dcf", &Value::from("")));
    if let Some(x) = regex_first_number(&dcf_str) {
        f.insert("dcf_intrinsic_yi".into(), jf(x));
    }
    let dcf = fv(mget(&f, "dcf_intrinsic_yi"));
    let mcap_yi = fv(mget(&f, "market_cap_yi"));
    f.insert(
        "safety_margin".into(),
        if mcap_yi > 0.0 {
            jf((dcf - mcap_yi) / mcap_yi * 100.0)
        } else {
            ji(0)
        },
    );

    // ── PEERS ──
    let peer_table = marr(peers, "peer_table");
    let peer_pes: Vec<f64> = peer_table
        .iter()
        .filter(|p| {
            let is_self = p.get("is_self").unwrap_or(&Value::Null);
            !truthy(is_self) && fv(p.get("pe").unwrap_or(&Value::Null)) > 0.0
        })
        .map(|p| fv(p.get("pe").unwrap_or(&Value::Null)))
        .collect();
    f.insert("peers_count".into(), ji(peer_table.len() as i64));
    f.insert(
        "peer_avg_pe".into(),
        if peer_pes.is_empty() {
            ji(0)
        } else {
            jf(peer_pes.iter().sum::<f64>() / peer_pes.len() as f64)
        },
    );
    let peer_avg = fv(mget(&f, "peer_avg_pe"));
    f.insert(
        "vs_peer_avg_pe".into(),
        if peer_avg > 0.0 {
            jf((pe - peer_avg) / peer_avg * 100.0)
        } else {
            ji(0)
        },
    );
    f.insert("is_industry_leader".into(), Value::Bool(false));
    if !peer_table.is_empty() {
        let self_idx = peer_table
            .iter()
            .position(|p| truthy(p.get("is_self").unwrap_or(&Value::Null)));
        f.insert(
            "industry_rank".into(),
            ji(match self_idx {
                Some(i) => i as i64 + 1,
                None => 0,
            }),
        );
    }

    // ── RESEARCH (SELL-SIDE) ──
    f.insert(
        "research_coverage".into(),
        jf(py_or_f64(
            fv(mget(research, "coverage_count")),
            fv(mget(research, "report_count")),
        )),
    );
    f.insert(
        "buy_rating_pct".into(),
        jf(fv(mget(research, "buy_rating_pct"))),
    );
    f.insert(
        "target_price_avg".into(),
        jf(fv(mget(research, "target_price_avg"))),
    );
    f.insert(
        "consensus_eps_2026".into(),
        jf(fv(mget(research, "consensus_eps_2026"))),
    );
    f.insert(
        "consensus_pe_2026".into(),
        jf(fv(mget(research, "consensus_pe_2026"))),
    );
    let price = fv(mget(&f, "price"));
    let target = fv(mget(&f, "target_price_avg"));
    f.insert(
        "upside_to_target".into(),
        if price > 0.0 && target > 0.0 {
            jf((target - price) / price * 100.0)
        } else {
            ji(0)
        },
    );
    let eps_latest = fv(mget(basic, "eps"));
    let consensus_eps = fv(mget(&f, "consensus_eps_2026"));
    f.insert(
        "consensus_growth_to_2026".into(),
        if eps_latest > 0.0 && consensus_eps > 0.0 {
            jf((consensus_eps / eps_latest - 1.0) * 100.0)
        } else {
            ji(0)
        },
    );

    // ── INDUSTRY ──
    f.insert(
        "industry_growth_pct".into(),
        jf(fv(mget(industry, "growth"))),
    );
    let lifecycle = Value::from(py::py_str(mget_or(industry, "lifecycle", &Value::from("—"))));
    f.insert("industry_lifecycle".into(), lifecycle.clone());
    f.insert(
        "industry_is_growing".into(),
        Value::Bool(lifecycle.as_str().unwrap_or("").contains("成长")),
    );
    f.insert(
        "industry_in_decline".into(),
        Value::Bool(lifecycle.as_str().unwrap_or("").contains("衰退")),
    );

    // ── CAPITAL FLOW ──
    let main_flow = marr(capital, "main_fund_flow_20d");
    let mut main_5d_net = 0.0f64;
    for rec in main_flow.iter().take(5) {
        if let Value::Object(_) = rec {
            main_5d_net += fv(vget_or(rec, "主力净流入-净额", &Value::Null));
        }
    }
    let main_rounded = jf(round(main_5d_net / 1e8, 2));
    f.insert("main_fund_5d_net_yi".into(), main_rounded.clone());
    f.insert(
        "main_fund_net_positive".into(),
        Value::Bool(main_5d_net > 0.0),
    );
    f.insert("northbound_20d_yi".into(), main_rounded);
    f.insert(
        "northbound_net_positive".into(),
        Value::Bool(main_5d_net > 0.0),
    );
    f.insert(
        "margin_trend".into(),
        Value::from(py::py_str(mget_or(capital, "margin_trend", &Value::from("—")))),
    );
    let holders_trend = Value::from(py::py_str(mget_or(
        capital,
        "holders_trend",
        &Value::from("—"),
    )));
    f.insert("holders_trend".into(), holders_trend.clone());
    f.insert(
        "holders_concentrating".into(),
        Value::Bool(holders_trend.as_str().unwrap_or("").contains("降")),
    );
    f.insert(
        "unlock_pressure_12m".into(),
        ji(marr(capital, "unlock_schedule").len() as i64),
    );

    // ── GOVERNANCE ──
    let pledge = marr(gov, "pledge");
    f.insert(
        "has_pledge_issue".into(),
        Value::Bool(
            !pledge.is_empty()
                && pledge.iter().any(|p| match p {
                    Value::Object(_) => fv(vget_or(p, "质押比例", &Value::from(0))) > 30.0,
                    _ => false,
                }),
        ),
    );
    f.insert(
        "insider_net_buy".into(),
        Value::Bool(!marr(gov, "insider_trades_1y").is_empty()),
    );
    f.insert("no_violations".into(), Value::Bool(true));

    // ── MOAT ──
    let moat_scores = mobj(moat, "scores");
    let moat_known = !moat_scores.is_empty();
    f.insert("moat_known".into(), Value::Bool(moat_known));
    if moat_known {
        f.insert("moat_intangible".into(), jf(fv(mget(moat_scores, "intangible"))));
        f.insert("moat_switching".into(), jf(fv(mget(moat_scores, "switching"))));
        f.insert("moat_network".into(), jf(fv(mget(moat_scores, "network"))));
        f.insert("moat_scale".into(), jf(fv(mget(moat_scores, "scale"))));
        let total = fv(mget(&f, "moat_intangible"))
            + fv(mget(&f, "moat_switching"))
            + fv(mget(&f, "moat_network"))
            + fv(mget(&f, "moat_scale"));
        f.insert("moat_total".into(), jf(total));
        f.insert("moat_clear".into(), Value::Bool(total >= 24.0));
    } else {
        f.insert("moat_intangible".into(), Value::Null);
        f.insert("moat_switching".into(), Value::Null);
        f.insert("moat_network".into(), Value::Null);
        f.insert("moat_scale".into(), Value::Null);
        f.insert("moat_total".into(), Value::Null);
        f.insert("moat_clear".into(), Value::Null);
    }

    // ── EVENTS ──
    let timeline = marr(events, "event_timeline");
    let text = timeline
        .iter()
        .map(|v| v.as_str().unwrap_or(""))
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase();
    f.insert("recent_events_count".into(), ji(timeline.len() as i64));
    f.insert(
        "has_positive_catalyst".into(),
        Value::Bool(["预告", "增长", "大订单", "新品", "合作", "并购"]
            .iter()
            .any(|kw| text.contains(kw))),
    );
    f.insert(
        "has_negative_catalyst".into(),
        Value::Bool(["亏损", "下修", "处罚", "诉讼", "风险"]
            .iter()
            .any(|kw| text.contains(kw))),
    );

    // ── LHB ──
    f.insert("lhb_30d_count".into(), jf(fv(mget(lhb, "lhb_count_30d"))));
    let matched_youzi = match mget(lhb, "matched_youzi") {
        Value::Array(a) => Value::Array(a.clone()),
        _ => Value::Array(vec![]),
    };
    f.insert("matched_youzi".into(), matched_youzi.clone());
    f.insert("matched_youzi_count".into(), ji(match &matched_youzi {
        Value::Array(a) => a.len() as i64,
        _ => 0,
    }));
    let inst_vs = mobj(lhb, "inst_vs_youzi");
    f.insert(
        "inst_net_buy_lhb".into(),
        jf(fv(mget(inst_vs, "institutional_net"))),
    );
    f.insert("youzi_net_buy_lhb".into(), jf(fv(mget(inst_vs, "youzi_net"))));

    // ── SENTIMENT ──
    f.insert(
        "sentiment_heat".into(),
        jf(fv(mget(sentiment, "thermometer_value"))),
    );
    f.insert(
        "sentiment_positive_pct".into(),
        jf(fv(mget(sentiment, "positive_pct"))),
    );
    f.insert(
        "sentiment_label".into(),
        Value::from(py::py_str(mget_or(
            sentiment,
            "sentiment_label",
            &Value::from("中性"),
        ))),
    );

    // ── TRAP ──
    let signals_hit = fv(mget(trap, "signals_hit_count"));
    f.insert(
        "trap_signals_hit".into(),
        if signals_hit != 0.0 {
            jf(signals_hit)
        } else {
            ji(0)
        },
    );
    let trap_level = Value::from(py::py_str(mget_or(
        trap,
        "trap_level",
        &Value::from("🟢 安全"),
    )));
    f.insert("trap_level".into(), trap_level.clone());
    f.insert(
        "is_safe".into(),
        Value::Bool(trap_level.as_str().unwrap_or("").contains("安全")),
    );

    // ── CONTESTS ──
    let contest_summary = mobj(contests, "summary");
    f.insert(
        "xq_cube_count".into(),
        jf(fv(mget(contest_summary, "xueqiu_cubes_total"))),
    );
    f.insert(
        "xq_high_return_count".into(),
        jf(fv(mget(contest_summary, "high_return_cubes"))),
    );

    // ── FUND MANAGERS (抄作业) ──
    let fms = match raw.get("fund_managers") {
        Some(Value::Array(a)) => a.as_slice(),
        _ => &[],
    };
    f.insert("fund_manager_count".into(), ji(fms.len() as i64));
    if !fms.is_empty() {
        let returns: Vec<f64> = fms
            .iter()
            .map(|m| fv(m.get("return_5y").unwrap_or(&Value::Null)))
            .collect();
        f.insert(
            "fund_manager_max_5y_return".into(),
            if returns.is_empty() {
                ji(0)
            } else {
                jf(returns.iter().cloned().fold(f64::NEG_INFINITY, f64::max))
            },
        );
        f.insert(
            "has_top_fund_holder".into(),
            Value::Bool(fms.iter().any(|m| {
                fv(m.get("return_5y").unwrap_or(&Value::Null)) > 100.0
            })),
        );
    } else {
        f.insert("fund_manager_max_5y_return".into(), ji(0));
        f.insert("has_top_fund_holder".into(), Value::Bool(false));
    }

    // ── MACRO ──
    let rate_cycle = Value::from(py::py_str(mget_or(
        macro_,
        "rate_cycle",
        &Value::from("中性"),
    )));
    let rc = rate_cycle.as_str().unwrap_or("");
    f.insert("macro_rate_cycle".into(), rate_cycle.clone());
    f.insert(
        "macro_rate_easing".into(),
        Value::Bool(rc.contains("利好") || rc.contains("降息") || rc.contains("宽松")),
    );
    f.insert(
        "macro_commodity".into(),
        Value::from(py::py_str(mget_or(macro_, "commodity", &Value::from("中性")))),
    );

    // ── POLICY ──
    let policy_dir = py::py_str(mget_or(policy, "policy_dir", &Value::from("")));
    f.insert(
        "policy_supportive".into(),
        Value::Bool(policy_dir.contains("积极")),
    );
    f.insert(
        "policy_tightening".into(),
        Value::Bool(policy_dir.contains("收紧")),
    );

    // ── FIN MODELS SUPPORT ──
    let mcap = fv(mget(&f, "market_cap_yi"));
    let px = fv(mget(&f, "price"));
    let shares = if px > 0.0 { round(mcap / px, 3) } else { 0.0 };
    f.insert(
        "shares_outstanding_yi".into(),
        if px > 0.0 { jf(shares) } else { ji(0) },
    );
    let latest_ni = last_val(marr(fin, "net_profit_history"), 0.0);
    f.insert(
        "eps".into(),
        if shares > 0.0 {
            jf(round(latest_ni / shares, 3))
        } else {
            ji(0)
        },
    );
    let eq = fv(mget(&f, "equity_yi"));
    f.insert(
        "bvps".into(),
        if shares > 0.0 {
            jf(round(eq / shares, 3))
        } else {
            ji(0)
        },
    );
    let real_ocf_raw = mget(fin, "operating_cash_flow_yi");
    let real_ocf = fv(real_ocf_raw);
    let fcf_known = !real_ocf_raw.is_null();
    f.insert("fcf_known".into(), Value::Bool(fcf_known));
    f.insert(
        "fcf_latest_yi".into(),
        if fcf_known {
            jf(round(real_ocf, 2))
        } else if latest_ni > 0.0 {
            jf(round(latest_ni * 0.8, 2))
        } else {
            ji(0)
        },
    );
    f.insert("fcf_is_proxy".into(), Value::Bool(!fcf_known));
    f.insert(
        "fcf_positive".into(),
        if fcf_known {
            Value::Bool(real_ocf > 0.0)
        } else {
            Value::Null
        },
    );
    f.insert(
        "ebitda_yi".into(),
        if latest_ni > 0.0 {
            jf(round(latest_ni / 0.6, 2))
        } else {
            ji(0)
        },
    );
    // `_f(health.get(k), 0)` — missing/unparseable yields the int 0.
    let health = mobj(fin, "financial_health");
    f.insert(
        "total_debt_yi".into(),
        match fin_opt(mget(health, "total_debt")) {
            Some(x) => jf(x),
            None => ji(0),
        },
    );
    f.insert(
        "cash_yi".into(),
        match fin_opt(mget(health, "cash")) {
            Some(x) => jf(x),
            None => ji(0),
        },
    );
    f.insert(
        "equity_yi".into(),
        match fin_opt(mget(health, "equity")) {
            Some(x) => jf(x),
            None => ji(0),
        },
    );
    let gross = fv(mget(fin, "gross_margin"));
    f.insert(
        "gross_margin".into(),
        if gross != 0.0 { jf(gross) } else { ji(0) },
    );
    let rev = fv(mget(&f, "revenue_latest_yi"));
    f.insert(
        "ps".into(),
        if rev > 0.0 {
            jf(round(mcap / rev, 2))
        } else {
            ji(0)
        },
    );

    // industry_growth: parse from `industry.growth`
    let growth_raw = mget(industry, "growth");
    let industry_growth = match growth_raw {
        Value::Number(_) => fv(growth_raw),
        Value::String(s) => regex_first_pct(s).unwrap_or(0.0),
        _ => 0.0,
    };
    f.insert("industry_growth".into(), jf(industry_growth));

    // market_share: company mcap / industry mcap × 100
    let cmcap_yi = market_cap_to_yi(py_or(
        mget(basic, "market_cap_yi"),
        mget(basic, "market_cap"),
    ));
    let cninfo = mobj(industry, "cninfo_metrics");
    let imcap_yi = fv(mget(cninfo, "total_mcap_yi"));
    f.insert(
        "market_share".into(),
        if cmcap_yi > 0.0 && imcap_yi > 0.0 {
            jf(round(cmcap_yi / imcap_yi * 100.0, 2))
        } else {
            jf(0.0)
        },
    );
    // dividend yield (v3.9.4 no longer overrides the real basic value)
    let div_basic = fv(mget(basic, "dividend_yield_ttm"));
    f.insert(
        "dividend_yield".into(),
        if div_basic != 0.0 {
            jf(div_basic)
        } else {
            match fin_opt(mget(valuation, "dividend_yield")) {
                Some(x) => jf(x),
                None => ji(0),
            }
        },
    );
    // PEG
    let g3y = fv(mget(&f, "revenue_growth_3y_cagr"));
    f.insert(
        "peg".into(),
        if g3y > 0.0 {
            jf(round(pe / g3y, 2))
        } else {
            ji(99)
        },
    );
    f.insert("gross_margin_expanding".into(), Value::Bool(false));
    // Ticker passthrough
    let ticker = vget_or(raw, "ticker", &Value::from("")).clone();
    f.insert("ticker".into(), ticker.clone());
    let ticker_str = ticker.as_str().unwrap_or("");
    let parsed_market = uzi_core::ticker::parse_ticker(ticker_str).market;
    f.insert(
        "market".into(),
        Value::from(if parsed_market == uzi_core::ticker::CRYPTO_MARKET {
            "C"
        } else if ticker_str.ends_with(".SZ") || ticker_str.ends_with(".SH") {
            "A"
        } else if ticker_str.ends_with(".HK") {
            "HK"
        } else {
            "US"
        }),
    );
    // ── CRYPTO FEATURES ──
    // Crypto panels must never inherit stock valuation/accounting defaults.
    if parsed_market == uzi_core::ticker::CRYPTO_MARKET {
        f.insert("market_cap_rank".into(), ji(fv(mget(basic, "market_cap_rank")) as i64));
        f.insert("change_30d_pct".into(), jf(fv(mget(basic, "change_30d_pct"))));
        f.insert("nvt_ratio".into(), jf(fv(mget(valuation, "nvt_ratio"))));
        f.insert("turnover_ratio".into(), jf(fv(mget(valuation, "turnover_ratio"))));
        f.insert("mcap_to_fdv".into(), jf(fv(mget(valuation, "mcap_to_fdv"))));
        f.insert("max_drawdown_1y".into(), jf(py_or_f64(
            fv(mget(valuation, "max_drawdown_1y")),
            fv(mget(mobj(kline, "kline_stats"), "max_drawdown")),
        )));
        f.insert("volatility_1y".into(), jf(py_or_f64(
            fv(mget(valuation, "volatility_1y_pct")),
            fv(mget(mobj(kline, "kline_stats"), "volatility")),
        )));
        f.insert("fear_greed".into(), jf(fv(mget(macro_, "fear_greed"))));
        f.insert("funding_rate_pct".into(), jf(fv(mget(dd(raw, "9_futures"), "funding_rate_pct"))));
        f.insert("circulating_ratio_pct".into(), jf(fv(mget(fin, "circulating_ratio_pct"))));
        f.insert("mcap_to_tvl_ratio".into(), jf(fv(mget(valuation, "mcap_to_tvl_ratio"))));
        f.insert("market_share_pct".into(), jf(fv(mget(moat, "market_share_pct"))));
        f.insert("btc_dominance_pct".into(), jf(fv(mget(macro_, "btc_dominance_pct"))));
        f.insert("eth_dominance_pct".into(), jf(fv(mget(macro_, "eth_dominance_pct"))));
        f.insert("volume_24h".into(), jf(fv(mget(basic, "volume_24h"))));
    }

    // ── AI 卡位 / 瓶颈点 (Serenity · H 组) ──

    // ── AI 卡位 / 瓶颈点 (Serenity · H 组) ──
    let chain_txt = if chain.is_empty() {
        String::new()
    } else {
        to_py_compact(&Value::Object(chain.clone()))
    };
    let ind_txt = if !industry.is_empty() {
        to_py_compact(&Value::Object(industry.clone()))
    } else {
        to_py_compact(&Value::Object(industry.clone()))
    };
    let blob = [
        py::py_str(mget(&f, "industry")),
        py::py_str(mget(&f, "name")),
        chain_txt,
        ind_txt,
        text.clone(),
    ]
    .join(" ")
    .to_lowercase();

    let ai_hit: Vec<&'static str> = AI_CHOKEPOINT_KW
        .iter()
        .filter(|kw| blob.contains(**kw))
        .copied()
        .collect();
    let ai_chain_hit = !ai_hit.is_empty();
    f.insert("ai_chain_hit".into(), Value::Bool(ai_chain_hit));
    f.insert(
        "ai_chain_keywords".into(),
        Value::Array(
            ai_hit
                .iter()
                .take(8)
                .map(|k| Value::from(*k))
                .collect(),
        ),
    );
    let irrepl = fv(mget(&f, "moat_switching")) + fv(mget(&f, "moat_scale"));
    f.insert("ai_irreplaceable".into(), Value::Bool(irrepl >= 12.0));
    let mc = fv(mget(&f, "market_cap_yi"));
    let elasticity = if mc <= 0.0 {
        0.5
    } else if mc < 100.0 {
        1.0
    } else if mc < 300.0 {
        0.8
    } else if mc < 800.0 {
        0.5
    } else if mc < 2000.0 {
        0.25
    } else {
        0.1
    };
    f.insert(
        "ai_smallcap".into(),
        Value::Bool(mc > 0.0 && mc < 300.0),
    );
    let mut inflection = 0.0f64;
    if truthy(mget(&f, "policy_supportive")) {
        inflection += 0.4;
    }
    if truthy(mget(&f, "has_positive_catalyst")) {
        inflection += 0.3;
    }
    if fv(mget(&f, "industry_growth")) >= 20.0 {
        inflection += 0.3;
    }
    let inflection = inflection.min(1.0);

    let (tier_name, tier_weight) = AI_TIER_MAP
        .iter()
        .find(|(_, _, kws)| kws.iter().any(|k| blob.contains(*k)))
        .map(|(n, w, _)| (*n, *w))
        .unwrap_or(("未分层", 0.55));
    f.insert("ai_chain_tier".into(), Value::from(tier_name));
    f.insert("ai_chain_tier_weight".into(), jf(tier_weight));

    let mut ev = 0i64;
    if truthy(mget(&f, "net_margin")) {
        ev += 1;
    }
    if truthy(mget(&f, "has_positive_catalyst")) {
        ev += 1;
    }
    if HARD_EVIDENCE_KW.iter().any(|k| blob.contains(*k)) {
        ev += 1;
    }
    let (ev_grade, ev_mult) = if ev >= 2 {
        ("strong", 1.0)
    } else if ev == 1 {
        ("medium", 0.85)
    } else {
        ("weak", 0.70)
    };
    f.insert("ai_evidence_grade".into(), Value::from(ev_grade));

    // 8 penalty factors
    let mut pen: Map<String, Value> = Map::new();
    let heat = fv(mget(&f, "sentiment_heat"));
    if ai_chain_hit && heat >= 70.0 && ev == 0 {
        pen.insert("hype_no_orders".into(), jf(0.30));
    }
    if mc > 0.0 && mc < 30.0 {
        pen.insert("liquidity".into(), jf(0.20));
    } else if mc >= 30.0 && mc < 50.0 {
        pen.insert("liquidity".into(), jf(0.10));
    }
    if !truthy(mget(&f, "is_safe")) {
        pen.insert("accounting_trap".into(), jf(0.25));
    }
    if truthy(mget(&f, "has_pledge_issue")) {
        pen.insert("governance".into(), jf(0.15));
    }
    if ["钢铁", "煤炭", "有色冶炼", "化工原料", "航运", "水泥", "养殖", "周期"]
        .iter()
        .any(|k| blob.contains(*k))
    {
        pen.insert("cyclicality".into(), jf(0.15));
    }
    if ["技术路线之争", "被替代", "替代风险", "路线分歧", "新技术冲击", "颠覆性替代"]
        .iter()
        .any(|k| blob.contains(*k))
    {
        pen.insert("alt_design".into(), jf(0.15));
    }
    if ["出口管制", "制裁", "实体清单", "断供"]
        .iter()
        .any(|k| blob.contains(*k))
        && !["国产替代", "自主可控", "进口替代"]
            .iter()
            .any(|k| blob.contains(*k))
    {
        pen.insert("geopolitics".into(), jf(0.15));
    }
    if ["定增", "增发", "可转债", "再融资", "配股", "解禁", "股权激励摊薄"]
        .iter()
        .any(|k| blob.contains(*k))
    {
        pen.insert("dilution".into(), jf(0.15));
    }
    let penalty_total_is_zero = pen.is_empty();
    let penalty_total = if penalty_total_is_zero {
        0.0
    } else {
        pen.values().map(fv).sum::<f64>().min(0.60)
    };
    f.insert("ai_penalties".into(), Value::Object(pen));
    f.insert(
        "ai_penalty_total".into(),
        if penalty_total_is_zero {
            ji(0)
        } else {
            jf(round(penalty_total, 2))
        },
    );

    let score = if ai_chain_hit {
        let kw_strength = (ai_hit.len().min(3) as f64) / 3.0;
        let irr_norm = (irrepl / 16.0).min(1.0);
        let mut base =
            0.35 * kw_strength + 0.30 * irr_norm + 0.20 * elasticity + 0.15 * inflection;
        base *= 0.70 + 0.30 * tier_weight;
        base *= ev_mult;
        base *= 1.0 - penalty_total;
        base * 100.0
    } else {
        8.0 * elasticity
    };
    f.insert("ai_chokepoint_score".into(), jf(round(score, 1)));

    // ── 兼容别名 · v3.9.4 ──
    f.insert("pe_ttm".into(), mget(&f, "pe").clone());
    f.insert(
        "rev_growth_3y".into(),
        mget(&f, "revenue_growth_3y_cagr").clone(),
    );
    f.insert(
        "rev_growth_yoy".into(),
        mget(&f, "revenue_growth_latest").clone(),
    );
    f.insert("roe".into(), mget(&f, "roe_latest").clone());
    f.insert(
        "net_profit_growth_3y".into(),
        mget(&f, "net_profit_growth_latest").clone(),
    );

    // ── 数据不足标记 · v3.9.4 ──
    for k in NO_DATA_KEYS {
        f.insert((*k).into(), Value::Null);
    }

    Value::Object(f)
}

/// Python `a or b` over two `_f` results.
fn py_or_f64(a: f64, b: f64) -> f64 {
    if a != 0.0 {
        a
    } else {
        b
    }
}

/// First `\d+` run in `s` (Python `re.search(r"(\d+)", s)`).
fn regex_first_int(s: &str) -> Option<i64> {
    static RE: LazyLock<regex::Regex> = LazyLock::new(|| regex::Regex::new(r"\d+").unwrap());
    RE.find(s).and_then(|m| m.as_str().parse::<i64>().ok())
}

/// First `[\d.]+` run in `s` (Python `re.search(r"([\d\.]+)", s)`).
fn regex_first_number(s: &str) -> Option<f64> {
    static RE: LazyLock<regex::Regex> = LazyLock::new(|| regex::Regex::new(r"[\d.]+").unwrap());
    RE.find(s).and_then(|m| m.as_str().parse::<f64>().ok())
}

/// First `([+\-]?\d{1,3}(?:\.\d+)?)\s*%` capture in `s`, as a float.
fn regex_first_pct(s: &str) -> Option<f64> {
    static RE: LazyLock<regex::Regex> =
        LazyLock::new(|| regex::Regex::new(r"([+\-]?\d{1,3}(?:\.\d+)?)\s*%").unwrap());
    RE.captures(s)
        .and_then(|c| c.get(1))
        .and_then(|m| m.as_str().parse::<f64>().ok())
}

// ── AI chokepoint keyword tables (verbatim from stock_features.py) ──

const AI_CHOKEPOINT_KW: &[&str] = &[
    "光模块", "光芯片", "cpo", "光引擎", "硅光", "光通信", "光器件", "激光器", "eml", "vcsel",
    "hbm", "cowos", "先进封装", "封装基板", "abf", "载板", "inp", "磷化铟", "砷化镓", "化合物半导体",
    "衬底", "外延", "晶体生长", "pcb", "高速铜", "铜连接", "铜缆", "背板连接器", "连接器", "液冷",
    "散热", "电源", "bbu", "服务器电源", "pdu", "交换机", "算力", "ai 芯片", "asic", "gpu",
    "risc-v", "存储", "ddr", "ai server", "ai 服务器", "数据中心", "data center", "光纤",
    "空芯光纤", "光学", "光电子", "光学元件", "光学薄膜", "光波导", "衍射光波导", "waveguide",
    "光栅", "滤光片", "镀膜", "棱镜", "微棱镜", "镜头", "摄像模组", "相机模组", "光学镜片",
    "增强现实", "虚拟现实", "混合现实", "头显", "近眼显示", "ar/vr", "ar 眼镜", "ar眼镜",
    "micro-led", "microled", "硅基oled", "车载光学", "衍射光学", "晶圆级光学", "人形机器人",
    "具身智能", "humanoid", "人形", "机器人", "robot", "谐波减速器", "谐波减速", "谐波",
    "rv减速器", "rv 减速器", "减速器", "精密减速器", "行星滚柱丝杠", "滚柱丝杠", "行星滚柱",
    "丝杠", "滚珠丝杠", "梯形丝杠", "灵巧手", "dexterous", "机械臂", "机械手", "关节模组",
    "执行器", "actuator", "空心杯电机", "空心杯", "无框电机", "无框力矩电机", "伺服电机",
    "伺服系统", "六维力", "力传感器", "力矩传感器", "触觉传感器", "电子皮肤", "扭矩传感器",
];

type Tier = (&'static str, f64, &'static [&'static str]);

const AI_TIER_MAP: &[Tier] = &[
    (
        "材料耗材",
        1.00,
        &[
            "inp", "磷化铟", "砷化镓", "化合物半导体", "衬底", "外延", "晶体生长", "高纯", "abf",
            "载板", "封装基板", "空芯光纤", "靶材", "电子特气", "光刻胶",
        ],
    ),
    ("制程/封装", 0.92, &["cowos", "先进封装", "硅光", "mbe", "键合"]),
    (
        "设备/测试",
        0.85,
        &["光刻", "刻蚀", "量测", "坩埚", "分选机", "测试机"],
    ),
    (
        "芯片/器件",
        0.78,
        &[
            "光芯片", "eml", "vcsel", "dfb", "激光器", "hbm", "ddr", "asic", "gpu", "risc-v",
            "六维力", "力传感器", "触觉传感器", "谐波减速器", "rv减速器", "精密减速器",
            "行星滚柱丝杠", "滚柱丝杠", "空心杯电机", "无框电机",
        ],
    ),
    (
        "基础设施",
        0.70,
        &["数据中心", "data center", "idc", "算力", "电网", "核电", "变压器"],
    ),
    (
        "模块/子系统",
        0.62,
        &[
            "光模块", "光引擎", "连接器", "电源", "bbu", "液冷", "散热", "灵巧手", "关节模组",
            "执行器", "减速器", "丝杠",
        ],
    ),
    (
        "系统集成",
        0.50,
        &["交换机", "ai server", "ai 服务器", "服务器", "机械臂", "整机"],
    ),
    (
        "下游需求",
        0.40,
        &["人形机器人", "humanoid", "机器人", "ar 眼镜", "ar眼镜", "头显", "近眼显示"],
    ),
];

const HARD_EVIDENCE_KW: &[&str] = &[
    "认证", "定点", "量产", "订单", "中标", "专利", "长协", "通过验证", "合格供应商", "独供",
    "送样", "小批量", "批量交付",
];

const NO_DATA_KEYS: &[&str] = &[
    "founder_active",
    "founder_ownership_pct",
    "ev_to_revenue",
    "governance_score",
    "insider_selling_recent",
    "retail_holding_pct",
    "ceo_promotional_score",
    "audit_qualified",
    "off_balance_debt_ratio",
    "rev",
    "revenue_b",
    "rd_intensity",
    "capex_growth_yoy",
    "btc_holdings_b",
    "cash_to_marketcap_ratio",
    "rev_growth_3y_pct",
];

/// Port of `stock_features.summary` (debug helper).
pub fn summary(features: &Value) -> String {
    let g = |k: &str| py::py_str(vget_or(features, k, &Value::Null));
    let mut lines = Vec::new();
    lines.push(format!("{} ({})", g("name"), g("code")));
    lines.push(format!(
        "  价格 ¥{} · 市值 {}亿 · 行业 {}",
        g("price"),
        g("market_cap_yi"),
        g("industry")
    ));
    lines.push(format!(
        "  PE {} · PB {} · PE 5Y分位 {}",
        g("pe"),
        g("pb"),
        g("pe_quantile_5y")
    ));
    lines.push(format!(
        "  ROE 最新 {}% · 5Y均 {:.1}% · 5Y>=15%: {}/5",
        g("roe_latest"),
        fv(vget_or(features, "roe_5y_avg", &Value::Null)),
        g("roe_5y_above_15")
    ));
    lines.push(format!(
        "  营收增速 {:.1}% · 净利率 {}% · 负债率 {}%",
        fv(vget_or(features, "revenue_growth_latest", &Value::Null)),
        g("net_margin"),
        g("debt_ratio")
    ));
    lines.push(format!(
        "  Stage {} · MA多头 {} · RSI {}",
        g("stage"),
        g("ma_bull_aligned"),
        g("rsi")
    ));
    lines.push(format!(
        "  研报覆盖 {} · 买入率 {}% · 目标涨幅 {:.1}%",
        g("research_coverage"),
        g("buy_rating_pct"),
        fv(vget_or(features, "upside_to_target", &Value::Null))
    ));
    lines.push(format!(
        "  护城河 {}/40 · 基金经理 {} · 杀猪盘 {}",
        g("moat_total"),
        g("fund_manager_count"),
        g("trap_level")
    ));
    lines.join("\n")
}
