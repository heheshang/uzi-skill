//! Port of `lib/tier1/` — single-stock / portfolio tier-1 products:
//! AI readiness, pre-earnings preview, incremental model update, portfolio
//! rebalance and returns attribution.

pub mod ai_readiness;
pub mod earnings_preview;
pub mod model_update;
pub mod rebalance;
pub mod returns_attrib;

pub use ai_readiness::build_ai_readiness;
pub use earnings_preview::build_earnings_preview;
pub use model_update::build_model_update;
pub use rebalance::build_rebalance;
pub use returns_attrib::build_returns_attribution;
