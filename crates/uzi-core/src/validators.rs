//! Port of `lib/pipeline/validators.py` — unified empty-value conventions.
//!
//! Missing data is always `null`/`None`; never `0` or `"—"`. Fetchers may use
//! any internal representation but every payload is normalised before it lands
//! in [`DimResult::data`], and renderers test with [`is_empty_value`] instead of
//! defaulting with `or 0`.

use serde_json::{Map, Value};

use crate::dim::{DimResult, FetcherSpec, Quality};

const EMPTY_SENTINELS: &[&str] = &["", "—", "-", "n/a", "N/A", "无数据", "暂无", "null", "NaN"];

/// Standard emptiness test used by every renderer and validator.
///
/// `0` and `false` are **valid** values.
pub fn is_empty_value(v: &Value) -> bool {
    match v {
        Value::Null => true,
        Value::Number(n) => match n.as_f64() {
            Some(x) => !x.is_finite(),
            None => false,
        },
        Value::String(s) => {
            let t = s.trim();
            t.is_empty() || EMPTY_SENTINELS.contains(&t)
        }
        Value::Array(a) => a.is_empty(),
        Value::Object(o) => o.is_empty(),
        Value::Bool(_) => false,
    }
}

/// Field is absent from `data` or its value is empty.
pub fn is_data_gap(data: &Value, field_name: &str) -> bool {
    match data.get(field_name) {
        None => true,
        Some(v) => is_empty_value(v),
    }
}

/// Recursively collapse every empty sentinel to `null`.
pub fn normalize_empty(value: Value) -> Value {
    match value {
        Value::Array(a) => Value::Array(a.into_iter().map(normalize_empty).collect()),
        Value::Object(o) => Value::Object(
            o.into_iter()
                .map(|(k, v)| (k, normalize_empty(v)))
                .collect(),
        ),
        other => {
            if is_empty_value(&other) {
                Value::Null
            } else {
                other
            }
        }
    }
}

/// Normalise a whole data object. `keep_zero_fields` preserves fields where `0`
/// is semantically meaningful.
pub fn normalize_data(data: &mut Value, keep_zero_fields: &[&str]) {
    let Some(obj) = data.as_object_mut() else {
        return;
    };
    let mut out = Map::new();
    for (k, v) in std::mem::take(obj) {
        if keep_zero_fields.contains(&k.as_str()) {
            out.insert(k, v);
        } else {
            out.insert(k, normalize_empty(v));
        }
    }
    *obj = out;
}

/// True when at least one real value is present; `0` and `false` count.
fn has_meaningful_data(value: &Value) -> bool {
    if is_empty_value(value) {
        return false;
    }
    match value {
        Value::Object(o) => o.values().any(has_meaningful_data),
        Value::Array(a) => a.iter().any(has_meaningful_data),
        _ => true,
    }
}

/// Validate a result against its spec: normalize, fill `data_gaps`, infer `Quality`.
pub fn validate_result(mut result: DimResult, spec: &FetcherSpec) -> DimResult {
    if result.quality == Quality::Error {
        return result;
    }

    normalize_data(&mut result.data, &[]);

    let missing_required: Vec<String> = spec
        .required_fields
        .iter()
        .filter(|f| is_data_gap(&result.data, f))
        .cloned()
        .collect();
    let missing_optional: Vec<String> = spec
        .optional_fields
        .iter()
        .filter(|f| is_data_gap(&result.data, f))
        .cloned()
        .collect();

    result.data_gaps = missing_required
        .iter()
        .chain(missing_optional.iter())
        .cloned()
        .collect();

    result.quality = if !has_meaningful_data(&result.data) {
        Quality::Missing
    } else if missing_required.is_empty() && missing_optional.is_empty() {
        Quality::Full
    } else if missing_required.is_empty() {
        Quality::Partial
    } else if missing_required.len() == spec.required_fields.len() {
        Quality::Missing
    } else {
        Quality::Partial
    };

    result
}

/// Data completeness in `0.0..=1.0`.
pub fn quality_score(result: &DimResult, spec: &FetcherSpec) -> f64 {
    let all: Vec<&String> = spec
        .required_fields
        .iter()
        .chain(spec.optional_fields.iter())
        .collect();
    if all.is_empty() {
        return if result.quality == Quality::Full {
            1.0
        } else {
            0.0
        };
    }
    let filled = all
        .iter()
        .filter(|f| !is_data_gap(&result.data, f))
        .count();
    filled as f64 / all.len() as f64
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn zero_and_false_are_valid_values() {
        assert!(!is_empty_value(&json!(0)));
        assert!(!is_empty_value(&json!(0.0)));
        assert!(!is_empty_value(&json!(false)));
        assert!(is_empty_value(&json!("")));
        assert!(is_empty_value(&json!("  ")));
        assert!(is_empty_value(&json!("—")));
        assert!(is_empty_value(&json!("暂无")));
        assert!(is_empty_value(&json!([])));
        assert!(is_empty_value(&json!({})));
        assert!(is_empty_value(&json!(null)));
    }

    #[test]
    fn normalizes_sentinels_recursively() {
        let v = json!({"a": "", "b": {"c": "—"}, "d": [1, "n/a"], "e": 0});
        let n = normalize_empty(v);
        assert_eq!(n["a"], Value::Null);
        assert_eq!(n["b"]["c"], Value::Null);
        assert_eq!(n["d"][1], Value::Null);
        assert_eq!(n["d"][0], json!(1));
        assert_eq!(n["e"], json!(0));
    }

    #[test]
    fn infers_quality_from_required_and_optional() {
        let spec = FetcherSpec::new("t")
            .required(&["roe", "revenue"])
            .optional(&["margin"]);

        let mut r = DimResult::new("t", "s");
        r.data = json!({"roe": 12.0, "revenue": 1e9, "margin": 0.3});
        assert_eq!(validate_result(r, &spec).quality, Quality::Full);

        let mut r = DimResult::new("t", "s");
        r.data = json!({"roe": 12.0, "revenue": 1e9});
        let r = validate_result(r, &spec);
        assert_eq!(r.quality, Quality::Partial);
        assert_eq!(r.data_gaps, vec!["margin".to_string()]);

        let mut r = DimResult::new("t", "s");
        r.data = json!({"roe": 12.0});
        let r = validate_result(r, &spec);
        assert_eq!(r.quality, Quality::Partial);
        assert_eq!(r.data_gaps, vec!["revenue".to_string(), "margin".to_string()]);

        let mut r = DimResult::new("t", "s");
        r.data = json!({});
        assert_eq!(validate_result(r, &spec).quality, Quality::Missing);

        // error results pass through untouched
        let err = DimResult::error_result("t", "boom", "s");
        assert_eq!(validate_result(err, &spec).quality, Quality::Error);
    }

    #[test]
    fn quality_score_counts_filled_fields() {
        let spec = FetcherSpec::new("t")
            .required(&["a", "b"])
            .optional(&["c", "d"]);
        let mut r = DimResult::new("t", "s");
        r.data = json!({"a": 1, "b": 2, "c": 3});
        assert!((quality_score(&r, &spec) - 0.75).abs() < 1e-9);
    }
}
