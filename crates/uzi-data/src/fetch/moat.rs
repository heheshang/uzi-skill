//! Port of `fetch_moat.py`.

use std::collections::HashSet;

use serde_json::{json, Map, Value};

use crate::web_search::{search, search_trusted};
use uzi_core::ticker::parse_ticker;

fn first_n(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

/// `_evaluate(text, pos_kws, neg_kws)` — 1-10 score from keyword matches.
fn evaluate(text: &str, pos_kws: &[&str], neg_kws: &[&str]) -> i64 {
    if text.is_empty() {
        return 5;
    }
    let text = text.to_lowercase();
    let mut pos = 0i64;
    for &kw in pos_kws {
        if text.contains(kw.to_lowercase().as_str()) {
            pos += 1;
        }
    }
    let mut neg = 0i64;
    for &kw in neg_kws {
        if text.contains(kw.to_lowercase().as_str()) {
            neg += 1;
        }
    }
    (5 + pos - neg).clamp(1, 10)
}

// Garbage patterns — dictionary/wikipedia pages about Chinese characters
const GARBAGE_PATTERNS: [&str; 14] = [
    "拼音",
    "汉语",
    "通用规范汉字",
    "常用字",
    "甲骨文",
    "部首",
    "笔画",
    "Unicode",
    "字形",
    "读音",
    "偏旁",
    "百科词条",
    "词条概述",
    "释义",
];

/// `_is_garbage(text)` — detect dictionary/wikipedia noise in search results.
fn is_garbage(text: &str) -> bool {
    if text.is_empty() {
        return false;
    }
    let mut hits = 0usize;
    for &p in &GARBAGE_PATTERNS {
        if text.contains(p) {
            hits += 1;
        }
    }
    hits >= 2
}

// v2.15.1 · 已知的"超级股票名"· DDGS 对生僻公司查询经常混入这些头部股票的结果
// 所以对这些词做严格过滤：结果里出现就 drop（除非目标公司本身就是这些）
const SUPERSTAR_POLLUTERS: [&str; 15] = [
    "贵州茅台",
    "五粮液",
    "泸州老窖",
    "洋河股份", // 白酒
    "宁德时代",
    "比亚迪", // 电池
    "中际旭创",
    "新易盛", // 光模块
    "腾讯",
    "阿里巴巴",
    "美团",
    "京东", // 互联网
    "招商银行",
    "工商银行",
    "建设银行", // 银行
];

fn result_mentions_company(
    result: &Value,
    company_name: &str,
    superstar_names: &HashSet<&str>,
) -> bool {
    if company_name.is_empty() {
        return true;
    }
    let title = result.get("title").and_then(|v| v.as_str()).unwrap_or("");
    let body = result.get("body").and_then(|v| v.as_str()).unwrap_or("");
    let text = format!("{title} {body}").to_lowercase();
    if text.trim().is_empty() {
        return false;
    }
    let name_lc = company_name.to_lowercase();
    let name_chars: Vec<char> = name_lc.chars().collect();
    let name_key: String = if name_chars.len() >= 2 {
        name_chars[name_chars.len() - 2..].iter().collect()
    } else {
        name_lc.clone()
    };
    if text.contains(&name_lc) || (name_chars.len() > 2 && text.contains(&name_key)) {
        return true;
    }
    for &polluter in superstar_names {
        if text.contains(polluter) {
            return false;
        }
    }
    false
}

/// `_top_body(key, n)` — join the first `n` snippet bodies (100 chars each).
fn top_body(results: &Map<String, Value>, key: &str, n: usize) -> String {
    results
        .get(key)
        .and_then(|v| v.get("snippets"))
        .and_then(|v| v.as_array())
        .map(|snips| {
            snips
                .iter()
                .take(n)
                .map(|s| {
                    first_n(
                        s.get("body").and_then(|v| v.as_str()).unwrap_or(""),
                        100,
                    )
                })
                .collect::<Vec<_>>()
                .join(" ")
        })
        .unwrap_or_default()
}

/// `results[key]["text"]` — the combined valid-snippet body text.
fn text_of<'a>(results: &'a Map<String, Value>, key: &str) -> &'a str {
    results
        .get(key)
        .and_then(|v| v.get("text"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
}

fn or_dash(s: String) -> String {
    if s.is_empty() {
        "—".to_string()
    } else {
        s
    }
}

pub fn main(ticker: &str) -> Result<Value, String> {
    let ti = parse_ticker(ticker);
    let basic = crate::sources::fetch_basic(&ti);
    let name = basic
        .get("name")
        .map(uzi_core::py::py_str)
        .unwrap_or_else(|| ti.code.clone());
    let full_name = basic
        .get("full_name")
        .filter(|v| uzi_core::py::truthy(v))
        .map(uzi_core::py::py_str)
        .unwrap_or_else(|| name.clone());

    // Search queries — use full name + stock context to avoid dictionary hits
    let stock_anchor = format!("{name} 上市公司");
    let queries: [(&str, String); 5] = [
        (
            "intangible",
            format!("{stock_anchor} 专利 核心技术 品牌壁垒 竞争优势"),
        ),
        (
            "switching",
            format!("{stock_anchor} 客户粘性 转换成本 认证壁垒 大客户"),
        ),
        ("network", format!("{stock_anchor} 平台效应 网络效应 用户生态")),
        (
            "scale",
            format!("{stock_anchor} 市场份额 行业地位 规模优势 龙头"),
        ),
        ("rd", format!("{stock_anchor} 研发投入 研发占比 技术实力")),
    ];

    // v2.15.1 · 计算 superstar polluters（排除目标本身）
    let superstar_set: HashSet<&str> = SUPERSTAR_POLLUTERS
        .iter()
        .copied()
        .filter(|p| !name.contains(*p) && !full_name.contains(*p))
        .collect();

    let mut results: Map<String, Value> = Map::new();
    for (key, q) in &queries {
        // v2.7.3 · 护城河查询用 14_moat 权威域（权威域未命中时用普通 search 补位）
        let res_t = search_trusted(q, "14_moat", 6, &[], 6);
        let res: Vec<Value> = if res_t.len() >= 3 {
            res_t
        } else {
            let mut combined = res_t;
            combined.extend(search(q, 6, "ws"));
            combined
        };
        let valid: Vec<&Value> = res
            .iter()
            .filter(|r| {
                if r.get("error").is_some() {
                    return false;
                }
                let text = format!(
                    "{}{}",
                    r.get("body").and_then(|v| v.as_str()).unwrap_or(""),
                    r.get("title").and_then(|v| v.as_str()).unwrap_or("")
                );
                !is_garbage(&text) && result_mentions_company(r, &name, &superstar_set)
            })
            .collect();
        let combined_text = valid
            .iter()
            .map(|r| r.get("body").and_then(|v| v.as_str()).unwrap_or(""))
            .collect::<Vec<_>>()
            .join(" ");
        let snippets = valid
            .iter()
            .take(2)
            .map(|r| {
                json!({
                    "title": first_n(r.get("title").and_then(|v| v.as_str()).unwrap_or(""), 80),
                    "body": first_n(r.get("body").and_then(|v| v.as_str()).unwrap_or(""), 200),
                    "url": r.get("url").and_then(|v| v.as_str()).unwrap_or(""),
                })
            })
            .collect::<Vec<_>>();
        results.insert(
            (*key).to_string(),
            json!({"text": combined_text, "snippets": snippets}),
        );
    }

    // Score each moat dimension (1-10)
    let intangible_score = evaluate(
        text_of(&results, "intangible"),
        &["专利", "核心技术", "自主", "垄断", "独家", "行业领先", "国产替代"],
        &["模仿", "同质", "无差异"],
    );
    let switching_score = evaluate(
        text_of(&results, "switching"),
        &["绑定", "独家", "长期合作", "认证", "唯一", "二供", "一供"],
        &["易替换", "议价弱"],
    );
    let network_score = evaluate(
        text_of(&results, "network"),
        &["平台", "生态", "网络", "用户基数"],
        &["单点", "无网络"],
    );
    let scale_score = evaluate(
        text_of(&results, "scale"),
        &["龙头", "第一", "领先", "最大", "份额", "国产替代"],
        &["追赶", "落后", "份额低"],
    );

    let mut evidence: Map<String, Value> = Map::new();
    for key in ["intangible", "switching", "network", "scale"] {
        evidence.insert(
            key.to_string(),
            json!(!text_of(&results, key).trim().is_empty()),
        );
    }
    let scores_available = evidence.values().any(uzi_core::py::truthy);

    let scores: Map<String, Value> = if scores_available {
        let mut m = Map::new();
        m.insert("intangible".into(), json!(intangible_score));
        m.insert("switching".into(), json!(switching_score));
        m.insert("network".into(), json!(network_score));
        m.insert("scale".into(), json!(scale_score));
        m
    } else {
        Map::new()
    };

    let mut web_search_snippets: Map<String, Value> = Map::new();
    for (k, v) in &results {
        web_search_snippets.insert(
            k.clone(),
            v.get("snippets").cloned().unwrap_or_else(|| json!([])),
        );
    }

    let scores_note: Value = if scores_available {
        Value::Null
    } else {
        json!("未评估：四个护城河维度均未检索到有效证据。")
    };

    Ok(json!({
        "ticker": ti.full,
        "data": {
            "intangible": or_dash(top_body(&results, "intangible", 1)),
            "switching": or_dash(top_body(&results, "switching", 1)),
            "network": or_dash(top_body(&results, "network", 1)),
            "scale": or_dash(top_body(&results, "scale", 1)),
            "scores": Value::Object(scores),
            "scores_available": scores_available,
            "scores_evidence": Value::Object(evidence),
            "scores_note": scores_note,
            "rd_summary": or_dash(top_body(&results, "rd", 2)),
            "web_search_snippets": Value::Object(web_search_snippets),
            "moat_framework": ["intangible", "switching", "network", "scale", "efficient_scale"],
        },
        "source": "web_search:ddgs + keyword scoring",
        "fallback": false,
    }))
}

#[cfg(test)]
mod tests {
    use super::{evaluate, is_garbage};

    #[test]
    fn evaluate_defaults_and_clamps() {
        assert_eq!(evaluate("", &["专利"], &["模仿"]), 5);
        let pos7 = ["专利", "核心技术", "自主", "垄断", "独家", "行业领先", "国产替代"];
        assert_eq!(evaluate(&pos7.join(" "), &pos7, &[]), 10);
        let neg5 = ["x", "y", "z", "w", "v"];
        assert_eq!(evaluate(&neg5.join(" "), &[], &neg5), 1);
        assert_eq!(evaluate("专利 核心技术", &["专利", "核心技术", "自主"], &["模仿"]), 7);
    }

    #[test]
    fn garbage_requires_two_patterns() {
        assert!(!is_garbage(""));
        assert!(!is_garbage("拼音 股票 分析"));
        assert!(is_garbage("拼音 汉语"));
    }
}
