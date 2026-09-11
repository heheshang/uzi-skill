//! Process-wide hook for the single-stock pipeline.
//!
//! Upstream `lib/versus_runner.py` / `lib/portfolio_runner.py` /
//! `lib/fund_holdings_runner.py` lazily `from lib.pipeline.run import
//! run_pipeline as _run_pipeline` and call it with `resume=True`. That module is
//! owned by `uzi-cli`, which depends on this crate, so the dependency cannot be
//! expressed directly. Instead `uzi-cli` registers its `run_pipeline` equivalent
//! here at startup; when it is absent the callers degrade exactly like upstream
//! does when the import raises.
//!
//! The hook receives the ticker and returns the report payload (`run_pipeline`'s
//! return value: upstream a `Path`/`str`, here the JSON value the CLI holds).

use serde_json::Value;
use std::sync::OnceLock;

type PipelineRunner = Box<dyn Fn(&str) -> anyhow::Result<Value> + Send + Sync>;

static PIPELINE: OnceLock<PipelineRunner> = OnceLock::new();

/// Register the pipeline entry point. Later calls are ignored (the CLI registers
/// once during startup).
pub fn set_pipeline_runner<F>(runner: F)
where
    F: Fn(&str) -> anyhow::Result<Value> + Send + Sync + 'static,
{
    let _ = PIPELINE.set(Box::new(runner));
}

/// `run_pipeline(ticker, resume=True)` — `None` when no runner is registered.
pub fn run_pipeline(ticker: &str) -> Option<anyhow::Result<Value>> {
    PIPELINE.get().map(|runner| runner(ticker))
}

/// Best-effort extraction of a report path from a pipeline result.
///
/// Upstream returns a `Path`; the Rust CLI may return a JSON object holding it
/// (`report_path` / `path`) or a bare string.
pub fn report_path_of(value: &Value) -> Option<String> {
    match value {
        Value::String(s) => Some(s.clone()),
        Value::Object(map) => ["report_path", "path", "summary_html"]
            .iter()
            .find_map(|key| map.get(*key).and_then(|v| v.as_str()).map(str::to_string)),
        _ => None,
    }
}
