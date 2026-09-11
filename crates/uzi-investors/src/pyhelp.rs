//! Python-semantics helpers for the rule engine (port of the runtime behaviour of
//! the `investor_criteria` lambdas plus `investor_evaluator._fmt_msg`).
//!
//! The upstream rule checks are Python lambdas over the feature dict. They use
//! `dict.get(key, default)`, so a key that is *present but `None`* bypasses the
//! default and makes the following numeric comparison raise `TypeError`; the
//! evaluator catches that and skips the rule entirely. This module reproduces
//! that three-way distinction (`missing` / `null` / value) instead of silently
//! treating `null` as the default.

use serde_json::Value;
use uzi_core::py as py;

/// A Python exception raised while evaluating a rule check
/// (`KeyError` / `TypeError` / `ValueError` / `ZeroDivisionError`).
///
/// `investor_evaluator._safe_check` maps it to `None` → the rule is skipped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PyErr;

pub type R<T> = Result<T, PyErr>;

/// `float(v)` / numeric use of a JSON value. `None` → `TypeError`; `bool` is an
/// `int` in Python; strings/containers are not comparable to numbers.
fn as_num(v: &Value) -> R<f64> {
    match v {
        Value::Number(n) => n.as_f64().ok_or(PyErr),
        Value::Bool(b) => Ok(if *b { 1.0 } else { 0.0 }),
        _ => Err(PyErr),
    }
}

/// `f.get(key, default)` where the result feeds a numeric comparison.
pub fn num(f: &Value, key: &str, default: f64) -> R<f64> {
    match f.get(key) {
        None => Ok(default),
        Some(v) => as_num(v),
    }
}

/// `f.get(key, default)` in a truthiness context (`bool(...)`).
pub fn truth(f: &Value, key: &str, default: bool) -> bool {
    match f.get(key) {
        None => default,
        Some(v) => py::truthy(v),
    }
}

/// `f.get(key)` in a truthiness context (missing → falsy).
pub fn truth_req(f: &Value, key: &str) -> bool {
    match f.get(key) {
        None => false,
        Some(v) => py::truthy(v),
    }
}

/// `f.get(key) == v` for a numeric literal. Equality never raises in Python.
pub fn eq_num(f: &Value, key: &str, v: f64) -> bool {
    match f.get(key) {
        Some(Value::Number(_)) | Some(Value::Bool(_)) => {
            as_num(f.get(key).unwrap()).map(|x| x == v).unwrap_or(false)
        }
        _ => false,
    }
}

/// `f.get(key) == v` for a string literal.
pub fn eq_str(f: &Value, key: &str, v: &str) -> bool {
    matches!(f.get(key), Some(Value::String(s)) if s == v)
}

/// `f.get(key, default)` as a `str` operand (concatenation, `.lower()`,
/// substring membership). A present `None` or non-string raises `TypeError`.
pub fn text(f: &Value, key: &str, default: &str) -> R<String> {
    match f.get(key) {
        None => Ok(default.to_string()),
        Some(Value::String(s)) => Ok(s.clone()),
        _ => Err(PyErr),
    }
}

/// Python `str(v)` for JSON values: scalars via `num_str`, containers via
/// `repr()` (so a list of strings renders `['光学', '光电子']`, not JSON).
pub fn py_str_full(v: &Value) -> String {
    match v {
        Value::Array(items) => {
            let inner: Vec<String> = items.iter().map(py_repr).collect();
            format!("[{}]", inner.join(", "))
        }
        Value::Object(map) => {
            let inner: Vec<String> = map
                .iter()
                .map(|(k, val)| format!("{}: {}", py_repr_str(k), py_repr(val)))
                .collect();
            format!("{{{}}}", inner.join(", "))
        }
        other => py::num_str(other),
    }
}

fn py_repr(v: &Value) -> String {
    match v {
        Value::String(s) => py_repr_str(s),
        other => py_str_full(other),
    }
}

fn py_repr_str(s: &str) -> String {
    let quote = if s.contains('\'') && !s.contains('"') { '"' } else { '\'' };
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

/// `str(f.get(key, default))` — never raises (`str(None)` is `"None"`).
pub fn str_of(f: &Value, key: &str, default: &str) -> String {
    match f.get(key) {
        None => default.to_string(),
        Some(v) => py_str_full(v),
    }
}

/// `f.get(a, da) + f.get(b, db)` for a string concatenation.
pub fn concat2(f: &Value, a: &str, da: &str, b: &str, db: &str) -> R<String> {
    let mut s = text(f, a, da)?;
    s.push_str(&text(f, b, db)?);
    Ok(s)
}

/// `x in (…)` membership against a tuple of string literals.
pub fn str_in(f: &Value, key: &str, default: &str, items: &[&str]) -> bool {
    match text(f, key, default) {
        Ok(s) => items.contains(&s.as_str()),
        Err(_) => false,
    }
}

/// `f.get(key, default) is False` — Python identity, not truthiness.
pub fn is_false(f: &Value, key: &str, default: bool) -> bool {
    match f.get(key) {
        None => !default,
        Some(Value::Bool(false)) => true,
        _ => false,
    }
}

/// `investor_criteria._known_fcf` — `ValueError` when the cash-flow direction is
/// unknown, which makes the evaluator skip the rule.
pub fn known_fcf(f: &Value) -> R<bool> {
    if !truth_req(f, "fcf_known") {
        return Err(PyErr);
    }
    Ok(truth_req(f, "fcf_positive"))
}

/// `f.get(a) or f.get(b, db)` where the result feeds a numeric comparison.
pub fn or_num(f: &Value, a: &str, b: &str, db: f64) -> R<f64> {
    match f.get(a) {
        Some(v) if py::truthy(v) => as_num(v),
        _ => num(f, b, db),
    }
}

/// `investor_criteria._peg` — `PE / revenue_growth_latest`, or 999 when either
/// side is non-positive.
pub fn peg(f: &Value) -> R<f64> {
    let pe = num(f, "pe", 0.0)?;
    let growth = num(f, "revenue_growth_latest", 0.0)?;
    if pe <= 0.0 || growth <= 0.0 {
        return Ok(999.0);
    }
    Ok(pe / growth)
}

// ────────────────────────────────────────────────────────────────
// Python `str.format_map` subset used by the reporter strings
// ────────────────────────────────────────────────────────────────

/// How a format-map lookup treats an unknown field name.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Missing {
    /// `KeyError`: the caller falls back to the raw template.
    Error,
    /// Rendered literally (the `?` of `_fmt_msg`'s `_MissingValue`).
    Literal(&'static str),
}

/// Port of `str.format_map` for the field/spec forms the ported modules use:
/// `{}` (str conversion), `{x:.Nf}` and `{x:.N%}`.
///
/// `lookup` returns the value for a field name, or `None` for a field the map
/// does not define. A malformed template, an unknown spec or a value that cannot
/// satisfy the spec yields `Err` — the same outcome as Python's
/// `ValueError`/`KeyError`/`IndexError`, which the callers convert to a raw
/// return of the template.
pub fn format_map(
    template: &str,
    lookup: &dyn Fn(&str) -> Option<Value>,
    missing: Missing,
) -> Result<String, ()> {
    let mut out = String::with_capacity(template.len());
    let mut it = template.char_indices().peekable();
    while let Some((_, ch)) = it.next() {
        match ch {
            '{' => {
                if let Some((_, '{')) = it.peek() {
                    it.next();
                    out.push('{');
                    continue;
                }
                let mut body = String::new();
                loop {
                    match it.next() {
                        Some((_, '}')) => break,
                        Some((_, c)) => body.push(c),
                        None => return Err(()), // unmatched '{' → ValueError
                    }
                }
                let (field, spec) = match body.split_once(':') {
                    Some((f, s)) => (f, Some(s)),
                    None => (body.as_str(), None),
                };
                if field.is_empty() {
                    return Err(()); // positional field → IndexError
                }
                let Some(value) = lookup(field) else {
                    match missing {
                        Missing::Error => return Err(()),
                        Missing::Literal(s) => {
                            out.push_str(s);
                            continue;
                        }
                    }
                };
                out.push_str(&render(&value, spec)?);
            }
            '}' => {
                if let Some((_, '}')) = it.peek() {
                    it.next();
                    out.push('}');
                    continue;
                }
                return Err(());
            }
            c => out.push(c),
        }
    }
    Ok(out)
}

fn render(value: &Value, spec: Option<&str>) -> Result<String, ()> {
    let Some(spec) = spec else {
        return Ok(py_str_full(value));
    };
    if spec.is_empty() {
        return Ok(py_str_full(value));
    }
    // `format()` converts bool to int for numeric specs.
    let x = match value {
        Value::Number(n) => n.as_f64().ok_or(())?,
        Value::Bool(b) => {
            if *b {
                1.0
            } else {
                0.0
            }
        }
        _ => return Err(()), // ValueError for str with a numeric spec
    };
    let (percent, rest) = match spec.strip_suffix('%') {
        Some(rest) => (true, rest),
        None => (false, spec),
    };
    let digits = rest.strip_suffix('f').unwrap_or(rest);
    let Some(prec) = digits.strip_prefix('.').and_then(|p| p.parse::<usize>().ok()) else {
        return Err(()); // unsupported format spec → Python ValueError
    };
    let scaled = if percent { x * 100.0 } else { x };
    let mut s = format!("{:.*}", prec, scaled);
    if percent {
        s.push('%');
    }
    Ok(s)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn fmt(t: &str, f: &Value) -> String {
        format_map(
            t,
            &|k| match f.get(k) {
                None | Some(Value::Null) => None,
                Some(v) => Some(v.clone()),
            },
            Missing::Literal("?"),
        )
        .unwrap_or_else(|_| t.to_string())
    }

    #[test]
    fn numeric_defaults_apply_only_when_the_key_is_absent() {
        let f = json!({"a": null});
        assert_eq!(num(&f, "a", 5.0), Err(PyErr)); // present null bypasses the default
        assert_eq!(num(&f, "b", 5.0), Ok(5.0));
        assert_eq!(num(&json!({"a": 2.5}), "a", 0.0), Ok(2.5));
    }

    #[test]
    fn equality_with_null_never_raises() {
        let f = json!({"s": null, "n": 2, "t": "2"});
        assert!(!eq_num(&f, "s", 2.0));
        assert!(!eq_num(&f, "missing", 2.0));
        assert!(eq_num(&f, "n", 2.0));
        assert!(!eq_num(&f, "t", 2.0));
    }

    #[test]
    fn format_placeholders_match_python() {
        let f = json!({"n": 12, "x": 3.456, "missing": null});
        assert_eq!(fmt("{x:.1f}", &f), "3.5");
        assert_eq!(fmt("{n:.0f}", &f), "12");
        assert_eq!(fmt("{x}", &f), "3.456");
        assert_eq!(fmt("{missing:.1f}", &f), "?");
        assert_eq!(fmt("{unknown}", &f), "?"); // _MissingValue ignores the spec
        let p = json!({"r": 0.0512});
        assert_eq!(fmt("{r:.0%}", &p), "5%");
        assert_eq!(fmt("{", &f), "{"); // malformed → raw template
    }

    #[test]
    fn identity_check_distinguishes_zero_from_false() {
        let f = json!({"a": false, "b": 0, "c": null});
        assert!(is_false(&f, "a", true));
        assert!(!is_false(&f, "b", false));
        assert!(!is_false(&f, "c", false));
        assert!(is_false(&f, "missing", false));
        assert!(!is_false(&f, "missing", true));
    }
}
