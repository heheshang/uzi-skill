//! Port of `lib/investor_profile.py` — the per-investor authentic decision profile
//! (`time_horizon` / `position_sizing` / `what_would_change_my_mind`).
//!
//! `src/data/profiles.json` holds `{profiles, group_default, generic_fallback}`
//! dumped verbatim from the upstream module.

use serde_json::{Map, Value};
use std::sync::LazyLock;

const PROFILES_JSON: &str = include_str!("data/profiles.json");

fn tables() -> &'static Value {
    static TABLES: LazyLock<Value> =
        LazyLock::new(|| serde_json::from_str(PROFILES_JSON).expect("embedded profiles json"));
    &TABLES
}

/// `investor_profile.get_profile` — authored profile, else group default, else
/// the generic `"—"` fallback. Returns a fresh dict in the source key order.
pub fn get_profile(investor_id: &str, group: &str) -> Value {
    let t = tables();
    if let Some(p) = t.get("profiles").and_then(|p| p.get(investor_id)) {
        return p.clone();
    }
    if !group.is_empty() {
        if let Some(g) = t.get("group_default").and_then(|g| g.get(group)) {
            return g.clone();
        }
    }
    t.get("generic_fallback").cloned().unwrap_or_else(|| {
        let mut m = Map::new();
        m.insert("time_horizon".into(), Value::String("—".into()));
        m.insert("position_sizing".into(), Value::String("—".into()));
        m.insert("what_would_change_my_mind".into(), Value::String("—".into()));
        Value::Object(m)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn authored_profile_wins_then_group_default_then_generic() {
        assert!(get_profile("buffett", "A")["time_horizon"]
            .as_str()
            .unwrap()
            .contains("10 年"));
        // buffett's group default is different from his authored profile
        assert_ne!(get_profile("buffett", "A"), get_profile("unknown_id", "A"));
        // every group has a fallback
        for g in ["A", "B", "C", "D", "E", "F", "G", "H", "I"] {
            assert_ne!(get_profile("no_such_investor", g)["time_horizon"], "—");
        }
        for v in get_profile("no_such_investor", "Z").as_object().unwrap().values() {
            assert_eq!(v, "—");
        }
    }
}
