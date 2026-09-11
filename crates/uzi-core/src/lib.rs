//! `uzi-core` — shared contracts for the Rust port of UZI-Skill's script layer.
//!
//! Mirrors the Python modules that define the data contract between fetch,
//! score, and render stages:
//!
//! | upstream | here |
//! |---|---|
//! | `lib/market_router.py` | [`ticker`] |
//! | `lib/pipeline/schema.py` | [`dim`] |
//! | `lib/pipeline/validators.py` | [`validators`] |
//! | `lib/cache.py` | [`cache`] |
//! | `lib/name_matcher.py` | [`name_matcher`] |
//! | CPython `random` | [`pyrandom`] |

pub mod assets;
pub mod cache;
pub mod crypto;
pub mod dim;
pub mod features;
pub mod json;
pub mod name_matcher;
pub mod py;
pub mod pyrandom;
pub mod testkit;
pub mod ticker;
pub mod validators;

pub use dim::{DimResult, FetcherSpec, Quality};
pub use ticker::{parse_ticker, TickerInfo};
