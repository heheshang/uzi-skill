//! Port of `lib/web_search.py` — cached web search with a ddgs primary and a
//! per-run budget.
//!
//! Upstream imports the Python `ddgs` package; the Rust port talks to the
//! DuckDuckGo HTML endpoint (`html.duckduckgo.com/html/?q=`) and parses the
//! result blocks. When the endpoint is unreachable it returns exactly the
//! upstream failure shape `[{"error": "ddgs: ..."}]`, and
//! [`search_trusted`] / [`quick_summary`] degrade identically.

use serde_json::{json, Map, Value};
use std::sync::{LazyLock, Mutex};

use crate::http;

pub const SEARCH_TTL: u64 = 12 * 60 * 60;

/// `TRUSTED_DOMAINS_BY_DIM`.
pub fn trusted_domains_for(dim_key: &str) -> &'static [&'static str] {
    match dim_key {
        "3_macro" => &[
            "stats.gov.cn",
            "pbc.gov.cn",
            "safe.gov.cn",
            "gov.cn",
            "chinamoney.com.cn",
            "chinabond.com.cn",
            "cs.com.cn",
            "cnstock.com",
            "stcn.com",
            "nbd.com.cn",
        ],
        "13_policy" => &[
            "gov.cn",
            "csrc.gov.cn",
            "miit.gov.cn",
            "ndrc.gov.cn",
            "samr.gov.cn",
            "pbc.gov.cn",
            "safe.gov.cn",
            "cs.com.cn",
            "cnstock.com",
            "stcn.com",
        ],
        "15_events" => &[
            "cs.com.cn",
            "cnstock.com",
            "stcn.com",
            "nbd.com.cn",
            "sse.com.cn",
            "szse.cn",
            "hkexnews.hk",
            "yicai.com",
            "cls.cn",
            "wallstreetcn.com",
        ],
        "17_sentiment" => &[
            "xueqiu.com",
            "guba.eastmoney.com",
            "tgb.cn",
            "jisilu.cn",
            "zhihu.com",
            "nbd.com.cn",
            "stcn.com",
        ],
        "18_trap" => &[
            "zhihu.com",
            "weibo.com",
            "xiaohongshu.com",
            "douyin.com",
            "tgb.cn",
            "guba.eastmoney.com",
            "cs.com.cn",
            "nbd.com.cn",
        ],
        "7_industry" => &[
            "stats.gov.cn",
            "miit.gov.cn",
            "ndrc.gov.cn",
            "cs.com.cn",
            "cnstock.com",
            "stcn.com",
            "nbd.com.cn",
        ],
        "14_moat" => &[
            "nbd.com.cn",
            "yicai.com",
            "cs.com.cn",
            "stcn.com",
            "wallstreetcn.com",
            "cls.cn",
        ],
        "8_materials" => &[
            "shfe.com.cn",
            "dce.com.cn",
            "czce.com.cn",
            "ine.cn",
            "100ppi.com",
            "fx678.com",
        ],
        "9_futures" => &[
            "shfe.com.cn",
            "dce.com.cn",
            "czce.com.cn",
            "ine.cn",
            "fx678.com",
        ],
        _ => &[],
    }
}

// ─────────────────────────────────────────────────────────────
// v2.10.1 · global ddgs budget
// ─────────────────────────────────────────────────────────────

#[derive(Default, Clone, Copy)]
struct BudgetState {
    used: u32,
    skipped: u32,
}

static BUDGET: LazyLock<Mutex<BudgetState>> = LazyLock::new(|| Mutex::new(BudgetState::default()));

fn budget_allows() -> bool {
    let cap_raw = std::env::var("UZI_DDG_BUDGET").ok();
    let Some(cap_raw) = cap_raw else { return true };
    let Ok(cap) = cap_raw.parse::<u32>() else {
        return true;
    };
    BUDGET.lock().map(|b| b.used < cap).unwrap_or(true)
}

fn budget_mark_used() {
    if let Ok(mut b) = BUDGET.lock() {
        b.used += 1;
    }
}

fn budget_mark_skipped() {
    if let Ok(mut b) = BUDGET.lock() {
        b.skipped += 1;
    }
}

/// `get_budget_state()`.
pub fn budget_state() -> Value {
    let b = BUDGET.lock().map(|b| *b).unwrap_or_default();
    json!({"used": b.used, "skipped": b.skipped})
}

/// Test-only reset for the process-wide budget counter.
#[doc(hidden)]
pub fn reset_budget() {
    if let Ok(mut b) = BUDGET.lock() {
        *b = BudgetState::default();
    }
}

// ─────────────────────────────────────────────────────────────
// ddgs search
// ─────────────────────────────────────────────────────────────

/// `_ddg_search(query, max_results, region)` — hard timeout, normalised fields.
pub fn ddg_search(query: &str, max_results: usize) -> Vec<Value> {
    let timeout: u64 = std::env::var("UZI_DDG_TIMEOUT")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(10);
    let url = http::with_query(
        "https://html.duckduckgo.com/html/",
        &[("q", query), ("kl", "cn-zh")],
    );
    let resp = match http::get(&url, &[], timeout) {
        Ok(r) => r,
        Err(e) => {
            return vec![json!({"error": format!("ddgs: timeout > {timeout}s（代理/网络不通？）: {e}")})]
        }
    };
    if !resp.is_ok() {
        return vec![json!({"error": format!("ddgs: HTTP {}", resp.status)})];
    }
    let html = resp.text();
    let mut out = Vec::new();
    // Result anchors carry class="result__a"; snippets class="result__snippet".
    static ANCHOR: LazyLock<regex::Regex> = LazyLock::new(|| {
        regex::Regex::new(r#"(?s)<a[^>]*class="result__a"[^>]*href="([^"]*)"[^>]*>(.*?)</a>"#)
            .unwrap()
    });
    static SNIPPET: LazyLock<regex::Regex> = LazyLock::new(|| {
        regex::Regex::new(r#"(?s)<a[^>]*class="result__snippet"[^>]*>(.*?)</a>"#).unwrap()
    });
    let snippets: Vec<String> = SNIPPET
        .captures_iter(&html)
        .map(|c| strip_tags(&c[1]))
        .collect();
    for (i, cap) in ANCHOR.captures_iter(&html).enumerate() {
        if out.len() >= max_results {
            break;
        }
        let href = decode_entities(&cap[1]);
        let title = strip_tags(&cap[2]);
        if title.is_empty() {
            continue;
        }
        out.push(json!({
            "title": title,
            "body": snippets.get(i).cloned().unwrap_or_default(),
            "url": unwrap_ddg_redirect(&href),
            "source": "ddgs",
        }));
    }
    if out.is_empty() {
        return vec![json!({"error": "ddgs: no results parsed（可能是反爬/网络受限）"})];
    }
    out
}

/// `_GARBAGE_PATTERNS` + `_is_garbage_result`.
const GARBAGE_PATTERNS: &[&str] = &[
    "拼音",
    "汉语",
    "通用规范汉字",
    "常用字",
    "甲骨文",
    "部首",
    "笔画",
    "Unicode",
    "字形演变",
    "偏旁",
    "百科词条概述",
    "释义",
    "本义",
    "引申义",
];

pub fn is_garbage_result(r: &Value) -> bool {
    let text = format!(
        "{} {}",
        r.get("body").and_then(|v| v.as_str()).unwrap_or(""),
        r.get("title").and_then(|v| v.as_str()).unwrap_or("")
    );
    GARBAGE_PATTERNS.iter().filter(|p| text.contains(**p)).count() >= 2
}

/// `search(query, max_results, cache_key_prefix)` — cached, budget-gated,
/// garbage-filtered.
pub fn search(query: &str, max_results: usize, cache_key_prefix: &str) -> Vec<Value> {
    let key = format!(
        "{cache_key_prefix}__{}__n{max_results}",
        query.chars().take(100).collect::<String>()
    );
    let q = query.to_string();
    let raw = uzi_core::cache::cached::<_, anyhow::Error>("_global", &key, SEARCH_TTL, move || {
        if !budget_allows() {
            budget_mark_skipped();
            return Ok(json!([{
                "_budget_exceeded": true,
                "body": "全局 ddgs 预算已用尽（UZI_DDG_BUDGET），agent 请用 cached / hardcoded 数据"
            }]));
        }
        budget_mark_used();
        Ok(Value::Array(ddg_search(&q, max_results)))
    })
    .unwrap_or_else(|_| json!([]));

    raw.as_array()
        .map(|arr| {
            arr.iter()
                .filter(|r| {
                    !is_garbage_result(r)
                        && !r
                            .get("_budget_exceeded")
                            .and_then(|v| v.as_bool())
                            .unwrap_or(false)
                })
                .cloned()
                .collect()
        })
        .unwrap_or_default()
}

/// `search_trusted(query, dim_key, max_results, extra_sites, max_sites)`.
pub fn search_trusted(
    query: &str,
    dim_key: &str,
    max_results: usize,
    extra_sites: &[&str],
    max_sites: usize,
) -> Vec<Value> {
    let mut domains: Vec<&str> = trusted_domains_for(dim_key).to_vec();
    domains.extend_from_slice(extra_sites);
    if domains.is_empty() {
        return search(query, max_results, "ws");
    }
    let truncated = &domains[..domains.len().min(max_sites)];
    let site_clause = truncated
        .iter()
        .map(|d| format!("site:{d}"))
        .collect::<Vec<_>>()
        .join(" OR ");
    let combined = format!("({site_clause}) {query}");
    search(&combined, max_results, &format!("wst_{dim_key}"))
}

/// `search_multi(queries, per_query)`.
pub fn search_multi(queries: &[String], per_query: usize) -> Value {
    let mut out = Map::new();
    for q in queries {
        out.insert(q.clone(), json!(search(q, per_query, "ws")));
    }
    Value::Object(out)
}

/// `extract_snippets(results, max_snippets, body_chars)`.
pub fn extract_snippets(results: &[Value], max_snippets: usize, body_chars: usize) -> Vec<String> {
    results
        .iter()
        .take(max_snippets)
        .filter(|r| r.get("error").is_none())
        .filter_map(|r| {
            let title = first_n(r.get("title").and_then(|v| v.as_str()).unwrap_or(""), 80);
            let body = first_n(
                r.get("body").and_then(|v| v.as_str()).unwrap_or(""),
                body_chars,
            );
            let url = r.get("url").and_then(|v| v.as_str()).unwrap_or("");
            if title.is_empty() && body.is_empty() {
                None
            } else {
                Some(format!("{title} · {body} · {url}"))
            }
        })
        .collect()
}

/// `quick_summary(query, max_snippets)`.
pub fn quick_summary(query: &str, max_snippets: usize) -> Value {
    let results = search(query, max_snippets * 2, "ws");
    let valid: Vec<&Value> = results.iter().filter(|r| r.get("error").is_none()).collect();
    json!({
        "query": query,
        "count": valid.len(),
        "snippets": valid.iter().take(max_snippets).map(|r| json!({
            "title": first_n(r.get("title").and_then(|v| v.as_str()).unwrap_or(""), 100),
            "body": first_n(r.get("body").and_then(|v| v.as_str()).unwrap_or(""), 280),
            "url": r.get("url").and_then(|v| v.as_str()).unwrap_or(""),
        })).collect::<Vec<_>>(),
        "has_data": !valid.is_empty(),
    })
}

fn first_n(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

fn strip_tags(s: &str) -> String {
    static TAGS: LazyLock<regex::Regex> = LazyLock::new(|| regex::Regex::new(r"<[^>]*>").unwrap());
    let text = TAGS.replace_all(s, "");
    decode_entities(text.trim())
}

fn decode_entities(s: &str) -> String {
    s.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#x27;", "'")
        .replace("&#39;", "'")
        .replace("&nbsp;", " ")
}

/// DDG wraps result URLs in `/l/?uddg=<encoded>`; return the real target.
fn unwrap_ddg_redirect(href: &str) -> String {
    if let Some(idx) = href.find("uddg=") {
        let enc = &href[idx + 5..];
        let enc = enc.split('&').next().unwrap_or(enc);
        return percent_decode(enc);
    }
    if href.starts_with("//") {
        return format!("https:{href}");
    }
    href.to_string()
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or("");
            if let Ok(b) = u8::from_str_radix(hex, 16) {
                out.push(b);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trusted_domain_lookup() {
        assert!(trusted_domains_for("13_policy").contains(&"csrc.gov.cn"));
        assert!(trusted_domains_for("no_such_dim").is_empty());
    }

    #[test]
    fn garbage_filter_requires_two_markers() {
        assert!(is_garbage_result(&json!({"title": "拼音 释义 本义", "body": ""})));
        assert!(!is_garbage_result(&json!({"title": "拼音", "body": ""})));
        assert!(!is_garbage_result(&json!({"title": "贵州茅台业绩", "body": "白酒" })));
    }

    #[test]
    fn ddg_redirect_unwrapping() {
        assert_eq!(
            unwrap_ddg_redirect("//duckduckgo.com/l/?uddg=https%3A%2F%2Fexample.com%2Fa&rut=x"),
            "https://example.com/a"
        );
        assert_eq!(unwrap_ddg_redirect("https://x.com"), "https://x.com");
    }

    #[test]
    fn snippets_and_summary_shapes() {
        let results = vec![
            json!({"title": "t1", "body": "b1", "url": "u1"}),
            json!({"error": "ddgs: x"}),
        ];
        let snips = extract_snippets(&results, 3, 200);
        assert_eq!(snips, vec!["t1 · b1 · u1".to_string()]);
    }

    #[test]
    fn budget_exhaustion_short_circuits() {
        reset_budget();
        std::env::set_var("UZI_DDG_BUDGET", "0");
        assert!(!budget_allows());
        std::env::remove_var("UZI_DDG_BUDGET");
        reset_budget();
        assert!(budget_allows());
    }
}
