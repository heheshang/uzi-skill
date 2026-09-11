//! Port of `fetch_trap_signals.py`.
//!
//! Dimension 18 · 杀猪盘检测 — 真实 web search 扫描 8 信号.

use serde_json::{json, Map, Value};

use uzi_core::ticker::parse_ticker;

use crate::sources;
use crate::web_search;

fn trunc(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

struct Signal {
    id: u64,
    name: &'static str,
    queries: &'static [&'static str],
    positive_kws: &'static [&'static str],
}

const SIGNALS: &[Signal] = &[
    Signal {
        id: 1,
        name: "大量低质量账号同时推荐",
        queries: &["{name} 强烈推荐 必涨", "{name} 内部消息 暴涨"],
        positive_kws: &["必涨", "强烈推荐", "内部", "稳赚"],
    },
    Signal {
        id: 2,
        name: "推荐话术模板化",
        queries: &["{name} 主力建仓完毕 即将爆发", "{name} 翻倍 目标价"],
        positive_kws: &["即将爆发", "主力建仓完毕", "翻倍", "目标翻倍"],
    },
    Signal {
        id: 3,
        name: "付费社群/VIP直播间引流",
        queries: &["{name} 股票 微信群", "{name} 老师 带单 VIP 直播间"],
        positive_kws: &["微信群", "VIP 直播", "老师带", "收费群", "加入群聊"],
    },
    Signal {
        id: 4,
        name: "基本面与热度脱节",
        queries: &["{name} 业绩亏损 推荐 暴涨", "{name} ST 推荐 拉升"],
        positive_kws: &["亏损但推荐", "ST", "垃圾股 推荐"],
    },
    Signal {
        id: 5,
        name: "K线异常配合",
        queries: &["{name} 异动 操纵 拉升"],
        positive_kws: &["异动", "操纵", "快速拉升", "直线拉升"],
    },
    Signal {
        id: 6,
        name: "老师/股神人设推广",
        queries: &["{name} 老师 股神 跟单", "{name} 实盘 老师"],
        positive_kws: &["老师", "股神", "跟单", "操盘手"],
    },
    Signal {
        id: 7,
        name: "跨平台联动推广",
        queries: &["小红书 {name} 股票 推荐", "抖音 {name} 股票"],
        positive_kws: &["小红书", "抖音", "快手", "B站 推荐"],
    },
    Signal {
        id: 8,
        name: "虚假研报/伪造消息",
        queries: &["{name} 虚假研报 谣言", "{name} 辟谣 澄清"],
        positive_kws: &["虚假", "谣言", "澄清", "辟谣", "伪造"],
    },
];

pub fn main(ticker_or_name: &str) -> Result<Value, String> {
    // If ticker, resolve to name
    let mut name = ticker_or_name.to_string();
    let stripped: String = ticker_or_name
        .replace('.', "")
        .replace("SZ", "")
        .replace("SH", "");
    if !stripped.is_empty() && stripped.chars().all(|c| c.is_ascii_digit()) {
        let ti = parse_ticker(ticker_or_name);
        let basic = sources::fetch_basic(&ti);
        let n = basic.get("name").and_then(|v| v.as_str()).unwrap_or("");
        name = if n.is_empty() {
            ti.code.clone()
        } else {
            n.to_string()
        };
    }

    let mut hit_signals: Vec<Value> = Vec::new();
    let mut all_snippets: Map<String, Value> = Map::new();
    for sig in SIGNALS {
        let mut combined_bodies: Vec<String> = Vec::new();
        // 1 query per signal to save search calls
        for q_template in sig.queries.iter().take(1) {
            let q = q_template.replace("{name}", &name);
            let res = web_search::search(&q, 3, "ws");
            let valid: Vec<&Value> = res.iter().filter(|r| r.get("error").is_none()).collect();
            combined_bodies.extend(
                valid
                    .iter()
                    .map(|r| r.get("body").and_then(|v| v.as_str()).unwrap_or("").to_string()),
            );
            let key = format!("signal_{}", sig.id);
            let entry = all_snippets
                .entry(key)
                .or_insert_with(|| Value::Array(Vec::new()));
            if let Some(arr) = entry.as_array_mut() {
                for r in valid.iter().take(2) {
                    arr.push(json!({
                        "title": trunc(r.get("title").and_then(|v| v.as_str()).unwrap_or(""), 80),
                        "body": trunc(r.get("body").and_then(|v| v.as_str()).unwrap_or(""), 180),
                        "url": r.get("url").and_then(|v| v.as_str()).unwrap_or(""),
                    }));
                }
            }
        }

        let combined_text = combined_bodies.join(" ");
        let hits: Vec<&str> = sig
            .positive_kws
            .iter()
            .filter(|kw| combined_text.contains(**kw))
            .copied()
            .collect();
        if hits.len() >= 2 {
            hit_signals.push(json!({
                "id": sig.id,
                "name": sig.name,
                "evidence_kws": hits.iter().take(3).copied().collect::<Vec<_>>(),
                "severity": if hits.len() >= 3 { "high" } else { "medium" },
            }));
        }
    }

    let n_hits = hit_signals.len();
    let (level, score, recommendation) = if n_hits <= 1 {
        (
            "🟢 安全",
            9u64,
            "数据正常，未发现明显推广痕迹。".to_string(),
        )
    } else if n_hits <= 3 {
        ("🟡 注意", 7, format!("发现 {n_hits} 个推广信号，建议核实信息源。"))
    } else if n_hits <= 5 {
        ("🟠 警惕", 4, format!("发现 {n_hits} 个推广信号，强烈建议谨慎。"))
    } else {
        (
            "🔴 高度可疑",
            1,
            format!("发现 {n_hits} 个推广信号，强烈建议回避。疑似杀猪盘特征。"),
        )
    };

    let evidence_count: usize = hit_signals
        .iter()
        .map(|s| {
            s.get("evidence_kws")
                .and_then(|v| v.as_array())
                .map(|a| a.len())
                .unwrap_or(0)
        })
        .sum();
    let high_risk_kw = if hit_signals.is_empty() {
        "未发现".to_string()
    } else {
        hit_signals
            .iter()
            .take(3)
            .filter_map(|s| s.get("name").and_then(|v| v.as_str()))
            .collect::<Vec<_>>()
            .join(", ")
    };

    Ok(json!({
        "ticker": ticker_or_name,
        "data": {
            "trap_level": level,
            "trap_score": score,
            "signals_hit": format!("{n_hits}/8"),
            "signals_hit_count": n_hits,
            "signals_hit_detail": hit_signals,
            "recommendation": recommendation,
            "evidence_count": evidence_count,
            "high_risk_kw": high_risk_kw,
            "snippets": all_snippets,
        },
        "source": "web_search:ddgs + 8-signal keyword scan",
        "fallback": false,
    }))
}
