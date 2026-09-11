//! Python-semantics helpers.
//!
//! The upstream UZI-Skill scripts are Python; several behaviours that look
//! incidental (truthiness, `round()`, `float(str(v).replace(...))`) are part of
//! the observable contract of scoring and rendering. Port them verbatim so the
//! Rust engine reproduces byte-identical artifacts.

use serde_json::Value;

/// Python `float(str(v).replace("%","").replace(",","").replace("+",""))`,
/// falling back to `default` on failure — the upstream `score_fns._f`.
pub fn f(v: &Value, default: f64) -> f64 {
    match v {
        Value::Number(n) => n.as_f64().unwrap_or(default),
        Value::Bool(b) => {
            if *b {
                1.0
            } else {
                0.0
            }
        }
        Value::String(s) => parse_float(s).unwrap_or(default),
        _ => default,
    }
}

/// `_f` with the upstream default of `0.0`.
pub fn f0(v: &Value) -> f64 {
    f(v, 0.0)
}

/// `stock_features._f` — like [`f`] but also strips `¥` and `亿`, and treats the
/// placeholder strings `-`, `—`, `None`, `nan`, `N/A` as missing.
pub fn f_fin(v: &Value, default: f64) -> f64 {
    let s = match v {
        Value::Null => return default,
        Value::Number(n) => return n.as_f64().unwrap_or(default),
        Value::Bool(b) => {
            return if *b {
                1.0
            } else {
                0.0
            }
        }
        Value::String(s) => s,
        _ => return default,
    };
    let cleaned: String = s
        .trim()
        .chars()
        .filter(|c| !matches!(c, ',' | '%' | '+' | '¥' | '亿' | ' '))
        .collect();
    if cleaned.is_empty() || matches!(cleaned.as_str(), "-" | "—" | "None" | "nan" | "N/A") {
        return default;
    }
    cleaned.parse::<f64>().unwrap_or(default)
}

/// Parse a float the way `float(s.replace("%","").replace(",","").replace("+",""))` does.
pub fn parse_float(s: &str) -> Option<f64> {
    let cleaned: String = s
        .chars()
        .filter(|c| *c != '%' && *c != ',' && *c != '+')
        .collect();
    let trimmed = cleaned.trim();
    if trimmed.is_empty() {
        return None;
    }
    match trimmed {
        "inf" | "Infinity" | "+inf" => Some(f64::INFINITY),
        "-inf" | "-Infinity" => Some(f64::NEG_INFINITY),
        _ => trimmed.parse::<f64>().ok(),
    }
}

/// Python truthiness for JSON values (`bool(v)`).
pub fn truthy(v: &Value) -> bool {
    match v {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().map(|x| x != 0.0).unwrap_or(true),
        Value::String(s) => !s.is_empty(),
        Value::Array(a) => !a.is_empty(),
        Value::Object(o) => !o.is_empty(),
    }
}

/// Python `round(x, ndigits)`.
///
/// Python rounds the exact decimal value of the double, ties-to-even. Rust's
/// `format!` machinery performs the same correct decimal rounding, so going
/// through the shortest-decimal parser reproduces it exactly.
pub fn round(x: f64, ndigits: i32) -> f64 {
    if !x.is_finite() {
        return x;
    }
    let nd = ndigits.max(0) as usize;
    match format!("{:.*}", nd, x).parse::<f64>() {
        Ok(v) => v,
        Err(_) => x,
    }
}

/// Python `round(x)` — ndigits omitted, returns an integer-valued float.
pub fn round0(x: f64) -> f64 {
    round(x, 0)
}

/// `str(v)` for JSON values, matching Python's `str()` on the parsed object for
/// the types that survive a JSON round-trip (numbers use Python repr rules).
pub fn py_str(v: &Value) -> String {
    match v {
        Value::Null => "None".to_string(),
        Value::Bool(b) => {
            if *b {
                "True".to_string()
            } else {
                "False".to_string()
            }
        }
        Value::Number(n) => match n.as_f64() {
            Some(x) if x.fract() == 0.0 && x.abs() < 1e16 => format!("{}", x as i64),
            _ => n.to_string(),
        },
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

/// Python `str(n)` for a JSON number: integers print without a decimal point,
/// floats always keep one (`18.0`).
pub fn num_str(v: &Value) -> String {
    match v {
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                i.to_string()
            } else if let Some(u) = n.as_u64() {
                u.to_string()
            } else if let Some(x) = n.as_f64() {
                float_str(x)
            } else {
                n.to_string()
            }
        }
        Value::String(s) => s.clone(),
        other => py_str(other),
    }
}

/// Python `str(float)` / `repr(float)` for the common cases.
pub fn float_str(x: f64) -> String {
    if !x.is_finite() {
        return match x {
            f64::INFINITY => "inf".into(),
            f64::NEG_INFINITY => "-inf".into(),
            _ => "nan".into(),
        };
    }
    if x.fract() == 0.0 && x.abs() < 1e16 {
        format!("{:.1}", x)
    } else {
        format!("{}", x)
    }
}

/// Python `str(v)` for a value interpolated into an f-string.
///
/// Strings pass through unquoted; containers fall back to `repr()` (which is
/// what `str()` does for lists and dicts).
pub fn py_display(v: &Value) -> String {
    match v {
        Value::Array(_) | Value::Object(_) => py_repr(v),
        other => py_str(other),
    }
}

/// Python `repr()` for JSON-representable values: single-quoted strings, `, `
/// separators, `True`/`False`/`None`.
pub fn py_repr(v: &Value) -> String {
    match v {
        Value::Null => "None".to_string(),
        Value::Bool(b) => {
            if *b {
                "True".to_string()
            } else {
                "False".to_string()
            }
        }
        Value::Number(_) => num_str(v),
        Value::String(s) => py_repr_str(s),
        Value::Array(a) => {
            let items: Vec<String> = a.iter().map(py_repr).collect();
            format!("[{}]", items.join(", "))
        }
        Value::Object(o) => {
            let items: Vec<String> = o
                .iter()
                .map(|(k, val)| format!("{}: {}", py_repr_str(k), py_repr(val)))
                .collect();
            format!("{{{}}}", items.join(", "))
        }
    }
}

/// Python string repr quoting rules.
fn py_repr_str(s: &str) -> String {
    let has_single = s.contains('\'');
    let has_double = s.contains('"');
    let quote = if has_single && !has_double { '"' } else { '\'' };
    let mut out = String::with_capacity(s.len() + 2);
    out.push(quote);
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c == quote => {
                out.push('\\');
                out.push(c);
            }
            c => out.push(c),
        }
    }
    out.push(quote);
    out
}

/// Index a JSON object, returning `Null` when absent (Python `.get(k)`).
pub fn get<'a>(v: &'a Value, key: &str) -> &'a Value {
    static NULL: Value = Value::Null;
    v.get(key).unwrap_or(&NULL)
}

/// `data.get(k) or {}` idiom: object or empty object.
pub fn get_obj<'a>(v: &'a Value, key: &str) -> &'a serde_json::Map<String, Value> {
    static EMPTY: std::sync::LazyLock<serde_json::Map<String, Value>> =
        std::sync::LazyLock::new(serde_json::Map::new);
    match v.get(key) {
        Some(Value::Object(o)) => o,
        _ => &EMPTY,
    }
}

/// `d.get(k) or 0.0` idiom.
pub fn num_or(v: &Value, key: &str, default: f64) -> f64 {
    match v.get(key) {
        None | Some(Value::Null) => default,
        Some(other) => {
            let x = f0(other);
            if x == 0.0 && !matches!(other, Value::Number(_)) {
                default
            } else {
                x
            }
        }
    }
}

/// Python `urllib.parse.quote(s)` with the default `safe='/'`.
///
/// Encodes the UTF-8 bytes of `s`, leaving ASCII letters, digits, and the
/// always-safe `_.-~` plus `/` untouched. Hex digits are uppercase.
pub fn py_url_quote(s: &str) -> String {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut out = String::with_capacity(s.len());
    for byte in s.as_bytes() {
        let c = *byte as char;
        let unreserved = c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-' | '~' | '/');
        if unreserved {
            out.push(c);
        } else {
            out.push('%');
            out.push(HEX[(byte >> 4) as usize] as char);
            out.push(HEX[(byte & 0x0F) as usize] as char);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_percent_thousands_and_signs() {
        assert_eq!(f0(&json!("+12.5%")), 12.5);
        assert_eq!(f0(&json!("1,234.5")), 1234.5);
        assert_eq!(f0(&json!("-3.2%")), -3.2);
        assert_eq!(f0(&json!("n/a")), 0.0);
        assert_eq!(f0(&json!(null)), 0.0);
        assert_eq!(f0(&json!(7)), 7.0);
    }

    #[test]
    fn python_rounding_matches_half_even_on_decimal_value() {
        // Python: round(2.675, 2) == 2.67 (binary value below the tie)
        assert_eq!(round(2.675, 2), 2.67);
        assert_eq!(round(0.5, 0), 0.0);
        assert_eq!(round(1.5, 0), 2.0);
        assert_eq!(round(2.5, 0), 2.0);
        assert_eq!(round(-1.25, 1), -1.2);
    }

    #[test]
    fn truthiness_includes_zero_as_falsy_but_empty_string_objects() {
        assert!(!truthy(&json!(0)));
        assert!(truthy(&json!(0.0)) == false);
        assert!(!truthy(&json!("")));
        assert!(truthy(&json!([])) == false);
        assert!(truthy(&json!([0])));
    }

    #[test]
    fn repr_matches_python_quoting_rules() {
        // python3 -c "print(repr(x))" for each of these
        assert_eq!(py_repr(&json!(["光学玻璃", "树脂"])), "['光学玻璃', '树脂']");
        assert_eq!(py_repr(&json!({"a": 1})), "{'a': 1}");
        assert_eq!(py_repr(&json!("x y")), "'x y'");
        assert_eq!(py_repr(&json!("it's")), "\"it's\"");
        assert_eq!(py_repr(&json!("he said \"hi\"")), "'he said \"hi\"'");
        assert_eq!(py_repr(&json!(1.5)), "1.5");
        assert_eq!(py_repr(&json!(null)), "None");
        assert_eq!(py_repr(&json!(true)), "True");
        assert_eq!(py_repr(&json!([])), "[]");
        assert_eq!(py_repr(&json!({})), "{}");
    }

    #[test]
    fn display_uses_str_for_scalars_and_repr_for_containers() {
        assert_eq!(py_display(&json!("abc")), "abc");
        assert_eq!(py_display(&json!(1)), "1");
        assert_eq!(py_display(&json!([1, 2])), "[1, 2]");
        assert_eq!(py_display(&json!(null)), "None");
    }

    #[test]
    fn num_str_and_float_str_follow_python_str() {
        // python3 -c "print(str(1e9), str(18.0), str(0.5), str(1))"
        assert_eq!(num_str(&json!(1000000000)), "1000000000");
        assert_eq!(float_str(18.0), "18.0");
        assert_eq!(float_str(0.5), "0.5");
        assert_eq!(num_str(&json!(1)), "1");
        assert_eq!(float_str(38.4), "38.4");
    }

    #[test]
    fn url_quote_matches_urllib_parse_quote() {
        // python3 -c "from urllib.parse import quote; print(quote(s))"
        assert_eq!(py_url_quote("水晶光电"), "%E6%B0%B4%E6%99%B6%E5%85%89%E7%94%B5");
        assert_eq!(
            py_url_quote("光学光电子 行业景气度 增速 市场规模 2026"),
            "%E5%85%89%E5%AD%A6%E5%85%89%E7%94%B5%E5%AD%90%20%E8%A1%8C%E4%B8%9A%E6%99%AF%E6%B0%94%E5%BA%A6%20%E5%A2%9E%E9%80%9F%20%E5%B8%82%E5%9C%BA%E8%A7%84%E6%A8%A1%202026"
        );
        assert_eq!(
            py_url_quote("水晶光电 老师 推荐"),
            "%E6%B0%B4%E6%99%B6%E5%85%89%E7%94%B5%20%E8%80%81%E5%B8%88%20%E6%8E%A8%E8%8D%90"
        );
        // `/` is in the default safe set; everything else reserved is escaped.
        assert_eq!(py_url_quote("a b+c/d?e=f&g"), "a%20b%2Bc/d%3Fe%3Df%26g");
        // Unreserved characters pass through untouched.
        assert_eq!(py_url_quote("~-_."), "~-_.");
        assert_eq!(py_url_quote(""), "");
        assert_eq!(py_url_quote("abcXYZ019"), "abcXYZ019");
    }
}
