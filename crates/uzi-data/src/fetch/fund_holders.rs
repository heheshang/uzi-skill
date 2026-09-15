//! Port of `fetch_fund_holders.py`.
//!
//! Data chain, matching the AkShare wrappers upstream names:
//!   * `stock_fund_stock_holder` → `vip.stock.finance.sina.com.cn/corp/go.php/
//!     vCI_FundStockHolder/stockid/{code}.phtml` (funds holding the stock)
//!   * `fund_open_fund_info_em(累计净值走势)` → `fund.eastmoney.com/pingzhongdata/
//!     {code}.js` (`Data_ACWorthTrend`)
//!   * `fund_individual_basic_info_xq` → `danjuanfunds.com/djapi/fund/{code}`
//! The AkShare-only `_holding_quarters` helper is dead code upstream (the
//! output rows hard-code `holding_quarters: 1`) and is not ported.

use std::sync::LazyLock;

use chrono::Datelike;
use regex::Regex;
use serde_json::{json, Value};

use uzi_core::cache::{cached, TTL_QUARTERLY};
use uzi_core::py::round;
use uzi_core::ticker::parse_ticker;

const UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/109.0.0.0 Safari/537.36";
const DJANJUAN_UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/80.0.3987.149 Safari/537.36";

/// `MANAGER_AVATAR_MAP`.
fn avatar_for(name: &str) -> &'static str {
    match name {
        "张坤" => "zhangkun",
        "谢治宇" => "xiezhiyu",
        "朱少醒" => "zhushaoxing",
        "冯柳" => "fengliu",
        "邓晓峰" => "dengxiaofeng",
        _ => "",
    }
}

/// HTML entity/whitespace cleanup for a scraped cell.
fn clean_cell(s: &str) -> String {
    static TAGS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"<[^>]*>").unwrap());
    let no_tags = TAGS.replace_all(s, "");
    no_tags
        .replace("&nbsp;", " ")
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .trim()
        .to_string()
}

/// `fetch_holding_funds(ticker_code)` — normalized holder rows, or the upstream
/// `{"error": ...}` / `[]` degradation.
fn fetch_holding_funds(code: &str) -> Vec<Value> {
    static ROW: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"(?is)<tr[^>]*>(.*?)</tr>").unwrap());
    static CELL: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"(?is)<td[^>]*>(.*?)</td>").unwrap());

    let url = format!(
        "https://vip.stock.finance.sina.com.cn/corp/go.php/vCI_FundStockHolder/stockid/{code}.phtml"
    );
    let resp = match crate::http::get(&url, &[("User-Agent", UA)], 20) {
        Ok(r) => r,
        Err(e) => return vec![json!({"error": format!("both fund lookup methods failed: {e}")})],
    };
    if !resp.is_ok() {
        return vec![json!({"error": format!("both fund lookup methods failed: HTTP {}", resp.status)})];
    }
    let html = resp.gbk_text();

    let table_start = html.find("id=\"FundHoldSharesTable\"");
    let Some(start) = table_start else {
        return Vec::new();
    };
    let Some(end_off) = html[start..].find("</table>") else {
        return Vec::new();
    };
    let table = &html[start..start + end_off];

    let mut current_date = String::new();
    let mut out: Vec<Value> = Vec::new();
    for row in ROW.captures_iter(table) {
        let cells: Vec<String> = CELL
            .captures_iter(&row[1])
            .map(|c| clean_cell(&c[1]))
            .collect();
        if cells.is_empty() {
            continue;
        }
        if cells[0].starts_with("截止日期") {
            current_date = cells.get(1).cloned().unwrap_or_default();
            continue;
        }
        if cells[0] == "基金名称" || current_date.is_empty() || cells.len() < 6 {
            continue;
        }
        let name = cells[0].clone();
        let fund_code = cells[1].clone();
        if name.is_empty() || fund_code.is_empty() {
            continue;
        }
        let num = |s: &str| -> Option<f64> { s.replace(',', "").parse::<f64>().ok() };
        let (Some(shares), Some(ratio), Some(mcap), Some(_nav)) =
            (num(&cells[2]), num(&cells[3]), num(&cells[4]), num(&cells[5]))
        else {
            continue;
        };
        out.push(json!({
            "基金名称": name,
            "基金代码": fund_code,
            "持仓数量": shares,
            "占流通股比例": ratio,
            "持股市值": mcap,
            // akshare 1.18.x exposes 占净值比例, so upstream's `占市值比例`
            // lookup is None (and `_pos_pct` falls back to 占流通股比例).
            "占市值比例": Value::Null,
            "截止日期": current_date,
        }));
    }
    out
}

/// Pull one `Data_<name> = [...]` array out of the pingzhongdata JS bundle.
fn js_array(text: &str, name: &str) -> Option<Value> {
    let at = text.find(name)?;
    let rest = &text[at..];
    let s = rest.find('[')? + at;
    let e = text[s..].find(';')? + s;
    serde_json::from_str(text[s..e].trim()).ok()
}

/// `compute_fund_stats(fund_code)` — 5Y return / annualized / max drawdown /
/// Sharpe from the cumulative-NAV series.
fn compute_fund_stats(fund_code: &str) -> Value {
    let url = format!("https://fund.eastmoney.com/pingzhongdata/{fund_code}.js");
    let resp = match crate::http::get(&url, &[("User-Agent", UA)], 20) {
        Ok(r) => r,
        Err(e) => return json!({"error": format!("fund_info fail: {e}")}),
    };
    if !resp.is_ok() {
        return json!({"error": format!("fund_info fail: HTTP {}", resp.status)});
    }
    let text = resp.text();
    let Some(arr) = js_array(&text, "Data_ACWorthTrend") else {
        return json!({});
    };
    let offset = chrono::FixedOffset::east_opt(8 * 3600);
    let mut series: Vec<(String, f64)> = Vec::new();
    if let Some(rows) = arr.as_array() {
        for row in rows {
            let (Some(ms), Some(val)) = (
                row.get(0).and_then(|v| v.as_f64()),
                row.get(1).and_then(|v| v.as_f64()),
            ) else {
                continue;
            };
            let Some(offset) = offset else { continue };
            let Some(dt) = chrono::DateTime::from_timestamp_millis(ms as i64) else {
                continue;
            };
            series.push((dt.with_timezone(&offset).format("%Y-%m-%d").to_string(), val));
        }
    }
    if series.is_empty() {
        return json!({});
    }
    series.sort_by(|a, b| a.0.cmp(&b.0));

    let cutoff = format!("{}-01-01", chrono::Local::now().year() - 5);
    let mut five_y: Vec<&(String, f64)> = series.iter().filter(|(d, _)| d.as_str() >= cutoff.as_str()).collect();
    if five_y.len() < 50 {
        let start = series.len().saturating_sub(1260);
        five_y = series[start..].iter().collect();
    }

    let navs: Vec<f64> = five_y
        .iter()
        .map(|(_, v)| *v)
        .filter(|v| *v > 0.0)
        .collect();
    if navs.len() < 10 {
        return json!({});
    }

    let start = navs[0];
    let end = navs[navs.len() - 1];
    let return_5y = (end - start) / start * 100.0;
    let years = (navs.len() as f64 / 252.0).max(0.5);
    let annualized = if start > 0.0 {
        ((end / start).powf(1.0 / years) - 1.0) * 100.0
    } else {
        0.0
    };

    let mut peak = navs[0];
    let mut max_dd = 0.0f64;
    for v in &navs {
        if *v > peak {
            peak = *v;
        }
        let dd = (*v - peak) / peak;
        if dd < max_dd {
            max_dd = dd;
        }
    }

    let daily: Vec<f64> = (1..navs.len())
        .filter(|i| navs[i - 1] > 0.0)
        .map(|i| navs[i] / navs[i - 1] - 1.0)
        .collect();
    let mut sharpe = 0.0f64;
    if !daily.is_empty() {
        let mean = daily.iter().sum::<f64>() / daily.len() as f64;
        let std = if daily.len() > 1 {
            let var = daily.iter().map(|r| (r - mean).powi(2)).sum::<f64>() / (daily.len() - 1) as f64;
            var.sqrt()
        } else {
            1.0
        };
        if std > 0.0 {
            sharpe = (mean * 252.0 - 0.03) / (std * 252.0f64.sqrt());
        }
    }

    let step = (navs.len() / 15).max(1);
    let end_norm = round(end / start, 3);
    let mut spark: Vec<f64> = navs.iter().step_by(step).take(20).map(|v| round(*v / start, 3)).collect();
    if spark.last().copied() != Some(end_norm) {
        spark.push(end_norm);
    }

    json!({
        "return_5y": round(return_5y, 1),
        "annualized_5y": round(annualized, 1),
        "max_drawdown": round(max_dd * 100.0, 1),
        "sharpe": round(sharpe, 2),
        "nav_history": spark,
    })
}

/// `fetch_fund_manager_name(fund_code)`.
fn fetch_fund_manager_name(fund_code: &str) -> Option<String> {
    let url = format!("https://danjuanfunds.com/djapi/fund/{fund_code}");
    let resp = crate::http::get(&url, &[("User-Agent", DJANJUAN_UA)], 15).ok()?;
    if !resp.is_ok() {
        return None;
    }
    let v = resp.json()?;
    let name = v.get("data")?.get("manager_name")?.as_str()?;
    let first = name.split(',').next().unwrap_or("").trim().to_string();
    if first.is_empty() {
        None
    } else {
        Some(first)
    }
}

/// `_is_active_fund(name)` — drop obvious ETF/index products.
fn is_active_fund(name: &str) -> bool {
    const MARKERS: [&str; 7] = ["ETF", "指数", "沪深300", "中证", "创业板", "科创", "红利指数"];
    !MARKERS.iter().any(|m| name.contains(m))
}

/// `_pos_pct(row)`.
fn pos_pct(row: &Value) -> f64 {
    let a = row.get("占市值比例");
    let b = row.get("占流通股比例");
    let pick = match a {
        Some(v) if uzi_core::py::truthy(v) => v,
        _ => match b {
            Some(v) if uzi_core::py::truthy(v) => v,
            _ => return 0.0,
        },
    };
    match pick {
        Value::Number(n) => n.as_f64().unwrap_or(0.0),
        Value::String(s) => s.replace(',', "").parse::<f64>().unwrap_or(0.0),
        _ => 0.0,
    }
}

fn stats_is_usable(stats: &Value) -> bool {
    stats.is_object()
        && !stats.as_object().map(|o| o.is_empty()).unwrap_or(true)
        && stats.get("error").is_none()
}

/// `_build_row_full(row)`.
fn build_row_full(row: &Value) -> Option<Value> {
    let fund_code = row
        .get("基金代码")
        .map(uzi_core::py::py_str)
        .unwrap_or_default();
    let fund_name = row
        .get("基金名称")
        .map(uzi_core::py::py_str)
        .unwrap_or_default();
    if fund_code.is_empty() {
        return None;
    }

    let stats = cached::<_, anyhow::Error>(&fund_code, &format!("fund_stats_{fund_code}"), TTL_QUARTERLY, || {
        Ok(compute_fund_stats(&fund_code))
    })
    .unwrap_or_else(|_| json!({}));
    let stats = if stats_is_usable(&stats) { stats } else { json!({}) };

    let manager_name = fetch_fund_manager_name(&fund_code).unwrap_or_else(|| "—".to_string());
    let position_pct = pos_pct(row);

    let has_real_stats = stats
        .get("return_5y")
        .map(|v| !v.is_null())
        .unwrap_or(false);
    if !has_real_stats {
        return Some(json!({
            "name": manager_name,
            "fund_name": fund_name,
            "fund_code": fund_code,
            "avatar": avatar_for(&manager_name),
            "position_pct": round(position_pct, 2),
            "rank_in_fund": 0,
            "holding_quarters": 1,
            "position_trend": "持有",
            "return_5y": Value::Null,
            "annualized_5y": Value::Null,
            "max_drawdown": Value::Null,
            "sharpe": Value::Null,
            "peer_rank_pct": Value::Null,
            "nav_history": json!([]),
            "fund_url": format!("https://fund.eastmoney.com/{fund_code}.html"),
            "_row_type": "lite",
            "_stats_note": "fund stats 不可用（网络或数据不足）",
        }));
    }

    let g = |k: &str| stats.get(k).cloned().unwrap_or_else(|| json!(0));
    Some(json!({
        "name": manager_name,
        "fund_name": fund_name,
        "fund_code": fund_code,
        "avatar": avatar_for(&manager_name),
        "position_pct": round(position_pct, 2),
        "rank_in_fund": 0,
        "holding_quarters": 1,
        "position_trend": "持有",
        "return_5y": g("return_5y"),
        "annualized_5y": g("annualized_5y"),
        "max_drawdown": g("max_drawdown"),
        "sharpe": g("sharpe"),
        "peer_rank_pct": 50,
        "nav_history": stats.get("nav_history").cloned().unwrap_or_else(|| json!([])),
        "fund_url": format!("https://fund.eastmoney.com/{fund_code}.html"),
        "_row_type": "full",
    }))
}

/// `_build_row_lite(row)`.
fn build_row_lite(row: &Value) -> Option<Value> {
    let fund_code = row
        .get("基金代码")
        .map(uzi_core::py::py_str)
        .unwrap_or_default();
    let fund_name = row
        .get("基金名称")
        .map(uzi_core::py::py_str)
        .unwrap_or_default();
    if fund_code.is_empty() {
        return None;
    }
    Some(json!({
        "name": "—",
        "fund_name": fund_name,
        "fund_code": fund_code,
        "avatar": "",
        "position_pct": round(pos_pct(row), 2),
        "rank_in_fund": 0,
        "holding_quarters": 1,
        "position_trend": "持有",
        "return_5y": Value::Null,
        "annualized_5y": Value::Null,
        "max_drawdown": Value::Null,
        "sharpe": Value::Null,
        "peer_rank_pct": Value::Null,
        "nav_history": json!([]),
        "fund_url": format!("https://fund.eastmoney.com/{fund_code}.html"),
        "_row_type": "lite",
    }))
}

pub fn main(ticker: &str) -> Result<Value, String> {
    let ti = parse_ticker(ticker);
    if ti.market != "A" {
        return Ok(json!({
            "ticker": ti.full,
            "data": {"fund_managers": [], "_note": "currently A-share only"},
            "source": "n/a",
            "fallback": true,
        }));
    }

    let holders = {
        let full = ti.full.clone();
        let code = ti.code.clone();
        cached::<_, anyhow::Error>(&full, "fund_holders_v2", TTL_QUARTERLY, move || {
            Ok(json!(fetch_holding_funds(&code)))
        })
        .unwrap_or_else(|_| json!([]))
    };
    let holders: Vec<Value> = holders.as_array().cloned().unwrap_or_default();

    let active_holders: Vec<Value> = holders
        .iter()
        .filter(|h| h.get("error").is_none())
        .filter(|h| is_active_fund(h.get("基金名称").and_then(|v| v.as_str()).unwrap_or("")))
        .cloned()
        .collect();
    let total_funds = holders.iter().filter(|h| h.get("error").is_none()).count();

    let stats_top_n: usize = std::env::var("UZI_FUND_STATS_TOP")
        .ok()
        .and_then(|v| v.trim().parse().ok())
        .unwrap_or(20);

    let mut sorted_holders = active_holders.clone();
    sorted_holders.sort_by(|a, b| {
        pos_pct(b)
            .partial_cmp(&pos_pct(a))
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    // `UZI_FUND_LIMIT=all` (or unset) keeps the full list; a number caps it.
    let limit: Option<usize> = match std::env::var("UZI_FUND_LIMIT") {
        Ok(v) => {
            let t = v.trim().to_lowercase();
            if t.is_empty() || t == "all" {
                None
            } else {
                t.parse::<usize>().ok()
            }
        }
        Err(_) => None,
    };
    let iter_holders: Vec<Value> = match limit {
        None => sorted_holders,
        Some(n) => sorted_holders.into_iter().take(n).collect(),
    };

    let top_full: Vec<Value> = iter_holders.iter().take(stats_top_n).cloned().collect();
    let rest_lite: Vec<Value> = iter_holders.iter().skip(stats_top_n).cloned().collect();

    let mut managers: Vec<Value> = Vec::new();
    for row in &top_full {
        if let Some(r) = build_row_full(row) {
            managers.push(r);
        }
    }
    let mut lite_count = 0usize;
    for row in &rest_lite {
        if let Some(r) = build_row_lite(row) {
            managers.push(r);
            lite_count += 1;
        }
    }

    managers.sort_by(|a, b| {
        let full_a = (a.get("_row_type").and_then(|v| v.as_str()) == Some("full")) as u8;
        let full_b = (b.get("_row_type").and_then(|v| v.as_str()) == Some("full")) as u8;
        let key_a = (
            1 - full_a,
            if full_a == 1 {
                -a.get("return_5y").and_then(|v| v.as_f64()).unwrap_or(0.0)
            } else {
                0.0
            },
            -a.get("position_pct").and_then(|v| v.as_f64()).unwrap_or(0.0),
        );
        let key_b = (
            1 - full_b,
            if full_b == 1 {
                -b.get("return_5y").and_then(|v| v.as_f64()).unwrap_or(0.0)
            } else {
                0.0
            },
            -b.get("position_pct").and_then(|v| v.as_f64()).unwrap_or(0.0),
        );
        key_a
            .0
            .cmp(&key_b.0)
            .then(key_a.1.partial_cmp(&key_b.1).unwrap_or(std::cmp::Ordering::Equal))
            .then(key_a.2.partial_cmp(&key_b.2).unwrap_or(std::cmp::Ordering::Equal))
    });

    let passive_count = total_funds.saturating_sub(active_holders.len());
    let note = format!(
        "共 {total_funds} 家基金持有 · 头部 {} 家算完整 5Y 业绩（按持仓排序），其余 {lite_count} 家只列清单（点 fund_url 跳东财看详情）· 过滤 {passive_count} 家 ETF/指数基金 · UZI_FUND_STATS_TOP=N 可调",
        top_full.len()
    );

    Ok(json!({
        "ticker": ti.full,
        "data": {
            "fund_managers": managers,
            "total_funds_holding": total_funds,
            "active_funds_count": active_holders.len(),
            "full_stats_count": top_full.len(),
            "lite_count": lite_count,
            "passive_funds_filtered": passive_count,
            "_note": note,
        },
        "source": "akshare:stock_fund_stock_holder + fund_open_fund_info_em(top N only)",
        "fallback": false,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fetch_holding_funds_live() {
        let rows = fetch_holding_funds("002273");
        assert!(!rows.is_empty(), "expected non-empty fund holders for 002273");
    }

    #[test]
    fn main_produces_managers() {
        let result = main("002273.SZ").expect("main should succeed");
        let arr = result["data"]["fund_managers"]
            .as_array()
            .expect("fund_managers should be array");
        assert!(!arr.is_empty(), "expected non-empty fund_managers");
    }

    #[test]
    fn collect_includes_fund_holders() {
        let raw = crate::collect::collect("002273.SZ", None, 4, None);
        let fm = raw["fund_managers"].as_array().expect("fund_managers should be array");
        assert!(!fm.is_empty(), "expected non-empty top-level fund_managers");
    }
}
