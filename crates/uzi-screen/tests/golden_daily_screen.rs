//! Differential tests for the daily-screen pipeline.
//!
//! Every expectation is produced by the upstream Python code on the checked-in
//! fixture; see `tests/fixtures/dump_upstream.py` and the command recorded in
//! the fixture header. The Rust port MUST reproduce the Python output for the
//! same input (key order included).

use serde_json::{json, Map, Value};
use std::path::{Path, PathBuf};
use uzi_screen::daily_screen::models::StockSnapshot;
use uzi_screen::daily_screen::ranker::{build_candidate, preselect, rank_candidates};
use uzi_screen::daily_screen::renderer::render_report;
use uzi_screen::daily_screen::runner::run_daily_screen_full;
use uzi_screen::daily_screen::themes::build_theme_context;
use uzi_screen::daily_screen::universe::{apply_hard_filters, normalize_universe_frame};

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

fn load(name: &str) -> Value {
    let path = fixtures().join(name);
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("invalid JSON in {}: {e}", path.display()))
}

fn diff(a: &Value, b: &Value, path: &str, out: &mut Vec<String>) {
    match (a, b) {
        (Value::Object(ma), Value::Object(mb)) => {
            for (k, va) in ma {
                match mb.get(k) {
                    Some(vb) => diff(va, vb, &format!("{path}.{k}"), out),
                    None => out.push(format!("{path}.{k}: missing on right")),
                }
            }
            for k in mb.keys() {
                if !ma.contains_key(k) {
                    out.push(format!("{path}.{k}: missing on left"));
                }
            }
        }
        (Value::Array(la), Value::Array(lb)) => {
            if la.len() != lb.len() {
                out.push(format!("{path}: length {} != {}", la.len(), lb.len()));
            }
            for (i, (va, vb)) in la.iter().zip(lb.iter()).enumerate() {
                diff(va, vb, &format!("{path}[{i}]"), out);
            }
        }
        _ => {
            if a != b {
                out.push(format!("{path}: {a} != {b}"));
            }
        }
    }
}

fn assert_same(actual: &Value, expected: &Value, label: &str) {
    let mut out = Vec::new();
    diff(actual, expected, "$", &mut out);
    if !out.is_empty() {
        panic!(
            "{label}: {} difference(s)\n{}",
            out.len(),
            out.iter().take(25).cloned().collect::<Vec<_>>().join("\n")
        );
    }
}

struct Fixture {
    input: Value,
    snapshots: Vec<StockSnapshot>,
}

fn build_snapshots() -> Fixture {
    let input = load("daily_screen_input.json");
    let mut snapshots: Vec<StockSnapshot> = Vec::new();
    for market in input["markets"].as_array().unwrap() {
        let market = market.as_str().unwrap();
        let rows = &input["market_rows"][market];
        let observed_at = input["observed_at"][market].as_str().unwrap();
        let source = input["source"][market].as_str().unwrap();
        let mut parsed = normalize_universe_frame(rows, market, observed_at, source);
        for stock in parsed.iter_mut() {
            if let Some(overlay) = input["overlays"].get(&stock.code) {
                if let Some(extra) = overlay.get("extra").and_then(|v| v.as_object()) {
                    for (k, v) in extra {
                        stock.extra.insert(k.clone(), v.clone());
                    }
                }
                if let Some(source) = overlay.get("source").and_then(|v| v.as_str()) {
                    stock.source = source.to_string();
                }
                if let Some(observed) = overlay.get("observed_at").and_then(|v| v.as_str()) {
                    stock.observed_at = observed.to_string();
                }
            }
        }
        snapshots.extend(parsed);
    }
    Fixture { input, snapshots }
}

#[test]
fn universe_stats_match_upstream() {
    let fixture = build_snapshots();
    let expected = load("expected_daily_screen.json");
    let mut actual = Map::new();
    for market in ["A", "H"] {
        let market_snaps: Vec<StockSnapshot> = fixture
            .snapshots
            .iter()
            .filter(|s| s.market == market)
            .cloned()
            .collect();
        let (_, stats) = apply_hard_filters(&market_snaps, 2e8).unwrap();
        actual.insert(market.to_string(), stats);
    }
    assert_same(
        &Value::Object(actual),
        &expected["universe_stats"],
        "universe_stats",
    );
    // Snapshots themselves (ST / 退市 / invalid-quote filtering included).
    let mut snaps = Map::new();
    for market in ["A", "H"] {
        let list: Vec<Value> = fixture
            .snapshots
            .iter()
            .filter(|s| s.market == market)
            .map(StockSnapshot::to_dict)
            .collect();
        snaps.insert(market.to_string(), Value::Array(list));
    }
    assert_same(&Value::Object(snaps), &expected["snapshots"], "snapshots");
}

#[test]
fn themes_features_shortlist_and_picks_match_upstream() {
    let fixture = build_snapshots();
    let expected = load("expected_daily_screen.json");
    let themes = build_theme_context(&fixture.snapshots);
    assert_same(&themes, &expected["themes"], "themes");

    let mut all_stocks: Vec<StockSnapshot> = Vec::new();
    for market in ["A", "H"] {
        let market_snaps: Vec<StockSnapshot> = fixture
            .snapshots
            .iter()
            .filter(|s| s.market == market)
            .cloned()
            .collect();
        let (kept, _) = apply_hard_filters(&market_snaps, 2e8).unwrap();
        all_stocks.extend(kept);
    }
    let shortlisted = preselect(&all_stocks, 40);
    let codes: Vec<Value> = shortlisted
        .iter()
        .map(|s| Value::String(s.code.clone()))
        .collect();
    assert_same(
        &Value::Array(codes),
        &expected["shortlist_codes"],
        "shortlist_codes",
    );

    let evaluated_at = chrono::DateTime::parse_from_rfc3339("2025-09-10T14:31:00+08:00").unwrap();
    let mut candidates = Vec::new();
    for stock in &shortlisted {
        let overlay = fixture.input["overlays"].get(&stock.code);
        let evidence: Vec<Value> = overlay
            .and_then(|o| o.get("evidence"))
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        let gaps = if overlay.is_some() {
            Vec::new()
        } else {
            vec!["snapshot_only".to_string()]
        };
        let theme = themes
            .get(&stock.code)
            .cloned()
            .unwrap_or_else(|| Value::Object(Map::new()));
        candidates.push(build_candidate(
            stock,
            &theme,
            &evidence,
            &gaps,
            &evaluated_at,
        ));
    }
    let actual: Vec<Value> = candidates.iter().map(|c| c.to_dict()).collect();
    assert_same(
        &Value::Array(actual),
        &expected["candidates"],
        "candidates",
    );

    let (picks, rejected) = rank_candidates(&candidates, 10, 70.0);
    assert_same(
        &Value::Array(picks.iter().map(|c| c.to_dict()).collect()),
        &expected["picks"],
        "picks",
    );
    assert_same(
        &Value::Array(rejected.iter().map(|c| c.to_dict()).collect()),
        &expected["rejected"],
        "rejected",
    );
}

#[test]
fn frozen_runner_report_matches_upstream() {
    let fixture = build_snapshots();
    let expected = load("expected_daily_screen_runner.json");
    let now = chrono::DateTime::parse_from_rfc3339("2025-09-10T16:30:00+08:00").unwrap();
    let tmp = std::env::temp_dir().join(format!("uzi-screen-runner-{}", std::process::id()));
    let report = run_daily_screen_full(
        "close",
        &["A".to_string(), "H".to_string()],
        10,
        2e8,
        false,
        false,
        Some(&tmp),
        Some(now),
        Some(&fixture.snapshots),
        Some(now),
    )
    .expect("runner report");
    let mut report = report;
    report.as_object_mut().unwrap().remove("report_path");
    assert_same(&report, &expected, "runner report");
    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn rendered_html_is_byte_identical() {
    let report = load("expected_report_input.json");
    let expected = std::fs::read(fixtures().join("expected_screen.html")).unwrap();
    let tmp = std::env::temp_dir().join(format!("uzi-screen-html-{}", std::process::id()));
    std::fs::create_dir_all(&tmp).unwrap();
    let out = tmp.join("index.html");
    render_report(&report, &out, &fixtures().join("no-avatars")).expect("render");
    let actual = std::fs::read(&out).unwrap();
    if actual != expected {
        let actual_text = String::from_utf8_lossy(&actual).to_string();
        let expected_text = String::from_utf8_lossy(&expected).to_string();
        let mut at = 0;
        for (i, (a, b)) in actual_text
            .chars()
            .zip(expected_text.chars())
            .enumerate()
        {
            if a != b {
                at = i;
                break;
            }
        }
        let start = at.saturating_sub(120);
        panic!(
            "rendered HTML differs at byte {} (len {} vs {})\nactual:   …{}\nexpected: …{}",
            at,
            actual.len(),
            expected.len(),
            actual_text.chars().skip(start).take(240).collect::<String>(),
            expected_text.chars().skip(start).take(240).collect::<String>(),
        );
    }
    let _ = std::fs::remove_dir_all(&tmp);
}

/// Guard the rendering of the JSON fixtures themselves: the golden input must
/// round-trip through the port's own serializer.
#[test]
fn report_input_round_trips() {
    let report = load("expected_report_input.json");
    let text = uzi_core::json::to_pretty(&report);
    let again: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(report, again);
    assert!(text.contains("as_of_by_market"));
    assert_eq!(report["filters"]["top_n"], json!(10));
}
