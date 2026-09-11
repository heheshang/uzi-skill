//! Port of `fetch_industry.py`.
//!
//! Priority chain: hand-curated `INDUSTRY_ESTIMATES` anchors → `search_trusted`
//! dynamic 景气度 extraction → cninfo aggregated PE (the cninfo API is a
//! mini_racer-signed POST with no GET-only equivalent, so it degrades to the
//! `{}` upstream returns after every date attempt fails).

use std::sync::LazyLock;

use chrono::Datelike;
use regex::Regex;
use serde_json::{json, Value};

use uzi_core::py::truthy;

/// Industry → (growth, TAM, penetration, lifecycle, note).
const ESTIMATES: &[(&str, &str, &str, &str, &str, &str)] = &[
    (
        "光学光电子",
        "+30%/年",
        "¥420 亿",
        "12%",
        "成长期",
        "AR/VR + 车载光学 + iPhone 相机模组驱动",
    ),
    ("半导体", "+18%/年", "¥7800 亿", "国产化率 15%", "成长期", "国产替代 + AI 算力需求"),
    ("医药生物", "+10%/年", "¥3.2 万亿", "—", "成熟期", "集采降价 + 创新药放量博弈"),
    ("电池", "+22%/年", "¥1.8 万亿", "电车 38%", "成长期", "动力电池 + 储能双驱动"),
    ("白酒", "+6%/年", "¥7500 亿", "—", "成熟期", "次高端分化 + 名酒企稳"),
    ("银行", "+4%/年", "—", "—", "成熟期", "净息差收窄 + 红利防御属性"),
    ("钢铁", "-2%/年", "—", "—", "衰退期", "供给侧 + 需求下行"),
];

/// `_best_industry_match(industry)`.
fn best_industry_match(industry: &str) -> Option<Value> {
    if industry.is_empty() {
        return None;
    }
    let prefix: String = industry.chars().take(2).collect();
    for (key, growth, tam, pen, life, note) in ESTIMATES {
        if key.contains(industry) || industry.contains(*key) || key.contains(&prefix) {
            return Some(json!({
                "growth": growth,
                "tam": tam,
                "penetration": pen,
                "lifecycle": life,
                "note": note,
            }));
        }
    }
    None
}

static GROWTH_CTX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?:增长|增速|CAGR|复合增长|同比|增幅|年均增长|涨超|涨幅|暴涨|翻倍|提升|上升|上涨|净利齐涨)[^%]{0,20}?([+\-]?\d{1,3}(?:\.\d+)?)\s*%",
    )
    .unwrap()
});
static GROWTH_ALT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?:行业|市场|产业)[^%]{0,30}?([+\-]?\d{1,3}(?:\.\d+)?)\s*%|([+\-]?\d{1,3}(?:\.\d+)?)\s*%\s*(?:的?增长|的?增速)",
    )
    .unwrap()
});
static TAM_CTX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?:市场规模|规模达|规模约|将达|产业规模|TAM|行业规模)[^亿]{0,20}?(\d{1,5}(?:\.\d+)?)\s*亿")
        .unwrap()
});
static TAM_ALT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(\d{1,5}(?:\.\d+)?)\s*亿\s*(?:元)?\s*(?:市场|规模)").unwrap());
static PEN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"渗透率[^%]{0,10}?(\d{1,3}(?:\.\d+)?)\s*%|(\d{1,3}(?:\.\d+)?)\s*%\s*的?渗透率").unwrap()
});

/// `_dynamic_industry_overview(industry)`.
fn dynamic_industry_overview(industry: &str) -> Value {
    let year = chrono::Local::now().year();
    let queries: [(&str, String); 3] = [
        ("景气度", format!("{year} {industry} 行业景气度 增速 市场规模")),
        ("TAM", format!("{industry} 行业规模 亿元 TAM 2026")),
        ("周期", format!("{industry} 生命周期 成长期 成熟期 下行")),
    ];

    let mut snippets = serde_json::Map::new();
    for (tag, q) in &queries {
        let res = crate::web_search::search_trusted(q, "7_industry", 4, &[], 6);
        let valid: Vec<&Value> = res.iter().filter(|r| r.get("error").is_none()).collect();
        snippets.insert(
            (*tag).to_string(),
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
    }

    let mut parts: Vec<String> = Vec::new();
    for items in snippets.values() {
        if let Some(arr) = items.as_array() {
            for s in arr {
                parts.push(format!(
                    "{} {}",
                    s.get("title").and_then(|v| v.as_str()).unwrap_or(""),
                    s.get("body").and_then(|v| v.as_str()).unwrap_or("")
                ));
            }
        }
    }
    let bodies = parts.join(" ");

    let mut growth = "—".to_string();
    if let Some(m) = GROWTH_CTX.captures(&bodies) {
        if let Some(g) = m.get(1) {
            growth = format!("{}%/年", g.as_str());
        }
    } else if let Some(m) = GROWTH_ALT.captures(&bodies) {
        if let Some(g) = m.get(1).or_else(|| m.get(2)) {
            growth = format!("{}%/年", g.as_str());
        }
    }

    let mut tam = "—".to_string();
    if let Some(m) = TAM_CTX.captures(&bodies) {
        if let Some(g) = m.get(1) {
            tam = format!("¥{}亿", g.as_str());
        }
    } else if let Some(m) = TAM_ALT.captures(&bodies) {
        if let Some(g) = m.get(1) {
            tam = format!("¥{}亿", g.as_str());
        }
    }

    let mut penetration = "—".to_string();
    if let Some(m) = PEN.captures(&bodies) {
        if let Some(g) = m.get(1).or_else(|| m.get(2)) {
            penetration = format!("{}%", g.as_str());
        }
    }

    let mut lifecycle = "—";
    for (keyword, label) in [
        ("成长期", "成长期"),
        ("成熟期", "成熟期"),
        ("下行期", "下行期"),
        ("衰退", "衰退期"),
        ("拐点", "拐点"),
        ("景气", "景气上行"),
    ] {
        if bodies.contains(keyword) {
            lifecycle = label;
            break;
        }
    }

    let snippet_count: usize = snippets
        .values()
        .map(|v| v.as_array().map(|a| a.len()).unwrap_or(0))
        .sum();

    json!({
        "growth_heuristic": growth,
        "tam_heuristic": tam,
        "penetration_heuristic": penetration,
        "lifecycle_heuristic": lifecycle,
        "web_snippets": Value::Object(snippets),
        "snippet_count": snippet_count,
    })
}

fn first_n(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

/// `webapi.cninfo.com.cn/api/sysapi/p_sysapi1087` needs a POST whose
/// `Accept-Enckey` is an AES token produced by `cninfo.js` under mini_racer; the
/// crate's http layer is GET-only with no JS engine, so this leg is `None`.
fn fetch_cninfo_industry_pe(_date: &str) -> Option<Value> {
    None
}

fn as_float(v: &Value) -> Option<f64> {
    match v {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.trim().parse::<f64>().ok(),
        _ => None,
    }
}

/// `_cninfo_industry_metrics` row projection (the cninfo frame is unavailable,
/// but the upstream computation is kept so the shape matches once it is).
fn cninfo_metrics_from_rows(industry: &str, rows: &Value, data_date: &str) -> Option<Value> {
    let row = crate::industry::resolve_csrc_industry(industry, rows)?;
    let first = rows.as_array().and_then(|a| a.first());
    let has = |c: &str| first.map(|r| r.get(c).is_some()).unwrap_or(false);

    let company_count = if has("公司数量") {
        row.get("公司数量")
            .and_then(as_float)
            .map(|f| json!(f as i64))
            .unwrap_or(Value::Null)
    } else {
        Value::Null
    };
    let total_mcap = if has("总市值-静态") {
        row.get("总市值-静态")
            .and_then(as_float)
            .map(|f| json!(f))
            .unwrap_or(Value::Null)
    } else {
        Value::Null
    };
    let net_profit = if has("净利润-静态") {
        row.get("净利润-静态")
            .and_then(as_float)
            .map(|f| json!(f))
            .unwrap_or(Value::Null)
    } else {
        Value::Null
    };
    let pe_col = first.and_then(|r| {
        r.as_object()
            .and_then(|o| o.keys().find(|k| k.contains("市盈率") && k.contains("加权")).cloned())
    });
    let pe_weighted = match pe_col {
        Some(c) => row
            .get(c.as_str())
            .and_then(as_float)
            .map(|f| json!(f))
            .unwrap_or(Value::Null),
        None => Value::Null,
    };
    let pe_median = if has("静态市盈率-中位数") {
        row.get("静态市盈率-中位数")
            .and_then(as_float)
            .map(|f| json!(f))
            .unwrap_or(Value::Null)
    } else {
        Value::Null
    };

    Some(json!({
        "industry_name_match": row.get("行业名称").map(uzi_core::py::py_str).unwrap_or_default(),
        "company_count": company_count,
        "total_mcap_yi": total_mcap,
        "net_profit_yi": net_profit,
        "industry_pe_weighted": pe_weighted,
        "industry_pe_median": pe_median,
        "data_date": data_date,
    }))
}

/// `_cninfo_industry_metrics(industry_name)`.
fn cninfo_industry_metrics(industry: &str) -> Value {
    if industry.is_empty() {
        return json!({});
    }
    let today = chrono::Local::now().date_naive();
    for i in 1..=7 {
        let d = (today - chrono::Duration::days(i)).format("%Y%m%d").to_string();
        let Some(rows) = fetch_cninfo_industry_pe(&d) else {
            continue;
        };
        if let Some(m) = cninfo_metrics_from_rows(industry, &rows, &d) {
            return m;
        }
    }
    json!({})
}

pub fn main(industry: &str) -> Result<Value, String> {
    let est = best_industry_match(industry);
    let lite = std::env::var("UZI_LITE").map(|v| v == "1").unwrap_or(false);
    let dynamic = if lite || est.is_some() {
        json!({})
    } else {
        dynamic_industry_overview(industry)
    };
    let cninfo_metrics = cninfo_industry_metrics(industry);

    let est_obj = est.clone().unwrap_or_else(|| json!({}));
    let pick = |ke: &str, kd: &str| -> String {
        est_obj
            .get(ke)
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .or_else(|| dynamic.get(kd).and_then(|v| v.as_str()).filter(|s| !s.is_empty()))
            .unwrap_or("—")
            .to_string()
    };
    let growth = pick("growth", "growth_heuristic");
    let tam = pick("tam", "tam_heuristic");
    let penetration = pick("penetration", "penetration_heuristic");
    let lifecycle = pick("lifecycle", "lifecycle_heuristic");
    let note = est_obj
        .get("note")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    let mut source_parts = vec!["cninfo:stock_industry_pe_ratio".to_string()];
    if est.is_some() {
        source_parts.push("INDUSTRY_ESTIMATES".to_string());
    }
    if truthy(&dynamic) {
        let n = dynamic.get("snippet_count").and_then(|v| v.as_i64()).unwrap_or(0);
        source_parts.push(format!("search_trusted:7_industry({n} snippets)"));
    }

    let needs_web_search = est.is_none() && !truthy(&dynamic);
    let queries = if needs_web_search {
        json!([
            format!("{industry} 行业景气度 2026"),
            format!("{industry} 市场规模 TAM"),
            format!("{industry} 渗透率 提升空间"),
        ])
    } else {
        json!([])
    };
    let fallback = !truthy(&cninfo_metrics) && !truthy(&dynamic);
    let total_companies = cninfo_metrics.get("company_count").cloned().unwrap_or(Value::Null);
    let industry_pe_weighted = cninfo_metrics
        .get("industry_pe_weighted")
        .cloned()
        .unwrap_or(Value::Null);

    Ok(json!({
        "data": {
            "industry": industry,
            "growth": growth,
            "tam": tam,
            "penetration": penetration,
            "lifecycle": lifecycle,
            "note": note,
            "cninfo_metrics": cninfo_metrics,
            "total_companies": total_companies,
            "industry_pe_weighted": industry_pe_weighted,
            "dynamic_snippets": dynamic.get("web_snippets").cloned().unwrap_or_else(|| json!({})),
            "needs_web_search": needs_web_search,
            "web_search_queries": queries,
        },
        "source": source_parts.join(" + "),
        "fallback": fallback,
    }))
}
