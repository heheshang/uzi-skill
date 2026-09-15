//! Network-dependent test for `fetch_research_reports` / `fetch::research::main`.
//!
//! Verifies the §2.5 fix: A-share research reports are actually fetched from
//! `reportapi.eastmoney.com/report/list` and column-renamed to the Chinese keys
//! `fetch/research.rs` reads (`报告名称`, `机构`, `东财评级`, `日期`,
//! `{year}-盈利预测-收益/市盈率`, `报告PDF链接`).
//!
//! Run with: `cargo test --release -p uzi-data test_research -- --nocapture --ignored`

use uzi_core::ticker::parse_ticker;
use uzi_data::fetch::research;
use uzi_data::sources::fetch_research_reports;

/// The raw fetch returns an array of renamed report objects for a known A-share.
#[test]
#[ignore = "network call to reportapi.eastmoney.com"]
fn test_research_fetch_returns_data() {
    let ti = parse_ticker("002273.SZ");
    let reports = fetch_research_reports(&ti);
    let arr = reports.as_array().expect("should be array");
    assert!(
        !arr.is_empty(),
        "research reports should not be empty for 002273.SZ"
    );

    let first = &arr[0];
    assert!(first.get("报告名称").is_some(), "should have 报告名称");
    assert!(first.get("机构").is_some(), "should have 机构");
    assert!(first.get("东财评级").is_some(), "should have 东财评级");
    assert!(first.get("日期").is_some(), "should have 日期");
    assert!(
        first.get("2026-盈利预测-收益").is_some(),
        "should have 2026-盈利预测-收益"
    );
    assert!(
        first.get("2026-盈利预测-市盈率").is_some(),
        "should have 2026-盈利预测-市盈率"
    );
    assert!(
        first.get("2027-盈利预测-收益").is_some(),
        "should have 2027-盈利预测-收益"
    );

    let pdf = first
        .get("报告PDF链接")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    assert!(
        pdf.contains("pdf.dfcfw.com"),
        "pdf url should be dfcfw: {pdf}"
    );

    println!("✅ {} reports fetched for 002273.SZ", arr.len());
}

/// The full `fetch::research::main` pipeline returns a non-fallback result with
/// `report_count > 0` and populated `coverage` / `brokers`.
#[test]
#[ignore = "network call to reportapi.eastmoney.com"]
fn test_research_main_non_fallback() {
    let result = research::main("002273.SZ").expect("should not error");
    let fallback = result
        .get("fallback")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);
    assert!(!fallback, "should not be fallback for 002273.SZ");

    let data = result.get("data").expect("should have data");
    let count = data
        .get("report_count")
        .and_then(|v| v.as_i64())
        .unwrap_or(0);
    assert!(count > 0, "report_count should be > 0, got {count}");

    let coverage = data
        .get("coverage")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    assert!(
        coverage.contains("家") || coverage.contains("份"),
        "coverage should mention brokers/reports: {coverage}"
    );

    let brokers = data.get("brokers").and_then(|v| v.as_array());
    assert!(brokers.is_some(), "brokers should be an array");
    assert!(
        brokers.map(|b| !b.is_empty()).unwrap_or(false),
        "brokers should not be empty"
    );

    println!(
        "✅ research::main: {} reports, {} brokers, coverage={}",
        count,
        brokers.map(|b| b.len()).unwrap_or(0),
        coverage
    );
}
