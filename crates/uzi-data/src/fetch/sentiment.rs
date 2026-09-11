//! Port of `fetch_sentiment.py`.
//!
//! Dimension 17 · 舆情与大V — 真实 web search (雪球 / 股吧 / 知乎 / 小红书).

use serde_json::{json, Map, Value};

use uzi_core::py::{float_str, round};
use uzi_core::ticker::parse_ticker;

use crate::hottrend;
use crate::news;
use crate::sources;
use crate::web_search;

fn trunc(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

const POSITIVE_KWS: &[&str] = &[
    "看好", "强势", "上涨", "涨停", "突破", "利好", "龙头", "加仓", "买入",
];
const NEGATIVE_KWS: &[&str] = &[
    "看空", "下跌", "亏损", "利空", "减仓", "卖出", "割肉", "杀跌",
];

pub fn main(ticker: &str) -> Result<Value, String> {
    let ti = parse_ticker(ticker);
    let basic = sources::fetch_basic(&ti);
    let name = {
        let n = basic.get("name").and_then(|v| v.as_str()).unwrap_or("");
        if n.is_empty() {
            ti.code.clone()
        } else {
            n.to_string()
        }
    };

    // Query each platform separately
    let platforms: Vec<(&str, String)> = vec![
        ("xueqiu", format!("site:xueqiu.com {name}")),
        ("guba", format!("site:guba.eastmoney.com {name}")),
        ("zhihu", format!("知乎 {name} 股票 分析")),
        ("weibo", format!("微博 {name} 股票")),
        ("xiaohongshu", format!("小红书 {name} 股票")),
        ("big_v", format!("{name} 大V 分析")),
    ];

    let mut snippets: Map<String, Value> = Map::new();
    let mut platform_hit: Map<String, Value> = Map::new();
    for (key, q) in &platforms {
        let res = web_search::search(q, 4, "ws");
        let valid: Vec<&Value> = res.iter().filter(|r| r.get("error").is_none()).collect();
        let snips: Vec<Value> = valid
            .iter()
            .take(3)
            .map(|r| {
                json!({
                    "title": trunc(r.get("title").and_then(|v| v.as_str()).unwrap_or(""), 80),
                    "body": trunc(r.get("body").and_then(|v| v.as_str()).unwrap_or(""), 200),
                    "url": r.get("url").and_then(|v| v.as_str()).unwrap_or(""),
                })
            })
            .collect();
        snippets.insert((*key).to_string(), Value::Array(snips));
        platform_hit.insert((*key).to_string(), json!(valid.len()));
    }

    // Positive/negative sentiment analysis
    let mut all_bodies: Vec<String> = Vec::new();
    for platform_snips in snippets.values() {
        if let Some(arr) = platform_snips.as_array() {
            for s in arr {
                all_bodies.push(s.get("body").and_then(|v| v.as_str()).unwrap_or("").to_string());
            }
        }
    }
    let text = all_bodies.join(" ").to_lowercase();

    let mut pos = POSITIVE_KWS.iter().filter(|kw| text.contains(**kw)).count();
    let mut neg = NEGATIVE_KWS.iter().filter(|kw| text.contains(**kw)).count();
    let mut has_signal = (pos + neg) > 0;
    let mut positive_pct: Option<f64> = if has_signal {
        Some(round(pos as f64 / (pos + neg) as f64 * 100.0, 0))
    } else {
        None
    };

    // Heat gauge (0-100) based on total platform hits
    let total_hits: usize = platform_hit
        .values()
        .filter_map(|v| v.as_u64())
        .map(|v| v as usize)
        .sum();
    let mut heat = (total_hits * 8 + pos * 2).min(100);

    // Big V level detection
    let big_v_text = snippets
        .get("big_v")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .map(|s| s.get("body").and_then(|v| v.as_str()).unwrap_or(""))
                .collect::<Vec<_>>()
                .join(" ")
        })
        .unwrap_or_default();
    let big_v_count = ["万粉", "百万", "博主", "专家"]
        .iter()
        .filter(|kw| big_v_text.contains(**kw))
        .count();

    // v2.12 · 6 平台热榜命中检测
    let hot_trend_mentions = hottrend::get_hot_mentions(&name, &[]);
    let hot_bonus = hot_trend_mentions
        .get("total_hits")
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as usize;
    heat = (heat + hot_bonus * 5).min(100);

    // v2.13.7 · 新闻情绪增强（金十/东财快讯/同花顺）
    let news_multi = news::get_news_multi_source(&ti.code, &name, 15);
    let mut news_text_parts: Vec<String> = Vec::new();
    if let Some(sources_obj) = news_multi.get("sources").and_then(|v| v.as_object()) {
        for items in sources_obj.values() {
            let Some(items) = items.as_array() else { continue };
            for it in items {
                if !it.is_object() || it.get("error").is_some() {
                    continue;
                }
                news_text_parts.push(format!(
                    "{} {}",
                    it.get("title").and_then(|v| v.as_str()).unwrap_or(""),
                    it.get("body").and_then(|v| v.as_str()).unwrap_or(""),
                ));
            }
        }
    }
    let news_text = news_text_parts.join(" ").to_lowercase();
    if !news_text.is_empty() {
        pos += POSITIVE_KWS.iter().filter(|kw| news_text.contains(**kw)).count();
        neg += NEGATIVE_KWS.iter().filter(|kw| news_text.contains(**kw)).count();
        has_signal = (pos + neg) > 0;
        positive_pct = if has_signal {
            Some(round(pos as f64 / (pos + neg) as f64 * 100.0, 0))
        } else {
            None
        };
    }
    if news_multi.is_object() && news_multi.get("error").is_none() {
        let news_hit = news_multi
            .get("total_hits")
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as usize;
        heat = (heat + news_hit * 2).min(100);
    }

    let positive_pct_str = if has_signal {
        format!("{}%", float_str(positive_pct.unwrap_or(0.0)))
    } else {
        "—".to_string()
    };
    let sentiment_label = if has_signal {
        let p = positive_pct.unwrap_or(0.0);
        if p > 60.0 {
            "乐观"
        } else if p < 40.0 {
            "悲观"
        } else {
            "中性"
        }
    } else {
        "数据缺失（未抓到有效舆情，非真实负面）"
    };
    let guba_volume = format!(
        "{} 条结果",
        platform_hit.get("guba").and_then(|v| v.as_u64()).unwrap_or(0)
    );
    let big_v_mentions = if big_v_count > 0 {
        format!("{big_v_count} 位大V提及")
    } else {
        "—".to_string()
    };

    Ok(json!({
        "ticker": ti.full,
        "data": {
            "xueqiu_heat": format!("热度 {heat}"),
            "thermometer_value": heat,
            "guba_volume": guba_volume,
            "big_v_mentions": big_v_mentions,
            "sentiment_data_available": has_signal,
            "positive_pct": positive_pct_str,
            "sentiment_label": sentiment_label,
            "platform_snippets": snippets,
            "platform_hits": platform_hit,
            "total_mentions": total_hits,
            "hot_trend_mentions": hot_trend_mentions,
            "hot_trend_hit_count": hot_trend_mentions.get("total_hits").cloned().unwrap_or(json!(0)),
            "news_multi_source": news_multi,
            "news_sources_ok": news_multi.get("sources_ok").cloned().unwrap_or(json!(0)),
            "news_total_hits": news_multi.get("total_hits").cloned().unwrap_or(json!(0)),
        },
        "source": "web_search:ddgs + hottrend (6 热榜) + news_providers (v2.13.7 · 4 新闻源)",
        "fallback": false,
    }))
}
