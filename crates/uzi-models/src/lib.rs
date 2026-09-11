//! uzi-models — Rust port of UZI-Skill script layer.
//!
//! Ports the institutional-modeling script layer of
//! `skills/deep-analysis/scripts/`:
//!
//! * [`fin_models`] — `lib/fin_models.py` (DCF / WACC / comps / 3-stmt / LBO / merger)
//! * [`deep_methods`] — `lib/deep_analysis_methods.py` (IC memo, unit econ, DD, Porter/BCG)
//! * [`research_workflow`] — `lib/research_workflow.py` (initiating coverage, screens …)
//! * [`global_peers`] — `lib/global_peers.py` (pure normalization / ranking half; the
//!   network provider classes live in `uzi-data`)
//! * [`compute`] — `compute_deep_methods.py` (dims 20/21/22)
//! * [`tier1`] — `lib/tier1/*` (AI readiness, earnings preview, model update, rebalance,
//!   returns attribution)
//!
//! Cross-crate interfaces are `serde_json::Value` (contract HARD RULE 1). Python
//! numerics go through `uzi_core::py`; the module-local `_num` variants that have
//! extra `str(...).replace(...)` semantics are implemented once in this module.

use serde_json::Value;

pub mod compute;
pub mod deep_methods;
pub mod fin_models;
pub mod global_peers;
pub mod research_workflow;
pub mod tier1;

pub use compute::{compute_dim_20, compute_dim_21, compute_dim_22};
pub use tier1::{
    build_ai_readiness, build_earnings_preview, build_model_update, build_rebalance,
    build_returns_attribution,
};

use std::sync::LazyLock;

static EMPTY_OBJ: LazyLock<Value> =
    LazyLock::new(|| Value::Object(serde_json::Map::new()));

/// `obj.get(key) or {}` returning a shared empty object for any miss.
pub(crate) fn obj_or_empty<'a>(v: &'a Value, key: &str) -> &'a Value {
    match v.get(key) {
        Some(x @ Value::Object(_)) => x,
        _ => &EMPTY_OBJ,
    }
}

/// `d.get(key, default)` — default only when the key is absent (Python `.get`).
pub(crate) fn get_or(v: &Value, key: &str, default: Value) -> Value {
    match v.get(key) {
        Some(x) => x.clone(),
        None => default,
    }
}

/// `raw.get("dimensions") or {}`.
pub(crate) fn dimensions(raw: &Value) -> &Value {
    obj_or_empty(raw, "dimensions")
}

/// `(raw["dimensions"].get(dim) or {}).get("data") or {}` — the `.data` payload
/// of a raw dimension record.
pub(crate) fn dim_data<'a>(raw: &'a Value, dim: &str) -> &'a Value {
    obj_or_empty(obj_or_empty(dimensions(raw), dim), "data")
}

/// `float(s)` for Python: surrounding whitespace ignored, `_` digit separators
/// allowed, `inf`/`infinity`/`nan` accepted case-insensitively.
pub(crate) fn py_float_str(s: &str) -> Option<f64> {
    let cleaned: String = s.trim().chars().filter(|c| *c != '_').collect();
    if cleaned.is_empty() {
        return None;
    }
    let lower = cleaned.to_ascii_lowercase();
    let signed = lower.strip_prefix(['+', '-']).unwrap_or(&lower);
    match signed {
        "inf" | "infinity" => {
            return Some(if lower.starts_with('-') {
                f64::NEG_INFINITY
            } else {
                f64::INFINITY
            })
        }
        "nan" => return Some(f64::NAN),
        _ => {}
    }
    cleaned.parse::<f64>().ok()
}

/// `lib/fin_models.py::_num` — `float(v)` with a fallback (no `%`/`,` stripping).
pub(crate) fn flt(v: &Value, default: f64) -> f64 {
    match v {
        Value::Number(n) => n.as_f64().unwrap_or(default),
        Value::Bool(b) => {
            if *b {
                1.0
            } else {
                0.0
            }
        }
        Value::String(s) => py_float_str(s).unwrap_or(default),
        _ => default,
    }
}

/// `float(str(v).replace("%","").replace(",","").strip())` with a fallback —
/// the `_num` shared by `deep_analysis_methods` / `research_workflow` / `tier1`.
pub(crate) fn pnum(v: &Value, default: f64) -> f64 {
    pnum_strip(v, default, false)
}

/// Same as [`pnum`] plus `.replace("¥","")` — `tier1/{earnings_preview,model_update}._num`.
pub(crate) fn pnum_yen(v: &Value, default: f64) -> f64 {
    pnum_strip(v, default, true)
}

fn pnum_strip(v: &Value, default: f64, yen: bool) -> f64 {
    let text = match v {
        Value::String(s) => s.clone(),
        Value::Number(_) => return v.as_f64().unwrap_or(default),
        _ => return default,
    };
    let mut cleaned = text.replace('%', "").replace(',', "");
    if yen {
        cleaned = cleaned.replace('¥', "");
    }
    let trimmed = cleaned.trim();
    if trimmed.is_empty() {
        return default;
    }
    trimmed.parse::<f64>().unwrap_or(default)
}

/// Python `str(v)` for values that appear in f-string interpolations.
///
/// Matches Python for the JSON scalars: `True`/`False`, `None`, ints, and float
/// repr (`2.0` stays `"2.0"`). Containers fall back to JSON text (upstream never
/// interpolates them bare).
pub(crate) fn py_str_py(v: &Value) -> String {
    match v {
        Value::Null => "None".to_string(),
        Value::Bool(b) => if *b { "True" } else { "False" }.to_string(),
        Value::Number(n) => n.to_string(),
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

/// JSON number from an `f64` (Python `str()` compatible repr via serde's ryu).
pub(crate) fn num_value(x: f64) -> Value {
    match serde_json::Number::from_f64(x) {
        Some(n) => Value::Number(n),
        None => Value::Null,
    }
}

/// Python `repr` of a list of strings, e.g. `['12%', '10%']`.
pub(crate) fn py_list_str_repr(items: &[String]) -> String {
    let inner: Vec<String> = items
        .iter()
        .map(|s| format!("'{}'", s.replace('\\', "\\\\").replace('\'', "\\'")))
        .collect();
    format!("[{}]", inner.join(", "))
}

/// Python `f"{x:,.0f}"` — round half-even to 0 decimals, then group thousands.
pub(crate) fn py_thousands0(x: f64) -> String {
    if !x.is_finite() {
        return format!("{}", x);
    }
    let r = uzi_core::py::round(x, 0);
    let neg = r.is_sign_negative() && r != 0.0;
    let digits = format!("{:.0}", r.abs());
    let mut out = String::new();
    let bytes = digits.as_bytes();
    for (i, c) in bytes.iter().enumerate() {
        if i > 0 && (bytes.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(*c as char);
    }
    if neg {
        format!("-{}", out)
    } else {
        out
    }
}

/// Shared statistical helpers mirroring the parts of `statistics` upstream uses.
pub(crate) mod stats {
    /// `statistics.median` over an already-sorted slice.
    pub fn median_sorted(sorted: &[f64]) -> f64 {
        let n = sorted.len();
        if n == 0 {
            return f64::NAN;
        }
        if n % 2 == 1 {
            sorted[n / 2]
        } else {
            (sorted[n / 2 - 1] + sorted[n / 2]) / 2.0
        }
    }

    /// `statistics.quantiles(data, n=k, method="exclusive")` over a sorted slice.
    ///
    /// Returns an empty vec for `len < 2` (upstream raises `StatisticsError`, and
    /// every caller guards with `len(...) > 1`). `delta` is computed in signed
    /// arithmetic: Python ints are arbitrary precision, so `i*m - j*n` may be
    /// negative for short samples (`len == 2`), which would underflow `usize`.
    pub fn quantiles_exclusive_sorted(sorted: &[f64], n: usize) -> Vec<f64> {
        let ld = sorted.len();
        if ld < 2 {
            return Vec::new();
        }
        let m = ld + 1;
        let mut out = Vec::with_capacity(n.saturating_sub(1));
        for i in 1..n {
            let mut j = (i * m) / n;
            if j < 1 {
                j = 1;
            } else if j > ld - 1 {
                j = ld - 1;
            }
            let delta = i as i64 * m as i64 - j as i64 * n as i64;
            let interpolated = (sorted[j - 1] * (n as i64 - delta) as f64
                + sorted[j] * delta as f64)
                / n as f64;
            out.push(interpolated);
        }
        out
    }
}

/// Local-time helpers (`datetime.now()`).
pub(crate) mod clock {
    use chrono::{Local, NaiveDateTime};

    /// `datetime.now()` (local, naive).
    pub fn now() -> NaiveDateTime {
        Local::now().naive_local()
    }

    /// `dt.strftime("%Y-%m-%d")`.
    pub fn date_str(dt: &NaiveDateTime) -> String {
        dt.format("%Y-%m-%d").to_string()
    }

    /// `datetime.strptime(s[:10], "%Y-%m-%d")`, falling back to `now`.
    pub fn parse_date_or_now(s: &str, now: &NaiveDateTime) -> NaiveDateTime {
        let head: String = s.chars().take(10).collect();
        chrono::NaiveDate::parse_from_str(&head, "%Y-%m-%d")
            .ok()
            .and_then(|d| d.and_hms_opt(0, 0, 0))
            .unwrap_or(*now)
    }
}
