//! The A-share name index must not cache a *failed* fetch.
//!
//! EastMoney's `push2` host is frequently blocked (the `em_push2` registry entry
//! records `blocked_often`), and the index lives behind a 7-day TTL. Persisting
//! the empty result of a blocked response would disable Chinese-name resolution
//! for a week, so `build_a_share_index` returns `Err` for an empty table and lets
//! `cached()` skip the write.
//!
//! Lives in its own integration binary: it sets `UZI_CACHE_ROOT` and the proxy
//! variables process-wide, which would race with any parallel test.

use serde_json::json;

#[test]
fn failed_index_fetch_is_not_cached() {
    let dir = std::env::temp_dir().join(format!("uzi-nameindex-offline-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::env::set_var("UZI_CACHE_ROOT", &dir);
    std::env::remove_var("STOCK_NO_CACHE");

    // Point every HTTP call at a closed port so the fetch fails fast.
    std::env::set_var("HTTP_PROXY", "http://127.0.0.1:9");
    std::env::set_var("HTTPS_PROXY", "http://127.0.0.1:9");
    std::env::set_var("ALL_PROXY", "http://127.0.0.1:9");
    std::env::set_var("UZI_HTTP_TIMEOUT", "2");

    let path = uzi_core::cache::cache_path("_global", "a_share_name_index");
    let _ = std::fs::remove_file(&path);

    let table = uzi_data::sources::build_a_share_index();
    assert_eq!(table, json!([]), "an unreachable endpoint yields no table");
    assert!(
        !path.exists(),
        "a failed fetch must not be persisted, or name resolution stays dead for {}s",
        7 * 24 * 3600
    );

    // Name resolution still degrades cleanly rather than panicking.
    let r = uzi_data::sources::resolve_chinese_name_rich("贵州茅台");
    assert_eq!(r["source"], json!("none"), "{r}");
    assert_eq!(r["resolved"], serde_json::Value::Null, "{r}");
}
