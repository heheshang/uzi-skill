//! Port of `fetch_policy.py`.

use std::collections::HashSet;
use std::sync::LazyLock;

use chrono::Datelike;
use regex::Regex;
use serde_json::{json, Map, Value};

use crate::http;
use crate::web_search::search_trusted;

/// Upstream passes its own UA for the cfachina homepage scrape.
const CFACHINA_UA: &str =
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) Chrome/120.0.0.0 Safari/537.36";

const CFACHINA_NOISE: [&str; 29] = [
    "首页",
    "协会",
    "联系",
    "办理",
    "业务",
    "资格",
    "会员",
    "管理",
    "委员会",
    "内容",
    "简介",
    "章程",
    "廉洁",
    "脱贫",
    "招聘",
    "采购",
    "信息公示",
    "基本情况",
    "历史情况",
    "人员信息",
    "分支机构",
    "股东信息",
    "诚信记录",
    "次级债",
    "诚信信息",
    "月度成交",
    "月度经营",
    "服务实体",
    "移动应用",
];

fn first_n(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

/// Per-category keyword sentiment (`积极` / `收紧` / `—` / `中性`).
fn policy_sentiment(text: &str) -> &'static str {
    let mut pos = 0usize;
    for &kw in &["扶持", "支持", "鼓励", "补贴", "优惠", "免税", "专项", "利好"] {
        if text.contains(kw) {
            pos += 1;
        }
    }
    let mut neg = 0usize;
    for &kw in &["处罚", "罚款", "违规", "禁止", "限制", "收紧", "调查", "约谈"] {
        if text.contains(kw) {
            neg += 1;
        }
    }
    if pos > neg + 1 {
        "积极"
    } else if neg > pos + 1 {
        "收紧"
    } else if pos == 0 && neg == 0 {
        "—"
    } else {
        "中性"
    }
}

/// `_fetch_cfachina_titles(limit)` — static HTML title scrape of the CTA site.
fn fetch_cfachina_titles(limit: usize) -> Vec<Value> {
    static RE: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r#"<a[^>]*href="([^"]+)"[^>]*>([^<]{8,60})</a>"#).unwrap()
    });

    let resp = match http::get(
        "http://www.cfachina.org/",
        &[("User-Agent", CFACHINA_UA)],
        12,
    ) {
        Ok(r) => r,
        Err(_) => return Vec::new(),
    };
    if resp.status != 200 {
        return Vec::new();
    }
    // `r.encoding = r.apparent_encoding or "utf-8"`: trust strict UTF-8, else GBK.
    let html = match std::str::from_utf8(&resp.body) {
        Ok(_) => resp.text(),
        Err(_) => resp.gbk_text(),
    };

    let mut titles = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    for cap in RE.captures_iter(&html) {
        let href = &cap[1];
        let t = cap[2].trim();
        if CFACHINA_NOISE.iter().any(|noise| t.contains(*noise)) {
            continue;
        }
        if !seen.insert(t.to_string()) {
            continue;
        }
        let url = if let Some(rest) = href.strip_prefix('/') {
            format!("http://www.cfachina.org/{rest}")
        } else if href.starts_with("http") {
            href.to_string()
        } else {
            let rest = href.trim_start_matches(|c| c == '.' || c == '/');
            format!("http://www.cfachina.org/{rest}")
        };
        titles.push(json!({
            "title": first_n(t, 80),
            "body": "",
            "url": url,
        }));
        if titles.len() >= limit {
            break;
        }
    }
    titles
}

pub fn main(industry: &str) -> Result<Value, String> {
    let year = chrono::Local::now().year();
    let queries: [(&str, String); 4] = [
        ("policy_dir", format!("{year} {industry} 国家政策 扶持 利好")),
        ("subsidy", format!("{year} {industry} 政府补贴 税收优惠")),
        ("monitoring", format!("{year} {industry} 监管 合规 风险")),
        ("anti_trust", format!("{year} {industry} 反垄断 调查")),
    ];

    // v2.13.7 · cfachina 直连（期货监管信号 · 补 ddgs 盲区）
    let cfa_titles = if ["期货", "衍生品", "商品", "金融", "证券"]
        .iter()
        .any(|kw| industry.contains(kw))
    {
        fetch_cfachina_titles(10)
    } else {
        Vec::new()
    };

    let mut snippets: Map<String, Value> = Map::new();
    let mut sentiment_map: Map<String, Value> = Map::new();

    // v2.7.3 · 政策 dim 全部用 13_policy 权威域（gov.cn / csrc / 中证网 / 证券时报 ...）
    for (key, q) in &queries {
        let res = search_trusted(q, "13_policy", 4, &[], 6);
        let valid: Vec<&Value> = res.iter().filter(|r| r.get("error").is_none()).collect();
        snippets.insert(
            (*key).to_string(),
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
            ),
        );

        // Heuristic sentiment per category
        let text = valid
            .iter()
            .map(|r| r.get("body").and_then(|v| v.as_str()).unwrap_or(""))
            .collect::<Vec<_>>()
            .join(" ");
        let label = policy_sentiment(&text);
        sentiment_map.insert((*key).to_string(), json!(label));
    }

    // 追加 cfachina 到 monitoring snippets（期货相关 industry 才抓）
    if !cfa_titles.is_empty() {
        let arr = snippets
            .entry("monitoring".to_string())
            .or_insert_with(|| json!([]));
        if let Some(list) = arr.as_array_mut() {
            list.extend(cfa_titles.iter().take(5).cloned());
        }
    }

    let sentiment_of = |key: &str| -> String {
        sentiment_map
            .get(key)
            .and_then(|v| v.as_str())
            .unwrap_or("—")
            .to_string()
    };

    Ok(json!({
        "data": {
            "policy_dir": sentiment_of("policy_dir"),
            "subsidy": sentiment_of("subsidy"),
            "monitoring": sentiment_of("monitoring"),
            "anti_trust": sentiment_of("anti_trust"),
            "snippets": Value::Object(snippets),
            "year": year,
            "industry": industry,
            "cfachina_titles_count": cfa_titles.len(),
        },
        "source": "web_search:ddgs + keyword sentiment + cfachina (v2.13.7 · 期货监管)",
        "fallback": false,
    }))
}

#[cfg(test)]
mod tests {
    use super::policy_sentiment;

    #[test]
    fn policy_sentiment_thresholds_match_upstream() {
        assert_eq!(policy_sentiment("扶持 支持"), "积极");
        assert_eq!(policy_sentiment("扶持"), "中性");
        assert_eq!(policy_sentiment("处罚 罚款"), "收紧");
        assert_eq!(policy_sentiment("处罚"), "中性");
        assert_eq!(policy_sentiment("扶持 处罚"), "中性");
        assert_eq!(policy_sentiment(""), "—");
    }
}
