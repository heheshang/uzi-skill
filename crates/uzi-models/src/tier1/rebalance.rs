//! Port of `lib/tier1/rebalance.py` — portfolio rebalance (A-share adapted, no TLH).

use crate::pnum;
use serde_json::{json, Map, Value};
use uzi_core::py::round;

/// Stamp duty by market (single side).
const STAMP_DUTY_A: f64 = 0.0005;
const STAMP_DUTY_HK: f64 = 0.001;
/// Commission by market (both sides).
const COMMISSION_A: f64 = 0.00025;
const COMMISSION_HK: f64 = 0.00025;
const DEFAULT_TOP_N: usize = 3;

/// `_infer_market`.
fn infer_market(ticker: &str, given: &Value) -> String {
    if uzi_core::py::truthy(given) {
        let g = crate::py_str_py(given).trim().to_uppercase();
        if ["A", "CN", "SH", "SZ", "A股"].contains(&g.as_str()) {
            return "A".to_string();
        }
        if ["HK", "港股"].contains(&g.as_str()) {
            return "HK".to_string();
        }
        if ["US", "美股"].contains(&g.as_str()) {
            return "US".to_string();
        }
    }
    let t = ticker.to_uppercase();
    let all_digits = !t.is_empty() && t.chars().all(|c| c.is_ascii_digit());
    if t.ends_with(".HK") || (all_digits && t.len() == 5) {
        return "HK".to_string();
    }
    if t.ends_with(".US")
        || (t.replace('.', "").chars().all(|c| c.is_ascii_alphabetic())
            && !t.replace(".US", "").contains('.'))
    {
        if t.replace(".US", "").chars().all(|c| c.is_ascii_alphabetic()) {
            return "US".to_string();
        }
    }
    if t.ends_with(".SH") || t.ends_with(".SZ") || t.ends_with(".BJ") {
        return "A".to_string();
    }
    "A".to_string()
}

/// `_normalize`.
fn normalize(weights: &[Value]) -> Vec<f64> {
    let mut vals: Vec<f64> = weights.iter().map(|w| pnum(w, 0.0).max(0.0)).collect();
    let mut total: f64 = vals.iter().sum();
    if total <= 0.0 {
        let n = vals.len().max(1);
        return vec![1.0 / n as f64; vals.len()];
    }
    if total > 1.5 {
        vals = vals.iter().map(|v| v / 100.0).collect();
        total = vals.iter().sum();
    }
    vals.iter().map(|v| v / total).collect()
}

/// `_resolve_targets`.
fn resolve_targets(holdings: &[Value], targets: Option<&Value>) -> Vec<f64> {
    let n = holdings.len();
    let Some(targets) = targets.filter(|t| uzi_core::py::truthy(t)) else {
        return vec![1.0 / n as f64; n];
    };
    let raw: Vec<Value> = holdings
        .iter()
        .map(|h| {
            let key = crate::py_str_py(uzi_core::py::get(h, "ticker"));
            targets.get(&key).cloned().unwrap_or(json!(0.0))
        })
        .collect();
    normalize(&raw)
}

/// `_concentration`.
fn concentration(rows: &[(f64, Value)], top_n: usize) -> Value {
    let mut sorted_w: Vec<f64> = rows.iter().map(|(w, _)| *w).collect();
    sorted_w.sort_by(|a, b| b.partial_cmp(a).unwrap());
    let top_n_sum: f64 = sorted_w.iter().take(top_n).sum();
    let max_single = sorted_w.first().copied().unwrap_or(0.0);
    let hhi: f64 = sorted_w.iter().map(|w| w * w).sum();

    let mut ind_order: Vec<String> = Vec::new();
    let mut ind_totals: Vec<f64> = Vec::new();
    for (w, r) in rows.iter() {
        let ind_raw = uzi_core::py::get(r, "industry");
        let ind = if uzi_core::py::truthy(ind_raw) {
            crate::py_str_py(ind_raw).trim().to_string()
        } else {
            "—".to_string()
        };
        let ind = if ind.is_empty() { "—".to_string() } else { ind };
        match ind_order.iter().position(|k| *k == ind) {
            Some(p) => ind_totals[p] += *w,
            None => {
                ind_order.push(ind);
                ind_totals.push(*w);
            }
        }
    }
    let top_industry = ind_totals.iter().cloned().fold(0.0f64, f64::max);
    let n_industries = ind_order.iter().filter(|k| k.as_str() != "—").count();

    let mut breakdown: Vec<(String, f64)> = ind_order
        .iter()
        .cloned()
        .zip(ind_totals.iter().cloned())
        .collect();
    breakdown.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
    let mut breakdown_map = Map::new();
    for (k, v) in breakdown {
        breakdown_map.insert(k, crate::num_value(round(v, 4)));
    }

    json!({
        "top_n": top_n,
        "top_n_weight": crate::num_value(round(top_n_sum, 4)),
        "max_single_weight": crate::num_value(round(max_single, 4)),
        "hhi": crate::num_value(round(hhi, 4)),
        "n_industries": n_industries,
        "top_industry_weight": crate::num_value(round(top_industry, 4)),
        "industry_breakdown": Value::Object(breakdown_map),
    })
}

/// `build_rebalance` — portfolio rebalance analysis (pure, no IO).
pub fn build_rebalance(holdings: &Value, targets: Option<&Value>, drift_threshold: f64) -> Value {
    let holdings_arr: Vec<Value> = match holdings.as_array() {
        Some(a) if !a.is_empty() => a.clone(),
        _ => return json!({"error": "holdings 为空", "method": "Portfolio Rebalance"}),
    };
    let n = holdings_arr.len();

    // Normalize current weights
    let weight_values: Vec<Value> = holdings_arr
        .iter()
        .map(|h| uzi_core::py::get(h, "weight").clone())
        .collect();
    let cur_weights = normalize(&weight_values);
    let tgt_weights = resolve_targets(&holdings_arr, targets);
    let target_mode = if targets.map(uzi_core::py::truthy).unwrap_or(false) {
        "显式目标".to_string()
    } else {
        format!("等权 (1/{})", n)
    };

    // Portfolio total value
    let mut total_value = 0.0f64;
    for h in holdings_arr.iter() {
        let v = uzi_core::py::get(h, "value");
        let mv = uzi_core::py::get(h, "market_value");
        let chosen = if uzi_core::py::truthy(v) { v } else { mv };
        total_value += pnum(chosen, 0.0);
    }
    let has_value = total_value > 0.0;

    // Drift table
    let mut drift_table: Vec<Value> = Vec::new();
    let mut markets: Vec<String> = Vec::new();
    for (i, h) in holdings_arr.iter().enumerate() {
        let ticker = crate::py_str_py(uzi_core::py::get(h, "ticker"));
        let mkt = infer_market(&ticker, uzi_core::py::get(h, "market"));
        markets.push(mkt.clone());
        let cw = cur_weights[i];
        let tw = tgt_weights[i];
        let drift_pp = (cw - tw) * 100.0;
        let breached = drift_pp.abs() > drift_threshold;
        let delta_value = if has_value {
            Some((tw - cw) * total_value)
        } else {
            None
        };
        let industry = {
            let ind = uzi_core::py::get(h, "industry");
            if uzi_core::py::truthy(ind) {
                ind.clone()
            } else {
                json!("—")
            }
        };
        drift_table.push(json!({
            "ticker": crate::py_str_py(uzi_core::py::get(h, "ticker")),
            "market": mkt,
            "industry": industry,
            "current_weight": crate::num_value(round(cw, 4)),
            "target_weight": crate::num_value(round(tw, 4)),
            "drift_pp": crate::num_value(round(drift_pp, 2)),
            "abs_drift_pp": crate::num_value(round(drift_pp.abs(), 2)),
            "breached": breached,
            "direction": if drift_pp > 0.0 { "超配→卖" } else if drift_pp < 0.0 { "低配→买" } else { "持平" },
            "delta_value": match delta_value { Some(v) => crate::num_value(round(v, 2)), None => Value::Null },
        }));
    }

    let breached_rows: Vec<Value> = drift_table
        .iter()
        .filter(|d| uzi_core::py::truthy(uzi_core::py::get(d, "breached")))
        .cloned()
        .collect();
    let any_breach = !breached_rows.is_empty();
    let max_drift = drift_table
        .iter()
        .map(|d| pnum(uzi_core::py::get(d, "abs_drift_pp"), 0.0))
        .fold(0.0f64, f64::max);

    // Trade list (only breached positions)
    let mut trades: Vec<Value> = Vec::new();
    for (i, d) in drift_table.iter().enumerate() {
        if !uzi_core::py::truthy(uzi_core::py::get(d, "breached")) {
            continue;
        }
        let h = &holdings_arr[i];
        let drift_pp = pnum(uzi_core::py::get(d, "drift_pp"), 0.0);
        let action = if drift_pp > 0.0 { "SELL" } else { "BUY" };
        let amount = match uzi_core::py::get(d, "delta_value") {
            Value::Null => None,
            v => Some(pnum(v, 0.0).abs()),
        };
        let price = pnum(uzi_core::py::get(h, "price"), 0.0);
        let mut shares: Option<i64> = None;
        if let Some(amount) = amount {
            if price > 0.0 {
                let mut s = (amount / price) as i64;
                if uzi_core::py::get(d, "market") == &json!("A") {
                    s = (s / 100) * 100;
                }
                shares = Some(s);
            }
        }
        trades.push(json!({
            "ticker": uzi_core::py::get(d, "ticker").clone(),
            "market": uzi_core::py::get(d, "market").clone(),
            "action": action,
            "action_cn": if action == "SELL" { "卖出" } else { "买入" },
            "weight_change_pp": crate::num_value(round(tgt_weights[i] * 100.0 - pnum(uzi_core::py::get(d, "current_weight"), 0.0) * 100.0, 2)),
            "amount": match amount { Some(a) => crate::num_value(round(a, 2)), None => Value::Null },
            "est_shares": match shares { Some(s) => json!(s), None => Value::Null },
            "reason": format!("漂移 {:+.1}pp 超阈值 {:.0}pp", drift_pp, drift_threshold),
        }));
    }

    // Turnover cost estimate (per-market breakdown)
    let mut cost_by_market: Map<String, Value> = Map::new();
    let mut total_cost = 0.0f64;
    let mut total_turnover = 0.0f64;
    for t in trades.iter() {
        let amount = match uzi_core::py::get(t, "amount") {
            Value::Null => continue,
            v => pnum(v, 0.0),
        };
        let mkt = crate::py_str_py(uzi_core::py::get(t, "market"));
        let action = uzi_core::py::get(t, "action").as_str().unwrap_or("");
        let comm = match mkt.as_str() {
            "A" => COMMISSION_A * amount,
            "HK" => COMMISSION_HK * amount,
            _ => 0.0,
        };
        let stamp = if mkt == "HK" {
            STAMP_DUTY_HK * amount
        } else if action == "SELL" {
            if mkt == "A" {
                STAMP_DUTY_A * amount
            } else {
                0.0
            }
        } else {
            0.0
        };
        let leg_cost = comm + stamp;
        let agg = cost_by_market.entry(mkt.clone()).or_insert_with(|| {
            json!({
                "market_label": match mkt.as_str() { "A" => "A 股", "HK" => "港股", "US" => "美股", _ => mkt.as_str() },
                "turnover": 0.0, "stamp_duty": 0.0, "commission": 0.0, "total": 0.0,
            })
        });
        if let Value::Object(m) = agg {
            for (k, delta) in [
                ("turnover", amount),
                ("stamp_duty", stamp),
                ("commission", comm),
                ("total", leg_cost),
            ] {
                let prev = pnum(m.get(k).unwrap_or(&Value::Null), 0.0);
                m.insert(k.to_string(), crate::num_value(prev + delta));
            }
        }
        total_cost += leg_cost;
        total_turnover += amount;
    }
    for agg in cost_by_market.values_mut() {
        if let Value::Object(m) = agg {
            for k in ["turnover", "stamp_duty", "commission", "total"] {
                let v = pnum(m.get(k).unwrap_or(&Value::Null), 0.0);
                m.insert(k.to_string(), crate::num_value(round(v, 2)));
            }
        }
    }

    let turnover_cost = json!({
        "has_value_input": has_value,
        "total_turnover": crate::num_value(round(total_turnover, 2)),
        "total_cost": crate::num_value(round(total_cost, 2)),
        "cost_pct_of_turnover": if total_turnover > 0.0 { crate::num_value(round(total_cost / total_turnover * 100.0, 4)) } else { json!(0.0) },
        "by_market": Value::Object(cost_by_market),
        "note": if has_value {
            "已按交易额估算；A 股卖出印花税 0.05%(2023-08 下调)+双边佣金~0.025%，港股印花税 0.1%(双边)，美股印花税近 0。".to_string()
        } else {
            "未提供组合市值 (value/price)，仅给出权重漂移与交易方向，无法估算金额/成本。".to_string()
        },
    });

    // Concentration change (before vs after)
    let cur_rows: Vec<(f64, Value)> = holdings_arr
        .iter()
        .zip(cur_weights.iter())
        .map(|(h, w)| (*w, json!({"industry": uzi_core::py::get(h, "industry").clone()})))
        .collect();
    let tgt_rows: Vec<(f64, Value)> = holdings_arr
        .iter()
        .zip(tgt_weights.iter())
        .map(|(h, w)| (*w, json!({"industry": uzi_core::py::get(h, "industry").clone()})))
        .collect();
    let conc_before = concentration(&cur_rows, DEFAULT_TOP_N);
    let conc_after = concentration(&tgt_rows, DEFAULT_TOP_N);
    let concentration_out = json!({
        "before": conc_before,
        "after": conc_after,
        "top_n_change_pp": crate::num_value(round(
            (pnum(uzi_core::py::get(&conc_after, "top_n_weight"), 0.0)
                - pnum(uzi_core::py::get(&conc_before, "top_n_weight"), 0.0)) * 100.0, 2)),
        "max_single_change_pp": crate::num_value(round(
            (pnum(uzi_core::py::get(&conc_after, "max_single_weight"), 0.0)
                - pnum(uzi_core::py::get(&conc_before, "max_single_weight"), 0.0)) * 100.0, 2)),
        "hhi_change": crate::num_value(round(
            pnum(uzi_core::py::get(&conc_after, "hhi"), 0.0)
                - pnum(uzi_core::py::get(&conc_before, "hhi"), 0.0), 4)),
    });

    // US tax-loss note
    let has_us = markets.iter().any(|m| m == "US");
    let us_tlh_note = if has_us {
        "持仓含美股：美股有资本利得税，卖出端可另议税损收割 (TLH) 与持有期 (短期/长期)；A 股 / 港股个人无资本利得税，本工具不做 TLH。"
    } else {
        "A 股 / 港股个人无资本利得税，本工具不做税损收割 (TLH)，仅算漂移 + 风险 + 换手成本。"
    };

    let verdict = if !any_breach {
        format!(
            "🟢 无需再平衡 · 最大漂移 {:.1}pp ≤ 阈值 {:.0}pp",
            max_drift, drift_threshold
        )
    } else {
        format!(
            "🟡 建议再平衡 · {} 只超阈值 (最大漂移 {:.1}pp) · {} 笔交易",
            breached_rows.len(),
            max_drift,
            trades.len()
        )
    };

    let summary = json!({
        "n_holdings": n,
        "target_mode": target_mode,
        "drift_threshold_pp": crate::num_value(drift_threshold),
        "any_breach": any_breach,
        "n_breached": breached_rows.len(),
        "max_drift_pp": crate::num_value(round(max_drift, 2)),
        "n_trades": trades.len(),
        "estimated_cost": if has_value { crate::num_value(round(total_cost, 2)) } else { Value::Null },
        "verdict": verdict,
        "tlh_note": us_tlh_note,
    });

    let cost_pct = pnum(uzi_core::py::get(&turnover_cost, "cost_pct_of_turnover"), 0.0);
    let cbefore_top = pnum(uzi_core::py::get(&conc_before, "top_n_weight"), 0.0);
    let cafter_top = pnum(uzi_core::py::get(&conc_after, "top_n_weight"), 0.0);
    let hhi_change = pnum(uzi_core::py::get(&concentration_out, "hhi_change"), 0.0);

    json!({
        "method": "Portfolio Rebalance (A股适配 · 去TLH)",
        "summary": summary,
        "drift_table": drift_table,
        "trades": trades,
        "turnover_cost": turnover_cost,
        "concentration": concentration_out,
        "methodology_log": [
            format!("Step 1 · 归一化当前权重 ({} 只) + 解析目标 ({})", n, target_mode),
            format!(
                "Step 2 · 计算漂移 · 阈值 {:.0}pp · {} 只超标 (最大 {:.1}pp)",
                drift_threshold, breached_rows.len(), max_drift
            ),
            format!(
                "Step 3 · 生成交易清单 {} 笔 {}",
                trades.len(),
                if has_value { "(已估金额/股数)" } else { "(仅方向，缺市值)" }
            ),
            format!(
                "Step 4 · 换手成本估算 {}",
                if has_value {
                    format!("¥{:.0} (占交易额 {:.3}%)", total_cost, cost_pct)
                } else {
                    "跳过 (无市值输入)".to_string()
                }
            ),
            format!(
                "Step 5 · 集中度变化 · 前{}大 {:.1}% → {:.1}% · HHI {:+.3}",
                DEFAULT_TOP_N, cbefore_top * 100.0, cafter_top * 100.0, hhi_change
            ),
            format!(
                "Step 6 · A 股无资本利得税 → 不做 TLH；{}",
                if has_us { "美股部分可另议税损" } else { "纯 A/港股，无税损议题" }
            ),
        ],
    })
}
