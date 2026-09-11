//! Port of `lib/pipeline/renderer/__init__.py` — section renderer registry.

pub mod base;
pub mod basic_header;
pub mod capital_flow;
pub mod chain;
pub mod contests;
pub mod events;
pub mod financials;
pub mod fund;
pub mod futures;
pub mod governance;
pub mod industry;
pub mod kline;
pub mod lhb;
pub mod macro_;
pub mod materials;
pub mod moat;
pub mod peers;
pub mod policy;
pub mod registry;
pub mod research;
pub mod sentiment;
pub mod trap;
pub mod valuation;

pub use base::{RenderContext, SectionRenderer};
pub use registry::{get_renderer, list_renderers, RENDERER_KEYS};
