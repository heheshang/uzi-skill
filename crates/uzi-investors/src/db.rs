//! Port of `lib/investor_db.py` — the 66-investor panel roster.
//!
//! The upstream table is pure data; `src/data/investors.json` is
//! `json.dumps(INVESTORS, ensure_ascii=False)` (identical to
//! `tools/golden/expected/*/investors.json`), so ids, names, groups, mandates,
//! field whitelists and **key order** are preserved exactly.

use serde_json::Value;
use std::sync::LazyLock;

const INVESTORS_JSON: &str = include_str!("data/investors.json");

/// `investor_db.INVESTORS`, in source order.
pub fn investors() -> &'static Vec<Value> {
    static INVESTORS: LazyLock<Vec<Value>> =
        LazyLock::new(|| serde_json::from_str(INVESTORS_JSON).expect("embedded INVESTORS json"));
    &INVESTORS
}

/// `investor_db.by_id` — `None` when the id is unknown.
pub fn investor_by_id(investor_id: &str) -> Option<&'static Value> {
    investors().iter().find(|i| i.get("id").and_then(Value::as_str) == Some(investor_id))
}

/// `investor_db.by_group`.
pub fn by_group(group: &str) -> Vec<&'static Value> {
    investors()
        .iter()
        .filter(|i| i.get("group").and_then(Value::as_str) == Some(group))
        .collect()
}

/// `investor_db.all_ids`.
pub fn all_ids() -> Vec<&'static str> {
    investors()
        .iter()
        .filter_map(|i| i.get("id").and_then(Value::as_str))
        .collect()
}

/// `investor_db.assert_count` — v3.9.0 expects 66 investors.
pub fn assert_count() {
    assert_eq!(
        investors().len(),
        66,
        "Expected 66 investors, got {}",
        investors().len()
    );
}

/// `investor_db.assert_50` (backwards-compat alias).
pub fn assert_50() {
    assert_count();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_entry_has_the_required_keys_in_source_order() {
        for inv in investors() {
            let obj = inv.as_object().expect("investor is a dict");
            let keys: Vec<&str> = obj.keys().map(String::as_str).collect();
            assert_eq!(keys[0], "id");
            assert_eq!(keys[1], "name");
            assert!(obj.contains_key("group"));
            assert!(obj.contains_key("fields"));
            assert!(!all_ids().is_empty());
        }
        assert_eq!(all_ids().len(), 66);
        assert_eq!(all_ids()[0], "buffett");
    }

    #[test]
    fn lookups_match_the_roster() {
        assert_eq!(
            investor_by_id("zhao_lg").unwrap().get("name").unwrap(),
            "赵老哥"
        );
        assert!(investor_by_id("nobody").is_none());
        let f = by_group("F");
        assert!(f.iter().all(|i| i["group"] == "F"));
        // short-sellers carry an explicit mandate; everyone else defaults to long
        assert_eq!(investor_by_id("burry").unwrap()["mandate"], "short");
        assert_eq!(investor_by_id("chanos").unwrap()["mandate"], "short");
        assert!(investor_by_id("buffett").unwrap().get("mandate").is_none());
    }
}
