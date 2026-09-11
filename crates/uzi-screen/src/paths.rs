//! Filesystem roots for the screening runners.
//!
//! Upstream derives every path from its own location:
//! `SCRIPTS_DIR = Path(__file__).resolve().parents[1]`,
//! `ASSETS_DIR = SCRIPTS_DIR.parent / "assets"`. A compiled binary has no
//! `__file__`, so the two production-equivalent roots are environment-overridable
//! and otherwise default to the working directory (which is how upstream is
//! invoked: `cd skills/deep-analysis && python run.py …`):
//!
//! * [`reports_root`] — upstream `<scripts>/reports`
//! * [`assets_dir`] — upstream `<deep-analysis>/assets`
//!
//! The cache root comes from `uzi_core::cache::cache_root` (`UZI_CACHE_ROOT`).

use std::path::PathBuf;

/// `<scripts>/reports` (`UZI_REPORTS_ROOT` overrides).
pub fn reports_root() -> PathBuf {
    match std::env::var("UZI_REPORTS_ROOT") {
        Ok(p) if !p.is_empty() => PathBuf::from(p),
        _ => PathBuf::from("reports"),
    }
}

/// `<deep-analysis>/assets` (`UZI_ASSETS_DIR` overrides).
pub fn assets_dir() -> PathBuf {
    match std::env::var("UZI_ASSETS_DIR") {
        Ok(p) if !p.is_empty() => PathBuf::from(p),
        _ => PathBuf::from("assets"),
    }
}

/// Upstream `<scripts>/.cache/_daily_screen/signals.jsonl`.
pub fn daily_screen_ledger() -> PathBuf {
    uzi_core::cache::cache_root()
        .join("_daily_screen")
        .join("signals.jsonl")
}
