//! Port of `lib/hottrend.py` — 6-platform hot-search aggregation
//! (微博 / 知乎 / 百度 / 抖音 / 头条 / B站) with a 5-minute file cache.
//!
//! Each platform fetcher has its own UA and degrades to an empty list when the
//! endpoint rejects the request, exactly like upstream's per-platform
//! `try/except` returning `[]`.

use serde_json::{json, Map, Value};
use std::path::PathBuf;

use crate::http;

pub const UA_PC: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36";
pub const UA_MAC: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36";
pub const UA_MOBILE: &str = "Mozilla/5.0 (iPhone; CPU iPhone OS 14_0 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/14.0 Mobile/15E148 Safari/604.1";

pub const CACHE_TTL_SEC: u64 = 300;

pub const SUPPORTED_PLATFORMS: &[(&str, &str)] = &[
    ("weibo", "微博热搜"),
    ("zhihu", "知乎热榜"),
    ("baidu", "百度热搜"),
    ("douyin", "抖音热点"),
    ("toutiao", "头条热榜"),
    ("bilibili", "B 站热搜"),
];

fn http_timeout() -> u64 {
    std::env::var("UZI_HTTP_TIMEOUT")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(20)
}

fn cache_dir() -> PathBuf {
    uzi_core::cache::cache_root()
        .join("_global")
        .join("hottrend")
}

fn now_secs() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

fn cache_get(platform: &str) -> Option<Vec<Value>> {
    let raw = std::fs::read_to_string(cache_dir().join(format!("{platform}.json"))).ok()?;
    let v: Value = serde_json::from_str(&raw).ok()?;
    let ts = v.get("ts").and_then(|t| t.as_f64()).unwrap_or(0.0);
    if now_secs() - ts > CACHE_TTL_SEC as f64 {
        return None;
    }
    v.get("items").and_then(|i| i.as_array()).cloned()
}

fn cache_set(platform: &str, items: &[Value]) {
    let dir = cache_dir();
    if std::fs::create_dir_all(&dir).is_err() {
        return;
    }
    let payload = json!({"ts": now_secs(), "items": items});
    let _ = std::fs::write(
        dir.join(format!("{platform}.json")),
        serde_json::to_string(&payload).unwrap_or_default(),
    );
}

/// `_http_json(url, ua, extra_headers)` with the per-platform UA.
fn http_json_ua(url: &str, ua: &str) -> Option<Value> {
    let resp = http::get(
        url,
        &[
            ("User-Agent", ua),
            ("Accept", "application/json, text/plain, */*"),
        ],
        http_timeout(),
    )
    .ok()?;
    if !resp.is_ok() {
        return None;
    }
    resp.json()
}

fn item(rank: usize, title: &str, url: &str, hot_score: i64, platform: &str, extra: &str) -> Value {
    json!({
        "rank": rank,
        "title": title,
        "url": url,
        "hot_score": hot_score,
        "platform": platform,
        "extra": extra,
    })
}

/// `fetch_weibo()`.
pub fn fetch_weibo() -> Vec<Value> {
    let Some(data) = http_json_ua("https://weibo.com/ajax/side/hotSearch", UA_MAC) else {
        return Vec::new();
    };
    let items = data
        .get("data")
        .and_then(|d| d.get("realtime"))
        .and_then(|r| r.as_array())
        .cloned()
        .unwrap_or_default();
    let mut out = Vec::new();
    for (i, it) in items.iter().take(50).enumerate() {
        let word = it.get("word").and_then(|v| v.as_str()).unwrap_or("");
        if word.is_empty() {
            continue;
        }
        out.push(item(
            i + 1,
            word,
            &format!("https://s.weibo.com/weibo?q={word}"),
            it.get("num").and_then(|v| v.as_i64()).unwrap_or(0),
            "weibo",
            it.get("category").and_then(|v| v.as_str()).unwrap_or(""),
        ));
    }
    out
}

/// `fetch_zhihu()`.
pub fn fetch_zhihu() -> Vec<Value> {
    let Some(data) = http_json_ua(
        "https://www.zhihu.com/api/v3/feed/topstory/hot-list-web?limit=50&desktop=true",
        UA_PC,
    ) else {
        return Vec::new();
    };
    let items = data
        .get("data")
        .and_then(|d| d.as_array())
        .cloned()
        .unwrap_or_default();
    let mut out = Vec::new();
    for (i, it) in items.iter().take(50).enumerate() {
        let tgt = it.get("target").cloned().unwrap_or_else(|| json!({}));
        let title = tgt
            .get("title_area")
            .and_then(|t| t.get("text"))
            .and_then(|v| v.as_str())
            .or_else(|| tgt.get("title").and_then(|v| v.as_str()))
            .unwrap_or("");
        if title.is_empty() {
            continue;
        }
        let link = tgt
            .get("link")
            .and_then(|l| l.get("url"))
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let metrics = tgt
            .get("metrics_area")
            .and_then(|m| m.get("text"))
            .and_then(|v| v.as_str())
            .unwrap_or("");
        out.push(item(i + 1, title, link, 0, "zhihu", metrics));
    }
    out
}

/// `fetch_baidu()`.
pub fn fetch_baidu() -> Vec<Value> {
    let Some(data) =
        http_json_ua("https://top.baidu.com/api/board?platform=wise&tab=realtime", UA_MOBILE)
    else {
        return Vec::new();
    };
    let cards = data
        .get("data")
        .and_then(|d| d.get("cards"))
        .and_then(|c| c.as_array())
        .cloned()
        .unwrap_or_default();
    let items: Vec<Value> = if !cards.is_empty() {
        cards[0]
            .get("content")
            .and_then(|c| c.as_array())
            .cloned()
            .unwrap_or_default()
    } else {
        Vec::new()
    };
    let mut out = Vec::new();
    for (i, it) in items.iter().take(50).enumerate() {
        let word = it
            .get("word")
            .and_then(|v| v.as_str())
            .or_else(|| it.get("query").and_then(|v| v.as_str()))
            .unwrap_or("");
        if word.is_empty() {
            continue;
        }
        let url = it
            .get("url")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string())
            .unwrap_or_else(|| format!("https://www.baidu.com/s?wd={word}"));
        out.push(item(
            i + 1,
            word,
            &url,
            it.get("hotScore").and_then(|v| v.as_i64()).unwrap_or(0),
            "baidu",
            "",
        ));
    }
    out
}

/// `fetch_douyin()`.
pub fn fetch_douyin() -> Vec<Value> {
    let Some(data) = http_json_ua("https://www.douyin.com/aweme/v1/web/hot/search/list/", UA_PC)
    else {
        return Vec::new();
    };
    let items = data
        .get("data")
        .and_then(|d| d.get("word_list"))
        .and_then(|w| w.as_array())
        .cloned()
        .unwrap_or_default();
    let mut out = Vec::new();
    for (i, it) in items.iter().take(50).enumerate() {
        let word = it.get("word").and_then(|v| v.as_str()).unwrap_or("");
        if word.is_empty() {
            continue;
        }
        out.push(item(
            i + 1,
            word,
            &format!("https://www.douyin.com/search/{word}"),
            it.get("hot_value").and_then(|v| v.as_i64()).unwrap_or(0),
            "douyin",
            "",
        ));
    }
    out
}

/// `fetch_toutiao()`.
pub fn fetch_toutiao() -> Vec<Value> {
    let Some(data) =
        http_json_ua("https://www.toutiao.com/hot-event/hot-board/?origin=toutiao_pc", UA_PC)
    else {
        return Vec::new();
    };
    let items = data
        .get("data")
        .and_then(|d| d.as_array())
        .cloned()
        .unwrap_or_default();
    let mut out = Vec::new();
    for (i, it) in items.iter().take(50).enumerate() {
        let title = it
            .get("Title")
            .and_then(|v| v.as_str())
            .or_else(|| it.get("title").and_then(|v| v.as_str()))
            .unwrap_or("");
        if title.is_empty() {
            continue;
        }
        let cid = it
            .get("ClusterIdStr")
            .or_else(|| it.get("ClusterId"))
            .map(|v| v.as_str().map(|s| s.to_string()).unwrap_or_else(|| v.to_string()))
            .unwrap_or_default();
        let url = if cid.is_empty() {
            String::new()
        } else {
            format!("https://www.toutiao.com/trending/{cid}/")
        };
        out.push(item(
            i + 1,
            title,
            &url,
            it.get("HotValue").and_then(|v| v.as_i64()).unwrap_or(0),
            "toutiao",
            it.get("LabelDesc").and_then(|v| v.as_str()).unwrap_or(""),
        ));
    }
    out
}

/// `fetch_bilibili()`.
pub fn fetch_bilibili() -> Vec<Value> {
    let Some(data) =
        http_json_ua("https://s.search.bilibili.com/main/hotword?limit=50", UA_PC)
    else {
        return Vec::new();
    };
    let items = data
        .get("list")
        .and_then(|l| l.as_array())
        .cloned()
        .unwrap_or_default();
    let mut out = Vec::new();
    for (i, it) in items.iter().take(50).enumerate() {
        let keyword = it
            .get("keyword")
            .and_then(|v| v.as_str())
            .or_else(|| it.get("show_name").and_then(|v| v.as_str()))
            .unwrap_or("");
        if keyword.is_empty() {
            continue;
        }
        out.push(item(
            i + 1,
            keyword,
            &format!("https://search.bilibili.com/all?keyword={keyword}"),
            it.get("heat_score").and_then(|v| v.as_i64()).unwrap_or(0),
            "bilibili",
            "",
        ));
    }
    out
}

fn fetch_platform(platform: &str) -> Vec<Value> {
    match platform {
        "weibo" => fetch_weibo(),
        "zhihu" => fetch_zhihu(),
        "baidu" => fetch_baidu(),
        "douyin" => fetch_douyin(),
        "toutiao" => fetch_toutiao(),
        "bilibili" => fetch_bilibili(),
        _ => Vec::new(),
    }
}

fn platform_cn(pid: &str) -> &str {
    SUPPORTED_PLATFORMS
        .iter()
        .find(|(p, _)| *p == pid)
        .map(|(_, cn)| *cn)
        .unwrap_or(pid)
}

/// `get_hot_trend(platform)`.
pub fn get_hot_trend(platform: &str) -> Value {
    if !SUPPORTED_PLATFORMS.iter().any(|(p, _)| *p == platform) {
        return json!({
            "platform": platform,
            "platform_cn": platform,
            "items": [],
            "updated_at": 0.0,
            "from_cache": false,
            "error": "unsupported platform",
        });
    }
    if let Some(cached) = cache_get(platform) {
        return json!({
            "platform": platform,
            "platform_cn": platform_cn(platform),
            "items": cached,
            "updated_at": now_secs(),
            "from_cache": true,
            "error": "",
        });
    }
    let items = fetch_platform(platform);
    if !items.is_empty() {
        cache_set(platform, &items);
    }
    json!({
        "platform": platform,
        "platform_cn": platform_cn(platform),
        "items": items,
        "updated_at": now_secs(),
        "from_cache": false,
        "error": "",
    })
}

/// `get_all_hot_trend()`.
pub fn get_all_hot_trend() -> Value {
    let mut out = Map::new();
    for (p, _) in SUPPORTED_PLATFORMS {
        out.insert((*p).to_string(), get_hot_trend(p));
    }
    Value::Object(out)
}

/// `get_hot_mentions(stock_name, extra_keywords)`.
pub fn get_hot_mentions(stock_name: &str, extra_keywords: &[String]) -> Value {
    let mut keywords: Vec<String> = vec![stock_name.to_string()];
    let chars: Vec<char> = stock_name.chars().collect();
    if chars.len() >= 3 {
        keywords.push(chars[..2].iter().collect());
    }
    if chars.len() >= 4 {
        keywords.push(chars[chars.len() - 2..].iter().collect());
    }
    for k in extra_keywords {
        if !k.trim().is_empty() {
            keywords.push(k.clone());
        }
    }
    let mut seen: Vec<String> = Vec::new();
    let mut cleaned: Vec<String> = Vec::new();
    for k in keywords {
        let ks = k.trim().to_string();
        if ks.chars().count() < 2 || seen.contains(&ks) {
            continue;
        }
        seen.push(ks.clone());
        cleaned.push(ks);
    }

    let mut mentions = Map::new();
    let mut by_count = Map::new();
    let mut ok_count = 0usize;
    for (platform, _) in SUPPORTED_PLATFORMS {
        let result = get_hot_trend(platform);
        let error = result.get("error").and_then(|v| v.as_str()).unwrap_or("");
        if !error.is_empty() {
            mentions.insert((*platform).to_string(), json!([]));
            by_count.insert((*platform).to_string(), json!(0));
            continue;
        }
        ok_count += 1;
        let mut hits: Vec<Value> = Vec::new();
        for it in result
            .get("items")
            .and_then(|i| i.as_array())
            .cloned()
            .unwrap_or_default()
        {
            let title = it.get("title").and_then(|v| v.as_str()).unwrap_or("");
            if cleaned.iter().any(|kw| title.contains(kw.as_str())) {
                hits.push(it);
            }
        }
        by_count.insert((*platform).to_string(), json!(hits.len()));
        mentions.insert((*platform).to_string(), Value::Array(hits));
    }
    let total: usize = by_count
        .values()
        .filter_map(|v| v.as_u64())
        .map(|v| v as usize)
        .sum();
    json!({
        "stock_name": stock_name,
        "keywords_used": cleaned,
        "platforms_checked": SUPPORTED_PLATFORMS.len(),
        "platforms_ok": ok_count,
        "mentions": Value::Object(mentions),
        "total_hits": total,
        "by_platform_count": Value::Object(by_count),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unsupported_platform_returns_error_shape() {
        let r = get_hot_trend("myspace");
        assert_eq!(r["error"], json!("unsupported platform"));
        assert_eq!(r["items"], json!([]));
    }

    #[test]
    fn mentions_shape_and_keyword_derivation() {
        let r = get_hot_mentions("贵州茅台", &[]);
        assert_eq!(r["keywords_used"], json!(["贵州茅台", "贵州", "茅台"]));
        assert_eq!(r["platforms_checked"], json!(6));
        assert!(r["mentions"].is_object());
        assert!(r["by_platform_count"].is_object());
    }

    #[test]
    fn extra_keywords_are_cleaned_and_deduped() {
        let r = get_hot_mentions("茅台", &["  茅台 ".into(), "x".into()]);
        // single-char keywords are dropped; 茅台 already present
        assert_eq!(r["keywords_used"], json!(["茅台"]));
    }
}
