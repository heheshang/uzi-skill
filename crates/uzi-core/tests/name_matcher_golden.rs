//! Differential test: `uzi_core::name_matcher` vs upstream `lib/name_matcher.py`.
//!
//! Upstream pulls its A-share index from akshare behind a 7-day cache; the
//! golden pins the matching rules by feeding both implementations the same fixed
//! index (`tools/golden/dump_name_matcher.py` patches `build_a_share_index`).
//! Queries cover exact hits, character reorders, one-edit typos, short queries
//! that relax the Jaccard floor, ties, misses, and untrimmed input.

use uzi_core::name_matcher::{self, NameEntry};
use uzi_core::testkit::{assert_json_eq, load_fixture, load_golden};

fn index() -> Vec<NameEntry> {
    let fixture = load_fixture("name_matcher");
    name_matcher::index_from_values(fixture["index"].as_array().unwrap())
}

/// Every query in the golden must reproduce upstream's candidate list exactly —
/// same length, same order, same codes/names, same rounded Jaccard values.
#[test]
fn fuzzy_match_matches_upstream_for_every_query() {
    let index = index();
    let golden = load_golden("name_matcher", "queries");
    let queries = golden["queries"].as_object().expect("golden.queries");

    assert!(!queries.is_empty(), "golden holds no queries");
    for (query, expected) in queries {
        let hits = name_matcher::fuzzy_match_default(query, &index, 5);
        let actual = serde_json::Value::Array(hits.iter().map(|h| h.to_value()).collect());
        assert_json_eq(&actual, expected, &format!("fuzzy_match({query:?})"));
    }
}

/// `levenshtein` and `char_set_jaccard` are the whole basis of the ranking, so
/// pin them directly against the same Python functions.
#[test]
fn similarity_primitives_match_upstream() {
    let golden = load_golden("name_matcher", "queries");
    let cases = golden["primitives"].as_array().expect("golden.primitives");
    assert!(!cases.is_empty(), "golden holds no primitives");

    for case in cases {
        let a = case["a"].as_str().unwrap();
        let b = case["b"].as_str().unwrap();
        assert_eq!(
            name_matcher::levenshtein(a, b) as u64,
            case["levenshtein"].as_u64().unwrap(),
            "levenshtein({a:?}, {b:?})"
        );
        let expected = case["jaccard"].as_f64().unwrap();
        let actual = name_matcher::char_set_jaccard(a, b);
        assert!(
            (actual - expected).abs() < 1e-12,
            "char_set_jaccard({a:?}, {b:?}): {actual} != {expected}"
        );
    }
}

/// The ordering contract the golden encodes: distance ascending, then Jaccard
/// descending, with ties keeping index order (Python's stable sort).
#[test]
fn ranking_is_distance_ascending_then_jaccard_descending() {
    let index = index();
    let golden = load_golden("name_matcher", "queries");

    let mut checked = 0;
    for query in golden["queries"].as_object().unwrap().keys() {
        let hits = name_matcher::fuzzy_match_default(query, &index, 5);
        for pair in hits.windows(2) {
            let (a, b) = (&pair[0], &pair[1]);
            assert!(
                a.distance < b.distance
                    || (a.distance == b.distance && a.jaccard >= b.jaccard),
                "ranking violated for {query:?}: {a:?} before {b:?}"
            );
        }
        checked += 1;
    }
    assert_eq!(checked, golden["queries"].as_object().unwrap().len());
}

/// A query the index cannot match yields nothing rather than a low-quality
/// guess — the property callers rely on when deciding to ask the user.
#[test]
fn unmatched_query_returns_empty() {
    let index = index();
    let golden = load_golden("name_matcher", "queries");
    let unmatched = golden["queries"]
        .as_object()
        .unwrap()
        .iter()
        .filter(|(_, v)| v.as_array().map(|a| a.is_empty()).unwrap_or(false))
        .map(|(k, _)| k.clone())
        .collect::<Vec<_>>();

    assert!(
        !unmatched.is_empty(),
        "golden must retain at least one miss to exercise this path"
    );
    for query in unmatched {
        assert!(
            name_matcher::fuzzy_match_default(&query, &index, 5).is_empty(),
            "{query:?} should not match"
        );
    }
}
