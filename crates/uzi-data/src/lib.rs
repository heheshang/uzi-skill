//! `uzi-data` — the data-acquisition layer of UZI-Skill.
//!
//! Mirrors upstream `skills/deep-analysis/scripts/`:
//!
//! | upstream | here |
//! |---|---|
//! | `lib/providers/*.py` | [`providers`] |
//! | `lib/data_sources.py` | [`sources`] |
//! | `lib/data_source_registry.py` | [`registry`] |
//! | `lib/hk_data_sources.py` | [`hk`] |
//! | `lib/news_providers.py` | [`news`] |
//! | `lib/mx_api.py` | [`mx`] |
//! | `lib/web_search.py` | [`web_search`] |
//! | `lib/industry_mapping.py` | [`industry`] |
//! | `lib/hottrend.py` | [`hottrend`] |
//! | `lib/global_peers.py` (network half) | [`global_peers`] |
//! | `lib/pipeline/collect.py` | [`collect`] |
//! | `lib/pipeline/base_fetcher.py` | [`base_fetcher`] |
//! | `lib/pipeline/fetchers/registry.py` | [`fetchers`] |
//! | `lib/pipeline/process_runner.py` | [`process_runner`] |
//! | `lib/pipeline/preflight_helpers.py` | [`preflight`] |
//! | `lib/network_preflight.py` | [`network_preflight`] |
//! | `lib/net_timeout_guard.py` | [`http`] |
//! | `lib/xueqiu_browser.py` + `lib/playwright_fallback.py` | [`browser`] |
//! | `lib/junk_filter.py` | [`junk_filter`] |
//! | `prewarm_cache.py` | [`prewarm`] |
//! | `fetch_*.py` | [`fetch`] |
//!
//! Cross-crate interfaces are `serde_json::Value` per the shared contract.

pub mod base_fetcher;
pub mod browser;
pub mod collect;
pub mod crypto;
pub mod em;
pub mod fetch;
pub mod fetchers;
pub mod global_peers;
pub mod hk;
pub mod hottrend;
pub mod http;
pub mod industry;
pub mod junk_filter;
pub mod mx;
pub mod network_preflight;
pub mod news;
pub mod prewarm;
pub mod preflight;
pub mod process_runner;
pub mod providers;
pub mod registry;
pub mod sources;
pub mod web_search;

pub use collect::collect;
pub use preflight::{autofill_qualitative_via_mx, prepare_target};

pub fn version() -> &'static str {
    "3.9.4"
}
