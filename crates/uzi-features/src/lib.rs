//! uzi-features — Rust port of UZI-Skill script layer.
//!
//! | upstream | here |
//! |---|---|
//! | `lib/stock_features.py` | [`stock_features`] |
//! | `lib/stock_style.py` | [`stock_style`] |
//! | `lib/quant_signal.py` | [`quant_signal`] (network-free) |
//! | `compute_friendly.py` | [`friendly`] |
//! | `lib/segmental_model.py`, `compute_segmental.py` | [`segmental`] |

use serde_json::Value;

pub mod friendly;
pub mod quant_signal;
pub mod segmental;
pub mod stock_features;
pub mod stock_style;

pub use friendly::{build_friendly, compute_exit_triggers, compute_scenarios};
pub use segmental::{discover_segments, render_skeleton_markdown, validate_model};
pub use stock_features::summary;
pub use stock_style::{apply_style_weights, detect_style, style_explanation, style_label};

/// `lib/stock_features.sanitize_features` (implemented in `uzi-core`).
pub use uzi_core::features::sanitize_features;

/// `stock_features.extract_features` — flat, typed feature dict.
pub fn extract_features(raw: &Value, dims: &Value) -> Value {
    stock_features::extract_features(raw, dims)
}
