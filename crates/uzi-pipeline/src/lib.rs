//! `uzi-pipeline` — Rust port of `lib/pipeline/score_fns.py`.
//!
//! Owns the scoring, panel and synthesis stages:
//! `score_dimensions` → `generate_panel` → `generate_synthesis`.

pub mod junk;
pub mod panel;
pub mod score;
pub mod summarize;
pub mod synthesis;

pub use panel::generate_panel;
pub use score::score_dimensions;
pub use synthesis::generate_synthesis;
