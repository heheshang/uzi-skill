//! Differential test: `uzi_data::browser::xueqiu` parsers vs upstream
//! `lib/xueqiu_browser.py`.
//!
//! The fetch half needs a browser and a XueQiu login; the **parsing** half is
//! pure and is what this port reimplements. `tools/golden/dump_xueqiu.py` feeds
//! the upstream parsers canned HTML with `fetch_with_browser` stubbed, so the
//! regexes, field projection, symbol normalization, dedup, and caps compared here
//! are upstream's own.

use serde_json::json;
use uzi_core::testkit::{assert_json_eq, load_golden};
use uzi_data::browser::xueqiu;

/// The same HTML the dump script serves, kept in one place per case.
const CUBES_HTML: &str = r#"<html><body>{"list":[{"name":"稳健组合","symbol":"ZH001","daily_gain":1.2,"monthly_gain":3.4,"total_gain":56.7,"annualized_gain_rate":12.3,"stocks_count":8,"view_rebalancing_count":4,"owner":{"screen_name":"张三"}},{"name":"激进组合","symbol":"ZH002","daily_gain":-0.5,"monthly_gain":9.1,"total_gain":120.4,"annualized_gain_rate":31.7,"stocks_count":15,"view_rebalancing_count":22,"owner":{"screen_name":"李四"}},{"name":"无主组合","symbol":"ZH003"}]}</body></html>"#;
const CUBES_ALT_KEY_HTML: &str = r#"<html>{"cubes":[{"name":"A","symbol":"ZH009","total_gain":5}]}</html>"#;
const CUBES_INVALID_HTML: &str = r#"<html><body>{"list": not valid json}</body></html>"#;
const NO_JSON_HTML: &str = "<html><body>no json here</body></html>";

const PEERS_HTML: &str = r#"
<div class="peers">
  <a href="/S/SH600519" class="x">贵州茅台</a>
  <a href="/S/SZ000582">北部湾港</a>
  <a href="/S/SH600519">贵州茅台</a>
  <a href="/S/SH600520">短</a>
  <a href="/S/HK00700">腾讯控股</a>
  <a href="/S/BJ430047">诺思兰德</a>
  <a href="/S/US/AAPL">苹果</a>
</div>
"#;
const PEERS_UNCLOSED: &str = "<a href='/S/SH600519'>unclosed";

fn capped_peers_html() -> String {
    (0..50)
        .map(|i| format!(r#"<a href="/S/SH6005{i:02}">公司{i:02}</a>"#))
        .collect()
}

/// Every cubes page must yield exactly the list upstream yields.
#[test]
fn cubes_parsing_matches_upstream() {
    let golden = load_golden("xueqiu", "parsers");
    let expected = golden["cubes"].as_object().expect("golden.cubes");

    let cases: [(&str, &str); 5] = [
        ("cubes", CUBES_HTML),
        ("cubes_alt_key", CUBES_ALT_KEY_HTML),
        ("cubes_invalid", CUBES_INVALID_HTML),
        ("no_json", NO_JSON_HTML),
        ("empty", ""),
    ];

    for (name, html) in cases {
        let actual = serde_json::Value::Array(xueqiu::parse_cubes(html));
        assert_json_eq(&actual, &expected[name], &format!("parse_cubes/{name}"));
    }
}

/// Every peers page must yield exactly the list upstream yields — including the
/// cap, the dedup, and the single-character filter.
#[test]
fn peers_parsing_matches_upstream() {
    let golden = load_golden("xueqiu", "parsers");
    let expected = golden["peers"].as_object().expect("golden.peers");
    let capped = capped_peers_html();

    let cases: [(&str, &str, usize); 4] = [
        ("mixed", PEERS_HTML, 20),
        ("capped", &capped, 5),
        ("unclosed", PEERS_UNCLOSED, 20),
        ("empty", "", 20),
    ];

    for (name, html, cap) in cases {
        let actual = serde_json::Value::Array(xueqiu::parse_peers(html, cap));
        assert_json_eq(&actual, &expected[name], &format!("parse_peers/{name}"));
    }
}

/// `xq_symbol`'s branch order determines the result, so pin every case —
/// including `00700`, which upstream sends to the SZ branch because the leading
/// `0` matches before the Hong Kong rule can.
#[test]
fn symbol_conversion_matches_upstream() {
    let golden = load_golden("xueqiu", "parsers");
    let expected = golden["symbols"].as_object().expect("golden.symbols");
    assert!(!expected.is_empty(), "golden holds no symbols");

    for (input, want) in expected {
        assert_eq!(
            xueqiu::xq_symbol(input),
            want.as_str().unwrap(),
            "xq_symbol({input:?})"
        );
    }
}

/// The login gate is the safety property callers depend on: without an explicit
/// opt-in, nothing touches the browser and the caller gets an empty payload.
#[test]
fn unauthenticated_paths_degrade_to_empty() {
    let previous = std::env::var("UZI_XQ_LOGIN").ok();
    std::env::remove_var("UZI_XQ_LOGIN");

    assert!(!xueqiu::is_login_enabled());
    assert!(xueqiu::fetch_cubes_via_browser("SH600519", 10).is_empty());
    assert!(xueqiu::fetch_peers_via_browser("600519", 20).is_empty());

    match previous {
        Some(v) => std::env::set_var("UZI_XQ_LOGIN", v),
        None => std::env::remove_var("UZI_XQ_LOGIN"),
    }
}

/// The projection always emits upstream's full field set, so downstream code can
/// index a cube's fields without an existence check.
#[test]
fn cube_projection_has_a_stable_schema() {
    let cubes = xueqiu::parse_cubes(CUBES_HTML);
    assert!(!cubes.is_empty());
    let keys: Vec<&str> = cubes[0].as_object().unwrap().keys().map(|s| s.as_str()).collect();
    assert_eq!(
        keys,
        vec![
            "name",
            "owner",
            "symbol",
            "daily_gain",
            "monthly_gain",
            "total_gain",
            "annualized_gain_rate",
            "url",
            "stocks_count",
            "view_rebalancing_count",
        ]
    );
    // Every cube carries a url built from its symbol.
    assert_eq!(cubes[2]["name"], json!("无主组合"));
    assert_eq!(cubes[2]["symbol"], json!("ZH003"));
    assert_eq!(cubes[2]["url"], json!("https://xueqiu.com/P/ZH003"));

    // A cube genuinely lacking a symbol gets a null url, never a dangling one.
    let bare = xueqiu::parse_cubes(r#"{"list":[{"name":"无代码"}]}"#);
    assert_eq!(bare.len(), 1);
    assert_eq!(bare[0]["url"], serde_json::Value::Null);
    assert_eq!(bare[0]["symbol"], serde_json::Value::Null);
}
