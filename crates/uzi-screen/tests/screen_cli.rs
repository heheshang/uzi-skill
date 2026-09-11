//! Argument validation, the F/I school gate, and the degraded-source output
//! shape for `screen.py`'s entry point.
//!
//! Expected messages/shapes are taken from the upstream Python source:
//! `lib/daily_screen/runner.py` and `screen.py`.

use serde_json::Value;
use std::path::PathBuf;
use uzi_screen::daily_screen::runner::{run_daily_screen, run_daily_screen_full};

fn now() -> chrono::DateTime<chrono::FixedOffset> {
    chrono::DateTime::parse_from_rfc3339("2025-09-10T16:30:00+08:00").unwrap()
}

fn tmp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("uzi-screen-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

/// `screen.py` errors out unless `--schools` is exactly the set `{F, I}`
/// (order-insensitive, duplicates collapse).
#[test]
fn school_gate_requires_f_and_i() {
    // Bad sets → the CLI's own error message.
    for schools in ["F", "I", "", "A,B", "F,I,G"] {
        let err = run_daily_screen("close", "A,H", schools, 10, 2e8, true).unwrap_err();
        assert_eq!(
            err.to_string(),
            "daily screen 当前要求 --schools F,I",
            "schools={schools:?}"
        );
    }
    // `"f, i"` passes the school gate: the run proceeds and fails later on the
    // market list, which proves the gate was satisfied.
    let err = run_daily_screen("close", "X", "f, i", 10, 2e8, true).unwrap_err();
    assert_eq!(err.to_string(), "markets only supports A,H");
}

/// `run_daily_screen` argument validation (upstream `ValueError` messages).
#[test]
fn argument_validation_matches_upstream() {
    let cases: [(Value, &str); 4] = [
        (
            serde_json::json!(["bogus", "A,H", "F,I", 10, 2e8, true]),
            "mode must be noon or close",
        ),
        (
            serde_json::json!(["close", "A,H", "F,I", 0, 2e8, true]),
            "top_n must be between 1 and 10",
        ),
        (
            serde_json::json!(["close", "A,H", "F,I", 11, 2e8, true]),
            "top_n must be between 1 and 10",
        ),
        (
            serde_json::json!(["close", "A,H", "F,I", 10, 1e8, true]),
            "minimum turnover must be finite and at least 200 million",
        ),
    ];
    for (args, message) in cases {
        let a = args.as_array().unwrap();
        let err = run_daily_screen(
            a[0].as_str().unwrap(),
            a[1].as_str().unwrap(),
            a[2].as_str().unwrap(),
            a[3].as_u64().unwrap() as usize,
            a[4].as_f64().unwrap(),
            a[5].as_bool().unwrap(),
        )
        .unwrap_err();
        assert_eq!(err.to_string(), message);
    }

    // `top = 10` and `min_turnover = 2e8` are both accepted (the run then fails
    // on the unreachable market, not on validation).
    let err = run_daily_screen("close", "X", "F,I", 10, 2e8, true).unwrap_err();
    assert_eq!(err.to_string(), "markets only supports A,H");
}

/// With no frozen snapshot the market degrades to counts of 0, an empty pick
/// list and a recorded `market_errors` entry — never invented data.
#[test]
fn unavailable_market_degrades_to_empty_output() {
    let out = tmp("degraded");
    let report = run_daily_screen_full(
        "close",
        &["A".to_string(), "H".to_string()],
        10,
        2e8,
        false,
        false,
        Some(&out),
        Some(now()),
        Some(&[]),
        Some(now()),
    )
    .expect("degraded report");
    let expected_error = "ValueError: market snapshot is empty or lacks observation timestamps";
    for market in ["A", "H"] {
        assert_eq!(
            report["universe_stats"][market],
            serde_json::json!({"input": 0, "liquid": 0, "error": expected_error}),
            "universe_stats[{market}]"
        );
        assert_eq!(report["market_errors"][market], expected_error);
    }
    assert_eq!(report["picks"], serde_json::json!([]));
    assert_eq!(report["rejected"], serde_json::json!([]));
    assert_eq!(report["action_summary"], serde_json::json!({}));
    assert_eq!(report["data_quality"]["shortlisted"], 0);
    assert_eq!(report["data_quality"]["markets_available"], serde_json::json!([]));
    assert_eq!(
        report["snapshot_kind"], "frozen",
        "empty frozen input still reports the frozen kind"
    );
    assert_eq!(report["analysis_basis"], "rule_only");
    assert_eq!(report["performance_contract"]["status"], "paper_trading");
    let html = std::fs::read_to_string(out.join("index.html")).unwrap();
    assert!(html.contains("当前数据不足以形成入榜候选"));
    assert!(report["report_path"].as_str().unwrap().ends_with("index.html"));
    // picks.json / report.meta.json are written next to the HTML.
    assert!(out.join("picks.json").exists());
    let meta: Value =
        serde_json::from_str(&std::fs::read_to_string(out.join("report.meta.json")).unwrap()).unwrap();
    assert_eq!(meta["html"], "index.html");
    assert_eq!(meta["picks_count"], 0);
    assert_eq!(meta["markets"], serde_json::json!(["A", "H"]));
    let _ = std::fs::remove_dir_all(&out);
}

/// Frozen snapshots are filtered by the mode/market cutoff
/// (`_cutoff(now, market, mode)`), so a quote observed after the close cutoff is
/// excluded rather than backdated.
#[test]
fn frozen_snapshots_respect_the_mode_cutoff() {
    use uzi_screen::daily_screen::models::StockSnapshot;
    let base = |code: &str, observed: &str| StockSnapshot {
        code: code.to_string(),
        name: "股票".to_string(),
        market: "A".to_string(),
        price: 10.0,
        change_pct: 1.0,
        amount: 5e8,
        industry: "白酒".to_string(),
        observed_at: observed.to_string(),
        source: "test".to_string(),
        ..Default::default()
    };
    let frozen = vec![
        base("600519.SH", "2025-09-10T14:30:00+08:00"), // before the 15:00 close cutoff
        base("000858.SZ", "2025-09-10T15:30:00+08:00"), // after it
    ];
    let out = tmp("cutoff");
    // `now` = 16:30, mode=close → A cutoff is 15:00.
    let report = run_daily_screen_full(
        "close",
        &["A".to_string()],
        10,
        2e8,
        false,
        false,
        Some(&out),
        Some(now()),
        Some(&frozen),
        Some(now()),
    )
    .unwrap();
    assert_eq!(report["universe_stats"]["A"]["input"], 1);
    assert_eq!(report["as_of_by_market"]["A"], "2025-09-10T14:30:00+08:00");
    let _ = std::fs::remove_dir_all(&out);
}
