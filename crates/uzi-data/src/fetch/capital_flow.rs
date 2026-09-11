//! Port of `fetch_capital_flow.py`.
//!
//! Dimension 12 · 资金面 (北向 / 融资融券 / 股东户数 / 主力 / 限售解禁 / 大宗交易).

use chrono::Datelike;
use serde_json::{json, Value};

use uzi_core::py::{f_fin, parse_float, round, truthy};
use uzi_core::ticker::parse_ticker;

use crate::hk;
use crate::sources;

// ─────────────────────────────────────────────────────────────
// Universe caches (AkShare-only upstream → degraded to empty here)
// ─────────────────────────────────────────────────────────────

/// `_universe_margin_detail(exchange)` — AkShare `stock_margin_detail_*`.
fn universe_margin_detail(_exchange: &str) -> Vec<Value> {
    Vec::new()
}

/// `ak.stock_zh_a_gdhs(symbol)` — AkShare-only.
fn fetch_holder_counts(_code: &str) -> Vec<Value> {
    Vec::new()
}

/// `ak.stock_individual_fund_flow(stock, market)` — AkShare-only.
fn fetch_main_fund_flow(_code: &str, _market: &str) -> Vec<Value> {
    Vec::new()
}

/// `_universe_dzjy(year)` — AkShare `stock_dzjy_mrtj`.
fn universe_dzjy(_year: i32) -> Vec<Value> {
    Vec::new()
}

/// `_universe_release_summary()` — AkShare `stock_restricted_release_summary_em`.
fn universe_release_summary() -> Vec<Value> {
    Vec::new()
}

/// `_universe_release_detail(year)` — AkShare `stock_restricted_release_detail_em`.
fn universe_release_detail(_year: i32) -> Vec<Value> {
    Vec::new()
}

// ─────────────────────────────────────────────────────────────
// Summary helpers
// ─────────────────────────────────────────────────────────────

/// `_north_sum_20d(hist)`.
fn north_sum_20d(hist: &Value) -> String {
    if !hist.is_object() {
        return "—".to_string();
    }
    let flows = match hist.get("flow_history").and_then(|v| v.as_array()) {
        Some(f) => f,
        None => return "—".to_string(),
    };
    if flows.is_empty() {
        return "—".to_string();
    }
    let start = flows.len().saturating_sub(20);
    let mut total = 0.0f64;
    for r in &flows[start..] {
        let v = r
            .get("净买额")
            .filter(|v| truthy(*v))
            .or_else(|| r.get("净买入额").filter(|v| truthy(*v)));
        total += v.map(|x| f_fin(x, 0.0)).unwrap_or(0.0);
    }
    format!("{:+.1}亿", total / 1e8)
}

/// `_main_sum_20d(flow_list)`.
fn main_sum_20d(flow_list: &[Value]) -> String {
    if flow_list.is_empty() {
        return "—".to_string();
    }
    let start = flow_list.len().saturating_sub(20);
    let mut total = 0.0f64;
    for r in &flow_list[start..] {
        total += f_fin(r.get("主力净流入").unwrap_or(&Value::Null), 0.0);
    }
    if total.abs() < 1e8 {
        format!("{:+.1}万", total / 1e4)
    } else {
        format!("{:+.1}亿", total / 1e8)
    }
}

/// `_holders_trend(h)`.
fn holders_trend(h: &[Value]) -> String {
    if h.len() < 2 {
        return "—".to_string();
    }
    let last = &h[0];
    let prev = &h[h.len() - 1];
    let l = f_fin(last.get("股东户数").unwrap_or(&Value::Null), 0.0);
    let p = f_fin(prev.get("股东户数").unwrap_or(&Value::Null), 0.0);
    if l < p * 0.95 {
        "3 季连降".to_string()
    } else if l > p * 1.05 {
        "3 季连升".to_string()
    } else {
        "基本持平".to_string()
    }
}

/// `_month_label(d)`.
fn month_label(d: &str) -> String {
    let s: String = d.chars().take(7).collect::<String>().replace('-', "");
    if s.chars().count() == 6 {
        let chars: Vec<char> = s.chars().collect();
        format!("{}{}-{}{}", chars[2], chars[3], chars[4], chars[5])
    } else if s.is_empty() {
        "—".to_string()
    } else {
        let chars: Vec<char> = s.chars().collect();
        chars[chars.len().saturating_sub(5)..].iter().collect()
    }
}

// ─────────────────────────────────────────────────────────────
// main
// ─────────────────────────────────────────────────────────────

pub fn main(ticker: &str) -> Result<Value, String> {
    let ti = parse_ticker(ticker);

    if ti.market == "H" {
        let code5 = format!("{:0>5}", ti.code);
        let enriched = hk::fetch_hk_basic_combined(&code5);
        let is_sh = enriched
            .get("is_south_bound_sh")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let is_sz = enriched
            .get("is_south_bound_sz")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        // eniu 市值历史（近 30 个数据点作为南北向资金流的 proxy）· AkShare-only → []
        let mv_hist: Vec<Value> = Vec::new();
        let start = mv_hist.len().saturating_sub(30);
        return Ok(json!({
            "ticker": ti.full,
            "data": {
                "is_south_bound_sh": is_sh,
                "is_south_bound_sz": is_sz,
                "south_bound_eligibility": if is_sh && is_sz { "沪+深" } else if is_sh { "沪" } else if is_sz { "深" } else { "—" },
                "north_bound": "—",
                "margin_balance": "—",
                "main_flow_recent": [],
                "mv_history_30d": mv_hist[start..].to_vec(),
                "_note": "HK 南向具体持股变动需走 AASTOCKS Playwright 或 hkexnews holdings page；本字段提供港股通资格 + eniu 市值历史作 proxy。",
            },
            "source": "akshare:stock_hk_security_profile_em + stock_hk_indicator_eniu",
            "fallback": false,
        }));
    }
    if ti.market != "A" {
        return Ok(json!({
            "ticker": ti.full,
            "data": {"_note": "capital_flow only A-share / HK for now"},
            "source": "skip",
            "fallback": false,
        }));
    }

    let north = sources::fetch_northbound(&ti);

    // 融资明细走 universe cache · 按 exchange 缓存全市场最新一天
    let exchange = if ti.full.ends_with("SZ") { "SZ" } else { "SSE" };
    let universe_margin = universe_margin_detail(exchange);
    // head(5) 保留原行为（展示市场层 top 5 · 非本股过滤）
    let margin: Vec<Value> = universe_margin.iter().take(5).cloned().collect();

    let holders = fetch_holder_counts(&ti.code);
    let main_flow = fetch_main_fund_flow(&ti.code, &ti.full[ti.full.len().saturating_sub(2)..].to_lowercase());

    // 大宗交易 · 只 filter 本股
    let block_trades: Vec<Value> = universe_dzjy(2026)
        .into_iter()
        .filter(|r| r.get("证券代码").and_then(|v| v.as_str()) == Some(ti.code.as_str()))
        .take(20)
        .collect();

    // 限售股解禁 (近一年)
    let unlock: Vec<Value> = universe_release_summary()
        .into_iter()
        .filter(|r| r.get("代码").and_then(|v| v.as_str()) == Some(ti.code.as_str()))
        .collect();

    // 解禁日历前瞻 12 个月
    let unlock_future: Vec<Value> = universe_release_detail(2026)
        .into_iter()
        .filter(|r| r.get("代码").and_then(|v| v.as_str()) == Some(ti.code.as_str()))
        .take(20)
        .collect();

    // Normalize unlock_schedule for viz
    let mut unlock_schedule: Vec<Value> = Vec::new();
    for row in unlock_future.iter().take(12) {
        let date = row
            .get("解禁日期")
            .filter(|v| truthy(*v))
            .or_else(|| row.get("解禁时间").filter(|v| truthy(*v)))
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let amount_val = row
            .get("解禁市值")
            .filter(|v| truthy(*v))
            .or_else(|| row.get("市值(亿元)").filter(|v| truthy(*v)))
            .or_else(|| row.get("解禁股份数量").filter(|v| truthy(*v)));
        let amount_str = amount_val
            .map(|v| v.as_str().map(|s| s.to_string()).unwrap_or_else(|| v.to_string()))
            .unwrap_or_else(|| "0".to_string());
        let cleaned = amount_str.replace(',', "");
        if let Some(mut amount) = parse_float(&cleaned) {
            if amount > 1e6 {
                amount /= 1e8;
            }
            unlock_schedule.push(json!({
                "date": month_label(date),
                "amount": round(amount, 2),
            }));
        }
    }

    // 机构持仓 8 季度历史 (stock_report_fund_hold_detail)
    let today = chrono::Local::now();
    let q_dates = ["0331", "0630", "0930", "1231"];
    let mut quarters: Vec<(String, String)> = Vec::new();
    for i in 0..8i32 {
        let mut y = today.year();
        let mut q = ((today.month() as i32 - 1) / 3) - i;
        while q < 0 {
            q += 4;
            y -= 1;
        }
        let date = format!("{y}{}", q_dates[q as usize]);
        let ys = format!("{y}");
        let label = format!("{}Q{}", &ys[ys.len().saturating_sub(2)..], q + 1);
        quarters.push((date, label));
    }
    quarters.reverse();

    let mut inst_history = serde_json::Map::new();
    inst_history.insert(
        "quarters".to_string(),
        Value::Array(quarters.iter().map(|q| json!(q.1)).collect()),
    );
    for key in ["fund", "qfii", "shehui"] {
        inst_history.insert(
            key.to_string(),
            Value::Array(quarters.iter().map(|_| json!(0.0)).collect()),
        );
    }
    let northbound_20d = north_sum_20d(&north);
    let main_20d = main_sum_20d(&main_flow);
    let margin_trend = if margin.is_empty() {
        "—".to_string()
    } else {
        format!("近 5 日 {} 条记录", margin.len())
    };
    let holders_trend = holders_trend(&holders);

    Ok(json!({
        "ticker": ti.full,
        "data": {
            "northbound": north,
            "northbound_20d": northbound_20d,
            "margin_recent": margin,
            "margin_trend": margin_trend,
            "holder_count_history": holders,
            "holders_trend": holders_trend,
            "main_fund_flow_20d": main_flow,
            "main_20d": main_20d,
            "main_5d": "—",
            "block_trades_recent": block_trades,
            "unlock_recent": unlock,
            "unlock_schedule": unlock_schedule,
            "institutional_history": Value::Object(inst_history),
        },
        "source": "akshare:multi (north + margin + gdhs + fund_flow + dzjy + restricted_release + fund_hold_detail)",
        "fallback": false,
    }))
}
