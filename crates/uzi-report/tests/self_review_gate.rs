//! Proves the self-review gate is wired, not a no-op.
//!
//! Upstream `assemble_report.assemble` refuses to render when the mechanical
//! self-review reports critical issues; a port that silently skipped the gate
//! would look fine on healthy data. This test drives the failing branch: a cache
//! whose dimensions carry no critical data at all.

use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf()
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "uzi_report_gate_{}_{}",
        name,
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// A cache where `raw_data` has no dimensions: self-review must report criticals.
#[test]
fn assemble_is_blocked_by_critical_self_review() {
    let tmp = scratch("critical");
    let cache = tmp.join("cache");
    let ticker_dir = cache.join("EMPTY.SZ");
    std::fs::create_dir_all(&ticker_dir).unwrap();

    let root = repo_root();
    // synthesis/panel are required to exist before the gate runs.
    for name in ["panel", "synthesis"] {
        std::fs::copy(
            root.join(format!("tools/golden/expected/empty/{name}.json")),
            ticker_dir.join(format!("{name}.json")),
        )
        .unwrap();
    }
    std::fs::copy(
        root.join("tools/golden/fixtures/raw_data_empty.json"),
        ticker_dir.join("raw_data.json"),
    )
    .unwrap();

    std::env::set_var("UZI_CACHE_ROOT", &cache);
    std::env::set_var("UZI_ASSETS_DIR", root.join("assets"));
    std::env::remove_var("UZI_SKIP_REVIEW");

    let err = uzi_report::assemble("EMPTY.SZ")
        .expect_err("assemble must refuse to render when self-review finds criticals");
    let msg = err.to_string();
    assert!(
        msg.contains("BLOCKED by self-review"),
        "unexpected error: {msg}"
    );

    // the review artifact is written for the agent to consume
    let issues = ticker_dir.join("_review_issues.json");
    assert!(
        issues.is_file(),
        "self-review issues must be persisted at {}",
        issues.display()
    );

    // ...and UZI_SKIP_REVIEW=1 bypasses the gate, proving the env escape hatch works
    std::env::set_var("UZI_SKIP_REVIEW", "1");
    let out = uzi_report::assemble("EMPTY.SZ").expect("UZI_SKIP_REVIEW=1 must bypass the gate");
    assert!(PathBuf::from(&out).is_file(), "report not written: {out}");
    std::env::remove_var("UZI_SKIP_REVIEW");

    let _ = std::fs::remove_dir_all(&tmp);
}
