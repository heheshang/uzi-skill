//! Port of `fetch_peers.py`.
//!
//! Upstream's A-share peer table comes from AkShare
//! `stock_board_industry_cons_em`, which resolves 板块名称 → 板块代码 through the
//! EastMoney board list (`fs=m:90 t:2 f:!50`) and then reads the constituent
//! cross-section (`fs=b:{code} f:!50`) — both are ported here against
//! `push2.eastmoney.com/api/qt/clist/get`, so `市盈率-动态` / `市净率` / `总市值`
//! stay real rather than being invented. The HK branch keeps the
//! `rank-in-universe` substitute; the XueQiu-playwright and `INDUSTRY_PEERS` tiers
//! are library-only and fall through to the documented self-only Tier 4 payload.

use std::time::Duration;

use serde_json::{json, Map, Value};

use uzi_core::py::{py_str, round, truthy};
use uzi_core::ticker::{parse_ticker, TickerInfo};

/// `_float(v, default=0.0)` — strips `,`/`%`, treats `""`/`nan`/`-`/`--`/`None`
/// as missing.
fn float_or(v: &Value, default: f64) -> f64 {
    let s = match v {
        Value::Null => return default,
        Value::Number(n) => return n.as_f64().unwrap_or(default),
        Value::Bool(_) => return default,
        Value::String(s) => s.clone(),
        _ => return default,
    };
    let cleaned: String = s.chars().filter(|c| *c != ',' && *c != '%').collect();
    if cleaned.is_empty() || matches!(cleaned.as_str(), "nan" | "-" | "--" | "None") {
        return default;
    }
    cleaned.parse::<f64>().unwrap_or(default)
}

fn num_of<'a>(v: Option<&'a Value>) -> &'a Value {
    static NULL: Value = Value::Null;
    v.unwrap_or(&NULL)
}

/// `_build_self_only_table(ti, basic)` — Tier 4 fallback, one row.
fn build_self_only_table(ti: &TickerInfo, basic: &Value) -> (Vec<Value>, Vec<Value>) {
    let name = basic
        .get("name")
        .filter(|v| truthy(v))
        .cloned()
        .unwrap_or_else(|| json!(ti.full));
    let pe = float_or(num_of(basic.get("pe_ttm")), 0.0);
    let pb = float_or(num_of(basic.get("pb")), 0.0);
    let mut row = Map::new();
    row.insert("name".into(), name);
    row.insert("code".into(), json!(ti.full));
    row.insert(
        "pe".into(),
        json!(if pe > 0.0 { format!("{pe:.1}") } else { "—".to_string() }),
    );
    row.insert(
        "pb".into(),
        json!(if pb > 0.0 { format!("{pb:.2}") } else { "—".to_string() }),
    );
    row.insert("roe".into(), json!("—"));
    row.insert("revenue_growth".into(), json!("—"));
    row.insert("is_self".into(), json!(true));
    (vec![Value::Object(row)], Vec::new())
}

/// `_parse_peer_df(df, self_ticker_code)` → `(peers_raw, peer_table, peer_comparison)`.
///
/// Upstream's AkShare-only per-row ROE supplement cannot run here, so every
/// `roe` stays `"—"` — which is exactly the value upstream leaves when that
/// optional call raises.
fn parse_peer_df(df: &[Value], self_code: &str, basic: &Value) -> (Vec<Value>, Vec<Value>, Vec<Value>) {
    let has_mcap = df.first().map(|r| r.get("总市值").is_some()).unwrap_or(false);

    let mut records: Vec<Value> = df
        .iter()
        .map(|r| {
            let mut o = r.as_object().cloned().unwrap_or_default();
            let mcap = if has_mcap {
                float_or(num_of(r.get("总市值")), 0.0)
            } else {
                0.0
            };
            o.insert("_mcap".into(), json!(mcap));
            Value::Object(o)
        })
        .collect();
    records.sort_by(|a, b| {
        let av = a.get("_mcap").and_then(|v| v.as_f64()).unwrap_or(0.0);
        let bv = b.get("_mcap").and_then(|v| v.as_f64()).unwrap_or(0.0);
        bv.partial_cmp(&av).unwrap_or(std::cmp::Ordering::Equal)
    });
    let raw: Vec<Value> = records.iter().take(20).cloned().collect();

    let mut self_row: Option<Value> = None;
    let mut peers_top5: Vec<Value> = Vec::new();
    for r in &raw {
        let code = r.get("代码").map(py_str).unwrap_or_default();
        let name = r.get("名称").cloned().unwrap_or_else(|| json!(""));
        let pe = float_or(num_of(r.get("市盈率-动态")), 0.0);
        let pb = float_or(num_of(r.get("市净率")), 0.0);
        let mut entry = Map::new();
        entry.insert("name".into(), name);
        entry.insert("code".into(), json!(code));
        entry.insert(
            "pe".into(),
            json!(if pe > 0.0 { format!("{pe:.1}") } else { "—".to_string() }),
        );
        entry.insert(
            "pb".into(),
            json!(if pb > 0.0 { format!("{pb:.2}") } else { "—".to_string() }),
        );
        entry.insert("roe".into(), json!("—"));
        entry.insert("revenue_growth".into(), json!("—"));
        if code == self_code {
            entry.insert("is_self".into(), json!(true));
            self_row = Some(Value::Object(entry));
        } else if peers_top5.len() < 5 {
            peers_top5.push(Value::Object(entry));
        }
    }

    let mut tbl: Vec<Value> = Vec::new();
    if let Some(s) = self_row {
        tbl.push(s);
    }
    tbl.extend(peers_top5);

    // ROE peers are all "—" here, so the mean is upstream's empty-average 0.0.
    let peer_roe_avg = 0.0f64;
    let self_roe = {
        let v = float_or(num_of(basic.get("roe")), 0.0);
        if v != 0.0 {
            json!(v)
        } else {
            Value::Null
        }
    };

    let avg = |col: &str| -> f64 {
        let vals: Vec<f64> = df
            .iter()
            .filter_map(|r| r.get(col))
            .map(|v| float_or(v, 0.0))
            .filter(|v| *v > 0.0)
            .collect();
        if vals.is_empty() {
            0.0
        } else {
            round(vals.iter().sum::<f64>() / vals.len() as f64, 2)
        }
    };

    let cmp = vec![
        json!({"name": "PE (越低越好)", "self": float_or(num_of(basic.get("pe_ttm")), 0.0), "peer": avg("市盈率-动态")}),
        json!({"name": "PB (越低越好)", "self": float_or(num_of(basic.get("pb")), 0.0), "peer": avg("市净率")}),
        json!({"name": "ROE (越高越好)", "self": self_roe, "peer": peer_roe_avg}),
    ];
    (raw, tbl, cmp)
}

/// `_attach_global_peers(data, ti, basic)`.
fn attach_global_peers(mut out: Map<String, Value>, ti: &TickerInfo, basic: &Value) -> Map<String, Value> {
    let disabled = std::env::var("UZI_DISABLE_GLOBAL_PEERS")
        .map(|v| v.trim().to_lowercase())
        .unwrap_or_default();
    if matches!(disabled.as_str(), "1" | "true" | "yes") {
        out.insert(
            "global_peer_comparison".into(),
            json!({"conclusion_status": "disabled", "peer_count": 0}),
        );
        return out;
    }
    if !(truthy(num_of(basic.get("name"))) && truthy(num_of(basic.get("industry")))) {
        out.insert(
            "global_peer_comparison".into(),
            json!({"conclusion_status": "insufficient_target_profile", "peer_count": 0}),
        );
        return out;
    }

    let raw_limit = std::env::var("UZI_GLOBAL_PEER_LIMIT").unwrap_or_else(|_| "8".to_string());
    let parsed: Result<i64, _> = raw_limit.trim().parse();
    let Ok(n) = parsed else {
        out.insert(
            "global_peer_comparison".into(),
            json!({
                "conclusion_status": "unavailable",
                "peer_count": 0,
                "error": format!(
                    "ValueError: invalid literal for int() with base 10: '{}'",
                    first_n(&raw_limit, 200)
                ),
            }),
        );
        return out;
    };
    let limit = n.clamp(3, 12) as usize;
    out.insert(
        "global_peer_comparison".into(),
        crate::global_peers::fetch_global_peer_comparison(ti, Some(basic), limit),
    );
    out
}

fn first_n(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

/// Board-list / board-constituents `ut` (the one AkShare's
/// `stock_board_industry_cons_em` uses).
const BOARD_UT: &str = "bd1d9ddb04089700cf9c27f6f7426281";
const CLIST: &str = "https://push2.eastmoney.com/api/qt/clist/get";

/// Shared `clist` fetch → the `data.diff` rows as an array.
fn clist(fs: &str, fields: &str) -> Vec<Value> {
    let params: Vec<(&str, &str)> = vec![
        ("pn", "1"),
        ("pz", "6000"),
        ("po", "1"),
        ("np", "1"),
        ("ut", BOARD_UT),
        ("fltt", "2"),
        ("invt", "2"),
        ("fid", "f3"),
        ("fs", fs),
        ("fields", fields),
    ];
    let Ok(v) = crate::http::get_json_q(CLIST, &params, &[], 15) else {
        return Vec::new();
    };
    match v.get("data").and_then(|d| d.get("diff")) {
        Some(Value::Array(a)) => a.clone(),
        Some(Value::Object(o)) => o.values().cloned().collect(),
        _ => Vec::new(),
    }
}

/// `__stock_board_industry_name_em` — 板块名称 → 板块代码 (`None` = the upstream
/// `KeyError` when the industry has no board).
fn board_code(industry: &str) -> Option<String> {
    let rows = clist("m:90 t:2 f:!50", "f12,f14");
    rows.iter()
        .find(|r| r.get("f14").and_then(|v| v.as_str()) == Some(industry))
        .and_then(|r| r.get("f12"))
        .map(uzi_core::py::py_str)
}

/// `ak.stock_board_industry_cons_em(symbol)` — board constituents renamed to
/// AkShare's column names, in AkShare's order.
fn board_constituents(bk: &str) -> Vec<Value> {
    let fs = format!("b:{bk} f:!50");
    let rows = clist(&fs, "f12,f14,f9,f23,f20");
    rows.iter()
        .map(|r| {
            let cell = |k: &str| match r.get(k) {
                None | Some(Value::Null) => Value::Null,
                Some(Value::String(s)) if s == "-" || s.is_empty() => Value::Null,
                Some(v) => v.clone(),
            };
            json!({
                "代码": r.get("f12").cloned().unwrap_or(Value::Null),
                "名称": r.get("f14").cloned().unwrap_or(Value::Null),
                "市盈率-动态": cell("f9"),
                "市净率": cell("f23"),
                "总市值": cell("f20"),
            })
        })
        .collect()
}

/// The rows `stock_board_industry_cons_em` yields for `industry` (empty when
/// the board list or constituents are unreachable — upstream's exception path).
fn industry_rows(industry: &str) -> Vec<Value> {
    match board_code(industry) {
        Some(bk) => board_constituents(&bk),
        None => Vec::new(),
    }
}

/// The HK branch (`ti.market == "H"`).
fn main_hk(ti: &TickerInfo, basic: &Value, industry: &str) -> Value {
    let ranks = basic.get("_ranks").cloned().unwrap_or_else(|| json!({}));
    let val = ranks.get("valuation").cloned().unwrap_or_else(|| json!({}));
    let scale = ranks.get("scale").cloned().unwrap_or_else(|| json!({}));
    let growth = ranks.get("growth").cloned().unwrap_or_else(|| json!({}));

    let pe_v = val.get("pe_ttm").cloned().unwrap_or_else(|| json!(0));
    let pb_v = val.get("pb_mrq").cloned().unwrap_or_else(|| json!(0));
    let rg_v = growth.get("revenue_yoy").cloned().unwrap_or_else(|| json!(0));

    let mut self_row = Map::new();
    self_row.insert(
        "name".into(),
        basic
            .get("name")
            .filter(|v| truthy(v))
            .cloned()
            .unwrap_or_else(|| json!(ti.full)),
    );
    self_row.insert("code".into(), json!(ti.full));
    self_row.insert(
        "pe".into(),
        json!(if truthy(&pe_v) { format!("{:.1}", float_or(&pe_v, 0.0)) } else { "—".to_string() }),
    );
    self_row.insert(
        "pb".into(),
        json!(if truthy(&pb_v) { format!("{:.2}", float_or(&pb_v, 0.0)) } else { "—".to_string() }),
    );
    self_row.insert("roe".into(), json!("—"));
    self_row.insert(
        "revenue_growth".into(),
        json!(if truthy(&rg_v) { format!("{:.1}%", float_or(&rg_v, 0.0)) } else { "—".to_string() }),
    );
    self_row.insert("is_self".into(), json!(true));

    let peer_comparison = vec![
        json!({"name": "PE-TTM 排名 (HK 全市场)", "self": val.get("pe_ttm_rank").cloned().unwrap_or(Value::Null), "peer": "—"}),
        json!({"name": "PB-MRQ 排名 (HK 全市场)", "self": val.get("pb_mrq_rank").cloned().unwrap_or(Value::Null), "peer": "—"}),
        json!({"name": "总市值排名 (HK 全市场)", "self": scale.get("market_cap_rank").cloned().unwrap_or(Value::Null), "peer": "—"}),
        json!({"name": "营收 YoY 排名", "self": growth.get("revenue_yoy_rank").cloned().unwrap_or(Value::Null), "peer": "—"}),
    ];

    let mcap_rank = scale.get("market_cap_rank").cloned().unwrap_or(Value::Null);
    let rank_str = if truthy(&mcap_rank) {
        format!("HK 第 {} 位（按总市值）", py_str(&mcap_rank))
    } else {
        "—".to_string()
    };

    let mut data = Map::new();
    data.insert(
        "industry".into(),
        json!(if industry.is_empty() {
            "未分类（akshare HK 无行业聚合）"
        } else {
            industry
        }),
    );
    data.insert("self".into(), basic.clone());
    data.insert("peer_table".into(), Value::Array(vec![Value::Object(self_row)]));
    data.insert("peer_comparison".into(), Value::Array(peer_comparison));
    data.insert("rank".into(), json!(rank_str));
    data.insert("peers_top20_raw".into(), json!([]));
    data.insert(
        "_note".into(),
        json!("HK peer LIST 需走 AASTOCKS Playwright 或问财；本字段提供 rank-in-universe 作替代"),
    );

    let data = attach_global_peers(data, ti, basic);
    json!({
        "ticker": ti.full,
        "data": Value::Object(data),
        "source": "akshare:hk_valuation_comparison_em + scale_comparison_em + growth_comparison_em",
        "fallback": false,
    })
}

pub fn main(ticker: &str) -> Result<Value, String> {
    let ti = parse_ticker(ticker);
    let mut basic = crate::sources::fetch_basic(&ti);

    // `_fetch_basic_hk` merges the HK valuation ranks; the crate's HK basic
    // chain carries them separately, so fold `_ranks` back in.
    if ti.market == "H" && basic.get("_ranks").is_none() {
        let code5 = format!("{:0>5}", ti.code);
        let combined = crate::hk::fetch_hk_basic_combined(&code5);
        if let Some(ranks) = combined.get("_ranks").cloned() {
            if let Some(o) = basic.as_object_mut() {
                o.insert("_ranks".into(), ranks);
            }
        }
    }

    let industry = basic
        .get("industry")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    if ti.market == "H" {
        return Ok(main_hk(&ti, &basic, &industry));
    }

    let mut fallback_used = false;
    let mut fallback_reason = String::new();
    let mut source_used = "akshare:stock_board_industry_cons_em".to_string();
    let mut peers_raw: Vec<Value> = Vec::new();
    let mut peer_table: Vec<Value> = Vec::new();
    let mut peer_comparison: Vec<Value> = Vec::new();

    if ti.market == "A" && industry.is_empty() {
        let (t, c) = build_self_only_table(&ti, &basic);
        peer_table = t;
        peer_comparison = c;
        fallback_used = true;
        fallback_reason = "basic.industry 缺失 · 仅返回公司自身".to_string();
        source_used.push_str(" (missing-industry self-only fallback)");
    } else if ti.market == "A" {
        // Tier 1 — industry board constituents.
        let rows = industry_rows(&industry);
        if !rows.is_empty() {
            let (rw, t, c) = parse_peer_df(&rows, &ti.code, &basic);
            peers_raw = rw;
            peer_table = t;
            peer_comparison = c;
        }

        // Tier 2 — one retry after a short pause.
        if peer_table.is_empty() {
            std::thread::sleep(Duration::from_secs_f64(2.5));
            let rows = industry_rows(&industry);
            if !rows.is_empty() {
                let (rw, t, c) = parse_peer_df(&rows, &ti.code, &basic);
                peers_raw = rw;
                peer_table = t;
                peer_comparison = c;
                fallback_used = true;
                fallback_reason = "Tier 1 网络失败 · Tier 2 retry 成功".to_string();
                source_used.push_str(" (retry)");
            }
        }

        // Tier 3 (XueQiu playwright) and Tier 3.5 (INDUSTRY_PEERS + AkShare
        // financial indicator) are library-only and skipped.

        // Tier 4 — self-only guarantee.
        if peer_table.is_empty() {
            let (t, c) = build_self_only_table(&ti, &basic);
            peer_table = t;
            peer_comparison = c;
            fallback_used = true;
            if fallback_reason.is_empty() {
                fallback_reason = "所有同行数据源失败 · 仅返回公司自身".to_string();
            }
            source_used.push_str(" (self-only fallback)");
        }
    }

    let mut local = Map::new();
    local.insert("industry".into(), json!(industry));
    local.insert("self".into(), basic.clone());
    local.insert("peer_table".into(), Value::Array(peer_table));
    local.insert("peer_comparison".into(), Value::Array(peer_comparison));
    local.insert("rank".into(), json!("—"));
    local.insert(
        "peers_top20_raw".into(),
        Value::Array(peers_raw.into_iter().take(20).collect()),
    );
    local.insert("fallback_reason".into(), json!(fallback_reason));
    let local = attach_global_peers(local, &ti, &basic);

    Ok(json!({
        "ticker": ti.full,
        "data": Value::Object(local),
        "source": source_used,
        "fallback": fallback_used,
    }))
}
