//! `UZI_SCHOOL` school-lock test.
//!
//! This binary deliberately contains a single test: the lock is read from the
//! process environment, and mutating it while sibling tests run would race.

#[test]
fn school_lock_skips_every_other_school() {
    let features = serde_json::json!({"market": "A", "name": "测试", "industry": "白酒"});

    // baseline: no lock → buffett evaluates, zhao_lg evaluates
    assert_ne!(
        uzi_investors::evaluate_investor("buffett", &features)["signal"],
        "skip"
    );

    std::env::set_var("UZI_SCHOOL", "F");
    assert_eq!(uzi_investors::evaluator::get_locked_school(), "F");

    let buffett = uzi_investors::evaluate_investor("buffett", &features);
    assert_eq!(buffett["signal"], "skip");
    assert_eq!(
        buffett["skip_reason"],
        "用户锁定 A 股游资 派视角 · 非该派评委不参与"
    );
    assert_eq!(buffett["headline"], Value::from("不适合 — 用户锁定 A 股游资 派视角 · 非该派评委不参与"));

    // an F-group 游资 still participates (and lands in its own射程 path)
    let zhao = uzi_investors::evaluate_investor(
        "zhao_lg",
        &serde_json::json!({"market": "A", "name": "测试", "industry": "半导体", "market_cap_yi": 80}),
    );
    assert_ne!(zhao["signal"], "skip");

    std::env::remove_var("UZI_SCHOOL");
    // invalid values are ignored, exactly like upstream
    std::env::set_var("UZI_SCHOOL", "z");
    assert_eq!(uzi_investors::evaluator::get_locked_school(), "");
    std::env::remove_var("UZI_SCHOOL");
}

use serde_json::Value;
