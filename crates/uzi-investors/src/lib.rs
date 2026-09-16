//! UZI-Skill investor layer — Rust port of the upstream Python modules
//! `lib/investor_db.py`, `lib/investor_criteria.py`, `lib/investor_evaluator.py`,
//! `lib/investor_personas.py`, `lib/investor_knowledge.py`,
//! `lib/investor_profile.py`, `lib/seat_db.py` and `lib/personas.py`.
//!
//! Cross-crate interfaces are `serde_json::Value` (dicts in, dicts out), matching
//! the upstream JSON contract.

pub mod criteria;
pub mod db;
pub mod evaluator;
pub mod knowledge;
pub mod persona_yaml;
pub mod personas;
pub mod profile;
pub mod seat_db;

pub(crate) mod pyhelp;

pub use db::{investor_by_id, investors};
pub use evaluator::evaluate_investor;
pub use personas::{crypto_persona_comment, persona_comment, persona_comment_seeded};

pub fn version() -> &'static str {
    "3.9.4"
}
