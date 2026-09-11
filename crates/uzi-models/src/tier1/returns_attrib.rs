//! Port of `lib/tier1/returns_attrib.py` — secondary-market portfolio returns
//! attribution (weight × return decomposition, sector / school grouping).

use serde_json::{json, Map, Value};
use uzi_core::py::round;

/// `_num(v, 0.0)` shorthand for JSON values already known numeric.
fn pnum1(v: &Value) -> f64 {
    crate::pnum(v, 0.0)
}

/// `_num(v, default=None)` — `None` distinguishes "missing" from `0`.
fn opt_num(v: &Value) -> Option<f64> {
    if v.is_null() {
        return None;
    }
    match v {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => {
            let cleaned: String = s.chars().filter(|c| *c != '%' && *c != ',').collect();
            cleaned.trim().parse::<f64>().ok()
        }
        _ => None,
    }
}

/// `_group_attribution` — aggregate contributions by `key`, descending.
fn group_attribution(rows: &[Value], key: &str) -> Value {
    let mut order: Vec<String> = Vec::new();
    let mut buckets: Map<String, Value> = Map::new();
    for c in rows.iter() {
        let gk_raw = uzi_core::py::get(c, key);
        let gk = if uzi_core::py::truthy(gk_raw) {
            crate::py_str_py(gk_raw)
        } else {
            "未分类".to_string()
        };
        if !buckets.contains_key(&gk) {
            order.push(gk.clone());
            let mut b = Map::new();
            b.insert(key.to_string(), Value::String(gk.clone()));
            b.insert("weight".into(), json!(0.0));
            b.insert("contribution_pct".into(), json!(0.0));
            b.insert("n".into(), json!(0));
            b.insert("needs_price_n".into(), json!(0));
            buckets.insert(gk.clone(), Value::Object(b));
        }
        if let Some(Value::Object(b)) = buckets.get_mut(&gk) {
            let w = pnum1(uzi_core::py::get(c, "weight"));
            let cp = pnum1(uzi_core::py::get(c, "contribution_pct"));
            let w_prev = pnum1(b.get("weight").unwrap_or(&Value::Null));
            let cp_prev = pnum1(b.get("contribution_pct").unwrap_or(&Value::Null));
            let n_prev = b.get("n").and_then(|v| v.as_i64()).unwrap_or(0);
            let np_prev = b.get("needs_price_n").and_then(|v| v.as_i64()).unwrap_or(0);
            let needs = uzi_core::py::truthy(uzi_core::py::get(c, "needs_price"));
            b.insert("weight".into(), crate::num_value(w_prev + w));
            b.insert("contribution_pct".into(), crate::num_value(cp_prev + cp));
            b.insert("n".into(), json!(n_prev + 1));
            b.insert("needs_price_n".into(), json!(np_prev + if needs { 1 } else { 0 }));
        }
    }
    let mut out: Vec<Value> = order
        .iter()
        .map(|k| {
            let mut b = buckets[k].clone();
            if let Value::Object(m) = &mut b {
                let w = pnum1(m.get("weight").unwrap_or(&Value::Null));
                let cp = pnum1(m.get("contribution_pct").unwrap_or(&Value::Null));
                m.insert("weight".into(), crate::num_value(round(w, 4)));
                m.insert("contribution_pct".into(), crate::num_value(round(cp, 3)));
            }
            b
        })
        .collect();
    out.sort_by(|a, b| {
        let ca = pnum1(uzi_core::py::get(a, "contribution_pct"));
        let cb = pnum1(uzi_core::py::get(b, "contribution_pct"));
        cb.partial_cmp(&ca).unwrap_or(std::cmp::Ordering::Equal)
    });
    Value::Array(out)
}

/// `_one_liner`.
fn one_liner(
    total: f64,
    top_c: &[Value],
    top_d: &[Value],
    benchmark: Option<&Value>,
    missing: &[String],
) -> String {
    let head = if total >= 0.0 {
        format!("组合区间总收益 {:+.2}%", total)
    } else {
        format!("组合区间总收益 {:+.2}%（下跌）", total)
    };
    let mut parts = vec![head];
    if let Some(c) = top_c.first() {
        parts.push(format!(
            "主升由 {} 贡献 {:+.2}pp",
            crate::py_str_py(uzi_core::py::get(c, "name")),
            pnum1(uzi_core::py::get(c, "contribution_pct"))
        ));
    }
    if let Some(d) = top_d.first() {
        parts.push(format!(
            "主要拖累 {} {:+.2}pp",
            crate::py_str_py(uzi_core::py::get(d, "name")),
            pnum1(uzi_core::py::get(d, "contribution_pct"))
        ));
    }
    if let Some(b) = benchmark {
        parts.push(format!(
            "{}基准 {:+.2}pp",
            if uzi_core::py::truthy(uzi_core::py::get(b, "outperform")) {
                "跑赢"
            } else {
                "跑输"
            },
            pnum1(uzi_core::py::get(b, "excess_return_pct"))
        ));
    }
    if !missing.is_empty() {
        parts.push(format!("⚠️ {} 只缺区间收益需补价格", missing.len()));
    }
    format!("{}。", parts.join("，"))
}

/// `build_returns_attribution` — portfolio returns attribution.
pub fn build_returns_attribution(holdings: &Value, benchmark_return: Option<f64>) -> Value {
    let mut log: Vec<Value> = Vec::new();
    let holdings_arr: Vec<Value> = match holdings.as_array() {
        Some(a) if !a.is_empty() => a.clone(),
        _ => {
            return json!({
                "method": "Returns Attribution (二级市场组合)",
                "error": "holdings 为空",
                "total_return": 0.0,
                "contribution_table": [],
                "sector_attribution": [],
                "school_attribution": [],
                "top_contributors": [],
                "top_detractors": [],
                "benchmark": null,
                "methodology_log": ["Step 0 · holdings 为空，无法归因"],
            });
        }
    };
    let n = holdings_arr.len();

    // Step 1 · weight normalization
    let mut raw_weights: Vec<Option<f64>> = Vec::new();
    for h in holdings_arr.iter() {
        let mut w = opt_num(uzi_core::py::get(h, "weight"));
        if let Some(v) = w {
            if v > 1.0 {
                w = Some(v / 100.0);
            }
        }
        raw_weights.push(w);
    }
    let weighted_idx: Vec<usize> = (0..n).filter(|i| raw_weights[*i].is_some()).collect();
    let norm_weights: Vec<f64> = if weighted_idx.is_empty() {
        let eq = 1.0 / n as f64;
        log.push(Value::String(format!(
            "Step 1 · 全部 {} 只无权重 → 等权 {:.3}",
            n, eq
        )));
        vec![eq; n]
    } else {
        let mut total_w: f64 = weighted_idx.iter().map(|i| raw_weights[*i].unwrap()).sum();
        let unweighted: Vec<usize> = (0..n).filter(|i| raw_weights[*i].is_none()).collect();
        let mut filled = raw_weights.clone();
        if !unweighted.is_empty() {
            let remain = (1.0 - total_w).max(0.0);
            let share = if remain > 0.0 {
                remain / unweighted.len() as f64
            } else {
                0.0
            };
            for i in unweighted.iter() {
                filled[*i] = Some(share);
            }
            total_w = (0..n).map(|i| filled[i].unwrap_or(0.0)).sum();
        }
        log.push(Value::String(format!(
            "Step 1 · 权重归一化 · {}/{} 只带权重 · {} 只均分剩余",
            weighted_idx.len(),
            n,
            unweighted.len()
        )));
        (0..n)
            .map(|i| {
                if total_w > 0.0 {
                    filled[i].unwrap_or(0.0) / total_w
                } else {
                    0.0
                }
            })
            .collect()
    };

    // Step 2 · per-holding contribution = weight × return
    let mut contribution_table: Vec<Value> = Vec::new();
    let mut missing: Vec<String> = Vec::new();
    let mut total_return = 0.0f64;
    for (i, h) in holdings_arr.iter().enumerate() {
        let ticker = crate::py_str_py(&crate::get_or(
            h,
            "ticker",
            json!(format!("#{}", i + 1)),
        ))
        .trim()
        .to_string();
        let name = {
            let nm = uzi_core::py::get(h, "name");
            if uzi_core::py::truthy(nm) {
                nm.clone()
            } else {
                let note = uzi_core::py::get(h, "note");
                if uzi_core::py::truthy(note) {
                    note.clone()
                } else {
                    Value::String(ticker.clone())
                }
            }
        };
        let w = norm_weights[i];
        let ret_raw = uzi_core::py::get(h, "return_pct");
        let ret = opt_num(ret_raw);
        let (ret_val, need_price) = match ret {
            Some(r) => (r, false),
            None => {
                missing.push(ticker.clone());
                (0.0, true)
            }
        };
        let contrib = w * ret_val;
        total_return += contrib;
        let industry = {
            let ind = uzi_core::py::get(h, "industry");
            if uzi_core::py::truthy(ind) {
                ind.clone()
            } else {
                json!("未分类")
            }
        };
        let note = if need_price {
            json!("需补价格区间")
        } else {
            let nt = uzi_core::py::get(h, "note");
            if uzi_core::py::truthy(nt) {
                nt.clone()
            } else {
                json!("")
            }
        };
        contribution_table.push(json!({
            "ticker": ticker,
            "name": name,
            "industry": industry,
            "school": uzi_core::py::get(h, "school").clone(),
            "weight": crate::num_value(round(w, 4)),
            "return_pct": if need_price { Value::Null } else { crate::num_value(round(ret_val, 2)) },
            "contribution_pct": crate::num_value(round(contrib, 3)),
            "needs_price": need_price,
            "note": note,
        }));
    }
    let total_return = round(total_return, 3);
    log.push(Value::String(format!(
        "Step 2 · 逐持仓贡献 Σ(权重×收益) = {:+.2}pp{}",
        total_return,
        if missing.is_empty() {
            String::new()
        } else {
            format!(" · {} 只缺区间收益(按0计)", missing.len())
        }
    )));

    // Step 3 · sector attribution
    let sector_attribution = group_attribution(&contribution_table, "industry");
    let sector_sum: f64 = sector_attribution
        .as_array()
        .unwrap()
        .iter()
        .map(|s| pnum1(uzi_core::py::get(s, "contribution_pct")))
        .sum();
    log.push(Value::String(format!(
        "Step 3 · 行业归因 · {} 个行业 · 贡献分组加总 = {:+.2}pp",
        sector_attribution.as_array().unwrap().len(),
        sector_sum
    )));

    // Step 3b · school attribution (only when at least one holding has a school)
    let has_school = contribution_table
        .iter()
        .any(|c| uzi_core::py::truthy(uzi_core::py::get(c, "school")));
    let school_attribution = if has_school {
        group_attribution(&contribution_table, "school")
    } else {
        Value::Array(Vec::new())
    };
    if has_school {
        log.push(Value::String(format!(
            "Step 3b · 流派归因 · {} 个流派",
            school_attribution.as_array().unwrap().len()
        )));
    }

    // Step 4 · top contributors / detractors (only holdings with return data)
    let scored: Vec<Value> = contribution_table
        .iter()
        .filter(|c| !uzi_core::py::truthy(uzi_core::py::get(c, "needs_price")))
        .cloned()
        .collect();
    let mut by_contrib = scored.clone();
    by_contrib.sort_by(|a, b| {
        let ca = pnum1(uzi_core::py::get(a, "contribution_pct"));
        let cb = pnum1(uzi_core::py::get(b, "contribution_pct"));
        cb.partial_cmp(&ca).unwrap_or(std::cmp::Ordering::Equal)
    });
    let top_contributors: Vec<Value> = by_contrib
        .iter()
        .filter(|c| pnum1(uzi_core::py::get(c, "contribution_pct")) > 0.0)
        .take(3)
        .cloned()
        .collect();
    let top_detractors: Vec<Value> = by_contrib
        .iter()
        .rev()
        .filter(|c| pnum1(uzi_core::py::get(c, "contribution_pct")) < 0.0)
        .take(3)
        .cloned()
        .collect();
    log.push(Value::String(format!(
        "Step 4 · Top {} 贡献 / Top {} 拖累",
        top_contributors.len(),
        top_detractors.len()
    )));

    // Step 5 · benchmark excess
    let benchmark = benchmark_return.map(|b| {
        let excess = round(total_return - b, 3);
        json!({
            "benchmark_return_pct": crate::num_value(round(b, 2)),
            "excess_return_pct": crate::num_value(excess),
            "outperform": excess > 0.0,
        })
    });
    if let (Some(b), Some(bench)) = (benchmark_return, benchmark.as_ref()) {
        let excess = pnum1(uzi_core::py::get(bench, "excess_return_pct"));
        log.push(Value::String(format!(
            "Step 5 · vs 基准 {:+.2}% → 超额 {:+.2}pp ({})",
            b,
            excess,
            if excess > 0.0 { "跑赢" } else { "跑输" }
        )));
    }

    // One-line verdict
    let verdict = one_liner(
        total_return,
        &top_contributors,
        &top_detractors,
        benchmark.as_ref(),
        &missing,
    );

    json!({
        "method": "Returns Attribution (二级市场组合)",
        "source": "改编自 anthropics/financial-services · private-equity/returns-analysis",
        "n_holdings": n,
        "n_missing_return": missing.len(),
        "missing_return_tickers": missing,
        "total_return": crate::num_value(total_return),
        "contribution_table": contribution_table,
        "sector_attribution": sector_attribution,
        "school_attribution": school_attribution,
        "top_contributors": top_contributors,
        "top_detractors": top_detractors,
        "benchmark": benchmark.unwrap_or(Value::Null),
        "verdict": verdict,
        "methodology_log": log,
    })
}
