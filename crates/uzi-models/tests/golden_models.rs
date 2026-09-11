//! Differential tests for the institutional-modeling dimensions (20/21/22).
//!
//! Golden artifacts come from `tools/golden/dump_models.py`, which mirrors
//! `run_real_test._run_modeling_and_scoring`: features are
//! `sanitize_features(extract_features(raw, raw["dimensions"]))`, then
//! `compute_dim_20 → compute_dim_21 → compute_dim_22` chained.

use serde_json::Value;
use uzi_core::testkit::{assert_json_eq, first_difference, load_fixture, load_golden};
use uzi_models::compute::{compute_dim_20, compute_dim_21, compute_dim_22};

/// Write the actual tree next to the golden so `tools/golden/compare.py` can list
/// every difference, not just the first.
fn check(actual: &Value, expected: &Value, label: &str) {
    if first_difference(expected, actual, "$").is_some() {
        let dir = std::env::temp_dir().join("uzi_models_actual");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join(format!("{}.json", label.replace('/', "_")));
        let _ = std::fs::write(&path, uzi_core::json::to_pretty(actual));
        eprintln!("actual written to {}", path.display());
    }
    assert_json_eq(actual, expected, label);
}

fn run_case(case: &str) {
    let raw = load_fixture(&format!("raw_data_{}", case));
    // The golden `features_sanitized.json` is the exact input upstream used; using
    // it here isolates the modeling modules from uzi-features.
    let features = load_golden(case, "features_sanitized");

    let d20 = compute_dim_20(&features, &raw);
    check(&d20, &load_golden(case, "dim_20"), &format!("dim_20/{}", case));

    // Upstream (`run_real_test._run_modeling_and_scoring`) passes the INNER data
    // dicts: `d20 = raw["dimensions"]["20_valuation_models"]["data"]`. Models
    // receives exactly that payload and treats it verbatim.
    let d20_data = d20.get("data").cloned().unwrap_or(Value::Null);
    let d21 = compute_dim_21(&features, &raw, &d20_data);
    check(&d21, &load_golden(case, "dim_21"), &format!("dim_21/{}", case));

    let d21_data = d21.get("data").cloned().unwrap_or(Value::Null);
    let d22 = compute_dim_22(&features, &raw, &d20_data, &d21_data);
    check(&d22, &load_golden(case, "dim_22"), &format!("dim_22/{}", case));
}

#[test]
fn synthetic_models_match_upstream() {
    run_case("synthetic");
}

#[test]
fn sparse_models_match_upstream() {
    run_case("sparse");
}

#[test]
fn empty_models_match_upstream() {
    run_case("empty");
}

/// The CLI must not pass the full dimension dict where upstream passes the inner
/// `data` dict: `compute_dim_21` reads `dim_20_data.get("dcf")`, which is absent
/// from the full dict, so DCF wiring would silently drop.
#[test]
fn full_dimension_dict_yields_no_dcf_wiring() {
    let raw = load_fixture("raw_data_synthetic");
    let features = load_golden("synthetic", "features_sanitized");
    let d20 = compute_dim_20(&features, &raw);
    let d20_data = d20.get("data").cloned().unwrap_or(Value::Null);
    assert!(
        d20.get("dcf").is_none(),
        "the full dimension dict has no top-level `dcf` key"
    );

    let wired = compute_dim_21(&features, &raw, &d20_data);
    let unwired = compute_dim_21(&features, &raw, &d20);
    let grade = |v: &Value| -> String {
        v.get("data")
            .and_then(|d| d.get("initiating_coverage"))
            .and_then(|c| c.get("headline"))
            .and_then(|h| h.get("rating"))
            .and_then(|r| r.as_str())
            .unwrap_or("")
            .to_string()
    };
    assert_ne!(
        grade(&wired),
        grade(&unwired),
        "passing the inner data dict must change the rating versus passing the wrapper"
    );
    assert_eq!(grade(&unwired), "未评级 (Not Rated)");
}

// ═══════════════════════════════════════════════════════════════
// Determinism, internal consistency and hand-derived arithmetic
// ═══════════════════════════════════════════════════════════════

fn dims_case(case: &str) -> (Value, Value) {
    let raw = load_fixture(&format!("raw_data_{}", case));
    let features = load_golden(case, "features_sanitized");
    (features, raw)
}

/// Re-implementation of the documented 2-stage DCF formula, written from the
/// upstream docstring rather than by calling `fin_models`, so it independently
/// cross-checks `summary.dcf_intrinsic`.
fn independent_dcf_intrinsic(features: &Value) -> Option<f64> {
    let fcf0 = features
        .get("fcf_latest_yi")
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0);
    let fcf0 = if fcf0 <= 0.0 {
        let rev = features
            .get("revenue_latest_yi")
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0);
        let nm = features
            .get("net_margin")
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0)
            / 100.0;
        rev * nm * 0.8
    } else {
        fcf0
    };
    if fcf0 <= 0.0 {
        return None;
    }
    // A-share defaults: rf 2.5%, ERP 6%, beta 1.0, pretax kd 4.5%, debt 30%, tax 25%.
    let cost_of_equity = 0.025 + 1.0 * 0.06;
    let after_tax_kd = 0.045 * (1.0 - 0.25);
    let wacc = uzi_core::py::round(0.7 * cost_of_equity + 0.3 * after_tax_kd, 4);

    // Stage 1 (5y @10%) then stage 2 (5y @5%), each step rounded to 3dp as upstream does.
    let mut cur = fcf0;
    let mut proj: Vec<f64> = Vec::new();
    for _ in 0..5 {
        cur *= 1.10;
        proj.push(uzi_core::py::round(cur, 3));
    }
    for _ in 0..5 {
        cur *= 1.05;
        proj.push(uzi_core::py::round(cur, 3));
    }
    let mut pv_sum = 0.0;
    for (i, f) in proj.iter().enumerate() {
        pv_sum += uzi_core::py::round(f / (1.0 + wacc).powi((i + 1) as i32), 3);
    }
    let pv_explicit = uzi_core::py::round(pv_sum, 3);
    // Gordon-growth terminal value at the end of the explicit period.
    let tv_at_end = if wacc - 0.025 <= 0.0 {
        0.0
    } else {
        proj[9] * (1.0 + 0.025) / (wacc - 0.025)
    };
    let tv_pv = uzi_core::py::round(tv_at_end / (1.0 + wacc).powi(10), 3);
    let enterprise_value = uzi_core::py::round(pv_explicit + tv_pv, 3);
    let net_debt = features
        .get("total_debt_yi")
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0)
        - features
            .get("cash_yi")
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0);
    let equity_value = uzi_core::py::round(enterprise_value - net_debt, 3);
    let mut shares = features
        .get("shares_outstanding_yi")
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0);
    if shares <= 0.0 {
        let mc = features
            .get("market_cap_yi")
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0);
        let px = features.get("price").and_then(|v| v.as_f64()).unwrap_or(0.0);
        shares = if px > 0.0 { mc / px } else { 1.0 };
    }
    if shares <= 0.0 {
        return Some(0.0);
    }
    Some(uzi_core::py::round(equity_value / shares, 2))
}

#[test]
fn dim_20_and_22_are_byte_identical_across_invocations() {
    for case in ["synthetic", "sparse", "empty"] {
        let (features, raw) = dims_case(case);
        let a = uzi_core::json::to_py_compact(&compute_dim_20(&features, &raw));
        let b = uzi_core::json::to_py_compact(&compute_dim_20(&features, &raw));
        assert_eq!(a, b, "compute_dim_20/{case} is not deterministic");

        let d20 = compute_dim_20(&features, &raw);
        let d20_data = d20.get("data").cloned().unwrap_or(Value::Null);
        let d21 = compute_dim_21(&features, &raw, &d20_data);
        let d21_data = d21.get("data").cloned().unwrap_or(Value::Null);
        let a =
            uzi_core::json::to_py_compact(&compute_dim_22(&features, &raw, &d20_data, &d21_data));
        let b =
            uzi_core::json::to_py_compact(&compute_dim_22(&features, &raw, &d20_data, &d21_data));
        assert_eq!(a, b, "compute_dim_22/{case} is not deterministic");
    }
}

#[test]
fn dim_20_summary_dcf_intrinsic_matches_independent_formula() {
    for case in ["synthetic", "sparse", "empty"] {
        let (features, raw) = dims_case(case);
        let d20 = compute_dim_20(&features, &raw);
        let summary = &d20["data"]["summary"];
        let independent = independent_dcf_intrinsic(&features);
        match summary["dcf_intrinsic"].as_f64() {
            Some(actual) => {
                let expected = independent.expect("formula must produce a value when DCF does");
                assert_eq!(
                    actual, expected,
                    "compute_dim_20/{case}: summary.dcf_intrinsic != independent DCF recomputation"
                );
                assert_eq!(
                    actual, d20["data"]["dcf"]["intrinsic_per_share"],
                    "compute_dim_20/{case}: summary disagrees with the dcf record"
                );
            }
            None => {
                assert!(
                    independent.is_none(),
                    "compute_dim_20/{case}: unexpected null DCF"
                );
                assert_eq!(
                    summary["dcf_verdict"],
                    Value::String("⛔ 数据不足 · 无法 DCF".to_string())
                );
            }
        }
    }
}

/// Sparse / empty inputs must degrade to the documented shapes without panicking.
#[test]
fn sparse_and_empty_return_documented_keys() {
    let d20_keys = ["dcf", "comps", "three_statement", "lbo", "summary"];
    let d22_keys = [
        "ic_memo",
        "unit_economics",
        "value_creation_plan",
        "dd_checklist",
        "competitive_analysis",
        "portfolio_rebalance",
        "summary",
    ];
    for case in ["sparse", "empty"] {
        let (features, raw) = dims_case(case);
        let d20 = compute_dim_20(&features, &raw);
        for key in d20_keys {
            assert!(d20["data"].get(key).is_some(), "dim_20/{case} missing {key}");
        }
        assert_eq!(d20["data"]["summary"]["dcf_intrinsic"], Value::Null);
        assert_eq!(d20["data"]["summary"]["dcf_safety_margin_pct"], Value::Null);
        assert_eq!(
            d20["data"]["summary"]["comps_verdict"],
            Value::String("⚪ 同行样本不足 · 无法对标".to_string())
        );
        // The 3-statement model reports a missing base revenue rather than faking one.
        assert_eq!(
            d20["data"]["three_statement"]["error"],
            Value::String("no base revenue".to_string())
        );

        let d20_data = d20.get("data").cloned().unwrap_or(Value::Null);
        let d21 = compute_dim_21(&features, &raw, &d20_data);
        let d21_data = d21.get("data").cloned().unwrap_or(Value::Null);
        let d22 = compute_dim_22(&features, &raw, &d20_data, &d21_data);
        for key in d22_keys {
            assert!(d22["data"].get(key).is_some(), "dim_22/{case} missing {key}");
        }
        assert_eq!(
            d22["source"],
            Value::String("compute:deep_analysis_methods (6 PE/IB/WM methods)".to_string())
        );
        assert_eq!(d22["fallback"], Value::Bool(false));
    }
}

/// Hand-derived DCF / WACC / LBO arithmetic on the synthetic fixture. Every
/// expected number below is computed from the formula in the comment; no value
/// is copied from the implementation.
#[test]
fn dcf_wacc_lbo_hand_derived_on_synthetic_features() {
    let (features, raw) = dims_case("synthetic");
    let d20 = compute_dim_20(&features, &raw);
    let dcf = &d20["data"]["dcf"];
    let lbo = &d20["data"]["lbo"];

    // WACC (A-share defaults):
    //   k_e = rf + beta·erp  = 0.025 + 1.00 × 0.06     = 0.085
    //   k_d = 0.045 × (1 − 0.25) = 0.045 × 0.75        = 0.03375 → round(·,4) = 0.0338
    //   WACC = 0.70 × 0.085 + 0.30 × 0.03375 = 0.0595 + 0.010125
    //        = 0.069625 → round(·,4) = 0.0696
    assert_eq!(dcf["wacc_breakdown"]["cost_of_equity"], Value::from(0.085));
    assert_eq!(dcf["wacc_breakdown"]["after_tax_kd"], Value::from(0.0338));
    assert_eq!(dcf["wacc_breakdown"]["wacc"], Value::from(0.0696));

    // Base FCF = 4.32 亿 (fcf_latest_yi present, no proxy needed).
    //   Y1 = round(4.32  × 1.10, 3) = 4.752
    //   Y2 = round(4.752 × 1.10, 3) = 5.227
    //   Y5 = round(6.325 × 1.10, 3) = 6.957 ; Y6 = round(6.957 × 1.05, 3) = 7.305
    assert_eq!(dcf["base_fcf_yi"], Value::from(4.32));
    assert_eq!(
        dcf["projected_fcf_yi"],
        serde_json::json!([4.752, 5.227, 5.75, 6.325, 6.957, 7.305, 7.671, 8.054, 8.457, 8.88])
    );

    // Discounting: PV(Y1) = round(4.752 / 1.0696, 3) = round(4.4427…) = 4.443
    assert_eq!(dcf["pv_fcf_yi"][0], Value::from(4.443));
    assert_eq!(dcf["pv_explicit_yi"], Value::from(47.032));

    // Terminal (Gordon growth):
    //   TV  = 8.88 × 1.025 / (0.0696 − 0.025) = 9.102 / 0.0446 = 204.081…
    //   PV(TV) = 204.081 / 1.0696^10 = 104.133 → EV = 47.032 + 104.133 = 151.165
    assert_eq!(dcf["terminal_value_yi"], Value::from(204.081));
    assert_eq!(dcf["tv_pv_yi"], Value::from(104.133));
    assert_eq!(dcf["enterprise_value_yi"], Value::from(151.165));
    // total_debt_yi = cash_yi = 0 → net debt 0; shares 13.923 亿股
    //   per_share  = round(151.165 / 13.923, 2) = round(10.8571…) = 10.86
    //   safety     = round((10.86 − 23.45) / 23.45 × 100, 1) = round(−53.688…) = −53.7
    assert_eq!(dcf["intrinsic_per_share"], Value::from(10.86));
    assert_eq!(dcf["safety_margin_pct"], Value::from(-53.7));
    assert_eq!(dcf["verdict"], Value::String("🔴 明显高估".to_string()));

    // Quick LBO with the upstream defaults (8x entry, 5x debt, 8x exit, 5y, +8%, 6%):
    //   EBITDA = 9.0 (ebitda_yi) → EV = 8 × 9.0 = 72.0 ; debt = 5 × 9.0 = 45.0
    //   entry equity = 72.0 − 45.0 = 27.0
    //   Y1 EBITDA = 9.0 × 1.08 = 9.72 ; interest = 45.0 × 0.06 = 2.7
    //   FCF = 9.72 × 0.5 − 2.7 = 2.16 ; paydown = 2.16 × 0.7 = 1.512 → debt 43.488 → 43.49
    //   … Y5 EBITDA = 13.22 → exit EV = 8 × 13.22 = 105.76, exit debt 33.71
    //   MOIC = (105.76 − 33.71) / 27.0 = 2.6681… → 2.67
    //   IRR  = 2.668…^(1/5) − 1 = 0.2166… → 21.7%
    assert_eq!(lbo["entry_ebitda_yi"], Value::from(9.0));
    assert_eq!(lbo["entry_ev_yi"], Value::from(72.0));
    assert_eq!(lbo["entry_debt_yi"], Value::from(45.0));
    assert_eq!(lbo["entry_equity_yi"], Value::from(27.0));
    assert_eq!(
        lbo["ebitda_path"],
        serde_json::json!([9.72, 10.5, 11.34, 12.24, 13.22])
    );
    assert_eq!(lbo["debt_schedule"][1], Value::from(43.49));
    assert_eq!(lbo["exit_ebitda_yi"], Value::from(13.22));
    assert_eq!(lbo["exit_ev_yi"], Value::from(105.76));
    assert_eq!(lbo["exit_equity_yi"], Value::from(72.05));
    assert_eq!(lbo["moic"], Value::from(2.67));
    assert_eq!(lbo["irr_pct"], Value::from(21.7));
    assert_eq!(lbo["pass_pe_test"], Value::Bool(true));
}

/// `max(0, x)` in Python yields the *int* 0 when `x <= 0`; the LBO debt schedule
/// must therefore serialise `0`, not `0.0`, once the debt is fully repaid.
#[test]
fn lbo_debt_schedule_keeps_python_int_zero() {
    let (features, raw) = dims_case("sparse");
    let d20 = compute_dim_20(&features, &raw);
    let schedule = d20["data"]["lbo"]["debt_schedule"].as_array().unwrap();
    assert_eq!(schedule.first().unwrap(), &Value::from(0.0));
    for entry in &schedule[1..] {
        // Path is all-zero for sparse features → every later entry is the int 0.
        assert!(
            entry.is_i64() || entry.is_u64(),
            "expected int 0 in debt_schedule, got {entry:?}"
        );
    }
}
