//! Port of `lib/news_providers.py` — multi-source financial news aggregation
//! (金十数据 / 东财快讯 / 东财公告 / 同花顺今日快讯) with a 10-minute file cache.

use serde_json::{json, Map, Value};
use std::path::PathBuf;
use std::sync::LazyLock;

use crate::http;

pub const CACHE_TTL_SEC: u64 = 600;

/// `UA_PC` used by every news source.
pub const UA_PC: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 Chrome/120.0.0.0 Safari/537.36";

/// `HTTP_TIMEOUT = int(os.environ.get("UZI_HTTP_TIMEOUT", "20"))`.
pub fn http_timeout() -> u64 {
    std::env::var("UZI_HTTP_TIMEOUT")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(20)
}

fn cache_dir() -> PathBuf {
    uzi_core::cache::cache_root().join("_global").join("news")
}

/// `_cache_get(key)` — `None` on miss or staleness.
fn cache_get(key: &str) -> Option<Vec<Value>> {
    let f = cache_dir().join(format!("{key}.json"));
    let raw = std::fs::read_to_string(f).ok()?;
    let v: Value = serde_json::from_str(&raw).ok()?;
    let ts = v.get("ts").and_then(|t| t.as_f64()).unwrap_or(0.0);
    let now = now_secs();
    if now - ts > CACHE_TTL_SEC as f64 {
        return None;
    }
    v.get("items").and_then(|i| i.as_array()).cloned()
}

/// `_cache_set(key, items)`.
fn cache_set(key: &str, items: &[Value]) {
    let dir = cache_dir();
    if std::fs::create_dir_all(&dir).is_err() {
        return;
    }
    let payload = json!({"ts": now_secs(), "items": items});
    let _ = std::fs::write(
        dir.join(format!("{key}.json")),
        serde_json::to_string(&payload).unwrap_or_default(),
    );
}

fn now_secs() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

/// `_http_get(url, timeout)` — `None` on non-200 or transport failure. Uses the
/// response charset the way upstream's `apparent_encoding` does (UTF-8 here,
/// since none of the four endpoints advertise GBK).
fn http_get(url: &str, timeout: u64) -> Option<String> {
    let resp = http::get(url, &[("User-Agent", UA_PC)], timeout).ok()?;
    if !resp.is_ok() {
        return None;
    }
    Some(resp.text())
}

/// `NewsItem` — the unified record shape.
#[derive(Debug, Clone)]
pub struct NewsItem {
    pub source: &'static str,
    pub title: String,
    pub body: String,
    pub url: String,
    pub publish_time: String,
    pub raw_ts: f64,
}

impl NewsItem {
    fn new(source: &'static str, title: String) -> Self {
        NewsItem {
            source,
            title,
            body: String::new(),
            url: String::new(),
            publish_time: String::new(),
            raw_ts: 0.0,
        }
    }

    /// `NewsItem.to_dict()` — dataclass field order.
    pub fn to_dict(&self) -> Value {
        json!({
            "source": self.source,
            "title": self.title,
            "body": self.body,
            "url": self.url,
            "publish_time": self.publish_time,
            "raw_ts": self.raw_ts,
        })
    }

    fn from_dict(v: &Value) -> NewsItem {
        NewsItem {
            source: "unknown",
            title: str_of(v, "title"),
            body: str_of(v, "body"),
            url: str_of(v, "url"),
            publish_time: str_of(v, "publish_time"),
            raw_ts: v.get("raw_ts").and_then(|t| t.as_f64()).unwrap_or(0.0),
        }
    }
}

fn str_of(v: &Value, key: &str) -> String {
    v.get(key)
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .to_string()
}

/// `fetch_jin10(limit)`.
pub fn fetch_jin10(limit: usize) -> Vec<NewsItem> {
    if let Some(cached) = cache_get("jin10") {
        return cached.iter().take(limit).map(NewsItem::from_dict).collect();
    }
    let Some(txt) = http_get("https://www.jin10.com/flash_newest.js", 12) else {
        return Vec::new();
    };
    static ARR: LazyLock<regex::Regex> =
        LazyLock::new(|| regex::Regex::new(r"(?s)var newest\s*=\s*(\[.*?\]);").unwrap());
    let Some(cap) = ARR.captures(&txt) else {
        return Vec::new();
    };
    let Ok(data) = serde_json::from_str::<Value>(&cap[1]) else {
        return Vec::new();
    };
    let mut items = Vec::new();
    for row in data.as_array().cloned().unwrap_or_default().iter().take(limit) {
        let empty = json!({});
        let d = row.get("data").unwrap_or(&empty);
        let body_raw = d.get("content").and_then(|v| v.as_str()).unwrap_or("");
        let body = strip_tags(&body_raw.chars().take(300).collect::<String>());
        let mut title = d
            .get("title")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        if title.is_empty() && !body.is_empty() {
            title = body.chars().take(80).collect();
        }
        if title.is_empty() {
            continue;
        }
        let mut item = NewsItem::new("jin10", first_n(title.trim(), 200));
        item.body = body.trim().to_string();
        item.url = "https://www.jin10.com/".to_string();
        item.publish_time = row.get("time").and_then(|v| v.as_str()).unwrap_or("").to_string();
        items.push(item);
    }
    cache_set("jin10", &items.iter().map(NewsItem::to_dict).collect::<Vec<_>>());
    items
}

/// `fetch_em_kuaixun(limit)`.
pub fn fetch_em_kuaixun(limit: usize) -> Vec<NewsItem> {
    if let Some(cached) = cache_get("em_kuaixun") {
        return cached.iter().take(limit).map(NewsItem::from_dict).collect();
    }
    let url = "https://newsapi.eastmoney.com/kuaixun/v1/getlist_102_ajaxResult_50_1_.html";
    let Some(txt) = http_get(url, 12) else {
        return Vec::new();
    };
    static OBJ: LazyLock<regex::Regex> =
        LazyLock::new(|| regex::Regex::new(r"(?s)var ajaxResult\s*=\s*(\{.*\})\s*;?\s*$").unwrap());
    let Some(cap) = OBJ.captures(&txt) else {
        return Vec::new();
    };
    let Ok(data) = serde_json::from_str::<Value>(&cap[1]) else {
        return Vec::new();
    };
    let mut items = Vec::new();
    for row in data
        .get("LivesList")
        .and_then(|l| l.as_array())
        .cloned()
        .unwrap_or_default()
        .iter()
        .take(limit)
    {
        let digest = row.get("digest").and_then(|v| v.as_str()).unwrap_or("");
        let mut title = row
            .get("title")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        if title.is_empty() {
            title = digest.chars().take(100).collect();
        }
        if title.is_empty() {
            continue;
        }
        let mut item = NewsItem::new("em_kuaixun", first_n(title.trim(), 200));
        item.body = first_n(digest.trim(), 300);
        item.url = row
            .get("url_mobile")
            .or_else(|| row.get("url_w"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        item.publish_time = row
            .get("showtime")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        items.push(item);
    }
    cache_set(
        "em_kuaixun",
        &items.iter().map(NewsItem::to_dict).collect::<Vec<_>>(),
    );
    items
}

/// `fetch_em_stock_ann(stock_code, limit)`.
pub fn fetch_em_stock_ann(stock_code: &str, limit: usize) -> Vec<NewsItem> {
    let key = format!("em_ann_{}", if stock_code.is_empty() { "all" } else { stock_code });
    if let Some(cached) = cache_get(&key) {
        return cached.iter().take(limit).map(NewsItem::from_dict).collect();
    }
    let url = format!(
        "https://np-anotice-stock.eastmoney.com/api/security/ann?sr=-1&page_size={limit}&page_index=1&ann_type=A&client_source=web"
    );
    let Some(txt) = http_get(&url, 12) else {
        return Vec::new();
    };
    let Ok(data) = serde_json::from_str::<Value>(&txt) else {
        return Vec::new();
    };
    let mut items = Vec::new();
    for row in data
        .get("data")
        .and_then(|d| d.get("list"))
        .and_then(|l| l.as_array())
        .cloned()
        .unwrap_or_default()
        .iter()
        .take(limit)
    {
        let title = row.get("title").and_then(|v| v.as_str()).unwrap_or("");
        if title.is_empty() {
            continue;
        }
        if !stock_code.is_empty() {
            let matches = row
                .get("codes")
                .and_then(|c| c.as_array())
                .map(|codes| {
                    codes.iter().any(|c| {
                        c.get("stock_code").and_then(|v| v.as_str()) == Some(stock_code)
                    })
                })
                .unwrap_or(false);
            if !matches {
                continue;
            }
        }
        let mut item = NewsItem::new("em_stock_ann", first_n(title.trim(), 200));
        item.body = row
            .get("art_code")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        item.url = "https://np-anotice-stock.eastmoney.com/".to_string();
        item.publish_time = row
            .get("notice_date")
            .or_else(|| row.get("display_time"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        items.push(item);
    }
    cache_set(&key, &items.iter().map(NewsItem::to_dict).collect::<Vec<_>>());
    items
}

/// `fetch_ths_news_today(limit)`.
pub fn fetch_ths_news_today(limit: usize) -> Vec<NewsItem> {
    if let Some(cached) = cache_get("ths_today") {
        return cached.iter().take(limit).map(NewsItem::from_dict).collect();
    }
    let Some(txt) = http_get("http://news.10jqka.com.cn/today_list/", 12) else {
        return Vec::new();
    };
    static TITLE_A: LazyLock<regex::Regex> = LazyLock::new(|| {
        regex::Regex::new(r#"<a[^>]*class="[^"]*title[^"]*"[^>]*>([^<]{5,120})</a>"#).unwrap()
    });
    static TITLE_B: LazyLock<regex::Regex> = LazyLock::new(|| {
        regex::Regex::new(r#"<li[^>]*>\s*<a[^>]*>([^<]{10,100})</a>"#).unwrap()
    });
    let mut titles: Vec<String> = TITLE_A
        .captures_iter(&txt)
        .map(|c| c[1].trim().to_string())
        .collect();
    if titles.is_empty() {
        titles = TITLE_B
            .captures_iter(&txt)
            .map(|c| c[1].trim().to_string())
            .collect();
    }
    let mut items = Vec::new();
    for t in titles {
        if items.len() >= limit {
            break;
        }
        if t.chars().count() < 10 {
            continue;
        }
        let mut item = NewsItem::new("ths_news_today", first_n(&t, 200));
        item.url = "http://news.10jqka.com.cn/today_list/".to_string();
        items.push(item);
    }
    cache_set("ths_today", &items.iter().map(NewsItem::to_dict).collect::<Vec<_>>());
    items
}

/// `get_news_multi_source(stock_code, stock_name, limit_per_source)`.
pub fn get_news_multi_source(stock_code: &str, stock_name: &str, limit_per_source: usize) -> Value {
    let mut sources = Map::new();
    let mut total_hits = 0usize;
    let mut sources_ok = 0usize;

    let results: Vec<(&str, Vec<NewsItem>)> = vec![
        ("jin10", fetch_jin10(limit_per_source)),
        ("em_kuaixun", fetch_em_kuaixun(limit_per_source)),
        ("em_stock_ann", fetch_em_stock_ann(stock_code, limit_per_source)),
        ("ths_news_today", fetch_ths_news_today(limit_per_source)),
    ];

    for (name, items) in results {
        let filtered: Vec<NewsItem> = if stock_name.chars().count() >= 2 {
            let mut keywords = vec![stock_name.to_string()];
            let chars: Vec<char> = stock_name.chars().collect();
            if chars.len() >= 3 {
                keywords.push(chars[chars.len() - 2..].iter().collect());
            }
            items
                .into_iter()
                .filter(|i| keywords.iter().any(|k| i.title.contains(k) || i.body.contains(k)))
                .collect()
        } else {
            items
        };
        total_hits += filtered.len();
        sources_ok += 1;
        sources.insert(
            name.to_string(),
            Value::Array(filtered.iter().map(NewsItem::to_dict).collect()),
        );
    }

    json!({
        "stock_name": stock_name,
        "stock_code": stock_code,
        "sources": Value::Object(sources),
        "total_hits": total_hits,
        "sources_ok": sources_ok,
    })
}

fn strip_tags(s: &str) -> String {
    static TAGS: LazyLock<regex::Regex> = LazyLock::new(|| regex::Regex::new(r"<[^>]+>").unwrap());
    TAGS.replace_all(s, "").to_string()
}

fn first_n(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn news_item_dict_field_order() {
        let item = NewsItem::new("jin10", "t".into());
        let d = item.to_dict();
        let keys: Vec<&String> = d.as_object().unwrap().keys().collect();
        assert_eq!(
            keys,
            vec!["source", "title", "body", "url", "publish_time", "raw_ts"]
        );
    }

    #[test]
    fn multi_source_shape_is_stable_offline() {
        let r = get_news_multi_source("", "", 1);
        assert!(r["sources"].is_object());
        for k in ["stock_name", "stock_code", "sources", "total_hits", "sources_ok"] {
            assert!(r.get(k).is_some(), "missing {k}");
        }
    }
}
