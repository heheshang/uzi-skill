//! `uzi-report` — rendering layer of the UZI-Skill port.
//!
//! Ports the upstream Python report modules:
//!
//! | upstream | here |
//! |---|---|
//! | `lib/report/security.py` | [`security`] |
//! | `lib/report/svg_primitives.py` | [`svg`] |
//! | `lib/report/dim_viz.py` | [`dim_viz`] |
//! | `lib/report/global_peers.py` | [`global_peers`] |
//! | `lib/report/panel_cards.py` | [`panel_cards`] |
//! | `lib/report/special_cards.py` | [`special_cards`] |
//! | `lib/report/institutional.py` | [`institutional`] |
//! | `lib/report/segmental.py` | [`segmental`] |
//! | `lib/pipeline/renderer/*` | [`renderer`] |
//! | `assemble_report.py` | [`assemble`] |
//! | `inline_assets.py` | [`inline`] |
//! | `render_share_card.py` / `render_war_report.py` | [`cards`] |
//! | `gen_pixel_avatars.py` | [`avatars`] |

mod pyfmt;

pub mod assemble;
pub mod avatars;
pub mod cards;
pub mod dim_viz;
pub mod global_peers;
pub mod inline;
pub mod institutional;
pub mod panel_cards;
pub mod renderer;
pub mod security;
pub mod segmental;
pub mod special_cards;
pub mod svg;

pub use assemble::assemble;

pub fn version() -> &'static str {
    "3.9.4"
}
