//! End-to-end assemble test against the checked-in golden artifacts.
//!
//! Copies `tools/golden/expected/synthetic/{dimensions,panel,synthesis}.json`
//! plus `tools/golden/fixtures/raw_data_synthetic.json` into a temp
//! `.cache/002273.SZ/`, points `UZI_CACHE_ROOT`/`UZI_REPORTS_DIR` at the temp
//! tree, and calls `assemble("002273.SZ")`.

use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf()
}

fn assets_dir() -> PathBuf {
    if let Ok(p) = std::env::var("UZI_ASSETS_DIR") {
        if !p.is_empty() {
            return PathBuf::from(p);
        }
    }
    // Vendored with the repo, so the test needs no upstream checkout.
    repo_root().join("assets")
}

#[test]
fn assemble_end_to_end_synthetic() {
    let assets = assets_dir();
    let template = assets.join("report-template.html");
    assert!(
        template.exists(),
        "upstream report template not found at {}. Set UZI_ASSETS_DIR or keep the \
         /tmp/uzi-src checkout (see tools/golden/README.md).",
        template.display()
    );

    let root = repo_root();
    let expected = root.join("tools/golden/expected/synthetic");
    let fixture = root.join("tools/golden/fixtures/raw_data_synthetic.json");

    let tmp = std::env::temp_dir().join(format!("uzi_report_e2e_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    let cache = tmp.join("cache");
    let reports = tmp.join("reports");
    let ticker_dir = cache.join("002273.SZ");
    std::fs::create_dir_all(&ticker_dir).unwrap();

    for name in ["dimensions", "panel", "synthesis"] {
        std::fs::copy(expected.join(format!("{name}.json")), ticker_dir.join(format!("{name}.json")))
            .unwrap();
    }
    std::fs::copy(&fixture, ticker_dir.join("raw_data.json")).unwrap();

    std::env::set_var("UZI_CACHE_ROOT", &cache);
    std::env::set_var("UZI_REPORTS_DIR", &reports);
    std::env::set_var("UZI_ASSETS_DIR", &assets);
    // Bypass the self-review gate: the synthetic temp cache has no agent_analysis.json.
    std::env::set_var("UZI_SKIP_REVIEW", "1");

    let out = uzi_report::assemble("002273.SZ").expect("assemble should succeed");
    let out_path = PathBuf::from(&out);
    assert!(out_path.exists(), "standalone html missing: {out}");
    assert!(
        out.ends_with("full-report-standalone.html"),
        "expected standalone path, got {out}"
    );

    let html = std::fs::read_to_string(&out_path).unwrap();
    assert!(html.len() > 10 * 1024, "html too small: {} bytes", html.len());

    // ticker name from raw_data_synthetic.json 0_basic.data.name / synthesis.name
    assert!(html.contains("水晶光电"), "ticker name missing from report");
    // panel consensus number: panel.json panel_consensus=72.2 → f"{v:.0f}" = 72
    assert!(html.contains(">72%<"), "panel consensus number missing");
    // verdict label from synthesis.json ({{VERDICT_LABEL}})
    assert!(
        html.contains("可以蹲（偏弱） · 3 派看多 / 2 派看空"),
        "verdict label missing from report"
    );

    eprintln!(
        "[e2e] standalone report: {} bytes ({} KB)",
        html.len(),
        html.len() / 1024
    );

    let _ = std::fs::remove_dir_all(&tmp);
}
