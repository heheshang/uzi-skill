//! Port of `lib/agent_review.py` — freshness contract between the collected
//! evidence snapshot and the agent role-play output.

use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::path::Path;

pub const CONTEXT_FILE: &str = "_agent_review_context.json";

/// Deep runs always demand role-play; otherwise a written context file can opt in.
pub fn requires_agent_review(cache_dir: &Path) -> bool {
    if std::env::var("UZI_DEPTH").map(|d| d == "deep").unwrap_or(false) {
        return true;
    }
    match std::fs::read_to_string(cache_dir.join(CONTEXT_FILE)) {
        Ok(text) => match serde_json::from_str::<Value>(&text) {
            Ok(Value::Object(map)) => map.get("depth").and_then(|d| d.as_str()) == Some("deep"),
            _ => false,
        },
        Err(_) => false,
    }
}

/// `sha256(json.dumps(raw, ensure_ascii=False, sort_keys=True, separators=(",", ":")))`.
///
/// Keys are sorted at every level and the compact separators are Python's
/// no-whitespace form, so the hash is stable across field insertion order.
pub fn analysis_input_hash(raw: &Value) -> String {
    let mut out = String::new();
    write_canonical(raw, &mut out);
    let mut hasher = Sha256::new();
    hasher.update(out.as_bytes());
    format!("{:x}", hasher.finalize())
}

fn write_canonical(value: &Value, out: &mut String) {
    match value {
        Value::Null => out.push_str("null"),
        Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Value::Number(n) => out.push_str(&n.to_string()),
        Value::String(s) => out.push_str(&serde_json::to_string(s).unwrap_or_default()),
        Value::Array(a) => {
            out.push('[');
            for (i, v) in a.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_canonical(v, out);
            }
            out.push(']');
        }
        Value::Object(o) => {
            let mut keys: Vec<&String> = o.keys().collect();
            keys.sort();
            out.push('{');
            for (i, k) in keys.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                out.push_str(&serde_json::to_string(k).unwrap_or_default());
                out.push(':');
                write_canonical(&o[*k], out);
            }
            out.push('}');
        }
    }
}

/// Write `_agent_review_context.json` and return its contents.
pub fn write_review_context(cache_dir: &Path, raw: &Value) -> std::io::Result<Value> {
    std::fs::create_dir_all(cache_dir)?;
    let context = json!({
        "ticker": raw.get("full").filter(|v| !v.is_null()).cloned()
            .unwrap_or_else(|| raw.get("ticker").cloned().unwrap_or(Value::Null)),
        "analysis_input_hash": analysis_input_hash(raw),
        "raw_fetched_at": raw.get("fetched_at").cloned().unwrap_or(Value::Null),
        "depth": std::env::var("UZI_DEPTH").unwrap_or_else(|_| "medium".into()),
        "generated_at": now_utc_seconds(),
    });
    let target = cache_dir.join(CONTEXT_FILE);
    let tmp = target.with_extension("json.tmp");
    std::fs::write(&tmp, uzi_core::json::to_pretty(&context))?;
    std::fs::rename(&tmp, &target)?;
    Ok(context)
}

fn now_utc_seconds() -> String {
    let now = chrono::Utc::now();
    now.format("%Y-%m-%dT%H:%M:%S+00:00").to_string()
}

/// Returns `(payload, reason)`. `payload` is `None` when the analysis must be
/// rejected (missing, stale fingerprint, or older than the raw snapshot).
pub fn load_fresh_agent_analysis(cache_dir: &Path, raw: &Value) -> (Option<Value>, String) {
    let analysis_path = cache_dir.join("agent_analysis.json");
    let raw_path = cache_dir.join("raw_data.json");
    if !analysis_path.exists() {
        return (None, "agent_analysis.json 缺失".into());
    }
    let payload: Value = match std::fs::read_to_string(&analysis_path)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
    {
        Some(v) => v,
        None => return (None, "agent_analysis.json 无法读取: JSONDecodeError".into()),
    };
    let is_object = payload.is_object();
    if !is_object || payload.get("agent_reviewed").and_then(|v| v.as_bool()) != Some(true) {
        return (None, "agent_reviewed 未设置为 true".into());
    }

    let current_hash = analysis_input_hash(raw);
    let declared = payload
        .get("analysis_input_hash")
        .and_then(|v| v.as_str())
        .map(str::to_string);
    if let Some(declared) = declared {
        if declared != current_hash {
            return (None, "analysis_input_hash 与当前 raw_data 不一致".into());
        }
        return (Some(payload), "fingerprint matched".into());
    }

    if requires_agent_review(cache_dir) {
        return (None, "deep 档必须提供 analysis_input_hash".into());
    }

    match (raw_path.metadata(), analysis_path.metadata()) {
        (Ok(raw_meta), Ok(analysis_meta)) => {
            let raw_ns = mtime_ns(&raw_meta);
            let analysis_ns = mtime_ns(&analysis_meta);
            if let (Some(raw_ns), Some(analysis_ns)) = (raw_ns, analysis_ns) {
                if analysis_ns < raw_ns {
                    return (None, "agent_analysis.json 早于当前 raw_data".into());
                }
            }
        }
        _ => return (None, "无法校验分析时间: OSError".into()),
    }
    (
        Some(payload),
        "mtime matched (legacy analysis without fingerprint)".into(),
    )
}

fn mtime_ns(meta: &std::fs::Metadata) -> Option<u128> {
    use std::time::UNIX_EPOCH;
    meta.modified().ok()?.duration_since(UNIX_EPOCH).ok().map(|d| d.as_nanos())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn hash_is_order_insensitive_and_stable() {
        let a = json!({"b": 1, "a": {"y": 2, "x": [1, 2]}});
        let b = json!({"a": {"x": [1, 2], "y": 2}, "b": 1});
        assert_eq!(analysis_input_hash(&a), analysis_input_hash(&b));
        // known vector: sha256('{"a":1,"b":2}')
        assert_eq!(
            analysis_input_hash(&json!({"b": 2, "a": 1})),
            "43258cff783fe7036d8a43033f830adfc60ec037382473548ac742b888292777"
        );
    }

    #[test]
    fn hash_separators_match_python_compact_form() {
        // python: json.dumps({"a": "x y"}, ensure_ascii=False, sort_keys=True, separators=(",", ":"))
        let payload = json!({"a": "x y"});
        let mut canonical = String::new();
        write_canonical(&payload, &mut canonical);
        assert_eq!(canonical, "{\"a\":\"x y\"}");
    }

    #[test]
    fn missing_analysis_is_rejected() {
        let dir = std::env::temp_dir().join("uzi_agent_review_missing");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let (payload, reason) = load_fresh_agent_analysis(&dir, &json!({"ticker": "X"}));
        assert!(payload.is_none());
        assert_eq!(reason, "agent_analysis.json 缺失");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn declared_hash_must_match_current_raw() {
        let dir = std::env::temp_dir().join("uzi_agent_review_stale");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let raw = json!({"ticker": "600519.SH"});
        std::fs::write(
            dir.join("agent_analysis.json"),
            json!({"agent_reviewed": true, "analysis_input_hash": "deadbeef"}).to_string(),
        )
        .unwrap();
        let (payload, reason) = load_fresh_agent_analysis(&dir, &raw);
        assert!(payload.is_none());
        assert_eq!(reason, "analysis_input_hash 与当前 raw_data 不一致");

        // and the matching hash is accepted
        let hash = analysis_input_hash(&raw);
        std::fs::write(
            dir.join("agent_analysis.json"),
            json!({"agent_reviewed": true, "analysis_input_hash": hash}).to_string(),
        )
        .unwrap();
        let (payload, reason) = load_fresh_agent_analysis(&dir, &raw);
        assert!(payload.is_some());
        assert_eq!(reason, "fingerprint matched");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
