//! Port of `lib/name_matcher.py` — fuzzy matching for Chinese stock names.
//!
//! Two-stage pipeline, identical to upstream:
//!
//! 1. **character-set Jaccard pre-filter** — cheap; rules out the bulk of the
//!    ~5000-name A-share universe before any edit-distance work;
//! 2. **Levenshtein ranking** on survivors — accurate; picks the right
//!    character reorder (upstream's example: `北部港湾` → `北部湾港`).
//!
//! The A-share `(code, name)` table is *data*, not logic: upstream asks akshare
//! (`stock_info_a_code_name`) behind a 7-day cache. That network half lives in
//! `uzi-data`; this module takes the index as a slice so the matching rules stay
//! pure, allocation-lean, and differentially testable against Python.
//!
//! All metrics iterate **Unicode scalar values**, matching CPython's `str`
//! iteration over code points.

use std::collections::HashSet;
use std::cmp::Ordering;

use serde_json::{Map, Value};

/// Upstream `fuzzy_match(top_k=5)`.
pub const DEFAULT_TOP_K: usize = 5;
/// Upstream `fuzzy_match(max_distance=2)`.
pub const DEFAULT_MAX_DISTANCE: usize = 2;
/// Upstream `fuzzy_match(min_jaccard=0.6)`.
pub const DEFAULT_MIN_JACCARD: f64 = 0.6;

/// One `(code, name)` row of the A-share index.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NameEntry {
    pub code: String,
    pub name: String,
}

impl NameEntry {
    /// Read one `{"code", "name"}` row; `None` when either field is unusable.
    ///
    /// Upstream keys the index by the akshare column names and skips rows whose
    /// name is falsy.
    pub fn from_value(v: &Value) -> Option<Self> {
        let name = v.get("name")?.as_str()?.to_string();
        if name.is_empty() {
            return None;
        }
        let code = match v.get("code") {
            Some(Value::String(s)) => s.clone(),
            Some(Value::Number(n)) => n.to_string(),
            _ => return None,
        };
        Some(NameEntry { code, name })
    }

    /// `{"code", "name"}` — the upstream index row shape.
    pub fn to_value(&self) -> Value {
        let mut m = Map::new();
        m.insert("code".into(), Value::from(self.code.clone()));
        m.insert("name".into(), Value::from(self.name.clone()));
        Value::Object(m)
    }
}

/// Build the index slice from the JSON rows the fetcher returns.
pub fn index_from_values(rows: &[Value]) -> Vec<NameEntry> {
    rows.iter().filter_map(NameEntry::from_value).collect()
}

/// One scored candidate — upstream `{"code", "name", "distance", "jaccard"}`.
#[derive(Debug, Clone, PartialEq)]
pub struct Match {
    pub code: String,
    pub name: String,
    pub distance: usize,
    /// Already rounded to 3 decimals, exactly as upstream emits it.
    pub jaccard: f64,
}

impl Match {
    pub fn to_value(&self) -> Value {
        let mut m = Map::new();
        m.insert("code".into(), Value::from(self.code.clone()));
        m.insert("name".into(), Value::from(self.name.clone()));
        m.insert("distance".into(), Value::from(self.distance as i64));
        m.insert("jaccard".into(), Value::from(self.jaccard));
        Value::Object(m)
    }
}

/// `levenshtein(a, b)` — classic two-row DP.
///
/// `O(len(a) * len(b))` time, `O(len(b))` space, iterating code points like
/// Python's `str`.
pub fn levenshtein(a: &str, b: &str) -> usize {
    if a == b {
        return 0;
    }
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    if a.is_empty() {
        return b.len();
    }
    if b.is_empty() {
        return a.len();
    }

    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut curr = vec![0usize; b.len() + 1];
    for (i, ca) in a.iter().enumerate() {
        curr[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let cost = usize::from(ca != cb);
            curr[j + 1] = (curr[j] + 1).min(prev[j + 1] + 1).min(prev[j] + cost);
        }
        std::mem::swap(&mut prev, &mut curr);
    }
    prev[b.len()]
}

/// `char_set_jaccard(a, b)` — character-set Jaccard similarity, order-insensitive.
pub fn char_set_jaccard(a: &str, b: &str) -> f64 {
    let sa: HashSet<char> = a.chars().collect();
    let sb: HashSet<char> = b.chars().collect();
    if sa.is_empty() && sb.is_empty() {
        return 1.0;
    }
    let union = sa.union(&sb).count();
    if union == 0 {
        return 0.0;
    }
    sa.intersection(&sb).count() as f64 / union as f64
}

/// `fuzzy_match(query, top_k=5, max_distance=2, min_jaccard=0.6)`.
///
/// Returns candidates sorted by `(distance asc, jaccard desc)` and truncated to
/// `top_k`. Empty when the index is empty or nothing clears the thresholds.
///
/// Queries shorter than 3 code points relax the Jaccard floor to `0.5`, since a
/// 2-character name cannot overlap much to begin with.
pub fn fuzzy_match(
    query: &str,
    index: &[NameEntry],
    top_k: usize,
    max_distance: usize,
    min_jaccard: f64,
) -> Vec<Match> {
    let query = query.trim();
    if query.is_empty() || index.is_empty() {
        return Vec::new();
    }

    let eff_jaccard = if query.chars().count() >= 3 {
        min_jaccard
    } else {
        0.5
    };

    // Stage 1 — cheap Jaccard pre-filter. The raw (unrounded) similarity is
    // carried into stage 2 and only rounded on output, like upstream.
    let mut shortlist: Vec<(&NameEntry, f64)> = Vec::new();
    for entry in index {
        let j = char_set_jaccard(query, &entry.name);
        if j >= eff_jaccard {
            shortlist.push((entry, j));
        }
    }
    if shortlist.is_empty() {
        return Vec::new();
    }

    // Stage 2 — Levenshtein ranking.
    let mut scored: Vec<Match> = Vec::new();
    for (entry, j) in shortlist {
        let distance = levenshtein(query, &entry.name);
        if distance <= max_distance {
            scored.push(Match {
                code: entry.code.clone(),
                name: entry.name.clone(),
                distance,
                jaccard: crate::py::round(j, 3),
            });
        }
    }

    scored.sort_by(|x, y| {
        x.distance
            .cmp(&y.distance)
            .then_with(|| y.jaccard.partial_cmp(&x.jaccard).unwrap_or(Ordering::Equal))
    });
    scored.truncate(top_k);
    scored
}

/// `fuzzy_match(query, top_k, max_distance)` with upstream's 0.6 Jaccard floor.
pub fn fuzzy_match_default(query: &str, index: &[NameEntry], top_k: usize) -> Vec<Match> {
    fuzzy_match(query, index, top_k, DEFAULT_MAX_DISTANCE, DEFAULT_MIN_JACCARD)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn idx(pairs: &[(&str, &str)]) -> Vec<NameEntry> {
        pairs
            .iter()
            .map(|(code, name)| NameEntry {
                code: (*code).to_string(),
                name: (*name).to_string(),
            })
            .collect()
    }

    #[test]
    fn levenshtein_matches_known_distances() {
        assert_eq!(levenshtein("", ""), 0);
        assert_eq!(levenshtein("abc", "abc"), 0);
        assert_eq!(levenshtein("", "abc"), 3);
        assert_eq!(levenshtein("abc", ""), 3);
        // Transposition is two edits, not one.
        assert_eq!(levenshtein("kitten", "sitting"), 3);
        assert_eq!(levenshtein("北部港湾", "北部湾港"), 2);
        assert_eq!(levenshtein("水晶光电", "水晶光电"), 0);
    }

    #[test]
    fn levenshtein_counts_code_points_not_bytes() {
        // Two 3-byte chars vs one: byte length would say 6, code points say 2.
        assert_eq!(levenshtein("光电", "水"), 2);
    }

    #[test]
    fn jaccard_is_order_insensitive_and_bounds_are_exact() {
        assert_eq!(char_set_jaccard("", ""), 1.0);
        assert_eq!(char_set_jaccard("abc", "bca"), 1.0);
        assert_eq!(char_set_jaccard("ab", "cd"), 0.0);
        assert_eq!(char_set_jaccard("ab", ""), 0.0);
        // 1 shared character of a 3-character union.
        assert_eq!(char_set_jaccard("ab", "bc"), 1.0 / 3.0);
    }

    #[test]
    fn reorder_typo_resolves_to_the_right_name() {
        let index = idx(&[
            ("000582", "北部湾港"),
            ("600519", "贵州茅台"),
            ("002273", "水晶光电"),
        ]);
        let hits = fuzzy_match("北部港湾", &index, 5, 2, 0.6);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].code, "000582");
        assert_eq!(hits[0].name, "北部湾港");
        assert_eq!(hits[0].distance, 2);
        assert_eq!(hits[0].jaccard, 1.0);
    }

    #[test]
    fn exact_name_scores_distance_zero() {
        let index = idx(&[("600519", "贵州茅台"), ("000001", "平安银行")]);
        let hits = fuzzy_match("贵州茅台", &index, 5, 2, 0.6);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].distance, 0);
        assert_eq!(hits[0].code, "600519");
    }

    #[test]
    fn empty_and_blank_queries_yield_nothing() {
        let index = idx(&[("600519", "贵州茅台")]);
        assert!(fuzzy_match("", &index, 5, 2, 0.6).is_empty());
        assert!(fuzzy_match("   ", &index, 5, 2, 0.6).is_empty());
        assert!(fuzzy_match("贵州茅台", &[], 5, 2, 0.6).is_empty());
    }

    #[test]
    fn distance_ceiling_and_top_k_are_enforced() {
        let index = idx(&[
            ("600519", "贵州茅台"),
            ("600520", "贵州茅太"),
            ("600521", "贵州茅大"),
            ("600522", "贵州茅天"),
        ]);
        // max_distance=1 admits the exact hit plus the three one-edit neighbours.
        let hits = fuzzy_match("贵州茅台", &index, 5, 1, 0.6);
        assert_eq!(hits.len(), 4, "{hits:?}");
        assert_eq!(hits[0].distance, 0);
        assert!(hits[1..].iter().all(|h| h.distance == 1));

        // max_distance=0 keeps only the exact name.
        let exact = fuzzy_match("贵州茅台", &index, 5, 0, 0.6);
        assert_eq!(exact.len(), 1);
        assert_eq!(exact[0].code, "600519");

        // top_k truncates after ranking, keeping the best.
        let capped = fuzzy_match("贵州茅台", &index, 2, 1, 0.6);
        assert_eq!(capped.len(), 2);
        assert_eq!(capped[0].code, "600519");
    }

    #[test]
    fn ordering_is_distance_then_jaccard_descending() {
        let index = idx(&[
            ("000001", "abcx"),
            ("000002", "abc"),
            ("000003", "abcy"),
        ]);
        // Query "abc": distance 0 for 000002; the two one-edit rows tie on
        // distance and fall back to jaccard descending.
        let hits = fuzzy_match("abc", &index, 5, 2, 0.6);
        assert_eq!(hits[0].code, "000002");
        assert_eq!(hits[0].distance, 0);
        assert_eq!(hits.len(), 3);
        assert!(hits[1].jaccard >= hits[2].jaccard);
    }

    #[test]
    fn short_queries_relax_the_jaccard_floor() {
        // A 2-code-point query shares 2 of 4 characters with a 4-char name →
        // 0.5, which only clears the floor because short queries relax it to 0.5.
        let index = idx(&[("000001", "平安银行")]);
        let hits = fuzzy_match("平安", &index, 5, 2, 0.6);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].jaccard, 0.5);
        assert_eq!(hits[0].distance, 2);

        // The same similarity is rejected once the query is long enough to use
        // the caller's stricter floor.
        let strict = fuzzy_match("平安银", &index, 5, 2, 0.9);
        assert!(strict.is_empty());
    }

    #[test]
    fn entry_and_match_round_trip_through_json() {
        let row = json!({"code": "000582", "name": "北部湾港", "extra": 1});
        let entry = NameEntry::from_value(&row).unwrap();
        assert_eq!(entry.to_value(), json!({"code": "000582", "name": "北部湾港"}));
        // Numeric codes stringify rather than dropping the row.
        let numeric = NameEntry::from_value(&json!({"code": 582, "name": "x"})).unwrap();
        assert_eq!(numeric.code, "582");
        // A blank name is dropped, matching upstream's falsy-name filter.
        assert!(NameEntry::from_value(&json!({"code": "1", "name": ""})).is_none());
        assert!(NameEntry::from_value(&json!({"code": "1"})).is_none());

        let m = Match {
            code: "000582".into(),
            name: "北部湾港".into(),
            distance: 2,
            jaccard: 1.0,
        };
        assert_eq!(
            m.to_value(),
            json!({"code": "000582", "name": "北部湾港", "distance": 2, "jaccard": 1.0})
        );
    }

    #[test]
    fn index_builder_skips_unusable_rows() {
        let rows = vec![
            json!({"code": "600519", "name": "贵州茅台"}),
            json!({"code": "000001", "name": ""}),
            json!({"name": "no code"}),
        ];
        let built = index_from_values(&rows);
        assert_eq!(built.len(), 1);
        assert_eq!(built[0].code, "600519");
    }
}
