//! Port of `fetch_contests.py`.

use std::sync::LazyLock;
use std::time::Duration;

use regex::Regex;
use serde_json::{json, Map, Value};

use crate::http;
use uzi_core::cache::cached;
use uzi_core::py;
use uzi_core::ticker::{parse_ticker, TickerInfo};

const UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36";

// ─────────────────────────────────────────────────────────────
// 1. 雪球 cubes (most reliable)
// ─────────────────────────────────────────────────────────────

/// Convert ticker to xueqiu symbol format: SH600519 / SZ002273 / HK00700 / AAPL.
fn xq_symbol(ti: &TickerInfo) -> String {
    match ti.market.as_str() {
        "A" => {
            let chars: Vec<char> = ti.full.chars().collect();
            let suffix: String = chars[chars.len().saturating_sub(2)..].iter().collect();
            format!("{suffix}{}", ti.code)
        }
        "H" => format!("HK{:0>5}", ti.code),
        _ => ti.code.clone(),
    }
}

/// Normalize raw xueqiu cube dicts to our schema.
fn normalize_cubes(cubes: &[Value]) -> Vec<Value> {
    let mut out = Vec::new();
    for c in cubes {
        if !c.is_object() {
            continue;
        }
        let symbol = c.get("symbol").cloned().unwrap_or(Value::Null);
        let url = if py::truthy(&symbol) {
            json!(format!("https://xueqiu.com/P/{}", py::py_str(&symbol)))
        } else {
            Value::Null
        };
        let owner = c
            .get("owner")
            .filter(|o| o.is_object())
            .and_then(|o| o.get("screen_name"))
            .cloned()
            .unwrap_or(Value::Null);
        out.push(json!({
            "name": c.get("name").cloned().unwrap_or(Value::Null),
            "owner": owner,
            "symbol": symbol,
            "daily_gain": c.get("daily_gain").cloned().unwrap_or(Value::Null),
            "monthly_gain": c.get("monthly_gain").cloned().unwrap_or(Value::Null),
            "total_gain": c.get("total_gain").cloned().unwrap_or(Value::Null),
            "annualized_gain_rate": c.get("annualized_gain_rate").cloned().unwrap_or(Value::Null),
            "url": url,
            "stocks_count": c.get("stocks_count").cloned().unwrap_or(Value::Null),
            "view_rebalancing_count": c.get("view_rebalancing_count").cloned().unwrap_or(Value::Null),
        }));
    }
    out
}

/// Returns `(cubes, meta)` where meta has `{http_status, source, login_required}`.
fn fetch_xueqiu_cubes(ti: &TickerInfo, limit: usize) -> (Vec<Value>, Value) {
    let symbol = xq_symbol(ti);
    let url = format!(
        "https://xueqiu.com/query/v1/search/cube/stock.json?q={symbol}&count={limit}&page=1"
    );
    let headers = [("User-Agent", UA), ("Referer", "https://xueqiu.com/")];
    let mut meta = json!({"http_status": Value::Null, "source": "http", "login_required": false});

    match http::get(&url, &headers, 15) {
        Ok(resp) => {
            meta["http_status"] = json!(resp.status);
            if resp.status == 200 {
                match resp.json() {
                    Some(data) if data.is_object() => {
                        let list = data
                            .get("list")
                            .filter(|v| py::truthy(v))
                            .or_else(|| data.get("cubes").filter(|v| py::truthy(v)))
                            .cloned()
                            .unwrap_or_else(|| json!([]));
                        let cubes: &[Value] =
                            list.as_array().map(|a| a.as_slice()).unwrap_or(&[]);
                        let out = normalize_cubes(cubes);
                        if !out.is_empty() {
                            return (out, meta);
                        }
                    }
                    Some(_) => {
                        meta["http_error"] =
                            json!("AttributeError: 'list' object has no attribute 'get'");
                    }
                    None => {
                        meta["http_error"] =
                            json!("ValueError: Expecting value: line 1 column 1 (char 0)");
                    }
                }
            }
        }
        Err(e) => {
            meta["http_error"] = json!(format!(
                "HTTPError: {}",
                e.chars().take(120).collect::<String>()
            ));
        }
    }

    // lib.xueqiu_browser (Playwright) is a Python-only login helper. The default
    // environment has UZI_XQ_LOGIN unset, so upstream records login-required and
    // returns no cubes; degrade identically instead of fabricating holdings.
    meta["login_required"] = json!(true);
    meta["hint"] =
        json!("set UZI_XQ_LOGIN=1 + run `python -m lib.xueqiu_browser login` (one-time)");
    (Vec::new(), meta)
}

// ─────────────────────────────────────────────────────────────
// 2. 淘股吧 (stock thread search; player ranking is behind login)
// ─────────────────────────────────────────────────────────────

/// Search 淘股吧 for threads mentioning this ticker.
fn fetch_tgb_mentions(ti: &TickerInfo) -> Vec<Value> {
    static RE: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r#"<a[^>]+href="(/Article/\d+/\d+)"[^>]*>([^<]{4,80})</a>"#).unwrap()
    });
    let url = format!("https://www.taoguba.com.cn/Article/list/all?keyword={}", ti.code);
    match http::get(&url, &[("User-Agent", UA)], 15) {
        Ok(resp) => {
            if resp.status != 200 {
                return Vec::new();
            }
            let html = resp.text();
            RE.captures_iter(&html)
                .take(30)
                .map(|cap| {
                    json!({
                        "title": cap[2].trim(),
                        "url": format!("https://www.taoguba.com.cn{}", &cap[1]),
                    })
                })
                .collect()
        }
        Err(e) => vec![json!({"error": format!("tgb fetch failed: {e}")})],
    }
}

// ─────────────────────────────────────────────────────────────
// 3. 同花顺模拟炒股 (public leaderboards)
// ─────────────────────────────────────────────────────────────

fn fetch_ths_simu(ti: &TickerInfo) -> Vec<Value> {
    static RE: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r#"<a[^>]+class="user[^"]*"[^>]*>([^<]+)</a>.*?(\d+\.\d+)%"#).unwrap()
    });
    let url = format!("https://moni.10jqka.com.cn/holder/?stock={}", ti.code);
    let headers = [
        ("User-Agent", UA),
        ("Referer", "https://moni.10jqka.com.cn/"),
    ];
    match http::get(&url, &headers, 12) {
        Ok(resp) => {
            let html = resp.text();
            if resp.status != 200 || html.contains("Just a moment") {
                return vec![json!({"note": "ths simu endpoint requires login or blocked"})];
            }
            RE.captures_iter(&html)
                .take(20)
                .map(|cap| {
                    let pct = cap[2].parse::<f64>().unwrap_or(0.0);
                    json!({"nickname": &cap[1], "return_pct": pct})
                })
                .collect()
        }
        Err(e) => vec![json!({"error": format!("ths simu fetch failed: {e}")})],
    }
}

// ─────────────────────────────────────────────────────────────
// 4. 大盘手网期货实盘大赛 (only if related futures exist)
// ─────────────────────────────────────────────────────────────

fn fetch_dpswang() -> Vec<Value> {
    match http::get("https://www.dpswang.com/match/list", &[("User-Agent", UA)], 12) {
        Ok(resp) if resp.status == 200 => {
            vec![json!({"note": "dpswang reachable, detailed holdings require player-page scrape"})]
        }
        _ => Vec::new(),
    }
}

// ─────────────────────────────────────────────────────────────
// Aggregator
// ─────────────────────────────────────────────────────────────

fn summarize(xq_cubes: &[Value], tgb: &[Value], ths: &[Value]) -> Map<String, Value> {
    let mut high_return = 0i64;
    let mut s_tier = 0i64;
    let mut a_tier = 0i64;
    let mut b_tier = 0i64;
    for c in xq_cubes {
        if c.get("error").is_some() {
            continue;
        }
        let tg = py::f(c.get("total_gain").unwrap_or(&Value::Null), 0.0);
        if tg > 50.0 {
            high_return += 1;
        }
        if tg > 200.0 {
            s_tier += 1;
        } else if tg > 100.0 {
            a_tier += 1;
        } else if tg > 50.0 {
            b_tier += 1;
        }
    }
    let mut m = Map::new();
    m.insert(
        "xueqiu_cubes_total".into(),
        json!(xq_cubes.iter().filter(|c| c.get("error").is_none()).count()),
    );
    m.insert("high_return_cubes".into(), json!(high_return));
    m.insert("s_tier_holders".into(), json!(s_tier));
    m.insert("a_tier_holders".into(), json!(a_tier));
    m.insert("b_tier_holders".into(), json!(b_tier));
    m.insert(
        "tgb_mentions_count".into(),
        json!(tgb.iter().filter(|t| t.get("error").is_none()).count()),
    );
    m.insert(
        "ths_evidence_count".into(),
        json!(ths
            .iter()
            .filter(|t| t.get("error").is_none() && t.get("note").is_none())
            .count()),
    );
    m
}

pub fn main(ticker: &str) -> Result<Value, String> {
    let ti = parse_ticker(ticker);

    // v2.7.1 · cached + (cubes, meta) signature
    let ti_xq = ti.clone();
    let xq_result = cached::<_, anyhow::Error>(
        &ti.full,
        &format!("xq_cubes__{}", ti.code),
        6 * 3600,
        move || {
            let (cubes, meta) = fetch_xueqiu_cubes(&ti_xq, 50);
            Ok(json!({"cubes": cubes, "meta": meta}))
        },
    )
    .unwrap_or_else(|_| json!({}));
    let xq = if xq_result.is_object() {
        xq_result.get("cubes").cloned().unwrap_or_else(|| json!([]))
    } else {
        json!([])
    };
    let xq_meta = if xq_result.is_object() {
        xq_result.get("meta").cloned().unwrap_or_else(|| json!({}))
    } else {
        json!({})
    };

    std::thread::sleep(Duration::from_millis(500));
    let ti_tgb = ti.clone();
    let tgb = cached::<_, anyhow::Error>(
        &ti.full,
        &format!("tgb__{}", ti.code),
        12 * 3600,
        move || Ok(json!(fetch_tgb_mentions(&ti_tgb))),
    )
    .unwrap_or_else(|_| json!([]));
    std::thread::sleep(Duration::from_millis(500));
    let ti_ths = ti.clone();
    let ths = cached::<_, anyhow::Error>(
        &ti.full,
        &format!("ths_simu__{}", ti.code),
        12 * 3600,
        move || Ok(json!(fetch_ths_simu(&ti_ths))),
    )
    .unwrap_or_else(|_| json!([]));
    let dps = json!(fetch_dpswang());

    let xq_arr = xq.as_array().cloned().unwrap_or_default();
    let tgb_arr = tgb.as_array().cloned().unwrap_or_default();
    let ths_arr = ths.as_array().cloned().unwrap_or_default();

    let mut summary = summarize(&xq_arr, &tgb_arr, &ths_arr);
    let login_required = py::truthy(xq_meta.get("login_required").unwrap_or(&Value::Null));
    let xq_source = xq_meta
        .get("source")
        .cloned()
        .unwrap_or_else(|| json!("http"));
    summary.insert("xueqiu_login_required".into(), json!(login_required));
    summary.insert("xueqiu_source".into(), xq_source);

    let note = if login_required && xq_arr.is_empty() {
        "⚠️ XueQiu cubes 接口 2026 起需登录。当前未启用 Playwright 登录，0 cube 收录。\
如需启用：export UZI_XQ_LOGIN=1 然后 python -m lib.xueqiu_browser login 一次性登录。\
或 --skip-login-sources 接受跳过（其他维度不影响）。"
    } else {
        "雪球 cubes API 是主数据源；其余 3 站点 ≥1 失败时 Claude 用 fallback_queries 补足"
    };

    let fallback = login_required && xq_arr.is_empty();

    Ok(json!({
        "ticker": ti.full,
        "data": {
            "xueqiu_cubes": xq,
            "xueqiu_meta": xq_meta,
            "tgb_mentions": tgb,
            "ths_simu": ths,
            "dpswang": dps,
            "summary": Value::Object(summary),
            "fallback_queries": [
                format!("淘股吧 实盘 {} 持仓", ti.code),
                format!("挑战者杯 {}", ti.code),
                format!("雪球 实盘组合 {} 50%", ti.code),
                format!("全国期货实盘大赛 {}", ti.code),
            ],
            "_note": note,
        },
        "source": "xueqiu + taoguba + 10jqka + dpswang",
        "fallback": fallback,
    }))
}

#[cfg(test)]
mod tests {
    use super::summarize;
    use serde_json::json;

    #[test]
    fn summarize_tier_boundaries_match_upstream() {
        let xq = vec![
            json!({"total_gain": 250}), // s tier + high return
            json!({"total_gain": 150}), // a tier + high return
            json!({"total_gain": 60}),  // b tier + high return
            json!({"total_gain": 50}),  // not > 50
            json!({"error": "x"}),      // excluded
        ];
        let tgb = vec![json!({"title": "a"}), json!({"error": "e"})];
        let ths = vec![
            json!({"nickname": "n", "return_pct": 1.0}),
            json!({"note": "blocked"}),
            json!({"error": "e"}),
        ];
        let m = summarize(&xq, &tgb, &ths);
        assert_eq!(m["xueqiu_cubes_total"], json!(4));
        assert_eq!(m["high_return_cubes"], json!(3));
        assert_eq!(m["s_tier_holders"], json!(1));
        assert_eq!(m["a_tier_holders"], json!(1));
        assert_eq!(m["b_tier_holders"], json!(1));
        assert_eq!(m["tgb_mentions_count"], json!(1));
        assert_eq!(m["ths_evidence_count"], json!(1));
    }
}
