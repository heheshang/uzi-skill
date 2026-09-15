//! Python number/string formatting helpers shared by the renderers.

use serde_json::Value;

/// Python `str(float)`: shortest repr, always with a decimal point or exponent.
pub(crate) fn pyf(v: f64) -> String {
    if v.is_nan() {
        return "nan".to_string();
    }
    if v.is_infinite() {
        return if v > 0.0 { "inf" } else { "-inf" }.to_string();
    }
    let s = format!("{}", v);
    if s.contains('.') || s.contains('e') || s.contains('E') {
        s
    } else {
        format!("{}.0", s)
    }
}

/// Python `str()` for a JSON value as used inside f-strings.
pub(crate) fn disp(v: &Value) -> String {
    match v {
        Value::Null => "None".to_string(),
        Value::Bool(b) => if *b { "True" } else { "False" }.to_string(),
        Value::String(s) => s.clone(),
        Value::Number(n) => {
            if n.is_i64() || n.is_u64() {
                n.to_string()
            } else {
                pyf(n.as_f64().unwrap_or(0.0))
            }
        }
        Value::Array(_) | Value::Object(_) => repr(v),
    }
}

/// Python `repr()` for strings (single quotes unless the value contains one).
fn quote_repr(s: &str) -> String {
    if s.contains('\'') && !s.contains('"') {
        return format!("\"{}\"", s.replace('\\', "\\\\"));
    }
    let escaped = s
        .replace('\\', "\\\\")
        .replace('\'', "\\'")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
        .replace('\t', "\\t");
    format!("'{escaped}'")
}

/// Python `repr()` for JSON values (list/dict use `, ` and `: ` separators with
/// single-quoted strings).
pub(crate) fn repr(v: &Value) -> String {
    match v {
        Value::Null => "None".to_string(),
        Value::Bool(b) => if *b { "True" } else { "False" }.to_string(),
        Value::String(s) => quote_repr(s),
        Value::Number(n) => {
            if n.is_i64() || n.is_u64() {
                n.to_string()
            } else {
                pyf(n.as_f64().unwrap_or(0.0))
            }
        }
        Value::Array(a) => format!("[{}]", a.iter().map(repr).collect::<Vec<_>>().join(", ")),
        Value::Object(o) => format!(
            "{{{}}}",
            o.iter()
                .map(|(k, val)| format!("{}: {}", quote_repr(k), repr(val)))
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}

/// Numeric coercion for render math (`float(v) or 0`).
pub(crate) fn num(v: &Value) -> f64 {
    match v {
        Value::Number(n) => n.as_f64().unwrap_or(0.0),
        Value::String(s) => s.trim().parse::<f64>().unwrap_or(0.0),
        Value::Bool(b) => {
            if *b {
                1.0
            } else {
                0.0
            }
        }
        _ => 0.0,
    }
}

/// Insert thousands separators into a non-negative integer string.
fn group_digits(digits: &str) -> String {
    let bytes = digits.as_bytes();
    let mut out = String::with_capacity(bytes.len() + bytes.len() / 3);
    let n = bytes.len();
    for (i, b) in bytes.iter().enumerate() {
        if i > 0 && (n - i) % 3 == 0 {
            out.push(',');
        }
        out.push(*b as char);
    }
    out
}

/// Python `f"{v:,}"` for an integer.
pub(crate) fn group_i(v: i64) -> String {
    let neg = v < 0;
    let s = group_digits(&v.unsigned_abs().to_string());
    if neg {
        format!("-{}", s)
    } else {
        s
    }
}

/// Python `f"{v:,.Nf}"`.
pub(crate) fn group_f(v: f64, decimals: usize) -> String {
    let s = format!("{:.*}", decimals, v);
    let (sign, body) = match s.strip_prefix('-') {
        Some(rest) => ("-", rest),
        None => ("", s.as_str()),
    };
    let (int_part, frac) = match body.find('.') {
        Some(i) => (&body[..i], &body[i..]),
        None => (body, ""),
    };
    format!("{}{}{}", sign, group_digits(int_part), frac)
}

/// Python `f"{v:+.Nf}"`.
pub(crate) fn signed(v: f64, decimals: usize) -> String {
    format!("{:+.1$}", v, decimals)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn matches_python_formatting() {
        assert_eq!(group_i(10000), "10,000");
        assert_eq!(group_i(-1234567), "-1,234,567");
        assert_eq!(group_f(12345.678, 1), "12,345.7");
        assert_eq!(group_f(-9876.5, 0), "-9,876");
        assert_eq!(pyf(6.0), "6.0");
        assert_eq!(pyf(23.45), "23.45");
        assert_eq!(disp(&json!(6)), "6");
        assert_eq!(disp(&json!(6.0)), "6.0");
    }
}
