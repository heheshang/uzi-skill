//! Live-network smoke tests.
//!
//! Run explicitly (they are `#[ignore]`d so the default `cargo test -p uzi-data`
//! stays offline):
//!
//! ```text
//! cargo test -p uzi-data --test live_quote -- --ignored
//! ```
//!
//! `fetch_one_quote_when_enabled` additionally runs only when `UZI_LIVE_TEST=1`:
//!
//! ```text
//! UZI_LIVE_TEST=1 cargo test -p uzi-data --test live_quote
//! ```

use serde_json::Value;
use uzi_data::providers::direct_http;

fn assert_tencent_quote(v: &Value) {
    let obj = v.as_object().expect("quote must be an object");
    assert!(
        obj.get("name").and_then(|n| n.as_str()).map(|s| !s.is_empty()).unwrap_or(false),
        "name missing: {v}"
    );
    let price = obj.get("price").and_then(|p| p.as_f64()).unwrap_or(0.0);
    assert!(price > 0.0, "price must be positive: {v}");
}

#[test]
#[ignore = "hits qt.gtimg.cn; run with `cargo test -p uzi-data --test live_quote -- --ignored`"]
fn tencent_quote_endpoint_is_reachable() {
    let quote = direct_http::fetch_quote("600519", "A").expect("quote endpoint reachable");
    assert_tencent_quote(&quote);
}

#[test]
fn fetch_one_quote_when_enabled() {
    if std::env::var("UZI_LIVE_TEST").as_deref() != Ok("1") {
        eprintln!("skipping: set UZI_LIVE_TEST=1 to run the live quote check");
        return;
    }
    for (code, market) in [("600519", "A"), ("002273", "A")] {
        let quote = direct_http::fetch_quote(code, market).expect("live quote");
        assert_tencent_quote(&quote);
    }
}
