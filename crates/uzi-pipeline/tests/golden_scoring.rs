//! Differential tests for `score_dimensions` against the upstream Python engine.

use uzi_core::testkit::{assert_json_eq, load_fixture, load_golden};
use uzi_pipeline::score::score_dimensions;

fn check(case: &str) {
    let raw = load_fixture(&format!("raw_data_{}", case));
    let actual = score_dimensions(&raw);
    let expected = load_golden(case, "dimensions");
    assert_json_eq(&actual, &expected, &format!("score_dimensions/{}", case));
}

#[test]
fn synthetic_snapshot_matches_upstream() {
    check("synthetic");
}

#[test]
fn sparse_snapshot_matches_upstream() {
    check("sparse");
}

#[test]
fn empty_snapshot_matches_upstream() {
    check("empty");
}
