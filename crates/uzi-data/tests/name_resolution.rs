//! `resolve_chinese_name_rich` tiers 2/3 — exact substring and local fuzzy match.
//!
//! Upstream resolves a Chinese name through the MX API, then an akshare exact
//! substring search, then `lib/name_matcher.fuzzy_match`. Only tiers 2/3 are
//! reachable without an API key, and they read the A-share code/name table from
//! the 7-day cache — so this test seeds that cache file directly and stays
//! network-free.
//!
//! Lives in its own integration binary because it mutates `UZI_CACHE_ROOT`
//! process-wide; the scenarios run sequentially in one `#[test]` because
//! swapping the cache root under concurrent tests would race.

use serde_json::{json, Value};

/// Seed `.cache/_global/api_cache/a_share_name_index__*.json` under `root`.
fn seed_index(root: &std::path::Path, rows: &Value) {
    std::env::set_var("UZI_CACHE_ROOT", root);
    let path = uzi_core::cache::cache_path("_global", "a_share_name_index");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs_f64();
    let payload = json!({"_cached_at": now, "data": rows, "_ttl": 7 * 24 * 3600});
    std::fs::write(&path, serde_json::to_string(&payload).unwrap()).unwrap();
}

fn fresh_root(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("uzi_name_res_{}_{}", tag, std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

#[test]
fn chinese_name_resolution_covers_every_reachable_tier() {
    std::env::remove_var("MX_APIKEY");

    // ── Scenario A: full-ish table ───────────────────────────────────────────
    let root = fresh_root("a");
    seed_index(
        &root,
        &json!([
            {"code": "000582", "name": "北部湾港"},
            {"code": "600519", "name": "贵州茅台"},
            {"code": "002273", "name": "水晶光电"},
            {"code": "601318", "name": "中国平安"},
            {"code": "000001", "name": "平安银行"}
        ]),
    );

    // Tier 3 · order-typo: "北部港湾" → 北部湾港 (distance 2). No table row
    // contains the query, so the substring tier cannot fire.
    let r = uzi_data::sources::resolve_chinese_name_rich("北部港湾");
    assert_eq!(r["source"], json!("fuzzy"), "{r}");
    // Not confident at distance 2 → no auto-resolution, candidates surfaced.
    assert_eq!(r["resolved"], Value::Null, "{r}");
    assert_eq!(r["candidates"][0]["code"], json!("000582.SZ"), "{r}");
    assert_eq!(r["candidates"][0]["name"], json!("北部湾港"), "{r}");
    assert_eq!(r["candidates"][0]["distance"], json!(2), "{r}");
    assert_eq!(r["user_input"], json!("北部港湾"), "{r}");

    // Tier 2 · substring search: the first row whose name *contains* the query
    // wins. Upstream's `str.contains` is deliberately loose, so a short query
    // like "平安" resolves here rather than falling through to fuzzy. Both 平安
    // rows qualify; the first index row is the one returned.
    let substring = uzi_data::sources::resolve_chinese_name_rich("平安");
    assert_eq!(substring["source"], json!("exact"), "{substring}");
    assert_eq!(substring["resolved"]["full"], json!("601318.SH"), "{substring}");
    assert_eq!(substring["candidates"][0]["distance"], json!(0), "{substring}");

    // An exact hit resolves and reports the tier that produced it.
    let exact = uzi_data::sources::resolve_chinese_name_rich("水晶光电");
    assert_eq!(exact["source"], json!("exact"), "{exact}");
    assert_eq!(exact["resolved"]["full"], json!("002273.SZ"), "{exact}");

    // A name that matches nothing stays unresolved rather than guessing.
    let miss = uzi_data::sources::resolve_chinese_name_rich("完全不存在的公司");
    assert_eq!(miss["source"], json!("none"), "{miss}");
    assert_eq!(miss["candidates"], json!([]), "{miss}");

    // ── Scenario B: fuzzy-only table ─────────────────────────────────────────
    let root = fresh_root("b");
    seed_index(
        &root,
        &json!([
            {"code": "000582", "name": "北部湾港"},
            {"code": "600519", "name": "贵州茅台"}
        ]),
    );

    // The fuzzy tier never auto-resolves a non-zero distance — the caller gets
    // candidates to disambiguate instead.
    let r = uzi_data::sources::resolve_chinese_name_rich("北部港湾");
    assert_eq!(r["source"], json!("fuzzy"), "{r}");
    assert_eq!(r["resolved"], Value::Null, "{r}");
    assert_eq!(r["candidates"][0]["distance"], json!(2), "{r}");
    assert_eq!(r["candidates"][0]["code"], json!("000582.SZ"), "{r}");

    // The legacy shim only returns a ticker when the resolver was confident...
    assert!(uzi_data::sources::resolve_chinese_name("北部港湾").is_none());
    // ...and does return one when it was.
    let hit = uzi_data::sources::resolve_chinese_name("贵州茅台").expect("exact hit resolves");
    assert_eq!(hit.full, "600519.SH");

    // ── Scenario C: cached table present but empty ───────────────────────────
    // Upstream's `if not index: return []` path: no candidates, no guesswork,
    // and no network round-trip (the cache entry is fresh).
    let root = fresh_root("c");
    seed_index(&root, &json!([]));
    let empty = uzi_data::sources::resolve_chinese_name_rich("贵州茅台");
    assert_eq!(empty["source"], json!("none"), "{empty}");
    assert_eq!(empty["resolved"], Value::Null, "{empty}");
    assert_eq!(empty["candidates"], json!([]), "{empty}");

    std::env::remove_var("UZI_CACHE_ROOT");
    let _ = std::fs::remove_dir_all(fresh_root("a"));
    let _ = std::fs::remove_dir_all(fresh_root("b"));
    let _ = std::fs::remove_dir_all(fresh_root("c"));
}
