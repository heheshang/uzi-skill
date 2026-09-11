//! Port of `fetch_research.py`.
//!
//! Primary source is `crate::sources::fetch_research_reports` (AkShare
//! `stock_research_report_em`); the cninfo forecast fallback is the library-only
//! mini_racer-signed `p_sysapi1089` POST, so it degrades to the empty list
//! upstream returns when every date attempt fails.

use serde_json::{json, Map, Value};

use uzi_core::py::{float_str, py_str, round};
use uzi_core::ticker::parse_ticker;

fn first_n(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

/// `float(r.get(k, 0) or 0)` — `Err` mirrors the Python `except: pass`.
fn forecast_float(v: Option<&Value>) -> Option<f64> {
    match v {
        None | Some(Value::Null) => Some(0.0),
        Some(Value::Number(n)) => n.as_f64(),
        Some(Value::Bool(b)) => Some(if *b { 1.0 } else { 0.0 }),
        Some(Value::String(s)) => {
            let t = s.trim();
            if t.is_empty() {
                Some(0.0)
            } else {
                t.parse::<f64>().ok()
            }
        }
        Some(_) => Some(0.0),
    }
}

fn avg(values: &[f64], ndigits: i32) -> Option<f64> {
    if values.is_empty() {
        None
    } else {
        Some(round(values.iter().sum::<f64>() / values.len() as f64, ndigits))
    }
}

pub fn main(ticker: &str) -> Result<Value, String> {
    let ti = parse_ticker(ticker);
    if ti.market != "A" {
        return Ok(json!({
            "ticker": ti.full,
            "data": {"report_count": 0},
            "source": "n/a",
            "fallback": true,
        }));
    }

    let reports: Vec<Value> = crate::sources::fetch_research_reports(&ti)
        .as_array()
        .cloned()
        .unwrap_or_default();
    // Upstream's `_fetch_cninfo_forecast` needs the mini_racer-signed cninfo POST
    // and is otherwise unused when `reports` is non-empty.
    let source = if reports.is_empty() {
        "akshare:stock_rank_forecast_cninfo"
    } else {
        "akshare:stock_research_report_em"
    };

    // Rating counts, first-seen order (Python `Counter`).
    let mut ratings: Vec<(String, i64)> = Vec::new();
    for r in &reports {
        let rating = py_str(
            r.get("东财评级")
                .or_else(|| r.get("评级"))
                .unwrap_or(&Value::Null),
        )
        .trim()
        .to_string();
        if rating.is_empty() || matches!(rating.as_str(), "nan" | "-" | "None") {
            continue;
        }
        if let Some(pos) = ratings.iter().position(|(k, _)| *k == rating) {
            ratings[pos].1 += 1;
        } else {
            ratings.push((rating, 1));
        }
    }

    let mut eps_2026: Vec<f64> = Vec::new();
    let mut pe_2026: Vec<f64> = Vec::new();
    let mut eps_2027: Vec<f64> = Vec::new();
    for r in &reports {
        let a = forecast_float(r.get("2026-盈利预测-收益"));
        let b = forecast_float(r.get("2026-盈利预测-市盈率"));
        let c = forecast_float(r.get("2027-盈利预测-收益"));
        let (Some(a), Some(b), Some(c)) = (a, b, c) else {
            continue;
        };
        if a > 0.0 {
            eps_2026.push(a);
        }
        if b > 0.0 {
            pe_2026.push(b);
        }
        if c > 0.0 {
            eps_2027.push(c);
        }
    }

    let avg_eps_2026 = avg(&eps_2026, 2);
    let avg_pe_2026 = avg(&pe_2026, 1);
    let avg_eps_2027 = avg(&eps_2027, 2);
    let target_price = match (avg_eps_2026, avg_pe_2026) {
        (Some(e), Some(p)) => Some(round(e * p, 2)),
        _ => None,
    };

    let mut brokers: Vec<String> = Vec::new();
    for r in &reports {
        let org = py_str(r.get("机构").unwrap_or(&Value::Null)).trim().to_string();
        if org.is_empty() || matches!(org.as_str(), "nan" | "None") {
            continue;
        }
        if !brokers.contains(&org) {
            brokers.push(org);
        }
    }
    brokers.sort();

    let mut rating_dist = Map::new();
    for (k, v) in &ratings {
        rating_dist.insert(k.clone(), json!(v));
    }
    let total: i64 = ratings.iter().map(|(_, c)| *c).sum();
    let buy_pct: Value = if total > 0 {
        let buy: i64 = ratings
            .iter()
            .filter(|(k, _)| k.contains("买入") || k.contains("增持"))
            .map(|(_, c)| *c)
            .sum();
        json!(round(buy as f64 / total as f64 * 100.0, 0))
    } else {
        json!(0)
    };
    let rating_str = if ratings.is_empty() {
        "—".to_string()
    } else {
        ratings
            .iter()
            .map(|(k, v)| format!("{k} {v}"))
            .collect::<Vec<_>>()
            .join(" / ")
    };

    let mut recent: Vec<Value> = Vec::new();
    for r in reports.iter().take(10) {
        let s = |k: &str| py_str(r.get(k).unwrap_or(&Value::Null));
        recent.push(json!({
            "date": first_n(&s("日期"), 10),
            "title": first_n(&s("报告名称"), 60),
            "broker": s("机构"),
            "rating": s("东财评级"),
            "pdf": s("报告PDF链接"),
            "eps_2026": r.get("2026-盈利预测-收益").cloned().unwrap_or(Value::Null),
            "pe_2026": r.get("2026-盈利预测-市盈率").cloned().unwrap_or(Value::Null),
        }));
    }

    let coverage = if brokers.is_empty() {
        format!("{} 份", reports.len())
    } else {
        format!("{} 家", brokers.len())
    };
    let target_avg = match target_price {
        Some(p) => format!("¥{}", float_str(p)),
        None => "—".to_string(),
    };

    Ok(json!({
        "ticker": ti.full,
        "data": {
            "report_count": reports.len(),
            "coverage": coverage,
            "coverage_count": brokers.len(),
            "rating": rating_str,
            "rating_distribution": Value::Object(rating_dist),
            "buy_rating_pct": buy_pct,
            "target_price_avg": target_price,
            "target_avg": target_avg,
            "consensus_eps_2026": avg_eps_2026,
            "consensus_pe_2026": avg_pe_2026,
            "consensus_eps_2027": avg_eps_2027,
            "recent_reports": recent,
            "brokers": brokers,
            "upside": Value::Null,
        },
        "source": source,
        "fallback": reports.is_empty(),
    }))
}
