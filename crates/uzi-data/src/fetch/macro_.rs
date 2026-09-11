//! Port of `fetch_macro.py`.

use chrono::Datelike;
use serde_json::{json, Map, Value};

use crate::web_search::{search, search_trusted};

fn first_n(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

/// `[{"title": r.get("title","")[:80], "body": r.get("body","")[:200],
///    "url": r.get("url","")} for r in valid[:3]]`.
fn snippets_of(results: &[Value]) -> Value {
    let valid: Vec<&Value> = results.iter().filter(|r| r.get("error").is_none()).collect();
    Value::Array(
        valid
            .iter()
            .take(3)
            .map(|r| {
                json!({
                    "title": first_n(r.get("title").and_then(|v| v.as_str()).unwrap_or(""), 80),
                    "body": first_n(r.get("body").and_then(|v| v.as_str()).unwrap_or(""), 200),
                    "url": r.get("url").and_then(|v| v.as_str()).unwrap_or(""),
                })
            })
            .collect(),
    )
}

/// `_sentiment(bodies)` — keyword heuristic over the joined snippet bodies.
fn sentiment(bodies: &[String]) -> &'static str {
    let text = bodies.join(" ").to_lowercase();
    let mut pos = 0usize;
    for &kw in &["降息", "宽松", "利好", "稳定", "回暖"] {
        if text.contains(kw.to_lowercase().as_str()) {
            pos += 1;
        }
    }
    let mut neg = 0usize;
    for &kw in &["加息", "紧缩", "利空", "下行", "衰退"] {
        if text.contains(kw.to_lowercase().as_str()) {
            neg += 1;
        }
    }
    if pos > neg + 1 {
        "利好"
    } else if neg > pos + 1 {
        "利空"
    } else {
        "中性"
    }
}

/// `_bodies(key)` — the body strings collected for one snippet bucket.
fn bodies_of(snippets: &Map<String, Value>, key: &str) -> Vec<String> {
    snippets
        .get(key)
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .map(|s| {
                    s.get("body")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string()
                })
                .collect()
        })
        .unwrap_or_default()
}

pub fn main(industry: &str) -> Result<Value, String> {
    let year = chrono::Local::now().year();

    // v2.7.3 · 利率/政策/汇率用 3_macro 权威域（stats.gov.cn / pbc / safe / 中证网...）
    // 行业宏观 + 大宗商品用普通 search（覆盖面更广）
    let trusted_queries: [(&str, String); 3] = [
        ("rate_cycle", format!("{year} 中国 利率 货币政策 降息 最新")),
        ("us_rate", format!("{year} 美联储 利率周期 最新")),
        ("fx_trend", format!("{year} 人民币 汇率 走势")),
    ];
    let generic_queries: [(&str, String); 3] = [
        ("geo_risk", format!("{year} 中美关系 贸易 制裁 {industry}")),
        ("commodity", format!("{year} 大宗商品 周期 CRB指数")),
        (
            "industry_macro",
            format!("{year} {industry} 宏观 政策 利好 利空"),
        ),
    ];

    let mut snippets: Map<String, Value> = Map::new();
    for (key, q) in &trusted_queries {
        let res = search_trusted(q, "3_macro", 4, &[], 6);
        snippets.insert((*key).to_string(), snippets_of(&res));
    }
    for (key, q) in &generic_queries {
        let res = search(q, 4, "ws");
        snippets.insert((*key).to_string(), snippets_of(&res));
    }

    let rate_cycle = sentiment(&bodies_of(&snippets, "rate_cycle"));
    let fx_trend = sentiment(&bodies_of(&snippets, "fx_trend"));
    let geo_risk = sentiment(&bodies_of(&snippets, "geo_risk"));
    let commodity = sentiment(&bodies_of(&snippets, "commodity"));

    Ok(json!({
        "data": {
            "rate_cycle": format!("{rate_cycle}（{year} 货币政策）"),
            "fx_trend": format!("{fx_trend}（人民币走势）"),
            "geo_risk": format!("{geo_risk}（地缘风险）"),
            "commodity": format!("{commodity}（大宗周期）"),
            "industry_macro_impact": sentiment(&bodies_of(&snippets, "industry_macro")),
            "web_search_snippets": Value::Object(snippets),
            "year": year,
            "industry": industry,
        },
        "source": "web_search:ddgs + heuristic sentiment",
        "fallback": false,
    }))
}

#[cfg(test)]
mod tests {
    use super::sentiment;

    #[test]
    fn sentiment_thresholds_match_upstream() {
        // pos > neg + 1 → 利好; pos == neg + 1 or less → 中性.
        assert_eq!(sentiment(&["降息 宽松".to_string()]), "利好");
        assert_eq!(sentiment(&["降息".to_string()]), "中性");
        assert_eq!(sentiment(&["加息 紧缩".to_string()]), "利空");
        assert_eq!(sentiment(&["加息".to_string()]), "中性");
        assert_eq!(sentiment(&["降息 加息".to_string()]), "中性");
        assert_eq!(sentiment(&[]), "中性");
    }
}
