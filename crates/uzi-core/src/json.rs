//! JSON serialization helpers matching Python's `json.dumps` conventions.

use serde_json::Value;
use std::path::Path;

/// `json.dumps(v, ensure_ascii=False, indent=2)`.
///
/// `serde_json` is compiled with `preserve_order`, so map key order is the
/// insertion order — same as a Python dict. Non-ASCII output is left unescaped,
/// matching `ensure_ascii=False`.
pub fn to_pretty(value: &Value) -> String {
    serde_json::to_string_pretty(value).unwrap_or_else(|_| "null".to_string())
}

/// `json.dumps(v, ensure_ascii=False)` — compact, no added whitespace.
///
/// Python separates with `", "` / `": "`; `serde_json::to_string` uses no spaces.
/// Callers that need Python's exact compact text should use [`to_py_compact`].
pub fn to_compact(value: &Value) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "null".to_string())
}

/// `json.dumps(v, ensure_ascii=False)` with Python's `", "` / `": "` separators.
pub fn to_py_compact(value: &Value) -> String {
    let mut out = String::new();
    write_py_compact(value, &mut out);
    out
}

fn write_py_compact(value: &Value, out: &mut String) {
    match value {
        Value::Null => out.push_str("null"),
        Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Value::Number(n) => out.push_str(&n.to_string()),
        Value::String(s) => out.push_str(&quote_py(s)),
        Value::Array(a) => {
            out.push('[');
            for (i, v) in a.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                write_py_compact(v, out);
            }
            out.push(']');
        }
        Value::Object(o) => {
            out.push('{');
            for (i, (k, v)) in o.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                out.push_str(&quote_py(k));
                out.push_str(": ");
                write_py_compact(v, out);
            }
            out.push('}');
        }
    }
}

/// Python-compatible string quoting (matches `json.dumps` for common cases).
fn quote_py(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Write pretty JSON to `path`, creating parent directories.
pub fn write_json(path: &Path, value: &Value) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, to_pretty(value))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn pretty_output_keeps_key_order_and_unicode() {
        let v = json!({"b": 1, "a": "贵州茅台", "c": [1, 2]});
        let text = to_pretty(&v);
        assert!(text.starts_with("{\n  \"b\": 1,"));
        assert!(text.contains("贵州茅台"));
        assert!(!text.contains("\\u"));
    }

    #[test]
    fn compact_uses_python_separators() {
        let v = json!({"a": 1, "b": ["x", "y"]});
        assert_eq!(to_py_compact(&v), "{\"a\": 1, \"b\": [\"x\", \"y\"]}");
    }
}
