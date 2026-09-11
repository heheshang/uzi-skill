//! `prewarm` steps that redirect the process-wide cache root.
//!
//! Lives in its own integration binary because `UZI_CACHE_ROOT` is read
//! process-wide. Both tests point at the *same* per-process directory (the
//! convention used by `collect_degraded.rs`), so they cannot race even though
//! cargo runs them in parallel — they only ever touch distinct cache keys.

use serde_json::{json, Value};

/// Redirect the cache at a stable per-process directory.
fn setup() {
    let dir = std::env::temp_dir().join(format!("uzi-prewarm-test-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    std::env::set_var("UZI_CACHE_ROOT", dir);
    std::env::remove_var("STOCK_NO_CACHE");
}

fn now_epoch() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs_f64()
}

/// Write a fresh cache entry in the format the reader consumes.
fn seed(ticker: &str, key: &str, data: Value) {
    let path = uzi_core::cache::cache_path(ticker, key);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(
        &path,
        serde_json::to_string(&json!({
            "_cached_at": now_epoch(),
            "data": data,
            "_ttl": 7 * 24 * 3600,
        }))
        .unwrap(),
    )
    .unwrap();
}

/// Regression: `warm_stock_name_table` judged success by whether the cache file
/// existed *before* the fetch — never true on a cold cache — so a successful
/// warm was always reported as skipped. It must report the row count of the
/// entry actually written.
#[test]
fn prewarm_reports_the_rows_it_wrote() {
    setup();
    seed(
        "_global",
        "a_share_name_index",
        json!([
            {"code": "600519", "name": "贵州茅台"},
            {"code": "002273", "name": "水晶光电"},
            {"code": "000582", "name": "北部湾港"}
        ]),
    );

    let mut report = uzi_data::prewarm::WarmReport::default();
    uzi_data::prewarm::warm_stock_name_table(&mut report);

    assert!(
        report.unsupported.is_empty(),
        "a populated table must not be reported as skipped: {:?}",
        report.unsupported
    );
    assert_eq!(report.written.len(), 1, "{:?}", report.written);
    assert!(
        report.written[0].contains("a_share_name_index"),
        "{:?}",
        report.written
    );
    assert!(report.written[0].contains("(3 rows)"), "{:?}", report.written);
}

#[test]
fn warmed_entries_are_served_without_refetching() {
    setup();
    let key = "prewarm_probe";
    let payload = json!([{"code": "600519", "name": "贵州茅台"}]);
    seed("_global", key, payload.clone());

    let fetched = std::cell::Cell::new(false);
    let got = uzi_core::cache::cached::<_, anyhow::Error>("_global", key, 7 * 24 * 3600, || {
        fetched.set(true);
        Ok(json!("SHOULD NOT RUN"))
    })
    .unwrap();

    assert_eq!(got, payload, "the prewarmed entry must be returned verbatim");
    assert!(!fetched.get(), "a fresh entry must not trigger the fetcher");

    // And the payload really is the numeric shape `cached()` consumes.
    let path = uzi_core::cache::cache_path("_global", key);
    let raw: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    assert!(raw["_cached_at"].is_f64());
}
