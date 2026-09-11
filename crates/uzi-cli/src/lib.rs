//! `uzi-cli` — the Rust replacement for `run.py` / `run_real_test.py`.
//!
//! Hosts the stage orchestration plus the CLI-side validation modules (depth
//! profile, agent-review contract, report serving, update check).

pub const VERSION: &str = "3.9.4";

pub mod agent_review;
pub mod methods;
pub mod preview;
pub mod profile;
pub mod serve;
pub mod stages;
pub mod update_check;
