//! Python `str()` / `repr()` formatting of JSON values.
//!
//! The upstream review modules interpolate raw dicts/lists/floats into
//! evidence strings (e.g. `f"bull={bull}"`, `f"missing_critical={mcl[:3]}"`).
//! Python `str()` on those containers is `repr()` of their elements, and
//! `repr(89.0)` is `"89.0"` — which `uzi_core::py::py_str` deliberately
//! normalises away for display values. These helpers reproduce Python exactly
//! for the shapes the review engine emits.

use serde_json::Value;

/// Python `str(v)`.
pub fn str_exact(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        other => repr(other),
    }
}

/// Python `repr(v)`.
pub fn repr(v: &Value) -> String {
    match v {
        Value::Null => "None".to_string(),
        Value::Bool(b) => if *b { "True" } else { "False" }.to_string(),
        Value::Number(n) => number_repr(n),
        Value::String(s) => quote(s),
        Value::Array(a) => {
            let inner: Vec<String> = a.iter().map(repr).collect();
            format!("[{}]", inner.join(", "))
        }
        Value::Object(o) => {
            let inner: Vec<String> = o
                .iter()
                .map(|(k, v)| format!("{}: {}", quote(k), repr(v)))
                .collect();
            format!("{{{}}}", inner.join(", "))
        }
    }
}

fn number_repr(n: &serde_json::Number) -> String {
    if n.is_i64() || n.is_u64() {
        return n.to_string();
    }
    float_repr(n.as_f64().unwrap_or(f64::NAN))
}

/// Python `repr(float)`: shortest round-trip, but integral values keep a `.0`.
fn float_repr(x: f64) -> String {
    if x.is_nan() {
        return "nan".to_string();
    }
    if x == f64::INFINITY {
        return "inf".to_string();
    }
    if x == f64::NEG_INFINITY {
        return "-inf".to_string();
    }
    if x.fract() == 0.0 && x.abs() < 1e16 {
        format!("{:.1}", x)
    } else {
        format!("{}", x)
    }
}

/// Python string literal quoting. Uses single quotes unless the string contains
/// a single quote and no double quote, in which case double quotes are used.
fn quote(s: &str) -> String {
    let q = if s.contains('\'') && !s.contains('"') {
        '"'
    } else {
        '\''
    };
    let mut out = String::with_capacity(s.len() + 2);
    out.push(q);
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c == q => {
                out.push('\\');
                out.push(c);
            }
            c if (c as u32) < 0x20 || c as u32 == 0x7f => {
                out.push_str(&format!("\\x{:02x}", c as u32));
            }
            c => out.push(c),
        }
    }
    out.push(q);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn matches_python_repr_for_evidence_shapes() {
        assert_eq!(repr(&json!("工业金属")), "'工业金属'");
        assert_eq!(repr(&json!(89.0)), "89.0");
        assert_eq!(repr(&json!(0)), "0");
        assert_eq!(str_exact(&json!(null)), "None");
        assert_eq!(str_exact(&json!(true)), "True");
        assert_eq!(
            repr(&json!({"dim": "0_basic", "path": "name", "label": "公司名称"})),
            "{'dim': '0_basic', 'path': 'name', 'label': '公司名称'}"
        );
        assert_eq!(
            repr(&json!([{"dim": "0_basic", "path": "name"}])),
            "[{'dim': '0_basic', 'path': 'name'}]"
        );
    }
}
