//! `self_review::review_all` against the golden `synthetic` cache artifacts,
//! compared to the upstream `lib/self_review.py` report.

use serde_json::Value;
use std::path::PathBuf;

fn golden(name: &str) -> Value {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden")
        .join(format!("{}.json", name));
    uzi_core::testkit::load_json(&p)
}

#[test]
fn review_all_on_golden_synthetic_artifacts() {
    // The report is profile/env-sensitive; pin the upstream baseline (medium
    // depth, no CLI-only flags) so the comparison is deterministic.
    for k in ["UZI_DEPTH", "UZI_LITE", "UZI_CLI_ONLY", "CI"] {
        std::env::remove_var(k);
    }

    let root = std::env::temp_dir().join(format!("uzi-review-selfreview-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::env::set_var("UZI_CACHE_ROOT", &root);

    let ticker = "002273.SZ";
    let raw = uzi_core::testkit::load_fixture("raw_data_synthetic");
    uzi_core::cache::write_task_output(ticker, "raw_data", &raw).unwrap();
    for name in ["dimensions", "panel", "synthesis"] {
        let g = uzi_core::testkit::load_golden("synthetic", name);
        uzi_core::cache::write_task_output(ticker, name, &g).unwrap();
    }

    let report = uzi_review::review_all(ticker, None);

    // Real output (visible with `cargo test -p uzi-review -- --nocapture`).
    println!("{}", uzi_review::format_human(&report));
    println!("{}", serde_json::to_string_pretty(&report).unwrap());

    // Human summary is byte-identical to upstream apart from the timestamp.
    let norm = |s: &str| {
        s.lines()
            .map(|l| {
                if l.starts_with("  reviewed_at=") {
                    "  reviewed_at=<TS>".to_string()
                } else {
                    l.to_string()
                }
            })
            .collect::<Vec<_>>()
            .join("\n")
    };
    let golden_human = std::fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/golden/synthetic_self_review_human.txt"),
    )
    .unwrap();
    assert_eq!(
        norm(&uzi_review::format_human(&report)),
        norm(golden_human.trim_end_matches('\n'))
    );

    // Exactly the upstream report, modulo the wall-clock timestamp.
    let mut actual = report.clone();
    let mut expected = golden("synthetic_self_review");
    actual["reviewed_at"] = serde_json::json!("<TS>");
    expected["reviewed_at"] = serde_json::json!("<TS>");
    uzi_core::testkit::assert_json_eq(&actual, &expected, "self_review/review_all/synthetic");

    // Well-formedness: 17 checks run, every issue carries the full contract.
    assert_eq!(uzi_review::checks().len(), 17);
    let issues = report["issues"].as_array().expect("issues array");
    assert!(!issues.is_empty());
    for i in issues {
        for k in ["severity", "category", "dim", "issue", "evidence", "suggested_fix"] {
            assert!(i.get(k).is_some(), "issue missing {}", k);
        }
        assert_ne!(i["dim"], "review-engine", "a check panicked: {}", i["issue"]);
    }
    let crit = issues.iter().filter(|i| i["severity"] == "critical").count() as i64;
    let warn = issues.iter().filter(|i| i["severity"] == "warning").count() as i64;
    let info = issues.iter().filter(|i| i["severity"] == "info").count() as i64;
    assert_eq!(report["critical_count"], crit);
    assert_eq!(report["warning_count"], warn);
    assert_eq!(report["info_count"], info);
    assert_eq!(report["passed"], crit == 0);
    assert_eq!(report["checks_run"].as_array().unwrap().len(), 17);

    // write_review honours UZI_CACHE_ROOT and returns the written path.
    let path = uzi_review::write_review(ticker, &report);
    assert!(path.exists(), "write_review path {} missing", path.display());
    let on_disk = uzi_core::cache::read_task_output(ticker, "_review_issues").expect("review written");
    assert_json_same(&on_disk, &report);

    // CLI wrapper: 1 critical issue → exit 1, and it prints the human report.
    let code = uzi_review::stage_review::run(&[
        "review_stage_output".to_string(),
        ticker.to_string(),
    ]);
    assert_eq!(code, 1);

    let _ = std::fs::remove_dir_all(&root);
}

fn assert_json_same(a: &Value, b: &Value) {
    assert_eq!(
        serde_json::to_string(a).unwrap(),
        serde_json::to_string(b).unwrap()
    );
}
