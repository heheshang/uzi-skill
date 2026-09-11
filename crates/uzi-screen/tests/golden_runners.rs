//! Differential tests for the versus / portfolio / fund-holdings runners.
//!
//! Expectations come from the upstream Python code on the checked-in fixture
//! caches and CSVs (`tests/fixtures/dump_upstream.py`).

use serde_json::{Map, Value};
use std::path::{Path, PathBuf};
use uzi_screen::fund_holdings::{confirm_and_run_holdings, estimate_runtime};
use uzi_screen::portfolio::{normalize_weights, parse_csv, portfolio_health, render_html};
use uzi_screen::versus::{extract_metrics, load_cache, render_comparison_grid, render_html as render_versus, render_verdict_cards};

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
            out.iter().take(20).cloned().collect::<Vec<_>>().join("\n")
        );
    }
}

fn normalize_now(text: &str) -> String {
    let re = regex::Regex::new(r"\d{4}-\d{2}-\d{2} \d{2}:\d{2}").unwrap();
    re.replace_all(text, "{{NOW}}").into_owned()
}

/// The fixture caches live under `tests/fixtures/.cache`, which is also the
/// upstream layout (`<scripts>/.cache/<ticker>/…`).
fn fixture_metrics() -> Vec<Value> {
    std::env::set_var("UZI_CACHE_ROOT", fixtures().join(".cache"));
    let tickers = [
        "600519.SH",
        "000858.SZ",
        "300750.SZ",
        "002594.SZ",
        "603501.SH",
    ];
    tickers
        .iter()
        .map(|t| {
            let bundle = load_cache(t).unwrap_or_else(|| panic!("fixture cache missing for {t}"));
            extract_metrics(&bundle)
        })
        .collect()
}

fn with_weights(metrics: &[Value], rows: &[Value]) -> Vec<Value> {
    let by_ticker: Map<String, Value> = rows
        .iter()
        .map(|row| {
            (
                row["ticker"].as_str().unwrap().to_string(),
                row.clone(),
            )
        })
        .collect();
    metrics
        .iter()
        .map(|m| {
            let mut m = m.clone();
            let row = by_ticker
                .get(m["ticker"].as_str().unwrap())
                .cloned()
                .unwrap_or_else(|| serde_json::json!({"weight": Value::Null, "note": ""}));
            let map = m.as_object_mut().unwrap();
            map.insert("_weight".into(), row["weight"].clone());
            map.insert("_note".into(), row["note"].clone());
            m
        })
        .collect()
}

#[test]
fn versus_metrics_match_upstream() {
    let expected = load("expected_portfolio.json");
    let metrics = fixture_metrics();
    // The golden snapshot was taken after upstream attached `_weight`/`_note`
    // to the same dicts, so strip those two keys before comparing.
    let stripped: Vec<Value> = metrics
        .iter()
        .map(|m| {
            let mut m = m.clone();
            let map = m.as_object_mut().unwrap();
            map.remove("_weight");
            map.remove("_note");
            m
        })
        .collect();
    let golden: Vec<Value> = expected["metrics"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| {
            let mut m = m.clone();
            let map = m.as_object_mut().unwrap();
            map.remove("_weight");
            map.remove("_note");
            m
        })
        .collect();
    assert_same(&Value::Array(stripped), &Value::Array(golden), "metrics");
}

#[test]
fn versus_rendering_matches_upstream() {
    let expected = load("expected_portfolio.json");
    let metrics = fixture_metrics();
    assert_eq!(
        render_comparison_grid(&metrics),
        expected["comparison_grid"].as_str().unwrap(),
        "comparison grid differs"
    );
    assert_eq!(
        render_verdict_cards(&metrics),
        expected["verdict_cards"].as_str().unwrap(),
        "verdict cards differ"
    );
    let html = normalize_now(&render_versus(&metrics, "lite"));
    let golden = std::fs::read_to_string(fixtures().join("expected_versus.html")).unwrap();
    if html != golden {
        let at = html
            .chars()
            .zip(golden.chars())
            .position(|(a, b)| a != b)
            .unwrap_or(0);
        let start = at.saturating_sub(150);
        panic!(
            "versus HTML differs at char {at} (len {} vs {})\nactual:   …{}\nexpected: …{}",
            html.chars().count(),
            golden.chars().count(),
            html.chars().skip(start).take(260).collect::<String>(),
            golden.chars().skip(start).take(260).collect::<String>(),
        );
    }
}

#[test]
fn portfolio_csv_parsing_matches_upstream() {
    let expected = load("expected_portfolio.json");
    let rows = parse_csv(&fixtures().join("holdings.csv")).unwrap();
    assert_same(&Value::Array(rows.clone()), &expected["csv_rows"], "csv_rows");

    let mut normalized = rows;
    let normalized = normalize_weights(&mut normalized);
    assert_same(
        &Value::Array(normalized),
        &expected["normalized"],
        "normalized",
    );

    let mut mixed = parse_csv(&fixtures().join("holdings_mixed.csv")).unwrap();
    let mixed = normalize_weights(&mut mixed);
    assert_same(
        &Value::Array(mixed),
        &expected["normalized_mixed"],
        "normalized_mixed",
    );

    let headerless = parse_csv(&fixtures().join("holdings_headerless.csv")).unwrap();
    assert_same(
        &Value::Array(headerless),
        &expected["headerless_rows"],
        "headerless_rows",
    );
}

#[test]
fn portfolio_health_and_html_match_upstream() {
    let expected = load("expected_portfolio.json");
    let metrics = fixture_metrics();
    let rows = parse_csv(&fixtures().join("holdings.csv")).unwrap();
    let metrics = with_weights(&metrics, &rows);

    let health = portfolio_health(&metrics);
    assert_same(&health, &expected["health"], "health");

    let html = normalize_now(&render_html("测试组合", &metrics, &health, "lite"));
    let golden = std::fs::read_to_string(fixtures().join("expected_portfolio.html")).unwrap();
    if html != golden {
        let at = html
            .chars()
            .zip(golden.chars())
            .position(|(a, b)| a != b)
            .unwrap_or(0);
        let start = at.saturating_sub(150);
        panic!(
            "portfolio HTML differs at char {at} (len {} vs {})\nactual:   …{}\nexpected: …{}",
            html.chars().count(),
            golden.chars().count(),
            html.chars().skip(start).take(260).collect::<String>(),
            golden.chars().skip(start).take(260).collect::<String>(),
        );
    }
}

#[test]
fn fund_holdings_paths_match_upstream() {
    let expected = load("expected_portfolio.json");
    let estimates = expected["estimate_runtime"].as_object().unwrap();
    assert_eq!(
        estimate_runtime(10, "lite"),
        estimates["lite_10"].as_str().unwrap()
    );
    assert_eq!(
        estimate_runtime(10, "medium"),
        estimates["medium_10"].as_str().unwrap()
    );
    assert_eq!(
        estimate_runtime(3, "deep"),
        estimates["deep_3"].as_str().unwrap()
    );
    assert_eq!(
        estimate_runtime(120, "nope"),
        estimates["unknown_120"].as_str().unwrap()
    );

    let holdings = vec![
        serde_json::json!({"rank": 1, "code": "600519.SH", "name": "贵州茅台", "weight_pct": 5.5}),
        serde_json::json!({"rank": 2, "code": "000858.SZ", "name": "五粮液", "weight_pct": 4.2}),
    ];
    let cancelled =
        confirm_and_run_holdings("510300.SH", "ETF", &holdings, "medium", false, Some(false)).unwrap();
    let mut actual = Map::new();
    actual.insert("holdings_cancel".into(), cancelled);
    actual.insert(
        "holdings_empty".into(),
        confirm_and_run_holdings("510300.SH", "ETF", &[], "medium", true, None).unwrap(),
    );
    let mut expected_subset = Map::new();
    expected_subset.insert(
        "holdings_cancel".into(),
        expected["holdings_cancel"].clone(),
    );
    expected_subset.insert("holdings_empty".into(), expected["holdings_empty"].clone());
    assert_same(
        &Value::Object(actual),
        &Value::Object(expected_subset),
        "fund holdings",
    );
}

/// `sources.py` quote/minute parsers vs. upstream on a synthetic provider
/// payload (see `dump_upstream.golden_source_parsers`).
#[test]
fn source_parsers_match_upstream() {
    use uzi_screen::daily_screen::sources::{parse_minutes, parse_tencent, parse_tencent_minutes};

    let expected = load("expected_sources.json");

    let mut fields = vec![String::new(); 60];
    fields[1] = "中际旭创".into();
    fields[2] = "300308".into();
    fields[3] = "156.80".into();
    fields[4] = "142.70".into();
    fields[5] = "145.00".into();
    fields[6] = "242000".into();
    fields[9] = "156.70".into();
    fields[10] = "1200".into();
    fields[19] = "156.90".into();
    fields[20] = "800".into();
    fields[30] = "20250910143000".into();
    fields[32] = "9.90".into();
    fields[33] = "158.00".into();
    fields[34] = "144.50".into();
    fields[37] = "380000".into();
    let a_text = format!("v_sz300308=\"{}\";", fields.join("~"));
    let mut hk_fields = fields.clone();
    hk_fields[2] = "00700".into();
    hk_fields[30] = "2025/09/10 15:30:00".into();
    hk_fields[37] = "3200000000".into();
    hk_fields[6] = "8400000".into();
    let hk_text = format!("v_hk00700=\"{}\";", hk_fields.join("~"));

    assert_same(
        &parse_tencent(&a_text, "300308.SZ").unwrap(),
        &expected["a_quote"],
        "a_quote",
    );
    assert_same(
        &parse_tencent(&hk_text, "00700.HK").unwrap(),
        &expected["hk_quote"],
        "hk_quote",
    );

    let trends = vec![
        "2025-09-10 14:29,0,156.70,0,0,0,130000000,0",
        "2025-09-10 14:30,0,156.80,0,0,0,100000000,0",
        "2025-09-10 14:30,0,156.85,0,0,0,101000000,0",
        "2025-09-10 15:01,0,157.00,0,0,0,90000000,0",
        "2025-09-09 14:30,0,150.00,0,0,0,80000000,0",
        "2025-09-10 14:28,0,0,0,0,0,70000000,0",
        "bad,row",
    ];
    let minutes = parse_minutes(
        &serde_json::json!({"data": {"code": "300308", "trends": trends}}),
        "300308.SZ",
        "2025-09-10T14:30:30+08:00",
    )
    .unwrap();
    assert_same(
        &Value::Array(minutes),
        &expected["minutes"],
        "minutes",
    );

    let tencent_payload = serde_json::json!({
        "data": {"sz300308": {"data": {"date": "20250910", "data": [
            "1430 156.80 1200 380000000",
            "1431 157.00 800 380090000",
        ]}}}
    });
    let tencent_minutes = parse_tencent_minutes(
        &tencent_payload,
        "300308.SZ",
        "2025-09-10T14:31:30+08:00",
    )
    .unwrap();
    assert_same(
        &Value::Array(tencent_minutes),
        &expected["tencent_minutes"],
        "tencent_minutes",
    );
}
