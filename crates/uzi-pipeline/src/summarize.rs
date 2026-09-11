//! Port of `lib/pipeline/score_fns.py::_auto_summarize_dim` — builds a readable
//! paragraph from raw dimension fields so a direct run (no agent) still yields an
//! informative report. Never returns a placeholder string.

use serde_json::{json, Value};
use uzi_core::py::truthy;

/// `x not in (None, "", "—", "-", [], {})` — Python's `in` uses `==`, so `0` and
/// `false` are NOT excluded here (only the listed sentinels).
fn is_sentinel(v: &Value) -> bool {
    match v {
        Value::Null => true,
        Value::String(s) => matches!(s.as_str(), "" | "—" | "-"),
        Value::Array(a) => a.is_empty(),
        Value::Object(o) => o.is_empty(),
        _ => false,
    }
}

/// `_v(*keys, default="—")` — first key whose value is not a sentinel.
fn pick<'a>(data: &'a Value, keys: &[&str], default: &'a str) -> Value {
    for k in keys {
        if let Some(v) = data.get(*k) {
            if !is_sentinel(v) {
                return v.clone();
            }
        }
    }
    json!(default)
}

/// `data.get(k) or {}` for the nested objects this summarizer reads.
fn sub<'a>(data: &'a Value, key: &str) -> Value {
    match data.get(key) {
        Some(v) if truthy(v) => v.clone(),
        _ => json!({}),
    }
}

/// `_join_list(lst, max_n, sep)` — `{title|name|date|str(x)}` truncated to 50 chars.
fn join_list(lst: &Value, max_n: usize, sep: &str) -> Option<String> {
    let arr = lst.as_array()?;
    if arr.is_empty() {
        return None;
    }
    let parts: Vec<String> = arr
        .iter()
        .take(max_n)
        .map(|x| {
            let text = match x {
                Value::Object(_) => {
                    let t = x
                        .get("title")
                        .filter(|v| truthy(v))
                        .or_else(|| x.get("name").filter(|v| truthy(v)))
                        .or_else(|| x.get("date").filter(|v| truthy(v)));
                    match t {
                        Some(v) => uzi_core::py::py_str(v),
                        None => uzi_core::py::py_str(x),
                    }
                }
                other => uzi_core::py::py_str(other),
            };
            text.chars().take(50).collect::<String>()
        })
        .collect();
    Some(parts.join(sep))
}

/// Python `f"{v}"` on a JSON value.
fn s(v: &Value) -> String {
    uzi_core::py::py_display(v)
}

/// True for payloads stamped by the crypto data layer (`asset_class = "crypto"`).
fn is_crypto_data(data: &Value) -> bool {
    data.get("asset_class").and_then(|v| v.as_str()) == Some("crypto")
}

/// Build a one-paragraph commentary from `raw_data` fields for one dimension.
pub fn auto_summarize_dim(dim_key: &str, label: &str, dim: &Value, score: f64) -> String {
    if !dim.is_object() {
        return String::new();
    }
    let data = dim.get("data").cloned().unwrap_or(json!({}));
    if !truthy(&data) {
        return format!("{}：未拉取到数据（fetcher 失败或返回空）。", label);
    }

    match dim_key {
        // ── Crypto venue · crypto-native fields ──
        "0_basic" if is_crypto_data(&data) => format!(
            "{}：{}（{}），{} 赛道。价格 {}，市值 {}，市值排名 #{}，24h 成交额 {}。",
            label,
            s(&pick(&data, &["name"], "—")),
            s(&pick(&data, &["code"], "—")),
            s(&pick(&data, &["industry"], "—")),
            s(&pick(&data, &["price"], "—")),
            s(&pick(&data, &["market_cap"], "—")),
            s(&pick(&data, &["market_cap_rank"], "—")),
            s(&pick(&data, &["volume_24h"], "—")),
        ),
        "1_financials" if is_crypto_data(&data) => format!(
            "{}：流通率 {}%，FDV/市值 {}，供应模型 {}。得分 {}/10。",
            label,
            s(&pick(&data, &["circulating_ratio_pct"], "—")),
            s(&pick(&data, &["fdv_to_mcap"], "—")),
            s(&pick(&data, &["supply_model"], "—")),
            format_score(score),
        ),
        "3_macro" if is_crypto_data(&data) => format!(
            "{}：加密总市值 24h {}%，BTC 占比 {}%，ETH 占比 {}%，恐慌贪婪 {}。得分 {}/10。",
            label,
            s(&pick(&data, &["mcap_change_24h_pct"], "—")),
            s(&pick(&data, &["btc_dominance_pct"], "—")),
            s(&pick(&data, &["eth_dominance_pct"], "—")),
            s(&pick(&data, &["fear_greed"], "—")),
            format_score(score),
        ),
        "4_peers" if is_crypto_data(&data) => {
            let peer_table = data
                .get("peer_table")
                .and_then(|v| v.as_array())
                .cloned()
                .unwrap_or_default();
            let names: Vec<Value> = peer_table
                .iter()
                .filter(|p| p.is_object() && p.get("is_self").and_then(|v| v.as_bool()) != Some(true))
                .take(5)
                .map(|p| p.get("name").cloned().unwrap_or(Value::Null))
                .collect();
            let peers_str = join_list(&Value::Array(names), 5, "、");
            format!(
                "{}：市值排名 #{}，{}。得分 {}/10。",
                label,
                s(&pick(&data, &["rank"], "—")),
                match peers_str {
                    Some(p) => format!("主要同行：{}", p),
                    None => "无同业样本".to_string(),
                },
                format_score(score),
            )
        }
        "7_industry" if is_crypto_data(&data) => format!(
            "{}：赛道 {}，市值占比 {}%，板块市值 {}。",
            label,
            s(&pick(&data, &["industry"], "—")),
            s(&pick(&data, &["market_share_pct"], "—")),
            s(&pick(&data, &["sector_market_cap"], "—")),
        ),
        "10_valuation" if is_crypto_data(&data) => format!(
            "{}：NVT {}，日换手 {}，区间位置 {}%，距 ATH {}%。得分 {}/10。",
            label,
            s(&pick(&data, &["nvt_ratio"], "—")),
            s(&pick(&data, &["turnover_ratio"], "—")),
            s(&pick(&data, &["price_range_position_pct"], "—")),
            s(&pick(&data, &["ath_drawdown_pct"], "—")),
            format_score(score),
        ),
        "17_sentiment" if is_crypto_data(&data) => format!(
            "{}：恐慌贪婪指数 {}（{}），看多占比 {}%，热搜第 {} 位。",
            label,
            s(&pick(&data, &["thermometer_value"], "—")),
            s(&pick(&data, &["sentiment_label"], "—")),
            s(&pick(&data, &["positive_pct"], "—")),
            s(&pick(&data, &["trending_rank"], "—")),
        ),
        "18_trap" if is_crypto_data(&data) => {
            let signals = data
                .get("pump_dump_signals")
                .and_then(|v| v.as_array())
                .cloned()
                .unwrap_or_default();
            format!(
                "{}：风险分 {}/100 · {}。{}",
                label,
                s(&pick(&data, &["risk_score"], "—")),
                s(&pick(&data, &["trap_level"], "—")),
                match join_list(&Value::Array(signals), 3, "；") {
                    Some(sig) => format!("信号：{}", sig),
                    None => "未触发拉盘/流动性信号".to_string(),
                }
            )
        }
        "0_basic" => format!(
            "{}：{}（{}），{} 行业。市值 {}，PE {}，PB {}。",
            label,
            s(&pick(&data, &["name"], "—")),
            s(&pick(&data, &["code"], "—")),
            s(&pick(&data, &["industry"], "—")),
            s(&pick(&data, &["market_cap"], "—")),
            s(&pick(&data, &["pe_ttm"], "—")),
            s(&pick(&data, &["pb"], "—")),
        ),
        "1_financials" => format!(
            "{}：ROE {}，营收同比 {}，净利同比 {}，净利率 {}。综合得分 {}/10。",
            label,
            s(&pick(&data, &["roe_latest", "roe"], "—")),
            s(&pick(&data, &["revenue_growth_yoy", "revenue_yoy"], "—")),
            s(&pick(&data, &["net_profit_yoy"], "—")),
            s(&pick(&data, &["net_margin", "gross_margin"], "—")),
            format_score(score),
        ),
        "2_kline" => format!(
            "{}：{} · 均线 {} · MACD {}。",
            label,
            s(&pick(&data, &["stage", "wyckoff_stage"], "—")),
            s(&pick(&data, &["ma_align", "trend"], "—")),
            s(&pick(&data, &["macd"], "—")),
        ),
        "3_macro" => format!(
            "{}：利率周期 {}；汇率 {}；地缘 {}；大宗商品 {}。得分 {}/10。",
            label,
            s(&pick(&data, &["rate_cycle"], "—")),
            s(&pick(&data, &["fx_trend"], "—")),
            s(&pick(&data, &["geo_risk"], "—")),
            s(&pick(&data, &["commodity", "commodity_trend"], "—")),
            format_score(score),
        ),
        "4_peers" => {
            let rank = pick(&data, &["rank"], "—");
            let peer_table = data
                .get("peer_table")
                .and_then(|v| v.as_array())
                .cloned()
                .unwrap_or_default();
            let names: Vec<Value> = peer_table
                .iter()
                .filter(|p| p.is_object() && p.get("is_self").and_then(|v| v.as_bool()) != Some(true))
                .take(5)
                .map(|p| p.get("name").cloned().unwrap_or(Value::Null))
                .collect();
            let peers_str = join_list(&Value::Array(names), 5, "、");
            format!(
                "{}：{} 行业，{}{}。得分 {}/10。",
                label,
                s(&pick(&data, &["industry"], "—")),
                s(&rank),
                match peers_str {
                    Some(p) => format!("，主要同行：{}", p),
                    None => String::new(),
                },
                format_score(score),
            )
        }
        "5_chain" => format!(
            "{}：上游 {}；下游 {}；客户集中度 {}。",
            label,
            s(&pick(&data, &["upstream"], "—")),
            s(&pick(&data, &["downstream"], "—")),
            s(&pick(&data, &["client_concentration"], "—")),
        ),
        "6_research" => format!(
            "{}：近期券商研报 {} 篇，一致评级 {}，目标价均值 {}。",
            label,
            s(&pick(&data, &["report_count", "n_reports"], "—")),
            s(&pick(&data, &["consensus_rating", "rating"], "—")),
            s(&pick(&data, &["avg_target_price", "target_price"], "—")),
        ),
        "7_industry" => {
            let cninfo = sub(&data, "cninfo_metrics");
            // upstream: `_v(...) or (data.get("cninfo_metrics") or {}).get(...)`
            // `_v`'s "—" default is truthy, so the fallback only fires for falsy picks
            let pick_fallback = |keys: &[&str], ckey: &str| -> Value {
                let primary = pick(&data, keys, "—");
                if truthy(&primary) {
                    primary
                } else {
                    pick_or_none(&cninfo, &[ckey])
                }
            };
            let ind_pe = pick_fallback(&["industry_pe_weighted"], "industry_pe_weighted");
            let ind_count = pick_fallback(&["total_companies"], "company_count");
            format!(
                "{}：所属 {} · 行业 PE 加权 {} · 上市公司数 {} · 增速 {}。",
                label,
                s(&pick(&data, &["industry"], "—")),
                s(&ind_pe),
                s(&ind_count),
                s(&pick(&data, &["growth"], "—")),
            )
        }
        "8_materials" => format!(
            "{}：核心原料 {}；近期价格走势 {}；占成本比例 {}。",
            label,
            s(&pick(&data, &["core_material"], "—")),
            s(&pick(&data, &["price_trend"], "—")),
            s(&pick(&data, &["cost_share"], "—")),
        ),
        "9_futures" => format!(
            "{}：关联合约 {}；近期走势 {}；{}。",
            label,
            s(&pick(&data, &["linked_contract"], "—")),
            s(&pick(&data, &["contract_trend"], "—")),
            s(&pick(&data, &["note"], "")),
        ),
        "10_valuation" => format!(
            "{}：PE 5 年分位 {}，PB 5 年分位 {}。得分 {}/10。",
            label,
            s(&pick(&data, &["pe_quantile_5y", "pe_quantile"], "—")),
            s(&pick(&data, &["pb_quantile_5y", "pb_quantile"], "—")),
            format_score(score),
        ),
        "11_governance" => format!(
            "{}：实控人 {}；近期变动 {}。",
            label,
            s(&pick(&data, &["actual_controller"], "—")),
            s(&pick(&data, &["recent_changes", "recent_holdings_change"], "—")),
        ),
        "12_capital_flow" => {
            // default=None here, so `_v` returns the default object when nothing matches
            let north = pick_or_none(&data, &["north_holding_pct", "north_change_5d"]);
            let margin = pick_or_none(&data, &["margin_balance"]);
            if truthy(&north) || truthy(&margin) {
                format!(
                    "{}：北向持股 {}；融资余额 {}。",
                    label,
                    if truthy(&north) { s(&north) } else { "—".to_string() },
                    if truthy(&margin) { s(&margin) } else { "—".to_string() },
                )
            } else {
                format!(
                    "{}：{}。",
                    label,
                    match pick_or_none(&data, &["_note"]) {
                        Value::Null => "资金面数据有限".to_string(),
                        v => s(&v),
                    }
                )
            }
        }
        "13_policy" => {
            let snippets = sub(&data, "snippets");
            let non_empty: Vec<(&String, &Value)> = snippets
                .as_object()
                .map(|m| m.iter().filter(|(_, v)| truthy(v)).collect())
                .unwrap_or_default();
            if !non_empty.is_empty() {
                let preview = non_empty
                    .iter()
                    .map(|(k, v)| {
                        let n = v.as_array().map(|a| a.len()).unwrap_or(1);
                        format!("{}: {} 条", k, n)
                    })
                    .collect::<Vec<_>>()
                    .join("；");
                format!(
                    "{}：{} {} 年政策检索：{}。",
                    label,
                    s(&pick(&data, &["industry"], "本行业")),
                    s(&pick(&data, &["year"], "")),
                    preview
                )
            } else {
                format!(
                    "{}：{} 政策搜索未命中具体内容（建议 web_search 补抓）。",
                    label,
                    s(&pick(&data, &["industry"], "本行业")),
                )
            }
        }
        "14_moat" => {
            let scores = sub(&data, "scores");
            let total: Option<i64> = scores.as_object().and_then(|m| {
                if m.is_empty() {
                    None
                } else {
                    Some(m.values().map(|v| uzi_core::py::f0(v) as i64).sum())
                }
            });
            match total {
                Some(total) => format!(
                    "{}：四力评分 无形资产 {}/10、转换成本 {}/10、网络效应 {}/10、规模 {}/10 · 综合 {}/40。",
                    label,
                    s(&pick(&scores, &["intangible"], "—")),
                    s(&pick(&scores, &["switching"], "—")),
                    s(&pick(&scores, &["network"], "—")),
                    s(&pick(&scores, &["scale"], "—")),
                    total,
                ),
                None => format!("{}：评估数据有限，得分 {}/10。", label, format_score(score)),
            }
        }
        "15_events" => {
            let timeline = data
                .get("event_timeline")
                .and_then(|v| v.as_array())
                .cloned()
                .unwrap_or_default();
            let recent_news = data
                .get("recent_news")
                .and_then(|v| v.as_array())
                .cloned()
                .unwrap_or_default();
            if !timeline.is_empty() {
                let head = timeline
                    .iter()
                    .take(3)
                    .map(|t| s(t).chars().take(60).collect::<String>())
                    .collect::<Vec<_>>()
                    .join("；");
                format!("{}：近期事件 {} 条，含：{}。", label, timeline.len(), head)
            } else if !recent_news.is_empty() {
                let head = recent_news
                    .iter()
                    .take(3)
                    .map(|n| {
                        n.get("title")
                            .map(|t| s(t).chars().take(60).collect::<String>())
                            .unwrap_or_default()
                    })
                    .collect::<Vec<_>>()
                    .join("；");
                format!("{}：近期新闻 {} 条，含：{}。", label, recent_news.len(), head)
            } else {
                format!("{}：暂无显著事件（fetcher 返回空）。", label)
            }
        }
        "16_lhb" => {
            let n = pick_or_none(&data, &["recent_lhb_count", "n_lhb_30d"]);
            let seats = {
                let recent = data.get("recent_seats");
                match recent.filter(|v| truthy(v)) {
                    Some(v) => v.clone(),
                    None => sub(&data, "top_seats"),
                }
            };
            let seats_arr = seats.as_array().cloned().unwrap_or_default();
            if truthy(&n) || !seats_arr.is_empty() {
                let seat_str = seats_arr
                    .iter()
                    .take(3)
                    .filter(|s| s.is_object())
                    .map(|s| {
                        s.get("name")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string()
                    })
                    .collect::<Vec<_>>()
                    .join("、");
                format!(
                    "{}：近 30 天上榜 {} 次{}。",
                    label,
                    if truthy(&n) { s(&n) } else { "—".to_string() },
                    if seat_str.is_empty() {
                        String::new()
                    } else {
                        format!("，主要席位：{}", seat_str)
                    }
                )
            } else {
                format!("{}：近期未上龙虎榜或非 A 股。", label)
            }
        }
        "17_sentiment" => format!(
            "{}：热度 {}；情绪 {}。",
            label,
            s(&pick(&data, &["hot_rank", "hot_score"], "—")),
            s(&pick(&data, &["sentiment_label", "sentiment"], "—")),
        ),
        "18_trap" => {
            let level = pick(&data, &["trap_level", "level"], "—");
            // `data.get("signals_hit", "?/8")` — the default applies only when the
            // key is absent; a present null prints as None
            let scanned = data
                .get("signals_hit")
                .cloned()
                .unwrap_or_else(|| json!("?/8"));
            let rec = pick(&data, &["recommendation"], "—");
            let detail = data
                .get("signals_hit_detail")
                .and_then(|v| v.as_array())
                .cloned()
                .unwrap_or_default();
            if !detail.is_empty() {
                let kws: Vec<String> = detail
                    .iter()
                    .take(3)
                    .map(|d| {
                        d.get("name")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string()
                    })
                    .collect();
                format!(
                    "{}：{} · 8 信号扫描命中 {}（{}）· 建议：{}",
                    label,
                    s(&level),
                    s(&scanned),
                    kws.join("、"),
                    s(&rec)
                )
            } else {
                format!(
                    "{}：{} · 8 信号扫描命中 {}（已扫 ddgs 24 条搜索结果）· 建议：{}",
                    label,
                    s(&level),
                    s(&scanned),
                    s(&rec)
                )
            }
        }
        "19_contests" => {
            let summary = sub(&data, "summary");
            let n_cubes = summary
                .get("xueqiu_cubes_total")
                .map(uzi_core::py::f0)
                .unwrap_or(0.0);
            let n_high = summary
                .get("high_return_cubes")
                .map(uzi_core::py::f0)
                .unwrap_or(0.0);
            let login_req = summary
                .get("xueqiu_login_required")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let src = summary
                .get("xueqiu_source")
                .and_then(|v| v.as_str())
                .unwrap_or("http")
                .to_string();
            if login_req && n_cubes == 0.0 {
                format!(
                    "{}：⚠️ XueQiu cubes 接口需登录（2026 起新政），未启用 → 0 cube。启用方式：export UZI_XQ_LOGIN=1 + python -m lib.xueqiu_browser login",
                    label
                )
            } else if n_cubes != 0.0 {
                format!(
                    "{}：雪球 {} 个组合持有本股（高收益 >50% 的有 {} 个）· 来源 {}",
                    label,
                    num_int(n_cubes),
                    num_int(n_high),
                    src
                )
            } else {
                format!("{}：雪球 0 个组合持有本股（可能小盘 / 冷门 / 接口未返）", label)
            }
        }
        _ => {
            let mut items: Vec<String> = Vec::new();
            if let Some(map) = data.as_object() {
                for (k, v) in map.iter().take(5) {
                    if !is_sentinel(v) && !k.starts_with('_') {
                        items.push(format!("{}={}", k, s(v).chars().take(30).collect::<String>()));
                    }
                }
            }
            if items.is_empty() {
                String::new()
            } else {
                format!("{}：{}。", label, items.join("、"))
            }
        }
    }
}

/// `_v(*keys, default=None)` — returns `null` when nothing matches.
fn pick_or_none(data: &Value, keys: &[&str]) -> Value {
    for k in keys {
        if let Some(v) = data.get(*k) {
            if !is_sentinel(v) {
                return v.clone();
            }
        }
    }
    Value::Null
}

/// `score` is interpolated as `{score}/10`; upstream passes the int from
/// `score_dimensions`, so an integral score must print without a decimal point.
fn format_score(score: f64) -> String {
    if score.is_finite() && score.fract() == 0.0 {
        format!("{}", score as i64)
    } else {
        uzi_core::py::float_str(score)
    }
}

fn num_int(x: f64) -> String {
    if x.fract() == 0.0 {
        format!("{}", x as i64)
    } else {
        uzi_core::py::float_str(x)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_dim_reports_fetch_failure() {
        let dim = json!({"data": {}});
        assert_eq!(
            auto_summarize_dim("0_basic", "基础信息", &dim, 5.0),
            "基础信息：未拉取到数据（fetcher 失败或返回空）。"
        );
    }

    #[test]
    fn zero_is_not_a_sentinel_but_dash_is() {
        let data = json!({"pe": 0, "pb": "—"});
        assert_eq!(pick(&data, &["pe"], "—"), json!(0));
        assert_eq!(pick(&data, &["pb"], "—"), json!("—"));
        assert_eq!(pick(&data, &["missing"], "—"), json!("—"));
    }

    #[test]
    fn moat_total_sums_all_four_forces() {
        let dim = json!({"data": {"scores": {"intangible": 7, "switching": 6, "network": 4, "scale": 8}}});
        let out = auto_summarize_dim("14_moat", "护城河", &dim, 6.0);
        assert!(out.contains("综合 25/40"), "got {}", out);
    }

    #[test]
    fn unknown_dim_falls_back_to_top_field_enumeration() {
        let dim = json!({"data": {"a": 1, "b": "x", "_hidden": 2}});
        let out = auto_summarize_dim("99_unknown", "其他", &dim, 5.0);
        assert_eq!(out, "其他：a=1、b=x。");
    }
}
