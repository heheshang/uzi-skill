//! `uzi-screen` — the Rust port of UZI-Skill's screening layer.
//!
//! | upstream | here |
//! |---|---|
//! | `lib/daily_screen/{models,universe,sources,events,themes,personas,ranker,execution,renderer,runner,tracker}.py` | [`daily_screen`] |
//! | `lib/versus_runner.py` | [`versus`] |
//! | `lib/portfolio_runner.py` | [`portfolio`] |
//! | `lib/fund_holdings_runner.py` | [`fund_holdings`] |
//! | `screen.py` | [`run_daily_screen`] |
//!
//! Cross-crate interfaces are `serde_json::Value` (shared contract HARD RULE 1).
//!
//! Two upstream imports cannot be reproduced by a crate dependency:
//!
//! * `lib.pipeline.run.run_pipeline` lives in `uzi-cli` (which depends on this
//!   crate), so `versus_runner` / `portfolio_runner` / `fund_holdings_runner`
//!   reach it through the process-wide hook in [`providers`].
//! * `lib/hottrend.py` is owned by `uzi-data` (see crate ownership in the
//!   shared contract) and is intentionally not ported here.

pub mod daily_screen;
pub mod fund_holdings;
pub mod paths;
pub mod portfolio;
pub mod providers;
pub mod versus;

mod csvlite;

pub use daily_screen::runner::run_daily_screen;
pub use fund_holdings::run_fund_holdings;
pub use portfolio::run_portfolio;
pub use versus::run_versus;

pub fn version() -> &'static str {
    "3.9.4"
}
