//! Port of `lib/global_peers.py` — the pure computational half: Yahoo
//! fundamentals normalization, FX application, issuer de-duplication,
//! candidate ranking, benchmark statistics and the `build_comps_table` adapter.
//!
//! The network providers (`YahooGlobalPeerProvider`, `YahooFxProvider`,
//! `fetch_global_peer_comparison`) are intentionally **not** ported here: this
//! crate has no network access (contract). They belong to `uzi-data`, which
//! feeds `build_global_peer_comparison`-shaped JSON into [`global_peers_to_comps`]
//! and [`benchmarks`].

use crate::{stats, pnum};
use regex::Regex;
use serde_json::{json, Map, Value};
use std::sync::LazyLock;

/// `_YAHOO_FACTS` — insertion order is significant (first matching key wins).
const YAHOO_FACTS: &[(&str, &str)] = &[
    ("annualTotalRevenue", "revenue"),
    ("annualGrossProfit", "gross_profit"),
    ("annualOperatingIncome", "operating_income"),
    ("annualNetIncome", "net_income"),
    ("annualStockholdersEquity", "equity"),
    ("annualOperatingCashFlow", "operating_cash_flow"),
    ("annualCapitalExpenditure", "capital_expenditure"),
];

static LEGAL_SUFFIXES: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\b(incorporated|inc|corporation|corp|company|co|limited|ltd|plc|group|holdings?)\b")
        .unwrap()
});

/// `_COMPARABLE_METRICS`.
const COMPARABLE_METRICS: &[&str] = &[
    "revenue_base",
    "net_income_base",
    "gross_margin",
    "operating_margin",
    "net_margin",
    "roe",
    "free_cash_flow_base",
];

/// `to_yahoo_symbol` — map canonical symbols to Yahoo's exchange suffix
/// conventions. `info` is the JSON form of `TickerInfo`
/// (`{market, code, full}`).
pub fn to_yahoo_symbol(info: &Value) -> String {
    let market = info.get("market");
    let code = info.get("code").and_then(|v| v.as_str()).unwrap_or("");
    let full = info.get("full").and_then(|v| v.as_str()).unwrap_or("");
    match market {
        None | Some(Value::Null) => {
            if !full.is_empty() {
                full.to_uppercase()
            } else {
                code.to_uppercase()
            }
        }
        Some(Value::String(m)) if m == "A" => {
            let suffix = full.rsplit('.').next().unwrap_or("").to_uppercase();
            let yahoo_suffix = if suffix == "SH" { "SS" } else { &suffix };
            format!("{}.{}", code, yahoo_suffix)
        }
        Some(Value::String(m)) if m == "H" => {
            let digits: String = code.chars().filter(|c| c.is_ascii_digit()).collect();
            format!("{:0>4}.HK", digits)
        }
        Some(_) => full.to_uppercase(),
    }
}

/// `_number` — float or `None` for bool / null / non-finite / unparsable.
fn number(value: &Value) -> Option<f64> {
    if value.is_null() || value.is_boolean() {
        return None;
    }
    let x = match value {
        Value::Number(n) => n.as_f64()?,
        Value::String(s) => s.trim().parse::<f64>().ok()?,
        _ => return None,
    };
    if x.is_finite() {
        Some(x)
    } else {
        None
    }
}

/// `_ratio`.
fn ratio(numerator: &Value, denominator: &Value) -> Option<f64> {
    let num = number(numerator)?;
    let den = number(denominator)?;
    if den == 0.0 {
        return None;
    }
    Some(uzi_core::py::round(num / den * 100.0, 2))
}

/// `normalize_yahoo_timeseries` — translate Yahoo annual fundamentals into an
/// auditable canonical series.
pub fn normalize_yahoo_timeseries(symbol: &str, payload: &Value) -> Value {
    let mut periods: Map<String, Value> = Map::new();
    let mut currency: Option<Value> = None;
    let result = payload
        .get("timeseries")
        .and_then(|t| t.get("result"))
        .and_then(|r| r.as_array())
        .cloned()
        .unwrap_or_default();

    for series in result.iter() {
        let Some(s) = series.as_object() else { continue };
        let meta_types = s
            .get("meta")
            .and_then(|m| m.get("type"))
            .and_then(|t| t.as_array())
            .cloned()
            .unwrap_or_default();
        let mut yahoo_key: Option<&str> = None;
        for t in meta_types.iter() {
            if let Some(ts) = t.as_str() {
                if let Some((k, _)) = YAHOO_FACTS.iter().find(|(k, _)| *k == ts) {
                    yahoo_key = Some(k);
                    break;
                }
            }
        }
        if yahoo_key.is_none() {
            yahoo_key = YAHOO_FACTS
                .iter()
                .find(|(k, _)| s.contains_key(*k))
                .map(|(k, _)| *k);
        }
        let Some(yahoo_key) = yahoo_key else { continue };
        let canonical = YAHOO_FACTS
            .iter()
            .find(|(k, _)| *k == yahoo_key)
            .map(|(_, v)| *v)
            .unwrap();
        let points = s.get(yahoo_key).and_then(|p| p.as_array()).cloned().unwrap_or_default();
        for point in points.iter() {
            let Some(p) = point.as_object() else { continue };
            let period_type = p.get("periodType");
            let period_ok = match period_type {
                None | Some(Value::Null) => true,
                Some(Value::String(s)) => s == "12M",
                _ => false,
            };
            if !period_ok {
                continue;
            }
            let period: String = p
                .get("asOfDate")
                .map(crate::py_str_py)
                .unwrap_or_default()
                .chars()
                .take(10)
                .collect();
            let value = number(
                &p.get("reportedValue")
                    .and_then(|r| r.get("raw"))
                    .cloned()
                    .unwrap_or(Value::Null),
            );
            let Some(value) = value else { continue };
            if period.is_empty() {
                continue;
            }
            if currency.is_none() {
                currency = p.get("currencyCode").cloned().filter(|v| {
                    !matches!(v, Value::Null) && uzi_core::py::truthy(v)
                });
            }
            let entry = periods
                .entry(period)
                .or_insert_with(|| Value::Object(Map::new()));
            if let Value::Object(m) = entry {
                m.insert(canonical.to_string(), crate::num_value(value));
            }
        }
    }

    // Derived ratios per period (insertion order preserved).
    for facts in periods.values_mut() {
        if let Value::Object(m) = facts {
            let gm = ratio(
                m.get("gross_profit").unwrap_or(&Value::Null),
                m.get("revenue").unwrap_or(&Value::Null),
            );
            m.insert("gross_margin".into(), opt_num(gm));
            let om = ratio(
                m.get("operating_income").unwrap_or(&Value::Null),
                m.get("revenue").unwrap_or(&Value::Null),
            );
            m.insert("operating_margin".into(), opt_num(om));
            let nm = ratio(
                m.get("net_income").unwrap_or(&Value::Null),
                m.get("revenue").unwrap_or(&Value::Null),
            );
            m.insert("net_margin".into(), opt_num(nm));
            let roe = ratio(
                m.get("net_income").unwrap_or(&Value::Null),
                m.get("equity").unwrap_or(&Value::Null),
            );
            m.insert("roe".into(), opt_num(roe));
            let ocf = number(m.get("operating_cash_flow").unwrap_or(&Value::Null));
            let capex = number(m.get("capital_expenditure").unwrap_or(&Value::Null));
            if let (Some(ocf), Some(capex)) = (ocf, capex) {
                let fcf = if capex < 0.0 { ocf + capex } else { ocf - capex };
                m.insert("free_cash_flow".into(), crate::num_value(fcf));
            }
        }
    }

    // `dict(sorted(periods.items()))`
    let mut keys: Vec<&String> = periods.keys().collect();
    keys.sort();
    let mut sorted = Map::new();
    for k in keys {
        sorted.insert(k.clone(), periods[k].clone());
    }

    json!({
        "symbol": symbol,
        "basis": "annual",
        "currency": currency.unwrap_or(Value::Null),
        "periods": Value::Object(sorted),
        "source": "yahoo_fundamentals_timeseries",
    })
}

fn opt_num(x: Option<f64>) -> Value {
    match x {
        Some(v) => crate::num_value(v),
        None => Value::Null,
    }
}

/// `apply_yearly_fx` — add comparable monetary facts while preserving reported values.
pub fn apply_yearly_fx(financials: &Value, yearly_rates: &Value, base_currency: &str) -> Value {
    let mut result = financials.as_object().cloned().unwrap_or_default();
    result.insert("base_currency".into(), Value::String(base_currency.into()));
    let mut periods = Map::new();
    if let Some(src) = financials.get("periods").and_then(|p| p.as_object()) {
        for (period, raw_facts) in src.iter() {
            let mut facts = raw_facts.as_object().cloned().unwrap_or_default();
            let year: String = period.chars().take(4).collect();
            let rate = number(yearly_rates.get(&year).unwrap_or(&Value::Null));
            if let Some(rate) = rate {
                if rate > 0.0 {
                    facts.insert("fx_rate_to_base".into(), crate::num_value(rate));
                    for metric in [
                        "revenue",
                        "gross_profit",
                        "operating_income",
                        "net_income",
                        "equity",
                        "operating_cash_flow",
                        "capital_expenditure",
                        "free_cash_flow",
                    ] {
                        let value = number(facts.get(metric).unwrap_or(&Value::Null));
                        if let Some(value) = value {
                            facts.insert(
                                format!("{}_base", metric),
                                crate::num_value(uzi_core::py::round(value * rate, 4)),
                            );
                        }
                    }
                }
            }
            periods.insert(period.clone(), Value::Object(facts));
        }
    }
    result.insert("periods".into(), Value::Object(periods));
    Value::Object(result)
}

/// `issuer_key` — legal-suffix-stripped, alphanumeric-only lower-case key.
pub fn issuer_key(name: &str) -> String {
    let normalized = LEGAL_SUFFIXES.replace_all(name, " ");
    let lowered = normalized.to_lowercase();
    lowered
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || ('\u{4e00}'..='\u{9fff}').contains(c))
        .collect()
}

/// `_candidate_score`.
fn candidate_score(target: &Value, candidate: &Value) -> (f64, Vec<String>) {
    let mut score = 0.0f64;
    let mut reasons: Vec<String> = Vec::new();

    let target_industry = uzi_core::py::get(target, "industry");
    if uzi_core::py::truthy(target_industry) && target_industry == uzi_core::py::get(candidate, "industry")
    {
        score += 40.0;
        reasons.push("细分行业一致".into());
    }
    let target_sector = uzi_core::py::get(target, "sector");
    if uzi_core::py::truthy(target_sector) && target_sector == uzi_core::py::get(candidate, "sector") {
        score += 15.0;
        reasons.push("行业板块一致".into());
    }

    let target_has_base = target
        .get("market_cap_base")
        .map(|v| !v.is_null())
        .unwrap_or(false);
    let candidate_has_base = candidate
        .get("market_cap_base")
        .map(|v| !v.is_null())
        .unwrap_or(false);
    let target_cap = number(if target_has_base {
        uzi_core::py::get(target, "market_cap_base")
    } else {
        uzi_core::py::get(target, "market_cap")
    });
    let candidate_cap = number(if candidate_has_base {
        uzi_core::py::get(candidate, "market_cap_base")
    } else {
        uzi_core::py::get(candidate, "market_cap")
    });
    let target_currency = uzi_core::py::get(target, "currency");
    let candidate_currency = uzi_core::py::get(candidate, "currency");
    let same_currency = uzi_core::py::truthy(target_currency)
        && uzi_core::py::truthy(candidate_currency)
        && target_currency == candidate_currency;
    let comparable_scale = (target_has_base && candidate_has_base) || same_currency;
    if comparable_scale {
        if let (Some(tc), Some(cc)) = (target_cap, candidate_cap) {
            if tc > 0.0 && cc > 0.0 {
                let distance = (cc / tc).log10().abs();
                let scale_score = (20.0 * (1.0 - distance / 3.0)).max(0.0);
                score += scale_score;
                if distance <= 1.0 {
                    reasons.push("规模可比".into());
                }
            }
        }
    }

    if let Some(coverage) = number(uzi_core::py::get(candidate, "data_coverage")) {
        score += coverage.clamp(0.0, 1.0) * 15.0;
        if coverage >= 0.7 {
            reasons.push("财务数据完整".into());
        }
    }
    if let Some(provider_score) = number(uzi_core::py::get(candidate, "provider_score")) {
        score += provider_score.clamp(0.0, 1.0) * 10.0;
    }
    if uzi_core::py::truthy(uzi_core::py::get(candidate, "is_secondary")) {
        score -= 25.0;
    }
    let score = uzi_core::py::round(score.max(0.0), 2);
    if reasons.is_empty() {
        reasons.push("候选来源关联".into());
    }
    (score, reasons)
}

/// `rank_global_candidates` — de-duplicate issuers, keep the most comparable.
pub fn rank_global_candidates(target: &Value, candidates: &[Value], limit: i64) -> Vec<Value> {
    let target_symbol = crate::py_str_py(uzi_core::py::get(target, "symbol")).to_uppercase();
    let target_issuer = issuer_key(&crate::py_str_py(uzi_core::py::get(target, "name")));
    let mut best_by_issuer: Map<String, Value> = Map::new();

    for raw in candidates.iter() {
        if !raw.is_object() {
            continue;
        }
        let symbol = crate::py_str_py(uzi_core::py::get(raw, "symbol")).to_uppercase();
        let name_or_symbol = {
            let n = uzi_core::py::get(raw, "name");
            if uzi_core::py::truthy(n) {
                crate::py_str_py(n)
            } else {
                symbol.clone()
            }
        };
        let key = issuer_key(&name_or_symbol);
        if symbol.is_empty()
            || symbol == target_symbol
            || (!target_issuer.is_empty() && key == target_issuer)
        {
            continue;
        }
        let (score, reasons) = candidate_score(target, raw);
        let mut candidate = raw.as_object().cloned().unwrap_or_default();
        candidate.insert("symbol".into(), Value::String(symbol));
        candidate.insert("relevance_score".into(), crate::num_value(score));
        candidate.insert(
            "selection_reasons".into(),
            Value::Array(reasons.into_iter().map(Value::String).collect()),
        );
        let candidate = Value::Object(candidate);
        let current_score = best_by_issuer
            .get(&key)
            .map(|c| pnum(uzi_core::py::get(c, "relevance_score"), 0.0));
        match current_score {
            Some(cur) if score <= cur => {}
            _ => {
                best_by_issuer.insert(key, candidate);
            }
        }
    }

    let mut ranked: Vec<Value> = best_by_issuer.into_values().collect();
    ranked.sort_by(|a, b| {
        let sa = pnum(uzi_core::py::get(a, "relevance_score"), 0.0);
        let sb = pnum(uzi_core::py::get(b, "relevance_score"), 0.0);
        let ca = match uzi_core::py::get(a, "data_coverage") {
            Value::Null => 0.0,
            v => pnum(v, 0.0),
        };
        let cb = match uzi_core::py::get(b, "data_coverage") {
            Value::Null => 0.0,
            v => pnum(v, 0.0),
        };
        sb.partial_cmp(&sa)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(cb.partial_cmp(&ca).unwrap_or(std::cmp::Ordering::Equal))
    });
    let limit = limit.max(0) as usize;
    ranked.truncate(limit);
    ranked
}

/// `_benchmarks`.
pub fn benchmarks(peers: &[Value]) -> Value {
    let mut metric_order: Vec<String> = Vec::new();
    let mut grouped: std::collections::HashMap<String, std::collections::HashMap<String, Vec<f64>>> =
        std::collections::HashMap::new();
    for peer in peers.iter() {
        let Some(periods) = peer
            .get("financials")
            .and_then(|f| f.get("periods"))
            .and_then(|p| p.as_object())
        else {
            continue;
        };
        for (period, facts) in periods.iter() {
            let year: String = period.chars().take(4).collect();
            if year.chars().count() != 4 || !facts.is_object() {
                continue;
            }
            for metric in COMPARABLE_METRICS {
                let value = number(uzi_core::py::get(facts, metric));
                if let Some(value) = value {
                    if !grouped.contains_key(*metric) {
                        metric_order.push((*metric).to_string());
                    }
                    grouped
                        .entry((*metric).to_string())
                        .or_default()
                        .entry(year.clone())
                        .or_default()
                        .push(value);
                }
            }
        }
    }

    let mut result = Map::new();
    for metric in metric_order.iter() {
        let years = &grouped[metric];
        let mut years_sorted: Vec<(&String, &Vec<f64>)> = years.iter().collect();
        years_sorted.sort_by(|a, b| a.0.cmp(b.0));
        let mut metric_out = Map::new();
        for (year, values) in years_sorted {
            let mut ordered = values.clone();
            ordered.sort_by(|a, b| a.partial_cmp(b).unwrap());
            let (p25, p75) = if ordered.len() >= 2 {
                let q = stats::quantiles_exclusive_sorted(&ordered, 4);
                (q[0], q[2])
            } else {
                (ordered[0], ordered[0])
            };
            metric_out.insert(
                year.clone(),
                json!({
                    "min": crate::num_value(uzi_core::py::round(ordered[0], 2)),
                    "p25": crate::num_value(uzi_core::py::round(p25, 2)),
                    "median": crate::num_value(uzi_core::py::round(stats::median_sorted(&ordered), 2)),
                    "p75": crate::num_value(uzi_core::py::round(p75, 2)),
                    "max": crate::num_value(uzi_core::py::round(ordered[ordered.len() - 1], 2)),
                    "n": ordered.len(),
                }),
            );
        }
        result.insert(metric.clone(), Value::Object(metric_out));
    }
    Value::Object(result)
}

/// `_latest_facts`.
fn latest_facts(financials: &Value) -> (Option<String>, Value) {
    let Some(periods) = financials.get("periods").and_then(|p| p.as_object()) else {
        return (None, json!({}));
    };
    if periods.is_empty() {
        return (None, json!({}));
    }
    let period = periods.keys().max().cloned();
    match period {
        Some(p) => {
            let facts = periods.get(&p).cloned().unwrap_or(Value::Null);
            let facts = if uzi_core::py::truthy(&facts) {
                facts
            } else {
                json!({})
            };
            (Some(p), facts)
        }
        None => (None, json!({})),
    }
}

/// `_target_percentile`.
pub fn target_percentile(target_financials: &Value, peers: &[Value]) -> Value {
    if peers.len() < 3 {
        return json!({});
    }
    let (target_period, target_facts) = latest_facts(target_financials);
    let Some(target_period) = target_period else {
        return json!({});
    };
    let year: String = target_period.chars().take(4).collect();
    let mut result = Map::new();
    for metric in COMPARABLE_METRICS {
        let target_value = number(uzi_core::py::get(&target_facts, metric));
        let mut values: Vec<f64> = Vec::new();
        for peer in peers.iter() {
            let Some(periods) = peer
                .get("financials")
                .and_then(|f| f.get("periods"))
                .and_then(|p| p.as_object())
            else {
                continue;
            };
            let matching: Vec<&Value> = periods
                .iter()
                .filter(|(period, _)| period.starts_with(year.as_str()))
                .map(|(_, facts)| facts)
                .collect();
            if let Some(last) = matching.last() {
                if let Some(value) = number(uzi_core::py::get(last, metric)) {
                    values.push(value);
                }
            }
        }
        if let Some(target_value) = target_value {
            if values.len() >= 3 {
                let below = values.iter().filter(|v| **v < target_value).count();
                result.insert(
                    metric.to_string(),
                    crate::num_value(uzi_core::py::round(
                        below as f64 / values.len() as f64 * 100.0,
                        1,
                    )),
                );
            }
        }
    }
    Value::Object(result)
}

/// `global_peers_to_comps` — adapt latest normalized global peer facts to
/// `fin_models::build_comps_table`.
pub fn global_peers_to_comps(comparison: &Value) -> Vec<Value> {
    let mut result = Vec::new();
    let Some(peers) = comparison.get("peers").and_then(|p| p.as_array()) else {
        return result;
    };
    for peer in peers.iter() {
        let Some(periods) = peer
            .get("financials")
            .and_then(|f| f.get("periods"))
            .and_then(|p| p.as_object())
        else {
            continue;
        };
        if periods.is_empty() {
            continue;
        }
        let mut ordered: Vec<&String> = periods.keys().collect();
        ordered.sort();
        let latest_raw = periods.get(ordered[ordered.len() - 1]).cloned().unwrap_or(Value::Null);
        let latest = if uzi_core::py::truthy(&latest_raw) {
            latest_raw
        } else {
            json!({})
        };
        let mut growth: Option<f64> = None;
        if ordered.len() >= 2 {
            let prev_raw = periods
                .get(ordered[ordered.len() - 2])
                .cloned()
                .unwrap_or(Value::Null);
            let prev = if uzi_core::py::truthy(&prev_raw) {
                prev_raw
            } else {
                json!({})
            };
            let previous = number(uzi_core::py::get(&prev, "revenue_base"));
            let current = number(uzi_core::py::get(&latest, "revenue_base"));
            if let Some(previous) = previous {
                if previous != 0.0 || previous.is_nan() {
                    if let Some(current) = current {
                        growth = Some(uzi_core::py::round(
                            (current / previous - 1.0) * 100.0,
                            2,
                        ));
                    }
                }
            }
        }
        let name = {
            let n = uzi_core::py::get(peer, "name");
            if uzi_core::py::truthy(n) {
                n.clone()
            } else {
                uzi_core::py::get(peer, "symbol").clone()
            }
        };
        result.push(json!({
            "name": name,
            "ticker": uzi_core::py::get(peer, "symbol").clone(),
            "pe": uzi_core::py::get(peer, "pe").clone(),
            "pb": uzi_core::py::get(peer, "pb").clone(),
            "ps": uzi_core::py::get(peer, "ps").clone(),
            "roe": uzi_core::py::get(&latest, "roe").clone(),
            "net_margin": uzi_core::py::get(&latest, "net_margin").clone(),
            "revenue_growth": growth.map(crate::num_value).unwrap_or(Value::Null),
            "market_cap_yi": 0,
        }));
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn issuer_key_strips_legal_suffixes_and_punctuation() {
        assert_eq!(issuer_key("Apple Inc."), "apple");
        assert_eq!(issuer_key("NVIDIA Corporation"), "nvidia");
        assert_eq!(issuer_key("贵州茅台股份有限公司"), "贵州茅台股份有限公司");
        assert_eq!(issuer_key(""), "");
    }

    #[test]
    fn yahoo_symbol_mapping_for_a_hk_and_us() {
        assert_eq!(
            to_yahoo_symbol(&json!({"market": "A", "code": "600519", "full": "600519.SH"})),
            "600519.SS"
        );
        assert_eq!(
            to_yahoo_symbol(&json!({"market": "H", "code": "700", "full": "00700.HK"})),
            "0700.HK"
        );
        assert_eq!(
            to_yahoo_symbol(&json!({"market": "US", "code": "AAPL", "full": "AAPL"})),
            "AAPL"
        );
    }

    #[test]
    fn global_peers_to_comps_computes_revenue_growth_from_prior_year() {
        let comparison = json!({"peers": [{
            "name": "Peer", "symbol": "P.US",
            "financials": {"periods": {
                "2022-12-31": {"revenue_base": 100.0, "roe": 10.0},
                "2023-12-31": {"revenue_base": 125.0, "roe": 12.0},
            }},
        }]});
        let out = global_peers_to_comps(&comparison);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0]["ticker"], json!("P.US"));
        assert_eq!(out[0]["revenue_growth"], json!(25.0));
        assert_eq!(out[0]["roe"], json!(12.0));
    }

    #[test]
    fn benchmarks_require_sorted_years_and_use_exclusive_quartiles() {
        let peers = vec![
            json!({"financials": {"periods": {"2023-12-31": {"net_margin": 10.0}}}}),
            json!({"financials": {"periods": {"2023-12-31": {"net_margin": 20.0}}}}),
            json!({"financials": {"periods": {"2023-12-31": {"net_margin": 30.0}}}}),
        ];
        let out = benchmarks(&peers);
        assert_eq!(out["net_margin"]["2023"]["median"], json!(20.0));
        assert_eq!(out["net_margin"]["2023"]["min"], json!(10.0));
        assert_eq!(out["net_margin"]["2023"]["max"], json!(30.0));
        assert_eq!(out["net_margin"]["2023"]["n"], json!(3));
    }
}
