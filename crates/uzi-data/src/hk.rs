//! Port of `lib/hk_data_sources.py` — HK basic info / valuation ranks /
//! HKEXNews announcements.
//!
//! Upstream's first three sources are AkShare's XueQiu and EastMoney HK
//! wrappers; the Rust port has no AkShare, so those functions return the
//! upstream failure shape (`{code5, _err: "akshare not installed"}` and empty
//! rank blocks) instead of inventing data. The HKEXNews scraper is pure HTTP and
//! is ported verbatim.

use serde_json::{json, Map, Value};
use std::sync::LazyLock;

use crate::http;

const UA: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36";

/// `_safe_get(df, col, default)` — first-row column getter.
pub fn safe_get(df: &Value, col: &str) -> Option<Value> {
    let row = df.as_array()?.first()?;
    let v = row.get(col)?;
    if v.is_null() || v.as_str() == Some("nan") {
        None
    } else {
        Some(v.clone())
    }
}

/// `fetch_hk_basic(code5)` — AkShare XueQiu/EM company profile is unavailable.
pub fn fetch_hk_basic(code5: &str) -> Value {
    json!({"code5": code5, "_err": "akshare not installed"})
}

/// `fetch_hk_valuation_ranks(code5)` — AkShare HK comparison tables unavailable.
pub fn fetch_hk_valuation_ranks(code5: &str) -> Value {
    json!({"code5": code5})
}

/// `fetch_hk_announcements(code5, limit)` — HKEXNews title search page scrape.
pub fn fetch_hk_announcements(code5: &str, limit: usize) -> Vec<Value> {
    let int_code = code5.trim_start_matches('0');
    let int_code = if int_code.is_empty() { "0" } else { int_code };
    let url = format!(
        "https://www1.hkexnews.hk/search/titlesearch.xhtml?lang=zh&category=0&t1code=&market=SEHK&stockId={int_code}"
    );
    let Ok(resp) = http::get(&url, &[("User-Agent", UA)], 15) else {
        return Vec::new();
    };
    if resp.status != 200 {
        return Vec::new();
    }
    let html = resp.text();
    static ANCHOR: LazyLock<regex::Regex> = LazyLock::new(|| {
        regex::Regex::new(r#"(?i)<a[^>]+href="([^"]+(?:listedco|listconews|filing)[^"]+)"[^>]*>([^<]{6,200})</a>"#)
            .unwrap()
    });
    let mut items: Vec<Value> = Vec::new();
    let mut seen: Vec<String> = Vec::new();
    for cap in ANCHOR.captures_iter(&html) {
        let href = cap[1].to_string();
        let title = cap[2].trim().to_string();
        if title.is_empty() || seen.contains(&title) {
            continue;
        }
        seen.push(title.clone());
        let full_url = if href.starts_with("http") {
            href
        } else {
            format!("https://www1.hkexnews.hk{href}")
        };
        items.push(json!({
            "date": "",
            "title": title,
            "url": full_url,
            "source": "hkexnews",
        }));
        if items.len() >= limit {
            break;
        }
    }
    items
}

/// `fetch_hk_basic_combined(code5)` — merges basic + ranks, projects PE/PB/mcap.
pub fn fetch_hk_basic_combined(code5: &str) -> Value {
    let mut basic = fetch_hk_basic(code5);
    let ranks = fetch_hk_valuation_ranks(code5);
    let mut obj: Map<String, Value> = basic.as_object().cloned().unwrap_or_default();
    let val = ranks.get("valuation").cloned().unwrap_or_else(|| json!({}));
    let scale = ranks.get("scale").cloned().unwrap_or_else(|| json!({}));
    if let Some(pe) = val.get("pe_ttm").filter(|v| !v.is_null()) {
        obj.insert("pe_ttm".into(), pe.clone());
    }
    if let Some(pb) = val.get("pb_mrq").filter(|v| !v.is_null()) {
        obj.insert("pb".into(), pb.clone());
    }
    if let Some(mc) = scale.get("market_cap").and_then(|v| v.as_f64()) {
        obj.insert("market_cap_raw".into(), json!(mc));
        obj.insert(
            "market_cap".into(),
            json!(format!("{}亿", uzi_core::py::round(mc / 1e8, 1))),
        );
    }
    obj.insert("_ranks".into(), ranks);
    basic = Value::Object(obj);
    basic
}

/// `fetch_hk_announcements_cached(code5, limit)` — TTL 1h.
pub fn fetch_hk_announcements_cached(code5: &str, limit: usize) -> Value {
    let key = format!("hk_anns_{limit}");
    let ticker = format!("HK_{code5}");
    let code = code5.to_string();
    uzi_core::cache::cached::<_, anyhow::Error>(
        &ticker,
        &key,
        uzi_core::cache::TTL_HOURLY,
        move || Ok(json!(fetch_hk_announcements(&code, limit))),
    )
    .unwrap_or_else(|_| json!([]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic_degrades_with_upstream_error_key() {
        let v = fetch_hk_basic("00700");
        assert_eq!(v["code5"], json!("00700"));
        assert_eq!(v["_err"], json!("akshare not installed"));
    }

    #[test]
    fn safe_get_handles_missing_and_nan() {
        let df = json!([{"a": 1, "b": "nan"}]);
        assert_eq!(safe_get(&df, "a"), Some(json!(1)));
        assert_eq!(safe_get(&df, "b"), None);
        assert_eq!(safe_get(&df, "c"), None);
        assert_eq!(safe_get(&json!([]), "a"), None);
    }

    #[test]
    fn combined_keeps_code5_and_ranks() {
        let v = fetch_hk_basic_combined("00700");
        assert_eq!(v["code5"], json!("00700"));
        assert!(v["_ranks"].is_object());
    }
}
