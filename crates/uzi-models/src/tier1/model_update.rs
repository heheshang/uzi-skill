//! Port of `lib/tier1/model_update.py` — incrementally update a financial model
//! with new assumptions and compute before → after deltas.

use crate::{dim_data, get_or, pnum_yen, py_str_py};
use serde_json::{json, Map, Value};
use uzi_core::py::round;

use crate::clock;

/// `_ASSUMPTION_SPECS` — key → (label, unit, before-source, impact channel).
struct Spec {
    key: &'static str,
    label: &'static str,
    unit: &'static str,
    channel: &'static str,
    from_after: Option<&'static str>,
    from_before: Option<&'static str>,
}

const SPECS: &[Spec] = &[
    Spec {
        key: "rev_growth",
        label: "营收增速",
        unit: "pct",
        channel: "dcf",
        from_after: Some("revenue_growth_latest"),
        from_before: Some("revenue_growth_3y_cagr"),
    },
    Spec {
        key: "gross_margin",
        label: "毛利率",
        unit: "pct",
        channel: "dcf",
        from_after: Some("gross_margin"),
        from_before: None,
    },
    Spec {
        key: "net_margin",
        label: "净利率",
        unit: "pct",
        channel: "both",
        from_after: Some("net_margin"),
        from_before: None,
    },
    Spec {
        key: "capex_pct",
        label: "Capex/营收",
        unit: "pct",
        channel: "dcf",
        from_after: None,
        from_before: None,
    },
    Spec {
        key: "stage1_growth",
        label: "DCF Stage1 增速",
        unit: "pct",
        channel: "dcf",
        from_after: None,
        from_before: None,
    },
    Spec {
        key: "terminal_g",
        label: "DCF 终值 g",
        unit: "pct",
        channel: "dcf",
        from_after: None,
        from_before: None,
    },
    Spec {
        key: "beta",
        label: "Beta",
        unit: "x",
        channel: "dcf",
        from_after: None,
        from_before: None,
    },
    Spec {
        key: "target_pe",
        label: "目标 PE",
        unit: "x",
        channel: "comps",
        from_after: Some("pe"),
        from_before: None,
    },
    Spec {
        key: "target_price",
        label: "目标价",
        unit: "price",
        channel: "comps",
        from_after: Some("target_price_avg"),
        from_before: Some("price"),
    },
];

fn spec(key: &str) -> Option<&'static Spec> {
    SPECS.iter().find(|s| s.key == key)
}

/// `_fmt`.
fn fmt(unit: &str, v: f64) -> String {
    match unit {
        "pct" => format!("{:.1}%", v),
        "x" => format!("{:.2}x", v),
        "price" => format!("¥{:.2}", v),
        _ => format!("{:.2}", v),
    }
}

/// `_delta_dir`.
fn delta_dir(after: f64, before: f64) -> &'static str {
    if after > before + 1e-9 {
        "↑"
    } else if after < before - 1e-9 {
        "↓"
    } else {
        "→"
    }
}

/// `_infer_before_after`.
fn infer_before_after(spec: &Spec, features: &Value, after_raw: Option<&Value>) -> (f64, f64) {
    let mut before = match spec.from_before {
        Some(k) => pnum_yen(uzi_core::py::get(features, k), 0.0),
        None => 0.0,
    };
    if let Some(after_raw) = after_raw {
        let after = pnum_yen(after_raw, 0.0);
        if let Some(src_after) = spec.from_after {
            if before == 0.0 {
                before = pnum_yen(uzi_core::py::get(features, src_after), 0.0);
            }
            if spec.from_before.is_none() {
                before = pnum_yen(uzi_core::py::get(features, src_after), 0.0);
            }
        }
        return (before, after);
    }
    let after = match spec.from_after {
        Some(k) => pnum_yen(uzi_core::py::get(features, k), 0.0),
        None => 0.0,
    };
    if spec.from_before.is_none() {
        before = round(after * 0.95, 3);
    }
    (before, after)
}

/// `_reprice_dcf` — first-order DCF re-pricing from the delta table.
fn reprice_dcf(dcf_result: &Value, deltas: &[Value], features: &Value) -> Value {
    let before_ps = pnum_yen(uzi_core::py::get(dcf_result, "intrinsic_per_share"), 0.0);
    if before_ps <= 0.0 {
        return json!({"available": false, "reason": "dcf_result 无有效 intrinsic_per_share"});
    }

    let a = get_or(dcf_result, "assumptions", json!({}));
    let a = if uzi_core::py::truthy(&a) { a } else { json!({}) };
    let g1_before = pnum_yen(uzi_core::py::get(&a, "stage1_growth"), 0.0);
    let tg_before = pnum_yen(uzi_core::py::get(&a, "terminal_g"), 0.0);

    let mut fcf_scale = 1.0f64;
    let mut g1_after = g1_before;
    let mut tg_after = tg_before;
    let beta0 = pnum_yen(uzi_core::py::get(&a, "beta"), 0.0);
    let mut beta_after = if beta0 != 0.0 { beta0 } else { 1.0 };
    let mut notes: Vec<String> = Vec::new();

    for d in deltas {
        let key = uzi_core::py::get(d, "key").as_str().unwrap_or("");
        let after = pnum_yen(uzi_core::py::get(d, "after"), 0.0);
        let before = pnum_yen(uzi_core::py::get(d, "before"), 0.0);
        match key {
            "stage1_growth" => {
                g1_after = after / 100.0;
                notes.push(format!(
                    "Stage1 增速 {:.1}%→{:.1}%",
                    before, after
                ));
            }
            "rev_growth" => {
                g1_after = (g1_before + (after - before) / 100.0 * 0.5).max(0.0);
                notes.push(format!(
                    "营收增速 {:.1}%→{:.1}% (传导 Stage1)",
                    before, after
                ));
            }
            "terminal_g" => {
                tg_after = after / 100.0;
                notes.push(format!("终值 g {:.1}%→{:.1}%", before, after));
            }
            "net_margin" | "gross_margin" => {
                if before > 0.0 {
                    fcf_scale *= after / before;
                    notes.push(format!(
                        "{} {:.1}%→{:.1}% (缩放基期 FCF)",
                        spec(key).unwrap().label,
                        before,
                        after
                    ));
                }
            }
            "beta" => {
                beta_after = after;
                notes.push(format!("Beta {:.2}→{:.2}", before, after));
            }
            "capex_pct" => {
                fcf_scale *= (1.0 - (after - before) / 100.0 * 3.0).max(0.1);
                notes.push(format!(
                    "Capex/营收 {:.1}%→{:.1}% (压低 FCF)",
                    before, after
                ));
            }
            _ => {}
        }
    }

    // WACC approximation: beta change → cost of equity → wacc.
    let wacc_b = get_or(dcf_result, "wacc_breakdown", json!({}));
    let wacc_b = if uzi_core::py::truthy(&wacc_b) {
        wacc_b
    } else {
        json!({})
    };
    let wb = pnum_yen(uzi_core::py::get(&wacc_b, "wacc"), 0.0);
    let wacc_before = if wb != 0.0 { wb } else { 0.08 };
    let inp = get_or(&wacc_b, "inputs", json!({}));
    let inp = if uzi_core::py::truthy(&inp) { inp } else { json!({}) };
    let _rf = pnum_yen(uzi_core::py::get(&inp, "rf"), 0.025);
    let erp = pnum_yen(uzi_core::py::get(&inp, "erp"), 0.06);
    let beta_b0 = pnum_yen(uzi_core::py::get(&inp, "beta"), 0.0);
    let beta_before = if beta_b0 != 0.0 { beta_b0 } else { 1.0 };
    let eq_w = pnum_yen(uzi_core::py::get(&wacc_b, "equity_weight"), 0.70);
    let wacc_after = wacc_before + eq_w * (beta_after - beta_before) * erp;

    let denom_before = (wacc_before - tg_before).max(1e-4);
    let denom_after = (wacc_after - tg_after).max(1e-4);
    let tv_factor = denom_before / denom_after;
    let growth_factor = if g1_before > -1.0 {
        (1.0 + g1_after).powf(5.0) / (1.0 + g1_before).powf(5.0)
    } else {
        1.0
    };

    let after_ps = round(before_ps * fcf_scale * tv_factor * growth_factor, 2);
    let delta_abs = round(after_ps - before_ps, 2);
    let delta_pct = if before_ps > 0.0 {
        round(delta_abs / before_ps * 100.0, 1)
    } else {
        0.0
    };

    let cur_price = {
        let p = pnum_yen(uzi_core::py::get(features, "price"), 0.0);
        if p != 0.0 {
            p
        } else {
            pnum_yen(uzi_core::py::get(dcf_result, "current_price"), 0.0)
        }
    };
    let sm_before = pnum_yen(uzi_core::py::get(dcf_result, "safety_margin_pct"), 0.0);
    let sm_after = if cur_price > 0.0 {
        round((after_ps - cur_price) / cur_price * 100.0, 1)
    } else {
        sm_before
    };

    json!({
        "available": true,
        "intrinsic_before": crate::num_value(before_ps),
        "intrinsic_after": crate::num_value(after_ps),
        "delta_abs": crate::num_value(delta_abs),
        "delta_pct": crate::num_value(delta_pct),
        "direction": delta_dir(after_ps, before_ps),
        "wacc_before_pct": crate::num_value(round(wacc_before * 100.0, 2)),
        "wacc_after_pct": crate::num_value(round(wacc_after * 100.0, 2)),
        "safety_margin_before_pct": crate::num_value(sm_before),
        "safety_margin_after_pct": crate::num_value(sm_after),
        "drivers": if notes.is_empty() { vec!["无 DCF 相关假设改动 → 内在价值不变".to_string()] } else { notes },
    })
}

/// `_reprice_comps` — EPS / target-multiple re-pricing of the comps implied price.
fn reprice_comps(comps_result: &Value, deltas: &[Value], features: &Value) -> Value {
    let implied_before = get_or(comps_result, "implied_price", json!({}));
    let implied_before = if uzi_core::py::truthy(&implied_before) {
        implied_before
    } else {
        json!({})
    };
    let pe_implied_before = pnum_yen(uzi_core::py::get(&implied_before, "via_median_pe"), 0.0);
    let pb_implied_before = pnum_yen(uzi_core::py::get(&implied_before, "via_median_pb"), 0.0);
    if pe_implied_before <= 0.0 && pb_implied_before <= 0.0 {
        return json!({"available": false, "reason": "comps_result 无 implied_price"});
    }

    let target = get_or(comps_result, "target", json!({}));
    let target = if uzi_core::py::truthy(&target) {
        target
    } else {
        json!({})
    };
    let eps0 = pnum_yen(uzi_core::py::get(&target, "eps"), 0.0);
    let eps = if eps0 != 0.0 {
        eps0
    } else {
        pnum_yen(uzi_core::py::get(features, "eps"), 0.0)
    };
    let peer_stats = get_or(comps_result, "peer_stats", json!({}));
    let peer_stats = if uzi_core::py::truthy(&peer_stats) {
        peer_stats
    } else {
        json!({})
    };
    let median_pe = pnum_yen(
        uzi_core::py::get(&get_or(&peer_stats, "pe", json!({})), "median"),
        0.0,
    );

    let mut eps_scale = 1.0f64;
    let mut pe_override: Option<f64> = None;
    let mut notes: Vec<String> = Vec::new();
    for d in deltas {
        let key = uzi_core::py::get(d, "key").as_str().unwrap_or("");
        let after = pnum_yen(uzi_core::py::get(d, "after"), 0.0);
        let before = pnum_yen(uzi_core::py::get(d, "before"), 0.0);
        if key == "net_margin" && before > 0.0 {
            eps_scale *= after / before;
            notes.push(format!("净利率 {:.1}%→{:.1}% (放大 EPS)", before, after));
        } else if key == "target_pe" {
            pe_override = Some(after);
            notes.push(format!("目标 PE {:.2}x→{:.2}x", before, after));
        }
    }

    let eps_after = eps * eps_scale;
    let pe_used = pe_override.unwrap_or(median_pe);
    let pe_implied_after = if pe_used > 0.0 && eps_after > 0.0 {
        round(pe_used * eps_after, 2)
    } else {
        round(pe_implied_before * eps_scale, 2)
    };

    let delta_abs = round(pe_implied_after - pe_implied_before, 2);
    let delta_pct = if pe_implied_before > 0.0 {
        round(delta_abs / pe_implied_before * 100.0, 1)
    } else {
        0.0
    };

    json!({
        "available": true,
        "implied_pe_before": crate::num_value(pe_implied_before),
        "implied_pe_after": crate::num_value(pe_implied_after),
        "implied_pb_before": crate::num_value(pb_implied_before),
        "delta_abs": crate::num_value(delta_abs),
        "delta_pct": crate::num_value(delta_pct),
        "direction": delta_dir(pe_implied_after, pe_implied_before),
        "drivers": if notes.is_empty() { vec!["无 Comps 相关假设改动 → 隐含价不变".to_string()] } else { notes },
    })
}

/// `_thesis_impact` — map each assumption change to its thesis pillar.
fn thesis_impact(deltas: &[Value]) -> Value {
    let pillar_map = |key: &str| -> Option<&'static str> {
        match key {
            "rev_growth" => Some("成长性（营收增速）"),
            "gross_margin" => Some("盈利质量（毛利率）"),
            "net_margin" => Some("盈利质量（净利率）"),
            "capex_pct" => Some("现金流 / 资本纪律"),
            "stage1_growth" => Some("成长性（中期增速）"),
            "terminal_g" => Some("长期价值（永续增长）"),
            "beta" => Some("风险 / 折现率"),
            "target_pe" => Some("估值锚（倍数）"),
            "target_price" => Some("估值锚（目标价）"),
            _ => None,
        }
    };
    let mut out: Vec<Value> = Vec::new();
    for d in deltas {
        let delta = pnum_yen(uzi_core::py::get(d, "delta"), 0.0);
        if delta.abs() < 1e-9 {
            continue;
        }
        let key = uzi_core::py::get(d, "key").as_str().unwrap_or("");
        let bearish_up = key == "capex_pct" || key == "beta";
        let up = uzi_core::py::get(d, "direction") == &json!("↑");
        let bullish = up != bearish_up;
        out.push(json!({
            "pillar": pillar_map(key).map(|s| s.to_string()).unwrap_or_else(|| py_str_py(uzi_core::py::get(d, "label"))),
            "change": format!(
                "{} → {} ({})",
                py_str_py(uzi_core::py::get(d, "before_fmt")),
                py_str_py(uzi_core::py::get(d, "after_fmt")),
                py_str_py(uzi_core::py::get(d, "direction"))
            ),
            "impact": if bullish { "💪 强化" } else { "⚠️ 削弱" },
        }));
    }
    if out.is_empty() {
        out.push(json!({"pillar": "（无实质假设改动）", "change": "—", "impact": "⚪ 中性"}));
    }
    Value::Array(out)
}

/// `_verdict`.
fn verdict(dcf_impact: &Value, comps_impact: &Value, thesis: &Value) -> Value {
    let mut score = 0.0f64;
    if uzi_core::py::truthy(uzi_core::py::get(dcf_impact, "available")) {
        score += pnum_yen(uzi_core::py::get(dcf_impact, "delta_pct"), 0.0);
    }
    if uzi_core::py::truthy(uzi_core::py::get(comps_impact, "available")) {
        score += pnum_yen(uzi_core::py::get(comps_impact, "delta_pct"), 0.0);
    }
    let strengthen = thesis
        .as_array()
        .unwrap()
        .iter()
        .filter(|t| {
            uzi_core::py::get(t, "impact")
                .as_str()
                .map(|s| s.starts_with("💪"))
                .unwrap_or(false)
        })
        .count();
    let weaken = thesis
        .as_array()
        .unwrap()
        .iter()
        .filter(|t| {
            uzi_core::py::get(t, "impact")
                .as_str()
                .map(|s| s.starts_with("⚠️"))
                .unwrap_or(false)
        })
        .count();
    score += (strengthen as f64 - weaken as f64) * 2.0;

    let (rating, action) = if score >= 10.0 {
        ("🟢 上修 (Upgrade)", "上调目标价 / 加仓候选")
    } else if score >= 3.0 {
        ("🟡 小幅上修", "维持评级，目标价微升")
    } else if score > -3.0 {
        ("⚪ 维持 (Maintain)", "数据落在噪声区间，观点不变")
    } else if score > -10.0 {
        ("🟠 小幅下修", "维持评级，目标价微降")
    } else {
        ("🔴 下修 (Downgrade)", "下调目标价 / 减仓候选")
    };

    json!({
        "rating": rating,
        "action": action,
        "composite_score": crate::num_value(round(score, 1)),
        "pillars_strengthened": strengthen,
        "pillars_weakened": weaken,
    })
}

/// `build_model_update` — incrementally update the financial model.
pub fn build_model_update(
    features: &Value,
    raw_data: &Value,
    updates: Option<&Value>,
    dcf_result: Option<&Value>,
    comps_result: Option<&Value>,
) -> Value {
    let basic = dim_data(raw_data, "0_basic");
    let name = {
        let n = get_or(&basic, "name", Value::Null);
        if uzi_core::py::truthy(&n) {
            n
        } else {
            get_or(features, "name", json!("—"))
        }
    };
    let code = {
        let c = get_or(&basic, "code", Value::Null);
        if uzi_core::py::truthy(&c) {
            c
        } else {
            let f = get_or(features, "code", Value::Null);
            if uzi_core::py::truthy(&f) {
                f
            } else {
                get_or(features, "ticker", json!("—"))
            }
        }
    };

    let demo_mode = updates.is_none();
    let empty = json!({});
    let updates = match updates {
        Some(u) if uzi_core::py::truthy(u) => u,
        _ => &empty,
    };
    let updates_map = updates.as_object().cloned().unwrap_or_default();

    // ① assumption before → after delta table
    let mut deltas: Vec<Value> = Vec::new();
    let keys: Vec<String> = if demo_mode {
        SPECS.iter().map(|s| s.key.to_string()).collect()
    } else {
        let mut keys: Vec<String> = updates_map.keys().cloned().collect();
        for s in SPECS {
            if s.from_after.is_some() && updates_map.contains_key(s.key) && !keys.contains(&s.key.to_string()) {
                keys.push(s.key.to_string());
            }
        }
        keys
    };
    for key in keys.iter() {
        let Some(sp) = spec(key) else { continue };
        let after_raw = updates_map.get(key).filter(|v| !v.is_null());
        if demo_mode && sp.from_after.is_none() {
            continue;
        }
        if !demo_mode && after_raw.is_none() {
            continue;
        }
        let (before, after) = infer_before_after(sp, features, after_raw);
        if demo_mode && before == 0.0 && after == 0.0 {
            continue;
        }
        let delta = round(after - before, 3);
        let mut entry = Map::new();
        entry.insert("key".into(), Value::String(sp.key.into()));
        entry.insert("label".into(), Value::String(sp.label.into()));
        entry.insert("unit".into(), Value::String(sp.unit.into()));
        entry.insert("channel".into(), Value::String(sp.channel.into()));
        entry.insert("before".into(), crate::num_value(round(before, 3)));
        entry.insert("after".into(), crate::num_value(round(after, 3)));
        entry.insert("before_fmt".into(), Value::String(fmt(sp.unit, before)));
        entry.insert("after_fmt".into(), Value::String(fmt(sp.unit, after)));
        entry.insert("delta".into(), crate::num_value(delta));
        entry.insert(
            "delta_fmt".into(),
            Value::String(format!(
                "{}{}",
                if delta >= 0.0 { "+" } else { "" },
                fmt(sp.unit, delta)
            )),
        );
        entry.insert("direction".into(), Value::String(delta_dir(after, before).into()));
        deltas.push(Value::Object(entry));
    }

    // ② DCF intrinsic-value impact
    let dcf_impact = match dcf_result.filter(|d| uzi_core::py::truthy(d)) {
        Some(d) => reprice_dcf(d, &deltas, features),
        None => json!({"available": false, "reason": "未提供 dcf_result"}),
    };

    // ③ Comps implied-price impact
    let comps_impact = match comps_result.filter(|c| uzi_core::py::truthy(c)) {
        Some(c) => reprice_comps(c, &deltas, features),
        None => json!({"available": false, "reason": "未提供 comps_result"}),
    };

    // ④ Thesis pillar impact
    let thesis = thesis_impact(&deltas);

    // ⑤ Updated verdict
    let verdict = verdict(&dcf_impact, &comps_impact, &thesis);

    // methodology_log
    let mut log: Vec<Value> = Vec::new();
    log.push(Value::String(format!(
        "Step 1 · {} → {} 条 delta",
        if demo_mode {
            "演示模式（最新 vs 上期推断）".to_string()
        } else {
            format!("用户传入 {} 项新假设", updates_map.len())
        },
        deltas.len()
    )));
    for d in deltas.iter().take(6) {
        log.push(Value::String(format!(
            "        · {}: {} → {} ({})",
            py_str_py(uzi_core::py::get(d, "label")),
            py_str_py(uzi_core::py::get(d, "before_fmt")),
            py_str_py(uzi_core::py::get(d, "after_fmt")),
            py_str_py(uzi_core::py::get(d, "direction"))
        )));
    }
    if uzi_core::py::truthy(uzi_core::py::get(&dcf_impact, "available")) {
        log.push(Value::String(format!(
            "Step 2 · DCF 内在价值 ¥{:.2} → ¥{:.2} ({:+.1}%)",
            pnum_yen(uzi_core::py::get(&dcf_impact, "intrinsic_before"), 0.0),
            pnum_yen(uzi_core::py::get(&dcf_impact, "intrinsic_after"), 0.0),
            pnum_yen(uzi_core::py::get(&dcf_impact, "delta_pct"), 0.0)
        )));
    } else {
        log.push(Value::String(format!(
            "Step 2 · DCF 影响：{}",
            py_str_py(uzi_core::py::get(&dcf_impact, "reason"))
        )));
    }
    if uzi_core::py::truthy(uzi_core::py::get(&comps_impact, "available")) {
        log.push(Value::String(format!(
            "Step 3 · Comps 隐含价 ¥{:.2} → ¥{:.2} ({:+.1}%)",
            pnum_yen(uzi_core::py::get(&comps_impact, "implied_pe_before"), 0.0),
            pnum_yen(uzi_core::py::get(&comps_impact, "implied_pe_after"), 0.0),
            pnum_yen(uzi_core::py::get(&comps_impact, "delta_pct"), 0.0)
        )));
    } else {
        log.push(Value::String(format!(
            "Step 3 · Comps 影响：{}",
            py_str_py(uzi_core::py::get(&comps_impact, "reason"))
        )));
    }
    log.push(Value::String(format!(
        "Step 4 · 投资逻辑：{} 强化 / {} 削弱",
        uzi_core::py::get(&verdict, "pillars_strengthened"),
        uzi_core::py::get(&verdict, "pillars_weakened")
    )));
    log.push(Value::String(format!(
        "Step 5 · 更新后评级：{}（综合分 {:+.1}）→ {}",
        py_str_py(uzi_core::py::get(&verdict, "rating")),
        pnum_yen(uzi_core::py::get(&verdict, "composite_score"), 0.0),
        py_str_py(uzi_core::py::get(&verdict, "action"))
    )));

    json!({
        "method": "Model Update (增量更新财务模型)",
        "company": {"name": name, "code": code},
        "mode": if demo_mode { "demo" } else { "explicit" },
        "generated_at": clock::date_str(&clock::now()),
        "assumption_deltas": deltas,
        "dcf_impact": dcf_impact,
        "comps_impact": comps_impact,
        "thesis_impact": thesis,
        "verdict": verdict,
        "methodology_log": log,
    })
}
