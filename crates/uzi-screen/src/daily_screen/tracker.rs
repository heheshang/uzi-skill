//! Port of `lib/daily_screen/tracker.py` — append-only paper-trading signal
//! ledger.

use serde_json::{json, Value};
use std::io::Write;
use std::path::Path;

/// `append_signals(path, report)` → number of lines appended.
pub fn append_signals(path: &Path, report: &Value) -> std::io::Result<usize> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let picks = report
        .get("picks")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let mut payload = String::new();
    let mut count = 0;
    for pick in picks {
        let signal = json!({
            "report_id": report.get("report_id").cloned().unwrap_or(Value::Null),
            "generated_at": report.get("generated_at").cloned().unwrap_or(Value::Null),
            "performance_contract": report
                .get("performance_contract")
                .cloned()
                .unwrap_or(Value::Null),
            "signal": pick,
        });
        // `separators=(",", ":")` — compact JSON with no padding.
        payload.push_str(&uzi_core::json::to_compact(&signal));
        payload.push('\n');
        count += 1;
    }
    if payload.is_empty() {
        return Ok(0);
    }
    let mut options = std::fs::OpenOptions::new();
    options.create(true).append(true);
    let mut file = options.open(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        // `os.open(path, O_APPEND|O_CREAT|O_WRONLY, 0o600)`.
        let _ = file.set_permissions(std::fs::Permissions::from_mode(0o600));
    }
    file.write_all(payload.as_bytes())?;
    Ok(count)
}
