//! Port of `lib/fin_models.py` — institutional-grade financial models:
//! 2-stage DCF + WACC, comparable-company table, 5-year 3-statement
//! projection, quick LBO, and merger accretion/dilution.
//!
//! Arithmetic is ported operation-by-operation in the upstream order: float
//! non-associativity means any reordering changes the last bits, and the golden
//! comparison is exact.

use crate::{flt, num_value, py_str_py, stats};
use serde_json::{json, Map, Value};
use uzi_core::py::round;

pub const DEFAULT_RF: f64 = 0.025;
pub const DEFAULT_ERP: f64 = 0.06;
pub const DEFAULT_BETA: f64 = 1.00;
pub const DEFAULT_TAX: f64 = 0.25;
pub const DEFAULT_TERMINAL_G: f64 = 0.025;
pub const DEFAULT_STAGE1_YEARS: i64 = 5;
pub const DEFAULT_STAGE2_YEARS: i64 = 5;
pub const DEFAULT_STAGE1_GROWTH: f64 = 0.10;
pub const DEFAULT_STAGE2_GROWTH: f64 = 0.05;

/// `compute_wacc` — CAPM cost of equity + after-tax cost of debt → WACC.
pub fn compute_wacc(
    rf: f64,
    erp: f64,
    beta: f64,
    cost_of_debt_pretax: f64,
    target_debt_ratio: f64,
    tax: f64,
) -> Value {
    let cost_of_equity = rf + beta * erp;
    let after_tax_kd = cost_of_debt_pretax * (1.0 - tax);
    let equity_weight = 1.0 - target_debt_ratio;
    let wacc = equity_weight * cost_of_equity + target_debt_ratio * after_tax_kd;
    let mut inputs = Map::new();
    inputs.insert("rf".into(), num_value(rf));
    inputs.insert("erp".into(), num_value(erp));
    inputs.insert("beta".into(), num_value(beta));
    inputs.insert("kd_pretax".into(), num_value(cost_of_debt_pretax));
    inputs.insert("tax".into(), num_value(tax));
    let mut out = Map::new();
    out.insert("wacc".into(), num_value(round(wacc, 4)));
    out.insert("cost_of_equity".into(), num_value(round(cost_of_equity, 4)));
    out.insert("after_tax_kd".into(), num_value(round(after_tax_kd, 4)));
    out.insert("equity_weight".into(), num_value(equity_weight));
    out.insert("debt_weight".into(), num_value(target_debt_ratio));
    out.insert("inputs".into(), Value::Object(inputs));
    Value::Object(out)
}

/// The default `compute_wacc()` call (upstream defaults).
pub fn compute_wacc_default() -> Value {
    compute_wacc(
        DEFAULT_RF,
        DEFAULT_ERP,
        DEFAULT_BETA,
        0.045,
        0.30,
        DEFAULT_TAX,
    )
}

/// `compute_dcf` — 2-stage DCF with 5x5 sensitivity table.
pub fn compute_dcf(features: &Value, assumptions: Option<&Value>) -> Value {
    let mut a = Map::new();
    a.insert("stage1_growth".into(), num_value(DEFAULT_STAGE1_GROWTH));
    a.insert("stage2_growth".into(), num_value(DEFAULT_STAGE2_GROWTH));
    a.insert("stage1_years".into(), json!(DEFAULT_STAGE1_YEARS));
    a.insert("stage2_years".into(), json!(DEFAULT_STAGE2_YEARS));
    a.insert("terminal_g".into(), num_value(DEFAULT_TERMINAL_G));
    a.insert("beta".into(), num_value(DEFAULT_BETA));
    a.insert("tax".into(), num_value(DEFAULT_TAX));
    a.insert("target_debt_ratio".into(), num_value(0.30));
    if let Some(Value::Object(extra)) = assumptions {
        for (k, v) in extra {
            a.insert(k.clone(), v.clone());
        }
    }
    let a = Value::Object(a);

    let beta = flt(&a["beta"], 0.0);
    let tax = flt(&a["tax"], 0.0);
    let target_debt_ratio = flt(&a["target_debt_ratio"], 0.0);
    let wacc_info = compute_wacc(
        DEFAULT_RF,
        DEFAULT_ERP,
        beta,
        0.045,
        target_debt_ratio,
        tax,
    );
    let wacc = flt(&wacc_info["wacc"], 0.0);

    // Base FCF — if missing, approximate from revenue × net_margin × 0.8
    let mut fcf0 = flt(uzi_core::py::get(features, "fcf_latest_yi"), 0.0);
    if fcf0 <= 0.0 {
        let rev = flt(uzi_core::py::get(features, "revenue_latest_yi"), 0.0);
        let nm = flt(uzi_core::py::get(features, "net_margin"), 0.0) / 100.0;
        fcf0 = rev * nm * 0.8;
    }
    if fcf0 <= 0.0 {
        // v3.9.4 · no fake "market-cap × 5% yield" backstop: report data
        // insufficiency rather than a falsely neutral conclusion.
        let mut out = Map::new();
        out.insert(
            "method".into(),
            Value::String("DCF (2-stage + Gordon Growth terminal)".into()),
        );
        out.insert(
            "verdict".into(),
            Value::String("⛔ 数据不足 · 无法 DCF".into()),
        );
        out.insert("intrinsic_per_share".into(), Value::Null);
        out.insert("safety_margin_pct".into(), Value::Null);
        out.insert(
            "error".into(),
            Value::String("FCF / 营收 / 净利率均缺失".into()),
        );
        out.insert(
            "methodology_log".into(),
            json!(["DCF 跳过 · FCF、营收、净利率均无数据"]),
        );
        out.insert("assumptions".into(), a);
        return Value::Object(out);
    }

    let stage1_growth = flt(&a["stage1_growth"], 0.0);
    let stage2_growth = flt(&a["stage2_growth"], 0.0);
    let stage1_years = a["stage1_years"].as_i64().unwrap_or(DEFAULT_STAGE1_YEARS);
    let stage2_years = a["stage2_years"].as_i64().unwrap_or(DEFAULT_STAGE2_YEARS);
    let terminal_g = flt(&a["terminal_g"], 0.0);

    // Stage 1: high growth
    let mut projected_fcf: Vec<f64> = Vec::new();
    let mut year_labels: Vec<Value> = Vec::new();
    let mut cur = fcf0;
    for i in 1..=stage1_years {
        cur *= 1.0 + stage1_growth;
        projected_fcf.push(round(cur, 3));
        year_labels.push(Value::String(format!("Y{}", i)));
    }
    // Stage 2: transitional
    for i in 1..=stage2_years {
        cur *= 1.0 + stage2_growth;
        projected_fcf.push(round(cur, 3));
        year_labels.push(Value::String(format!("Y{}", stage1_years + i)));
    }

    // Discount factors
    let mut pv_fcf: Vec<f64> = Vec::new();
    for (idx, fcf) in projected_fcf.iter().enumerate() {
        let df = 1.0 / (1.0 + wacc).powf((idx + 1) as f64);
        pv_fcf.push(round(fcf * df, 3));
    }
    let pv_explicit = round(pv_fcf.iter().sum(), 3);

    // Terminal value (Gordon Growth at end of explicit period)
    let terminal_fcf = projected_fcf[projected_fcf.len() - 1] * (1.0 + terminal_g);
    let tv_at_end = if wacc - terminal_g <= 0.0 {
        0.0
    } else {
        terminal_fcf / (wacc - terminal_g)
    };
    let n_years = projected_fcf.len();
    let tv_pv = round(tv_at_end / (1.0 + wacc).powf(n_years as f64), 3);

    // Enterprise value → equity value
    let enterprise_value = round(pv_explicit + tv_pv, 3);
    // v3.9.4 · explicit net-debt-bridge-missing marker.
    let td_key_present = features.get("total_debt_yi").is_some();
    let cash_key_present = features.get("cash_yi").is_some();
    let has_debt_data = (td_key_present
        && !is_none_or_zero(uzi_core::py::get(features, "total_debt_yi")))
        || (cash_key_present && !is_none_or_zero(uzi_core::py::get(features, "cash_yi")));
    let td = flt(uzi_core::py::get(features, "total_debt_yi"), 0.0);
    let cash = flt(uzi_core::py::get(features, "cash_yi"), 0.0);
    let net_debt = td - cash;
    let equity_value = round(enterprise_value - net_debt, 3);
    let net_debt_note = if has_debt_data {
        ""
    } else {
        "（净债桥缺失 · EV≈股权价值 · 高杠杆公司会高估）"
    };

    let mut shares_yi = flt(uzi_core::py::get(features, "shares_outstanding_yi"), 0.0);
    if shares_yi <= 0.0 {
        let mc = flt(uzi_core::py::get(features, "market_cap_yi"), 0.0);
        let px = flt(uzi_core::py::get(features, "price"), 0.0);
        shares_yi = if px > 0.0 { mc / px } else { 1.0 };
    }
    let per_share = if shares_yi > 0.0 {
        num_value(round(equity_value / shares_yi, 2))
    } else {
        json!(0)
    };
    let per_share_f = if shares_yi > 0.0 {
        equity_value / shares_yi
    } else {
        0.0
    };

    // Safety margin vs. current price
    let cur_price = flt(uzi_core::py::get(features, "price"), 0.0);
    let safety_margin = if cur_price > 0.0 && per_share_f > 0.0 {
        round((per_share_f - cur_price) / cur_price * 100.0, 1)
    } else {
        0.0
    };
    let safety_margin_json = if cur_price > 0.0 && per_share_f > 0.0 {
        num_value(safety_margin)
    } else {
        json!(0)
    };

    // 5x5 sensitivity: WACC ±100bp, terminal g ±50bp
    let sensitivity = sensitivity_table(
        fcf0,
        &a,
        net_debt,
        shares_yi,
        wacc,
        terminal_g,
    );

    let tv_pct_of_ev = if enterprise_value > 0.0 {
        num_value(round(tv_pv / enterprise_value * 100.0, 1))
    } else {
        json!(0)
    };
    let tv_pv_pct_0 = if enterprise_value > 0.0 {
        round(tv_pv / enterprise_value * 100.0, 0)
    } else {
        0.0
    };

    let cost_of_equity = flt(&wacc_info["cost_of_equity"], 0.0);
    let after_tax_kd = flt(&wacc_info["after_tax_kd"], 0.0);
    let methodology_log = json!([
        format!(
            "Step 1 · WACC: CAPM k_e={:.2}%, 税后 k_d={:.2}%, 加权 WACC={:.2}%",
            cost_of_equity * 100.0,
            after_tax_kd * 100.0,
            wacc * 100.0
        ),
        format!("Step 2 · 基期 FCF={:.2} 亿", fcf0),
        format!(
            "Step 3 · 两段增长 {:.0}% ({}年) → {:.0}% ({}年)",
            stage1_growth * 100.0,
            stage1_years,
            stage2_growth * 100.0,
            stage2_years
        ),
        format!("Step 4 · 显式期 PV 合计 {:.1} 亿", pv_explicit),
        format!(
            "Step 5 · 终值 @ g={:.1}% → PV={:.1} 亿（占 EV 的 {:.0}%）",
            terminal_g * 100.0,
            tv_pv,
            tv_pv_pct_0
        ),
        format!(
            "Step 6 · EV {:.1} 亿 − 净债 {:.1} 亿 = 股权价值 {:.1} 亿{}",
            enterprise_value, net_debt, equity_value, net_debt_note
        ),
        format!(
            "Step 7 · 每股内在价值 ¥{:.2}（当前价 ¥{:.2}，安全边际 {:+.1}%）",
            per_share_f, cur_price, safety_margin
        ),
    ]);

    let mut out = Map::new();
    out.insert(
        "method".into(),
        Value::String("DCF (2-stage + Gordon Growth terminal)".into()),
    );
    out.insert("wacc_breakdown".into(), wacc_info);
    out.insert("base_fcf_yi".into(), num_value(round(fcf0, 3)));
    out.insert("projected_fcf_yi".into(), json!(projected_fcf));
    out.insert("pv_fcf_yi".into(), json!(pv_fcf));
    out.insert("year_labels".into(), Value::Array(year_labels));
    out.insert("pv_explicit_yi".into(), num_value(pv_explicit));
    // Python `tv_at_end = 0` (int) when `wacc - g <= 0`; `round(int, 3)` stays int.
    out.insert(
        "terminal_value_yi".into(),
        if wacc - terminal_g <= 0.0 {
            json!(0)
        } else {
            num_value(round(tv_at_end, 3))
        },
    );
    out.insert("tv_pv_yi".into(), num_value(tv_pv));
    out.insert("tv_pct_of_ev".into(), tv_pct_of_ev);
    out.insert("enterprise_value_yi".into(), num_value(enterprise_value));
    out.insert("net_debt_yi".into(), num_value(round(net_debt, 3)));
    out.insert("equity_value_yi".into(), num_value(equity_value));
    out.insert(
        "net_debt_bridge_note".into(),
        Value::String(net_debt_note.into()),
    );
    out.insert("shares_yi".into(), num_value(round(shares_yi, 3)));
    out.insert("intrinsic_per_share".into(), per_share);
    out.insert("current_price".into(), num_value(cur_price));
    out.insert("safety_margin_pct".into(), safety_margin_json);
    out.insert(
        "verdict".into(),
        Value::String(dcf_verdict(safety_margin).into()),
    );
    out.insert("sensitivity_table".into(), sensitivity);
    out.insert("assumptions".into(), a);
    out.insert("methodology_log".into(), methodology_log);
    Value::Object(out)
}

/// `_has_debt_data` helper: Python `v in (None, 0)` membership equality.
fn is_none_or_zero(v: &Value) -> bool {
    match v {
        Value::Null => true,
        Value::Bool(b) => !*b,
        Value::Number(n) => n.as_f64() == Some(0.0),
        _ => false,
    }
}

/// `_sensitivity_table` — 5x5 sensitivity on WACC (rows) × terminal g (cols).
fn sensitivity_table(
    fcf0: f64,
    a: &Value,
    net_debt: f64,
    shares_yi: f64,
    wacc_center: f64,
    g_center: f64,
) -> Value {
    let wacc_row = [
        wacc_center - 0.02,
        wacc_center - 0.01,
        wacc_center,
        wacc_center + 0.01,
        wacc_center + 0.02,
    ];
    let g_col = [
        g_center - 0.01,
        g_center - 0.005,
        g_center,
        g_center + 0.005,
        g_center + 0.01,
    ];
    let stage1_years = a["stage1_years"].as_i64().unwrap_or(DEFAULT_STAGE1_YEARS);
    let stage2_years = a["stage2_years"].as_i64().unwrap_or(DEFAULT_STAGE2_YEARS);
    let stage1_growth = flt(&a["stage1_growth"], 0.0);
    let stage2_growth = flt(&a["stage2_growth"], 0.0);

    let mut rows: Vec<Value> = Vec::new();
    for &w in wacc_row.iter() {
        let mut row: Vec<Value> = Vec::new();
        for &g in g_col.iter() {
            let mut cur = fcf0;
            let mut proj: Vec<f64> = Vec::new();
            for _ in 0..stage1_years {
                cur *= 1.0 + stage1_growth;
                proj.push(cur);
            }
            for _ in 0..stage2_years {
                cur *= 1.0 + stage2_growth;
                proj.push(cur);
            }
            let mut pv_exp = 0.0;
            for (i, f) in proj.iter().enumerate() {
                pv_exp += f / (1.0 + w).powf((i + 1) as f64);
            }
            let tv = if w - g > 0.0 {
                proj[proj.len() - 1] * (1.0 + g) / (w - g)
            } else {
                0.0
            };
            let tv_pv = tv / (1.0 + w).powf(proj.len() as f64);
            let ev = pv_exp + tv_pv;
            let eq = ev - net_debt;
            // Python `ps = eq / shares_yi if shares_yi > 0 else 0` (int 0).
            let ps = if shares_yi > 0.0 {
                num_value(round(eq / shares_yi, 2))
            } else {
                json!(0)
            };
            row.push(ps);
        }
        rows.push(Value::Array(row));
    }

    let wacc_axis: Vec<Value> = wacc_row
        .iter()
        .map(|w| Value::String(format!("{}%", py_str_py(&num_value(round(w * 100.0, 1))))))
        .collect();
    let g_axis: Vec<Value> = g_col
        .iter()
        .map(|g| Value::String(format!("{}%", py_str_py(&num_value(round(g * 100.0, 1))))))
        .collect();
    let center_cell = rows[2][2].clone();

    let mut out = Map::new();
    out.insert("wacc_axis".into(), Value::Array(wacc_axis));
    out.insert("g_axis".into(), Value::Array(g_axis));
    out.insert("values_per_share".into(), Value::Array(rows));
    out.insert("center_cell".into(), center_cell);
    Value::Object(out)
}

/// `_dcf_verdict`.
pub fn dcf_verdict(safety_margin: f64) -> &'static str {
    if safety_margin >= 30.0 {
        "🟢 深度低估 — 安全边际充足"
    } else if safety_margin >= 15.0 {
        "🟡 略微低估 — 可关注"
    } else if safety_margin >= -15.0 {
        "⚪ 基本合理"
    } else if safety_margin >= -30.0 {
        "🟠 略微高估"
    } else {
        "🔴 明显高估"
    }
}

/// `build_comps_table` — peer multiples benchmarking.
pub fn build_comps_table(target: &Value, peers: &[Value]) -> Value {
    let valid_peers: Vec<Value> = peers
        .iter()
        .filter(|p| p.is_object() && !same_company(target, p))
        .cloned()
        .collect();

    if valid_peers.len() < 2 {
        let mut out = Map::new();
        out.insert(
            "method".into(),
            Value::String("Comparable Company Analysis (peer multiples)".into()),
        );
        out.insert("target".into(), target.clone());
        out.insert("peers".into(), Value::Array(valid_peers.clone()));
        out.insert("peer_count".into(), json!(valid_peers.len()));
        out.insert("peer_stats".into(), Value::Object(Map::new()));
        out.insert("target_percentile".into(), Value::Object(Map::new()));
        out.insert("implied_price".into(), Value::Object(Map::new()));
        out.insert(
            "current_price".into(),
            num_value(flt(uzi_core::py::get(target, "price"), 0.0)),
        );
        out.insert(
            "valuation_verdict".into(),
            Value::String("⚪ 同行样本不足 · 无法对标".into()),
        );
        out.insert(
            "methodology_log".into(),
            json!([
                format!(
                    "Step 1 · 有效同行池 n={}（已剔除目标公司自身）",
                    valid_peers.len()
                ),
                "Step 2 · 有效同行少于 2 家，跳过分位数与估值结论",
            ]),
        );
        return Value::Object(out);
    }

    let metrics = [
        "pe",
        "pb",
        "ps",
        "ev_ebitda",
        "ev_sales",
        "roe",
        "net_margin",
        "revenue_growth",
    ];

    // Compute peer medians & quartiles
    let mut stats_map = Map::new();
    for m in metrics {
        let values: Vec<f64> = valid_peers
            .iter()
            .map(|p| flt(uzi_core::py::get(p, m), 0.0))
            .filter(|v| *v > 0.0)
            .collect();
        if values.is_empty() {
            continue;
        }
        let mut sorted = values.clone();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let q = if sorted.len() > 1 {
            stats::quantiles_exclusive_sorted(&sorted, 4)
        } else {
            vec![sorted[0], sorted[0], sorted[0]]
        };
        let mean = values.iter().sum::<f64>() / values.len() as f64;
        let mut entry = Map::new();
        entry.insert("min".into(), num_value(round(sorted[0], 2)));
        entry.insert("p25".into(), num_value(round(q[0], 2)));
        entry.insert(
            "median".into(),
            num_value(round(stats::median_sorted(&sorted), 2)),
        );
        entry.insert("p75".into(), num_value(round(q[2], 2)));
        entry.insert(
            "max".into(),
            num_value(round(sorted[sorted.len() - 1], 2)),
        );
        entry.insert("mean".into(), num_value(round(mean, 2)));
        entry.insert("n".into(), json!(values.len()));
        stats_map.insert(m.to_string(), Value::Object(entry));
    }

    // Target's percentile vs. peer universe
    let mut target_pct = Map::new();
    for (m, _s) in stats_map.iter() {
        let tv = flt(uzi_core::py::get(target, m), 0.0);
        if tv <= 0.0 {
            continue;
        }
        let mut values: Vec<f64> = valid_peers
            .iter()
            .map(|p| flt(uzi_core::py::get(p, m), 0.0))
            .filter(|v| *v > 0.0)
            .collect();
        values.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let rank = values.iter().filter(|v| **v < tv).count();
        // Python `... if values else 50` — int 50 when no peer has the metric.
        let pct = if values.is_empty() {
            json!(50)
        } else {
            num_value(round(rank as f64 / values.len() as f64 * 100.0, 0))
        };
        target_pct.insert(m.clone(), pct);
    }

    // Implied price from median multiples
    let cur_px = flt(uzi_core::py::get(target, "price"), 0.0);
    let mut implied = Map::new();
    let pe_median = stats_map
        .get("pe")
        .map(|s| flt(uzi_core::py::get(s, "median"), 0.0));
    if let Some(pe_median) = pe_median {
        if uzi_core::py::truthy(uzi_core::py::get(target, "eps")) {
            implied.insert(
                "via_median_pe".into(),
                num_value(round(
                    pe_median * flt(uzi_core::py::get(target, "eps"), 0.0),
                    2,
                )),
            );
        }
    }
    let pb_median = stats_map
        .get("pb")
        .map(|s| flt(uzi_core::py::get(s, "median"), 0.0));
    if let Some(pb_median) = pb_median {
        if uzi_core::py::truthy(uzi_core::py::get(target, "bvps")) {
            implied.insert(
                "via_median_pb".into(),
                num_value(round(
                    pb_median * flt(uzi_core::py::get(target, "bvps"), 0.0),
                    2,
                )),
            );
        }
    }

    // Valuation verdict
    let pe_pct = target_pct
        .get("pe")
        .map(|v| flt(v, 50.0))
        .unwrap_or(50.0);
    let val_verdict = if pe_pct <= 25.0 {
        "🟢 便宜（PE 低于 75% 同行）"
    } else if pe_pct <= 50.0 {
        "🟡 合理偏低"
    } else if pe_pct <= 75.0 {
        "⚪ 合理偏高"
    } else {
        "🔴 昂贵（PE 高于 75% 同行）"
    };

    let pe_median_str = stats_map
        .get("pe")
        .map(|s| uzi_core::py::get(s, "median").clone())
        .unwrap_or_else(|| Value::String("-".into()));
    let target_pe_str = target
        .get("pe")
        .cloned()
        .unwrap_or_else(|| Value::String("-".into()));
    let implied_pe_str = implied
        .get("via_median_pe")
        .cloned()
        .unwrap_or_else(|| Value::String("-".into()));

    let mut out = Map::new();
    out.insert(
        "method".into(),
        Value::String("Comparable Company Analysis (peer multiples)".into()),
    );
    out.insert("target".into(), target.clone());
    out.insert("peers".into(), Value::Array(valid_peers.clone()));
    out.insert("peer_count".into(), json!(valid_peers.len()));
    out.insert("peer_stats".into(), Value::Object(stats_map));
    out.insert(
        "target_percentile".into(),
        Value::Object(target_pct.clone()),
    );
    out.insert("implied_price".into(), Value::Object(implied));
    out.insert("current_price".into(), num_value(cur_px));
    out.insert(
        "valuation_verdict".into(),
        Value::String(val_verdict.into()),
    );
    out.insert(
        "methodology_log".into(),
        json!([
            format!(
                "Step 1 · 有效同行池 n={}（已剔除目标公司自身）",
                valid_peers.len()
            ),
            format!(
                "Step 2 · PE 中位数 {}，目标 PE {}",
                py_str_py(&pe_median_str),
                py_str_py(&target_pe_str)
            ),
            format!(
                "Step 3 · 目标 PE 分位 {}%",
                match target_pct.get("pe") {
                    Some(v) => py_str_py(v),
                    None => "50".to_string(),
                }
            ),
            format!("Step 4 · 隐含价 (中位 PE × EPS) = ¥{}", py_str_py(&implied_pe_str)),
            format!("Step 5 · 结论: {}", val_verdict),
        ]),
    );
    Value::Object(out)
}

/// `_same_company` — drop the target itself from the peer pool.
fn same_company(target: &Value, peer: &Value) -> bool {
    if uzi_core::py::truthy(uzi_core::py::get(peer, "is_self")) {
        return true;
    }
    let target_ticker = ticker_or_code(target);
    let peer_ticker = ticker_or_code(peer);
    if !target_ticker.is_empty()
        && !peer_ticker.is_empty()
        && target_ticker == peer_ticker
    {
        return true;
    }
    let target_name = str_strip(uzi_core::py::get(target, "name"));
    let peer_name = str_strip(uzi_core::py::get(peer, "name"));
    !target_name.is_empty() && !peer_name.is_empty() && target_name == peer_name
}

fn ticker_or_code(v: &Value) -> String {
    let t = str_strip(uzi_core::py::get(v, "ticker"));
    if !t.is_empty() {
        return t;
    }
    str_strip(uzi_core::py::get(v, "code"))
}

fn str_strip(v: &Value) -> String {
    match v {
        Value::Null => String::new(),
        Value::String(s) => s.trim().to_string(),
        other => py_str_py(other).trim().to_string(),
    }
}

/// `project_three_stmt` — simplified 5-year IS / BS / CF forecast, internally linked.
pub fn project_three_stmt(features: &Value, assumptions: Option<&Value>) -> Value {
    let mut a = Map::new();
    a.insert("revenue_growth_y1".into(), num_value(0.12));
    a.insert("revenue_growth_y2".into(), num_value(0.10));
    a.insert("revenue_growth_y3".into(), num_value(0.08));
    a.insert("revenue_growth_y4".into(), num_value(0.06));
    a.insert("revenue_growth_y5".into(), num_value(0.05));
    a.insert("gross_margin".into(), num_value(0.35));
    a.insert("opex_pct_revenue".into(), num_value(0.18));
    a.insert("tax_rate".into(), num_value(DEFAULT_TAX));
    a.insert("capex_pct_revenue".into(), num_value(0.05));
    a.insert("dep_pct_revenue".into(), num_value(0.04));
    a.insert("nwc_pct_revenue".into(), num_value(0.10));
    if let Some(Value::Object(extra)) = assumptions {
        for (k, v) in extra {
            a.insert(k.clone(), v.clone());
        }
    }
    let a = Value::Object(a);

    let rev0 = flt(uzi_core::py::get(features, "revenue_latest_yi"), 0.0);
    if rev0 <= 0.0 {
        let mut out = Map::new();
        out.insert("error".into(), Value::String("no base revenue".into()));
        out.insert("methodology_log".into(), json!(["缺少基期营收"]));
        return Value::Object(out);
    }

    let growth = [
        flt(&a["revenue_growth_y1"], 0.0),
        flt(&a["revenue_growth_y2"], 0.0),
        flt(&a["revenue_growth_y3"], 0.0),
        flt(&a["revenue_growth_y4"], 0.0),
        flt(&a["revenue_growth_y5"], 0.0),
    ];
    let gross_margin = flt(&a["gross_margin"], 0.0);
    let opex_pct = flt(&a["opex_pct_revenue"], 0.0);
    let tax_rate = flt(&a["tax_rate"], 0.0);
    let capex_pct = flt(&a["capex_pct_revenue"], 0.0);
    let dep_pct = flt(&a["dep_pct_revenue"], 0.0);
    let nwc_pct = flt(&a["nwc_pct_revenue"], 0.0);

    // Income statement
    let mut rev: Vec<f64> = Vec::new();
    let mut cogs: Vec<f64> = Vec::new();
    let mut gross: Vec<f64> = Vec::new();
    let mut opex: Vec<f64> = Vec::new();
    let mut ebit: Vec<f64> = Vec::new();
    let mut tax: Vec<f64> = Vec::new();
    let mut ni: Vec<f64> = Vec::new();
    let mut prev_rev = rev0;
    for &g in growth.iter() {
        let r = prev_rev * (1.0 + g);
        let c = r * (1.0 - gross_margin);
        let gp = r - c;
        let op = r * opex_pct;
        let e = gp - op;
        let t = e * tax_rate;
        let n = e - t;
        rev.push(round(r, 2));
        cogs.push(round(c, 2));
        gross.push(round(gp, 2));
        opex.push(round(op, 2));
        ebit.push(round(e, 2));
        tax.push(round(t, 2));
        ni.push(round(n, 2));
        prev_rev = r;
    }

    // Cash flow — simplified
    let dep: Vec<f64> = rev.iter().map(|r| round(r * dep_pct, 2)).collect();
    let capex: Vec<f64> = rev.iter().map(|r| round(r * capex_pct, 2)).collect();
    let mut nwc_chg: Vec<f64> = Vec::new();
    for i in 0..rev.len() {
        let prev = if i > 0 { rev[i - 1] } else { rev0 };
        nwc_chg.push(round((rev[i] - prev) * nwc_pct, 2));
    }
    let ocf: Vec<f64> = (0..rev.len())
        .map(|i| round(ni[i] + dep[i] - nwc_chg[i], 2))
        .collect();
    let fcf: Vec<f64> = (0..rev.len()).map(|i| round(ocf[i] - capex[i], 2)).collect();

    // Simplified balance sheet evolution
    let mut equity0 = flt(uzi_core::py::get(features, "equity_yi"), 0.0);
    if equity0 <= 0.0 {
        let pb = flt(
            &features
                .get("pb")
                .cloned()
                .unwrap_or(Value::Null),
            2.0,
        );
        equity0 = flt(uzi_core::py::get(features, "market_cap_yi"), 0.0) / pb.max(0.1);
    }
    let mut equity_series: Vec<f64> = Vec::new();
    let mut eq = equity0;
    for &n in ni.iter() {
        eq += n;
        equity_series.push(round(eq, 2));
    }

    let growth_path: Vec<String> = growth
        .iter()
        .map(|g| format!("{:.0}%", g * 100.0))
        .collect();

    let mut income_statement = Map::new();
    income_statement.insert("revenue".into(), json!(rev));
    income_statement.insert("cogs".into(), json!(cogs));
    income_statement.insert("gross_profit".into(), json!(gross));
    income_statement.insert("opex".into(), json!(opex));
    income_statement.insert("ebit".into(), json!(ebit));
    income_statement.insert("tax".into(), json!(tax));
    income_statement.insert("net_income".into(), json!(ni));

    let mut cash_flow = Map::new();
    cash_flow.insert("net_income".into(), json!(ni));
    cash_flow.insert("dep_amort".into(), json!(dep));
    cash_flow.insert("nwc_change".into(), json!(nwc_chg));
    cash_flow.insert("ocf".into(), json!(ocf));
    cash_flow.insert("capex".into(), json!(capex));
    cash_flow.insert("fcf".into(), json!(fcf));

    let mut balance_sheet = Map::new();
    balance_sheet.insert("equity_rollforward".into(), json!(equity_series));

    let mut out = Map::new();
    out.insert(
        "method".into(),
        Value::String("3-Statement Projection (5-year, linked)".into()),
    );
    out.insert(
        "years".into(),
        json!(["Y1", "Y2", "Y3", "Y4", "Y5"]),
    );
    out.insert("income_statement".into(), Value::Object(income_statement));
    out.insert("cash_flow".into(), Value::Object(cash_flow));
    out.insert("balance_sheet".into(), Value::Object(balance_sheet));
    out.insert("assumptions".into(), a);
    out.insert("growth_path".into(), json!(growth_path));
    out.insert(
        "methodology_log".into(),
        json!([
            format!(
                "Step 1 · 基期营收 {:.1} 亿 · 5 年增速路径 {}",
                rev0,
                crate::py_list_str_repr(&growth_path)
            ),
            format!(
                "Step 2 · 毛利率假设 {:.0}% · 运营费率 {:.0}%",
                gross_margin * 100.0,
                opex_pct * 100.0
            ),
            format!(
                "Step 3 · Y5 营收 {:.1} 亿 · 净利 {:.1} 亿",
                rev[rev.len() - 1],
                ni[ni.len() - 1]
            ),
            format!("Step 4 · 5 年累计 FCF {:.1} 亿", fcf.iter().sum::<f64>()),
        ]),
    );
    Value::Object(out)
}

/// `quick_lbo` with the upstream default assumptions.
pub fn quick_lbo(features: &Value) -> Value {
    quick_lbo_with(features, 8.0, 5.0, 8.0, 5, 0.08, 0.06)
}

/// `quick_lbo` — private-equity style quick LBO test.
#[allow(clippy::too_many_arguments)]
pub fn quick_lbo_with(
    features: &Value,
    entry_multiple: f64,
    debt_multiple: f64,
    exit_multiple: f64,
    hold_years: i64,
    ebitda_growth: f64,
    interest_rate: f64,
) -> Value {
    // Infer EBITDA from features or approximate
    let mut ebitda = flt(uzi_core::py::get(features, "ebitda_yi"), 0.0);
    if ebitda <= 0.0 {
        let rev = flt(uzi_core::py::get(features, "revenue_latest_yi"), 0.0);
        let nm = flt(uzi_core::py::get(features, "net_margin"), 0.0) / 100.0;
        let ni = rev * nm;
        ebitda = if ni > 0.0 { ni / 0.6 } else { rev * 0.15 };
    }

    let entry_ev = entry_multiple * ebitda;
    let entry_debt = debt_multiple * ebitda;
    let entry_equity = entry_ev - entry_debt;

    // Project EBITDA
    let mut path: Vec<f64> = Vec::new();
    let mut cur = ebitda;
    for _ in 1..=hold_years {
        cur *= 1.0 + ebitda_growth;
        path.push(round(cur, 2));
    }

    // Debt paydown (assume 30% of FCF paid down annually, FCF ≈ 50% of EBITDA)
    //
    // Python `max(0, x)` returns the *int* 0 when `x <= 0` (the first argument
    // wins on ties, including `max(0, 0.0)`), and `round(0, 2)` on an int stays
    // `0`. Track that so the JSON serialises `0` rather than `0.0` for every
    // post-paydown entry, exactly like upstream.
    let mut debt = entry_debt;
    let mut debt_schedule: Vec<Value> = vec![num_value(round(debt, 2))];
    for &y_ebitda in path.iter() {
        let interest = debt * interest_rate;
        let fcf = y_ebitda * 0.5 - interest;
        let paydown = (fcf * 0.7).max(0.0);
        let remaining = debt - paydown;
        if remaining <= 0.0 {
            debt = 0.0;
            debt_schedule.push(json!(0));
        } else {
            debt = remaining;
            debt_schedule.push(num_value(round(debt, 2)));
        }
    }

    // Exit
    let exit_ebitda = path[path.len() - 1];
    let exit_ev = exit_multiple * exit_ebitda;
    let exit_debt = debt;
    let exit_equity = exit_ev - exit_debt;

    // Returns
    let returns_taken = entry_equity > 0.0 && exit_equity > 0.0;
    let (moic, irr) = if returns_taken {
        let moic = exit_equity / entry_equity;
        let irr = moic.powf(1.0 / hold_years as f64) - 1.0;
        (moic, irr)
    } else {
        (0.0, 0.0)
    };
    // Python keeps `moic = 0` / `irr = 0` as ints when the branch is not taken,
    // so `round(...)` stays an int in the JSON.
    let moic_json = if returns_taken {
        num_value(round(moic, 2))
    } else {
        json!(0)
    };
    let irr_pct_json = if returns_taken {
        num_value(round(irr * 100.0, 1))
    } else {
        json!(0)
    };

    let verdict = if irr >= 0.20 {
        "🟢 PE 买方可赚 20%+ IRR"
    } else if irr >= 0.15 {
        "🟡 PE 买方 15-20% IRR"
    } else {
        "🔴 低于 PE 收益门槛"
    };

    let mut out = Map::new();
    out.insert("method".into(), Value::String("Quick LBO Test".into()));
    out.insert("entry_ebitda_yi".into(), num_value(round(ebitda, 2)));
    out.insert("entry_multiple".into(), num_value(entry_multiple));
    out.insert("entry_ev_yi".into(), num_value(round(entry_ev, 2)));
    out.insert("entry_debt_yi".into(), num_value(round(entry_debt, 2)));
    out.insert("entry_equity_yi".into(), num_value(round(entry_equity, 2)));
    out.insert("leverage_turns".into(), num_value(debt_multiple));
    out.insert("ebitda_path".into(), json!(path));
    out.insert("debt_schedule".into(), Value::Array(debt_schedule));
    out.insert("exit_ebitda_yi".into(), num_value(round(exit_ebitda, 2)));
    out.insert("exit_multiple".into(), num_value(exit_multiple));
    out.insert("exit_ev_yi".into(), num_value(round(exit_ev, 2)));
    out.insert("exit_equity_yi".into(), num_value(round(exit_equity, 2)));
    out.insert("moic".into(), moic_json);
    out.insert("irr_pct".into(), irr_pct_json);
    out.insert("pass_pe_test".into(), Value::Bool(irr >= 0.20));
    out.insert("verdict".into(), Value::String(verdict.into()));
    out.insert(
        "methodology_log".into(),
        json!([
            format!(
                "Step 1 · 入场 EBITDA {:.1} 亿 × {}x = EV {:.1} 亿",
                ebitda,
                py_str_py(&num_value(entry_multiple)),
                entry_ev
            ),
            format!(
                "Step 2 · {}x 杠杆 → 债 {:.1} 亿 + 股本 {:.1} 亿",
                py_str_py(&num_value(debt_multiple)),
                entry_debt,
                entry_equity
            ),
            format!(
                "Step 3 · {} 年 {:.0}% 成长 → Y{} EBITDA {:.1} 亿",
                hold_years,
                ebitda_growth * 100.0,
                hold_years,
                exit_ebitda
            ),
            format!(
                "Step 4 · 退出 {}x × {:.1} = {:.1} 亿 EV",
                py_str_py(&num_value(exit_multiple)),
                exit_ebitda,
                exit_ev
            ),
            format!(
                "Step 5 · 退出股权 {:.1} 亿 / 入场股权 {:.1} 亿 = {:.2}x MOIC ({:.1}% IRR)",
                exit_equity,
                entry_equity,
                moic,
                irr * 100.0
            ),
        ]),
    );
    Value::Object(out)
}

/// `accretion_dilution` with the upstream default assumptions.
pub fn accretion_dilution(acquirer: &Value, target: &Value) -> Value {
    accretion_dilution_with(acquirer, target, 0.30, 0.50, 0.0, 0.05)
}

/// `accretion_dilution` — merger model pro-forma EPS impact.
pub fn accretion_dilution_with(
    acquirer: &Value,
    target: &Value,
    premium_pct: f64,
    cash_pct: f64,
    synergies_yi: f64,
    new_debt_rate: f64,
) -> Value {
    let a_px = flt(uzi_core::py::get(acquirer, "price"), 0.0);
    let a_shares = flt(uzi_core::py::get(acquirer, "shares_yi"), 0.0);
    let a_eps = flt(uzi_core::py::get(acquirer, "eps"), 0.0);
    let a_ni = flt(uzi_core::py::get(acquirer, "net_income_yi"), 0.0);

    let t_px = flt(uzi_core::py::get(target, "price"), 0.0);
    let t_shares = flt(uzi_core::py::get(target, "shares_yi"), 0.0);
    let t_ni = flt(uzi_core::py::get(target, "net_income_yi"), 0.0);

    let offer_px = t_px * (1.0 + premium_pct);
    let equity_value = offer_px * t_shares;
    let cash_needed = equity_value * cash_pct;
    let stock_needed = equity_value * (1.0 - cash_pct);

    let new_shares_taken = a_px > 0.0;
    let new_shares_issued = if new_shares_taken { stock_needed / a_px } else { 0.0 };
    let after_shares = a_shares + new_shares_issued;

    let after_tax_interest = cash_needed * new_debt_rate * (1.0 - DEFAULT_TAX);
    let pro_forma_ni = a_ni + t_ni + synergies_yi - after_tax_interest;
    let eps_taken = after_shares > 0.0;
    let pro_forma_eps = if eps_taken { pro_forma_ni / after_shares } else { 0.0 };
    let accretion_taken = a_eps > 0.0;
    let accretion = if accretion_taken {
        (pro_forma_eps - a_eps) / a_eps * 100.0
    } else {
        0.0
    };

    let verdict = if accretion > 3.0 {
        "🟢 增厚"
    } else if (-3.0..=3.0).contains(&accretion) {
        "⚪ 中性"
    } else {
        "🔴 摊薄"
    };

    let mut out = Map::new();
    out.insert("method".into(), Value::String("Accretion/Dilution".into()));
    out.insert("offer_price".into(), num_value(round(offer_px, 2)));
    out.insert("equity_value_yi".into(), num_value(round(equity_value, 2)));
    out.insert("cash_portion_yi".into(), num_value(round(cash_needed, 2)));
    out.insert("stock_portion_yi".into(), num_value(round(stock_needed, 2)));
    // Python keeps the int `0` from the untaken branches, so `round(0, n)` stays
    // an int in the JSON.
    out.insert(
        "new_shares_issued_yi".into(),
        if new_shares_taken {
            num_value(round(new_shares_issued, 3))
        } else {
            json!(0)
        },
    );
    out.insert("pro_forma_shares_yi".into(), num_value(round(after_shares, 3)));
    out.insert("pro_forma_ni_yi".into(), num_value(round(pro_forma_ni, 2)));
    out.insert(
        "pro_forma_eps".into(),
        if eps_taken {
            num_value(round(pro_forma_eps, 3))
        } else {
            json!(0)
        },
    );
    out.insert("standalone_eps".into(), num_value(round(a_eps, 3)));
    out.insert(
        "accretion_pct".into(),
        if accretion_taken {
            num_value(round(accretion, 1))
        } else {
            json!(0)
        },
    );
    out.insert("verdict".into(), Value::String(verdict.into()));
    out.insert(
        "methodology_log".into(),
        json!([
            format!(
                "Step 1 · 报价 ¥{:.2}（溢价 {:.0}%）→ 总对价 {:.1} 亿",
                offer_px,
                premium_pct * 100.0,
                equity_value
            ),
            format!(
                "Step 2 · 现金 {:.0}% = {:.1} 亿; 换股 {:.1} 亿 → 新增 {:.2} 亿股",
                cash_pct * 100.0,
                cash_needed,
                stock_needed,
                new_shares_issued
            ),
            format!(
                "Step 3 · 合并 NI = 收购方 {:.1} + 标的 {:.1} + 协同 {:.1} − 利息 {:.1} = {:.1}",
                a_ni, t_ni, synergies_yi, after_tax_interest, pro_forma_ni
            ),
            format!(
                "Step 4 · Pro-forma EPS = {:.3}（vs 独立 {:.3}，{} {:.1}%）",
                pro_forma_eps,
                a_eps,
                if accretion > 0.0 { "增厚" } else { "摊薄" },
                accretion.abs()
            ),
        ]),
    );
    Value::Object(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn wacc_defaults_match_capm_arithmetic() {
        // rf 2.5% + 1.0 × 6% = 8.5%; after-tax kd 4.5% × 0.75 = 3.375% → 3.38% (round 4dp)
        // WACC = 0.7 × 0.085 + 0.3 × 0.03375 = 0.069625 → 0.0696
        let w = compute_wacc_default();
        assert_eq!(w["cost_of_equity"], json!(0.085));
        assert_eq!(w["after_tax_kd"], json!(0.0338));
        assert_eq!(w["wacc"], json!(0.0696));
        assert_eq!(w["equity_weight"], json!(0.7));
    }

    #[test]
    fn dcf_without_fcf_revenue_or_margin_reports_insufficient_data() {
        let out = compute_dcf(&json!({"price": 10.0}), None);
        assert_eq!(out["verdict"], json!("⛔ 数据不足 · 无法 DCF"));
        assert_eq!(out["intrinsic_per_share"], Value::Null);
        assert_eq!(out["safety_margin_pct"], Value::Null);
    }

    #[test]
    fn comps_below_two_peers_skips_percentiles() {
        let target = json!({"name": "T", "price": 10.0, "pe": 20.0});
        let out = build_comps_table(&target, &[json!({"name": "T", "pe": 30.0})]);
        assert_eq!(out["peer_count"], json!(0));
        assert_eq!(out["valuation_verdict"], json!("⚪ 同行样本不足 · 无法对标"));
        assert_eq!(out["peer_stats"], json!({}));
    }

    #[test]
    fn lbo_verdict_thresholds_follow_irr() {
        // ebitda 100, 8x entry, 5x debt → equity 300; 5y 0% growth, 8x exit:
        // debt pays down, MOIC > 1 → IRR well above 20%
        let out = quick_lbo(&json!({"ebitda_yi": 100.0}));
        assert_eq!(out["pass_pe_test"], json!(true));
        assert_eq!(out["verdict"], json!("🟢 PE 买方可赚 20%+ IRR"));
        // zero ebitda & zero revenue → no equity → no return
        let zero = quick_lbo(&json!({}));
        assert_eq!(zero["irr_pct"], json!(0));
        assert_eq!(zero["verdict"], json!("🔴 低于 PE 收益门槛"));
    }
}
