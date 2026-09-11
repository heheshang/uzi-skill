//! Differential-testing helpers shared by every ported crate.
//!
//! Golden artifacts come from the upstream Python implementation via
//! `tools/golden/dump_python.py`; see `tools/golden/README.md`.

use serde_json::Value;
use std::path::PathBuf;

/// Directory holding golden reference output.
///
/// `UZI_GOLDEN_DIR` overrides; otherwise `<repo>/tools/golden/expected`.
pub fn golden_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("UZI_GOLDEN_DIR") {
        if !dir.is_empty() {
            return PathBuf::from(dir);
        }
    }
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("tools/golden/expected")
}

/// Directory holding generated input fixtures.
pub fn fixture_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("UZI_FIXTURE_DIR") {
        if !dir.is_empty() {
            return PathBuf::from(dir);
        }
    }
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("tools/golden/fixtures")
}

/// Load `golden_dir()/<case>/<name>.json`, or `panic` with a useful message.
pub fn load_golden(case: &str, name: &str) -> Value {
    let path = golden_dir().join(case).join(format!("{}.json", name));
    load_json(&path)
}

/// Load `fixture_dir()/<name>.json`.
pub fn load_fixture(name: &str) -> Value {
    let path = fixture_dir().join(format!("{}.json", name));
    load_json(&path)
}

pub fn load_json(path: &std::path::Path) -> Value {
    let text = std::fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("cannot read {}: {}", path.display(), e));
    serde_json::from_str(&text)
        .unwrap_or_else(|e| panic!("invalid JSON in {}: {}", path.display(), e))
}

/// First structural difference between two JSON trees, if any.
///
/// Mirrors `tools/golden/compare.py`: key order is significant, floats compare
/// exactly (the port must reproduce Python's arithmetic), and the *first line*
/// of a `comment` string is ignored because upstream picks it with an unseeded
/// `random.choice`. Persona-line correctness is verified separately by
/// [`assert_persona_line_known`].
pub fn first_difference(expected: &Value, actual: &Value, path: &str) -> Option<String> {
    match (expected, actual) {
        (Value::Object(e), Value::Object(a)) => {
            for (k, ev) in e {
                match a.get(k) {
                    None => return Some(format!("{}.{}: MISSING in actual", path, k)),
                    Some(av) => {
                        if let Some(d) = first_difference(ev, av, &format!("{}.{}", path, k)) {
                            return Some(d);
                        }
                    }
                }
            }
            for k in a.keys() {
                if !e.contains_key(k) {
                    return Some(format!("{}.{}: EXTRA in actual", path, k));
                }
            }
            let ek: Vec<&String> = e.keys().collect();
            let ak: Vec<&String> = a.keys().collect();
            if ek != ak {
                return Some(format!("{}: KEY ORDER {:?} != {:?}", path, ek, ak));
            }
            None
        }
        (Value::Array(e), Value::Array(a)) => {
            if e.len() != a.len() {
                return Some(format!("{}: LENGTH {} != {}", path, e.len(), a.len()));
            }
            for (i, (ev, av)) in e.iter().zip(a.iter()).enumerate() {
                if let Some(d) = first_difference(ev, av, &format!("{}[{}]", path, i)) {
                    return Some(d);
                }
            }
            None
        }
        (Value::String(e), Value::String(a)) if e.contains('\n') && a.contains('\n') => {
            let e_rest = e.split_once('\n').map(|x| x.1).unwrap_or("");
            let a_rest = a.split_once('\n').map(|x| x.1).unwrap_or("");
            if e_rest != a_rest {
                return Some(format!("{}: {} != {} (tail)", path, e_rest, a_rest));
            }
            None
        }
        (e, a) if e == a => None,
        (e, a) => Some(format!("{}: {} != {}", path, e, a)),
    }
}

/// Assert two JSON trees are identical under the golden comparison rules.
#[track_caller]
pub fn assert_json_eq(actual: &Value, expected: &Value, label: &str) {
    if let Some(diff) = first_difference(expected, actual, "$") {
        panic!("{}: JSON mismatch\n  {}", label, diff);
    }
}

/// Assert a persona flavor line is one of the lines upstream could have chosen.
///
/// Upstream calls `random.choice(lines)` over `investor_personas.PERSONAS[id][signal]`
/// (or the generic fallback when the investor is unregistered), then formats it
/// with the context values. The pool is dumped to
/// `golden_dir()/<case>/persona_pools.json` as `{id: {signal: [lines...]}}`.
#[track_caller]
pub fn assert_persona_line_known(
    pool: &Value,
    investor_id: &str,
    signal: &str,
    line: &str,
    ctx: &Value,
) {
    let entry = pool
        .get(investor_id)
        .and_then(|v| v.get(signal))
        .and_then(|v| v.as_array());
    let Some(entry) = entry else {
        return; // investor not in pool: upstream falls back to a generic line
    };
    let ctx_map = ctx.as_object();
    let mut rendered: Vec<String> = Vec::new();
    for line_tpl in entry {
        let Some(t) = line_tpl.as_str() else { continue };
        let mut out = t.to_string();
        if let Some(map) = ctx_map {
            for (k, v) in map {
                let s = match v {
                    Value::Null => "—".to_string(),
                    Value::String(s) => s.clone(),
                    other => crate::py::py_str(other),
                };
                out = out.replace(&format!("{{{}}}", k), &s);
            }
        }
        rendered.push(out);
    }
    assert!(
        rendered.iter().any(|r| r == line),
        "persona line for {} / {} not in upstream pool:\n  got: {}\n  pool: {:?}",
        investor_id,
        signal,
        line,
        rendered
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn detects_value_and_order_differences() {
        assert!(first_difference(&json!({"a": 1}), &json!({"a": 1}), "$").is_none());
        assert!(first_difference(&json!({"a": 1}), &json!({"a": 2}), "$").is_some());
        assert!(first_difference(&json!({"a": 1, "b": 2}), &json!({"b": 2, "a": 1}), "$")
            .unwrap()
            .contains("KEY ORDER"));
        assert!(first_difference(&json!({"a": 1}), &json!({"b": 1}), "$").is_some());
    }

    #[test]
    fn ignores_persona_flavor_line_only() {
        let e = json!({"comment": "random A\n看空核心：PE 34.2"});
        let a = json!({"comment": "random B\n看空核心：PE 34.2"});
        assert!(first_difference(&e, &a, "$").is_none());
        let a = json!({"comment": "random B\n看空核心：PE 99"});
        assert!(first_difference(&e, &a, "$").is_some());
    }

    #[test]
    fn persona_pool_membership_after_formatting() {
        let pool = json!({"buffett": {"bullish": ["ROE {roe} 可以", "安全边际够"]}});
        let ctx = json!({"roe": "11.2"});
        assert_persona_line_known(&pool, "buffett", "bullish", "ROE 11.2 可以", &ctx);
        assert_persona_line_known(&pool, "buffett", "bullish", "安全边际够", &ctx);
        let unknown = std::panic::catch_unwind(|| {
            assert_persona_line_known(&json!({"x": {"bullish": ["a"]}}), "buffett", "bullish", "zzz", &json!({}));
        });
        assert!(unknown.is_ok(), "unregistered investor must not fail");
    }
}
