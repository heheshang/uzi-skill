//! Differential test: `uzi_core::pyrandom` vs CPython's `random` module.
//!
//! `preview_with_mock.py` seeds a fixed integer and draws from it, so the port
//! must reproduce CPython's MT19937 stream exactly. Coverage includes
//! multi-word integer seeds (2^31, 2^32-1, 2^40+7), negative ranges, and the
//! 624-word state regeneration boundary.

use uzi_core::pyrandom::PyRandom;
use uzi_core::testkit::load_golden;

#[test]
fn every_seed_reproduces_cpython_exactly() {
    let golden = load_golden("pyrandom", "streams");
    let choice_seq: Vec<&str> = golden["choice_seq"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    let cases = golden["cases"].as_array().expect("golden.cases");
    assert!(!cases.is_empty(), "golden holds no seeds");

    for case in cases {
        let seed = case["seed"].as_u64().unwrap();
        let draws = &case["draws"];

        // `dump_pyrandom.py` seeds once and then draws each type in order, so the
        // sequence is continuous: `randint` continues where `random` stopped.
        let mut r = PyRandom::seed_u64(seed);

        let expected = draws["random"].as_array().unwrap();
        for (i, want) in expected.iter().enumerate() {
            let got = r.random();
            let want = want.as_f64().unwrap();
            assert_eq!(got, want, "seed {seed}: random()[{i}]");
        }

        for (i, want) in draws["randint_55_95"].as_array().unwrap().iter().enumerate() {
            assert_eq!(
                r.randint(55, 95),
                want.as_i64().unwrap(),
                "seed {seed}: randint(55,95)[{i}]"
            );
        }

        for (i, want) in draws["randrange_5_96"].as_array().unwrap().iter().enumerate() {
            assert_eq!(
                r.randrange(5, 96),
                want.as_i64().unwrap(),
                "seed {seed}: randrange(5,96)[{i}]"
            );
        }

        // Negative starts exercise the offset arithmetic in randrange.
        for (i, want) in draws["randint_-8_5"].as_array().unwrap().iter().enumerate() {
            assert_eq!(
                r.randint(-8, 5),
                want.as_i64().unwrap(),
                "seed {seed}: randint(-8,5)[{i}]"
            );
        }

        for (i, want) in draws["choice"].as_array().unwrap().iter().enumerate() {
            assert_eq!(
                r.choice_str(&choice_seq),
                want.as_str().unwrap(),
                "seed {seed}: choice[{i}]"
            );
        }

        // 700 words crosses the 624-word regeneration boundary.
        let mut r = PyRandom::seed_u64(seed);
        for (i, want) in case["getrandbits32"].as_array().unwrap().iter().enumerate() {
            assert_eq!(
                r.getrandbits(32),
                want.as_u64().unwrap(),
                "seed {seed}: getrandbits(32)[{i}]"
            );
        }

        // `_randbelow` rejection sampling over a large modulus.
        let mut r = PyRandom::seed_u64(seed);
        for (i, want) in case["randbelow_1e9"].as_array().unwrap().iter().enumerate() {
            assert_eq!(
                r.randrange(0, 1_000_000_007),
                want.as_i64().unwrap(),
                "seed {seed}: randrange(0,1e9+7)[{i}]"
            );
        }
    }
}
