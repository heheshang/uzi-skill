//! Port of `fetch_events.py`.
//!
//! Dimension 15 · 事件驱动 — 用 cninfo 公告 + 新闻兜底.

use serde_json::{json, Map, Value};
use std::time::Duration;

use uzi_core::py::{get, truthy};
use uzi_core::ticker::parse_ticker;

use crate::hk;
use crate::news;
use crate::sources;
use crate::web_search;

fn trunc(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

fn as_str_or<'a>(v: &'a Value, key: &str, default: &'a str) -> &'a str {
    v.get(key).and_then(|x| x.as_str()).unwrap_or(default)
}

// ─────────────────────────────────────────────────────────────
// cninfo direct API (v3.6.2 · pageSize=30, pageNum=1)
// ─────────────────────────────────────────────────────────────

/// `_cninfo_direct_api(code, page_size=30, timeout=15)`.
fn cninfo_direct_api(code: &str, page_size: usize, timeout: u64) -> Vec<Value> {
    let prefix: String = code.chars().take(3).collect();
    let (column, stock_code) = if matches!(prefix.as_str(), "000" | "001" | "002")
        || code.starts_with('3')
    {
        ("szse", code.to_string())
    } else if matches!(prefix.as_str(), "600" | "601" | "603" | "605" | "688" | "689") {
        ("sse", code.to_string())
    } else {
        ("bse", code.to_string())
    };

    let url = "http://www.cninfo.com.cn/new/hisAnnouncement/query";
    let pairs: Vec<(&str, String)> = vec![
        ("pageNum", "1".into()),
        ("pageSize", page_size.to_string()),
        ("column", column.into()),
        ("tabName", "fulltext".into()),
        ("plate", String::new()),
        ("stock", stock_code.clone()),
        ("searchkey", String::new()),
        ("secid", String::new()),
        ("category", String::new()),
        ("trade", String::new()),
        ("seDate", String::new()),
        ("sortName", String::new()),
        ("sortType", String::new()),
        ("isHLtitle", "true".into()),
    ];

    let agent = ureq::Agent::new_with_config(
        ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(timeout.max(1))))
            .http_status_as_error(false)
            .build(),
    );
    let result = agent
        .post(url)
        .header(
            "User-Agent",
            "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36",
        )
        .header("Accept", "application/json")
        .header("Origin", "http://www.cninfo.com.cn")
        .header(
            "Referer",
            "http://www.cninfo.com.cn/new/commonUrl/pageOfSearch?url=disclosure/list/search",
        )
        .send_form(pairs.iter().map(|(k, v)| (*k, v.as_str())));
    let mut resp = match result {
        Ok(r) => r,
        Err(_) => return Vec::new(),
    };
    if resp.status().as_u16() != 200 {
        return Vec::new();
    }
    let text = resp.body_mut().read_to_string().unwrap_or_default();
    let data: Value = match serde_json::from_str(&text) {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };

    let announcements: &[Value] = data
        .get("announcements")
        .and_then(|v| v.as_array())
        .map(|a| a.as_slice())
        .unwrap_or(&[]);
    let mut rows = Vec::new();
    for a in announcements.iter().take(page_size) {
        let ts = a.get("announcementTime").and_then(|v| v.as_i64()).unwrap_or(0);
        let date_str = if ts != 0 {
            use chrono::TimeZone;
            chrono::Local
                .timestamp_millis_opt(ts)
                .single()
                .map(|d| d.format("%Y-%m-%d").to_string())
                .unwrap_or_default()
        } else {
            String::new()
        };
        let mut ann_url = as_str_or(a, "adjunctUrl", "").to_string();
        if !ann_url.is_empty() && !ann_url.starts_with("http") {
            ann_url = format!(
                "http://static.cninfo.com.cn/{}",
                ann_url.trim_start_matches('/')
            );
        }
        rows.push(json!({
            "date": date_str,
            "title": a.get("announcementTitle").and_then(|v| v.as_str()).unwrap_or(""),
            "url": ann_url,
            "type": "cninfo 公告",
        }));
    }
    rows
}

/// `_cninfo_disclosures(code, days_back=180)`.
///
/// The AkShare slow path is disabled by default upstream (it only runs when the
/// user exports `UZI_AK_CNINFO_FALLBACK=1`); without AkShare it degrades to the
/// direct-API result (empty on failure), exactly like upstream's default.
fn cninfo_disclosures(code: &str, _days_back: i64) -> Vec<Value> {
    cninfo_direct_api(code, 30, 15)
}

// ─────────────────────────────────────────────────────────────
// Noise filter
// ─────────────────────────────────────────────────────────────

/// `_NOISE_KWS` — board-level / index / capital-flow headlines.
const NOISE_KWS: &[&str] = &[
    "主力资金净流",
    "资金流向日报",
    "资金流出榜",
    "资金流入榜",
    "科创板主力资金",
    "科创板平均股价",
    "创业板主力",
    "沪深主力",
    "行业资金流",
    "板块资金",
    "个股主力资金净",
    "只股中线走稳",
    "站上半年线",
    "股价超百元",
    "融资客大手笔",
    "融资净买入",
    "融资余额",
    "北向资金",
    "两融",
    "龙虎榜汇总",
    "板块涨幅",
    "行业今日",
    "大盘分析",
    "行业4月",
    "行业3月",
    "行业2月",
    "行业1月",
    "股涨停",
    "跌停",
    "涨幅榜",
    "家公司的调研",
    "解密主力资金出逃股",
    "收盘价创历史新高股",
    "只股获",
];

/// `_is_noise_news(title)`.
fn is_noise_news(title: &str) -> bool {
    if title.is_empty() {
        return true;
    }
    NOISE_KWS.iter().any(|kw| title.contains(*kw))
}

// ─────────────────────────────────────────────────────────────
// Web-search fallback
// ─────────────────────────────────────────────────────────────

/// `_web_search_events(name, max_results=6)`.
fn web_search_events(name: &str, max_results: usize) -> Vec<Value> {
    let queries = [
        format!("{name} 上市公司 最新动态 合同 订单 产品"),
        format!("{name} 业绩 研发 突破 合作"),
    ];
    let mut results: Vec<Value> = Vec::new();
    let mut seen: Vec<String> = Vec::new();
    for q in queries {
        let res_trusted = web_search::search_trusted(&q, "15_events", max_results, &[], 6);
        let res_generic = if res_trusted.len() < 3 {
            web_search::search(&q, max_results, "ws")
        } else {
            Vec::new()
        };
        for r in res_trusted.iter().chain(res_generic.iter()) {
            if r.get("error").is_some() {
                continue;
            }
            let title = trunc(as_str_or(r, "title", ""), 80);
            if !title.is_empty() && !seen.contains(&title) && !is_noise_news(&title) {
                seen.push(title.clone());
                results.push(json!({
                    "date": "—",
                    "title": title,
                    "type": "web_search",
                    "source": as_str_or(r, "url", ""),
                }));
            }
        }
    }
    results.truncate(8);
    results
}

// ─────────────────────────────────────────────────────────────
// main
// ─────────────────────────────────────────────────────────────

pub fn main(ticker: &str) -> Result<Value, String> {
    let ti = parse_ticker(ticker);

    if ti.market == "H" {
        let basic = sources::fetch_basic(&ti);
        let company_name = {
            let n = get(&basic, "name").as_str().unwrap_or("");
            let n = if n.is_empty() {
                get(&basic, "full_name").as_str().unwrap_or("")
            } else {
                n
            };
            if n.is_empty() {
                ti.code.clone()
            } else {
                n.to_string()
            }
        };
        let code5 = format!("{:0>5}", ti.code);
        let anns: Vec<Value> = hk::fetch_hk_announcements_cached(&code5, 20)
            .as_array()
            .cloned()
            .unwrap_or_default();
        let ws_events = if anns.len() < 5 {
            web_search_events(&company_name, 6)
        } else {
            Vec::new()
        };

        let mut timeline: Vec<Value> = Vec::new();
        for a in anns.iter().chain(ws_events.iter()) {
            let date = a.get("date").and_then(|v| v.as_str()).unwrap_or("—");
            let title = trunc(a.get("title").and_then(|v| v.as_str()).unwrap_or(""), 80);
            timeline.push(json!(format!("{date} · {title}")));
        }
        timeline.truncate(30);

        let recent_news: Vec<Value> = anns
            .iter()
            .map(|a| {
                json!({
                    "date": a.get("date").and_then(|v| v.as_str()).unwrap_or(""),
                    "title": a.get("title").and_then(|v| v.as_str()).unwrap_or(""),
                    "url": a.get("url").and_then(|v| v.as_str()).unwrap_or(""),
                    "source": a.get("source").and_then(|v| v.as_str()).unwrap_or("hkexnews"),
                })
            })
            .collect();

        return Ok(json!({
            "ticker": ti.full,
            "data": {
                "event_timeline": timeline,
                "recent_news": recent_news,
                "recent_notices": [],
                "catalysts": [],
                "warnings": [],
                "_note": "HK 公司公告原文走 hkexnews 法定披露源 + 中文 web search 兜底；若需更精确事件抽取，agent 用 Playwright 打开 hkexnews titlesearch.xhtml POST",
            },
            "source": "hkexnews + ddgs",
            "fallback": false,
        }));
    }
    if ti.market != "A" {
        return Ok(json!({
            "ticker": ti.full,
            "data": {},
            "source": "n/a",
            "fallback": true,
        }));
    }

    // Get company name for web search fallback
    let basic = sources::fetch_basic(&ti);
    let company_name = {
        let n = get(&basic, "name").as_str().unwrap_or("");
        if n.is_empty() {
            ti.code.clone()
        } else {
            n.to_string()
        }
    };

    let disclosures = cninfo_disclosures(&ti.code, 180);
    let mut news_items = try_news(&ti);

    // v2.13.7 · 多源新闻聚合（金十/东财快讯/东财公告/同花顺）
    let multi = news::get_news_multi_source(&ti.code, &company_name, 10);
    if let Some(sources_obj) = multi.get("sources").and_then(|v| v.as_object()) {
        for (src, items) in sources_obj {
            let Some(items) = items.as_array() else { continue };
            for it in items {
                if !it.is_object() || truthy(get(it, "error")) {
                    continue;
                }
                let title = trunc(as_str_or(it, "title", ""), 80);
                if title.is_empty() || is_noise_news(&title) {
                    continue;
                }
                let date = {
                    let pt = trunc(as_str_or(it, "publish_time", ""), 16);
                    if pt.is_empty() {
                        "—".to_string()
                    } else {
                        pt
                    }
                };
                news_items.push(json!({
                    "date": date,
                    "title": title,
                    "type": format!("news_providers:{src}"),
                    "source": as_str_or(it, "url", ""),
                }));
            }
        }
    }

    // If filtered news is too sparse, supplement with web search
    if news_items.len() < 3 {
        news_items.extend(web_search_events(&company_name, 6));
    }

    // Merge + dedupe + sort by date desc
    let mut merged: Map<String, Value> = Map::new();
    for item in disclosures.iter().chain(news_items.iter()) {
        if item.get("error").is_some() {
            continue;
        }
        let k = trunc(as_str_or(item, "title", ""), 80);
        if !k.is_empty() && !merged.contains_key(&k) {
            merged.insert(k, item.clone());
        }
    }
    let mut sorted_events: Vec<Value> = merged.into_values().collect();
    sorted_events.sort_by(|a, b| {
        let da = as_str_or(a, "date", "");
        let db = as_str_or(b, "date", "");
        db.cmp(da)
    });

    // Build a compact timeline for the viz
    let mut timeline: Vec<Value> = Vec::new();
    for ev in sorted_events.iter().take(10) {
        let date = {
            let d = trunc(as_str_or(ev, "date", ""), 10);
            if d.is_empty() {
                "—".to_string()
            } else {
                d
            }
        };
        let title = trunc(as_str_or(ev, "title", ""), 70);
        timeline.push(json!(format!("{date} · {title}")));
    }

    // Extract forward-looking catalysts (from disclosure titles)
    let catalyst_kws = [
        "合同", "中标", "业绩", "研发", "获批", "专利", "投资", "合作", "股权", "分红", "回购",
    ];
    let mut catalysts: Vec<Value> = Vec::new();
    for item in disclosures.iter().take(20) {
        let title = as_str_or(item, "title", "");
        if catalyst_kws.iter().any(|kw| title.contains(*kw)) {
            catalysts.push(json!({
                "date": as_str_or(item, "date", ""),
                "event": trunc(title, 80),
                "impact": "medium",
            }));
        }
    }
    catalysts.truncate(5);

    // Warnings from disclosure titles
    let warning_kws = [
        "风险",
        "立案",
        "违规",
        "退市",
        "ST",
        "商誉减值",
        "资产减值",
        "业绩下滑",
    ];
    let mut warning_items: Vec<String> = Vec::new();
    for item in disclosures.iter().take(20) {
        let title = as_str_or(item, "title", "");
        if warning_kws.iter().any(|kw| title.contains(*kw)) {
            warning_items.push(trunc(title, 80));
        }
    }

    let news_count = news_items.len();
    let news_label = if news_count > 0 {
        format!("{news_count} 条新闻")
    } else {
        "—".to_string()
    };

    Ok(json!({
        "ticker": ti.full,
        "data": {
            "event_timeline": timeline,
            "recent_news": news_items.iter().take(10).cloned().collect::<Vec<_>>(),
            "recent_notices": disclosures.iter().take(20).cloned().collect::<Vec<_>>(),
            "disclosures_count": disclosures.len(),
            "news_count": news_count,
            "recent_news_label": news_label,
            "catalyst": catalysts,
            "warnings": warning_items,
        },
        "source": "cninfo:stock_zh_a_disclosure_report + akshare:stock_news_em + news_providers(jin10/em/ths) + web_search",
        "fallback": false,
    }))
}

/// `_try_news(code)` — AkShare `stock_news_em` rows (empty when unavailable).
fn try_news(ti: &uzi_core::ticker::TickerInfo) -> Vec<Value> {
    let rows = sources::fetch_news(ti, 30);
    let mut out: Vec<Value> = Vec::new();
    let Some(rows) = rows.as_array() else {
        return out;
    };
    for r in rows.iter().take(30) {
        let title = {
            let t = as_str_or(r, "新闻标题", "");
            if t.is_empty() {
                as_str_or(r, "title", "")
            } else {
                t
            }
        };
        if is_noise_news(title) {
            continue;
        }
        let date = {
            let d = as_str_or(r, "发布时间", "");
            let d = if d.is_empty() {
                as_str_or(r, "publish_time", "")
            } else {
                d
            };
            trunc(d, 16)
        };
        let source = {
            let s = as_str_or(r, "文章来源", "");
            if s.is_empty() {
                as_str_or(r, "source", "")
            } else {
                s
            }
        };
        let url = {
            let u = as_str_or(r, "新闻链接", "");
            if u.is_empty() {
                as_str_or(r, "url", "")
            } else {
                u
            }
        };
        out.push(json!({
            "date": date,
            "title": title,
            "type": "新闻",
            "source": source,
            "url": url,
        }));
        if out.len() >= 12 {
            break;
        }
    }
    out
}
