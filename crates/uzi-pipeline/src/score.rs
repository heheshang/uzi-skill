//! Port of `lib/pipeline/score_fns.py::score_dimensions` — the 22-dimension score
//! table plus the weighted fundamental score.

use serde_json::{json, Map, Value};
use uzi_core::py::{f, f0, round, truthy};

/// `_get(key)` helper: `raw["dimensions"][key]["data"]`.
fn dim_data<'a>(dims: &'a Value, key: &str) -> &'a Value {
    static NULL: Value = Value::Null;
    dims.get(key)
        .and_then(|d| d.get("data"))
        .unwrap_or(&NULL)
}

fn list_len(v: &Value) -> usize {
    v.as_array().map(|a| a.len()).unwrap_or(0)
}

/// `x // 5` for a JSON number, matching Python's floor division.
fn int_div(v: &Value, divisor: i64) -> i64 {
    match v {
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                i.div_euclid(divisor)
            } else {
                let x = n.as_f64().unwrap_or(0.0);
                (x / divisor as f64).floor() as i64
            }
        }
        Value::String(s) => uzi_core::py::parse_float(s)
            .map(|x| (x / divisor as f64).floor() as i64)
            .unwrap_or(0),
        _ => 0,
    }
}

/// Python `sum(v for k, v in ratings.items() if "买入" in str(k) or "增持" in str(k))`.
fn buy_count(ratings: &Value) -> i64 {
    let Some(map) = ratings.as_object() else {
        return 0;
    };
    map.iter()
        .filter(|(k, _)| k.contains("买入") || k.contains("增持"))
        .map(|(_, v)| v.as_i64().unwrap_or_else(|| f0(v) as i64))
        .sum()
}

/// Extract the first integer in a string (`re.search(r"(\d+)", s)`).
fn first_int(s: &str) -> Option<i64> {
    let mut digits = String::new();
    for c in s.chars() {
        if c.is_ascii_digit() {
            digits.push(c);
        } else if !digits.is_empty() {
            break;
        }
    }
    digits.parse().ok()
}

/// The 22-dimension score table.
pub fn score_dimensions(raw: &Value) -> Value {
    if is_crypto_raw(raw) {
        return score_crypto(raw);
    }
    let dims = raw.get("dimensions").cloned().unwrap_or(json!({}));
    let mut out = Map::new();

    // ── 1 · 财报 ──────────────────────────────────────────────
    let fin = dim_data(&dims, "1_financials");
    let roe = f(fin.get("roe").unwrap_or(&Value::Null), 0.0);
    let roe_hist = fin.get("roe_history").cloned().unwrap_or(Value::Null);
    let last_roe = match roe_hist.as_array().filter(|a| !a.is_empty()) {
        Some(a) => f0(a.last().unwrap()),
        None => roe,
    };
    let net_margin = f(fin.get("net_margin").unwrap_or(&Value::Null), 0.0);
    let health = fin.get("financial_health").cloned().unwrap_or(Value::Null);
    let debt = f(
        health.get("debt_ratio").unwrap_or(&Value::Null),
        0.0,
    );
    let rev_hist = fin
        .get("revenue_history")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let growth = if rev_hist.len() >= 2 {
        let last = f0(&rev_hist[rev_hist.len() - 1]);
        let prev = f0(&rev_hist[rev_hist.len() - 2]);
        if prev != 0.0 {
            (last - prev) / prev * 100.0
        } else {
            0.0
        }
    } else {
        0.0
    };
    let mut score_1 = 5;
    if last_roe >= 15.0 {
        score_1 += 2;
    } else if last_roe >= 10.0 {
        score_1 += 1;
    } else if last_roe < 5.0 {
        score_1 -= 2;
    }
    if net_margin >= 15.0 {
        score_1 += 1;
    }
    if growth >= 20.0 {
        score_1 += 1;
    }
    if debt >= 60.0 {
        score_1 -= 1;
    }
    let score_1 = score_1.clamp(1, 10);
    let mut reasons_pass_1: Vec<String> = Vec::new();
    let mut reasons_fail_1: Vec<String> = Vec::new();
    if last_roe >= 15.0 {
        reasons_pass_1.push(format!("ROE 最新 {:.1}%", last_roe));
    } else if last_roe < 8.0 {
        reasons_fail_1.push(format!("ROE 最新 {:.1}% 偏低", last_roe));
    }
    if growth >= 20.0 {
        reasons_pass_1.push(format!("营收增速 {:.1}%", growth));
    } else if growth < 5.0 {
        reasons_fail_1.push(format!("营收增速 {:.1}% 停滞", growth));
    }
    if debt < 40.0 {
        reasons_pass_1.push(format!("资产负债率 {:.0}% 健康", debt));
    } else if debt > 60.0 {
        reasons_fail_1.push(format!("资产负债率 {:.0}% 偏高", debt));
    }
    out.insert(
        "1_financials".into(),
        json!({
            "score": score_1,
            "weight": 5,
            "label": format!("ROE {:.1}% · 营收增速 {:+.1}% · 负债率 {:.0}%", last_roe, growth, debt),
            "reasons_pass": reasons_pass_1,
            "reasons_fail": reasons_fail_1,
        }),
    );

    // ── 2 · K 线 ──────────────────────────────────────────────
    let kline = dim_data(&dims, "2_kline");
    let stage = py_str_of(kline.get("stage"));
    let ma_align = py_str_of(kline.get("ma_align"));
    let stats = kline.get("kline_stats").cloned().unwrap_or(Value::Null);
    let mut score_2 = 5;
    if stage.contains("Stage 2") {
        score_2 += 2;
    } else if stage.contains("Stage 1") {
        score_2 += 1;
    } else if stage.contains("Stage 3") || stage.contains("Stage 4") {
        score_2 -= 2;
    }
    if ma_align.contains("多头") {
        score_2 += 1;
    }
    let dd_str = stats
        .get("max_drawdown")
        .cloned()
        .unwrap_or_else(|| json!("0%"));
    let dd = f(&dd_str, 0.0);
    if dd <= -30.0 {
        score_2 -= 1;
    }
    let score_2 = score_2.clamp(1, 10);
    let mut label_2 = format!("{} · 均线{}", stage, ma_align);
    if truthy(stats.get("ytd_return").unwrap_or(&Value::Null)) {
        label_2.push_str(&format!(
            " · YTD {}",
            uzi_core::py::py_str(&stats["ytd_return"])
        ));
    }
    out.insert(
        "2_kline".into(),
        json!({
            "score": score_2,
            "weight": 4,
            "label": label_2,
            "reasons_pass": if stage.contains("Stage 2") { vec![stage.clone()] } else { Vec::<String>::new() },
            "reasons_fail": if dd <= -25.0 { vec![format!("最大回撤 {:.1}%", dd)] } else { Vec::<String>::new() },
        }),
    );

    // ── 3 · 宏观（qualitative — middle） ──────────────────────
    out.insert(
        "3_macro".into(),
        json!({"score": 6, "weight": 3, "label": "宏观环境中性"}),
    );

    // ── 4 · 同行 ──────────────────────────────────────────────
    let peers = dim_data(&dims, "4_peers");
    let peer_table = peers
        .get("peer_table")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let global_peer_count = peers
        .get("global_peer_comparison")
        .and_then(|g| g.get("peer_count"))
        .map(|v| f0(v) as i64)
        .unwrap_or(0);
    let mut score_4 = 5;
    if peer_table.len() > 1 {
        score_4 = 7;
        if let Some(self_row) = peer_table
            .iter()
            .find(|p| p.get("is_self").and_then(|v| v.as_bool()) == Some(true))
        {
            let self_pe = f(self_row.get("pe").unwrap_or(&Value::Null), 0.0);
            let others: Vec<&Value> = peer_table
                .iter()
                .filter(|p| p.get("is_self").and_then(|v| v.as_bool()) != Some(true))
                .collect();
            let avg_pe = others
                .iter()
                .map(|p| f(p.get("pe").unwrap_or(&Value::Null), 0.0))
                .sum::<f64>()
                / others.len().max(1) as f64;
            if self_pe > 0.0 && avg_pe > 0.0 {
                if self_pe < avg_pe * 0.9 {
                    score_4 += 1;
                } else if self_pe > avg_pe * 1.2 {
                    score_4 -= 1;
                }
            }
        }
    } else if global_peer_count >= 3 {
        score_4 = 7;
    }
    let local_peer_count = list_len(&Value::Array(peer_table.clone())).saturating_sub(1);
    let peer_label = if global_peer_count != 0 {
        format!("全球同行 {} 家对比", global_peer_count)
    } else if local_peer_count != 0 {
        format!("同业 {} 家对比", local_peer_count)
    } else {
        "无同行数据".to_string()
    };
    out.insert(
        "4_peers".into(),
        json!({"score": score_4, "weight": 4, "label": peer_label, "reasons_pass": [], "reasons_fail": []}),
    );

    // ── 5 · 上下游 ────────────────────────────────────────────
    let chain = dim_data(&dims, "5_chain");
    let breakdown = chain
        .get("main_business_breakdown")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let has_breakdown = !breakdown.is_empty();
    let score_5 = if has_breakdown { 6 } else { 5 };
    out.insert(
        "5_chain".into(),
        json!({
            "score": score_5,
            "weight": 4,
            "label": if has_breakdown { format!("主营 {} 类业务已识别", breakdown.len()) } else { "产业链数据不完整".to_string() },
            "reasons_pass": [],
            "reasons_fail": [],
        }),
    );

    // ── 6 · 研报 ──────────────────────────────────────────────
    let research = dim_data(&dims, "6_research");
    let coverage = research.get("report_count").cloned().unwrap_or(json!(0));
    let ratings = research
        .get("rating_distribution")
        .cloned()
        .unwrap_or(Value::Null);
    let buy = buy_count(&ratings);
    let mut score_6 = 5 + int_div(&coverage, 5).min(3);
    if buy >= 10 {
        score_6 += 1;
    }
    let score_6 = score_6.min(10);
    let coverage_str = uzi_core::py::num_str(&coverage);
    let coverage_truthy = truthy(&coverage);
    out.insert(
        "6_research".into(),
        json!({
            "score": score_6,
            "weight": 3,
            "label": if coverage_truthy { format!("{} 份研报 · 买入/增持 {} 份", coverage_str, buy) } else { "研报数据稀少".to_string() },
            "reasons_pass": if truthy(&coverage) && f0(&coverage) >= 10.0 { vec![format!("覆盖券商 {} 家", coverage_str)] } else { Vec::<String>::new() },
            "reasons_fail": if coverage_truthy { Vec::<String>::new() } else { vec!["缺乏覆盖".to_string()] },
        }),
    );

    // ── 7/8/9 · 定性中位 ──────────────────────────────────────
    out.insert(
        "7_industry".into(),
        json!({"score": 7, "weight": 4, "label": "行业处于成长期"}),
    );
    out.insert(
        "8_materials".into(),
        json!({"score": 6, "weight": 3, "label": "原材料成本关注中"}),
    );
    out.insert(
        "9_futures".into(),
        json!({"score": 5, "weight": 2, "label": "无强关联期货品种"}),
    );

    // ── 10 · 估值 ─────────────────────────────────────────────
    let val = dim_data(&dims, "10_valuation");
    let pe_q_str = py_str_of(val.get("pe_quantile"));
    let pe_q = first_int(&pe_q_str).unwrap_or(50);
    let score_10 = if pe_q < 30 {
        9
    } else if pe_q < 50 {
        7
    } else if pe_q < 70 {
        5
    } else if pe_q < 85 {
        3
    } else {
        2
    };
    out.insert(
        "10_valuation".into(),
        json!({
            "score": score_10,
            "weight": 5,
            "label": format!(
                "PE {} · 5 年 {} 分位 · 行业均值 {}",
                fmt_or_dash(val.get("pe")),
                pe_q,
                fmt_or_dash(val.get("industry_pe"))
            ),
            "reasons_pass": if pe_q < 50 { vec!["PE 在 5 年中位数以下".to_string()] } else { Vec::<String>::new() },
            "reasons_fail": if pe_q >= 75 { vec!["PE 已在 5 年高位区".to_string()] } else { Vec::<String>::new() },
        }),
    );

    // ── 11 · 治理 ─────────────────────────────────────────────
    let gov = dim_data(&dims, "11_governance");
    // upstream: `pledge = gov.get("pledge") or []` — falsy values become []
    let pledge_raw = gov.get("pledge").cloned().unwrap_or(Value::Null);
    let pledge = if truthy(&pledge_raw) {
        pledge_raw
    } else {
        json!([])
    };
    let has_insider = truthy(gov.get("insider_trades_1y").unwrap_or(&Value::Null));
    let mut score_11 = 6;
    let pledge_len = pledge.as_array().map(|a| a.len());
    if pledge_len.unwrap_or(0) == 0 {
        score_11 += 1;
    }
    if has_insider {
        score_11 += 1;
    }
    let pledge_display = match pledge_len {
        Some(n) => n.to_string(),
        None => "—".to_string(),
    };
    out.insert(
        "11_governance".into(),
        json!({
            "score": score_11.min(10),
            "weight": 4,
            "label": format!(
                "质押记录 {} · 内部交易 {}",
                pledge_display,
                if has_insider { "有" } else { "无" }
            ),
        }),
    );

    // ── 12 · 资金面 ───────────────────────────────────────────
    let cap = dim_data(&dims, "12_capital_flow");
    let main_flow = cap
        .get("main_fund_flow_20d")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let mut main_5d_net = 0.0f64;
    for rec in main_flow.iter().take(5) {
        if let Some(obj) = rec.as_object() {
            if let Some(v) = obj.get("主力净流入-净额") {
                main_5d_net += f(v, 0.0);
            }
        }
    }
    let main_5d_label = if main_5d_net != 0.0 {
        format!("{:+.1}亿", main_5d_net / 1e8)
    } else {
        "—".to_string()
    };
    let unlock = cap
        .get("unlock_schedule")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let mut score_12 = 5;
    if main_5d_net > 0.0 {
        score_12 += 2;
    } else if main_5d_net < 0.0 {
        score_12 -= 1;
    }
    if unlock.is_empty() {
        score_12 += 1;
    }
    let score_12 = score_12.clamp(1, 10);
    out.insert(
        "12_capital_flow".into(),
        json!({
            "score": score_12,
            "weight": 4,
            "label": format!("主力 5日 {} · 12 个月解禁 {} 次", main_5d_label, unlock.len()),
            "reasons_pass": if main_5d_net > 0.0 { vec![format!("主力资金 5 日净流入 {}", main_5d_label)] } else { Vec::<String>::new() },
            "reasons_fail": if main_5d_net < 0.0 { vec![format!("主力资金 5 日净流出 {}", main_5d_label)] } else { Vec::<String>::new() },
        }),
    );

    // ── 13/14 · 定性中位 ──────────────────────────────────────
    out.insert(
        "13_policy".into(),
        json!({"score": 6, "weight": 3, "label": "政策环境中性"}),
    );
    out.insert(
        "14_moat".into(),
        json!({"score": 6, "weight": 3, "label": "护城河需定性评估"}),
    );

    // ── 15 · 事件 ─────────────────────────────────────────────
    let events = dim_data(&dims, "15_events");
    let news_len = list_len(events.get("news").unwrap_or(&Value::Null));
    let notices_len = list_len(events.get("recent_notices").unwrap_or(&Value::Null));
    let score_15 = 5 + (news_len / 10).min(3);
    out.insert(
        "15_events".into(),
        json!({
            "score": score_15,
            "weight": 4,
            "label": format!("近期新闻 {} 条 · 公告 {} 份", news_len, notices_len),
        }),
    );

    // ── 16 · 龙虎榜 ───────────────────────────────────────────
    let lhb = dim_data(&dims, "16_lhb");
    let lhb_count = f0(lhb.get("lhb_count_30d").unwrap_or(&Value::Null)) as i64;
    let matched: Vec<String> = lhb
        .get("matched_youzi")
        .and_then(|v| v.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|s| s.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    let mut score_16 = 5 + (lhb_count / 2).min(3);
    if !matched.is_empty() {
        score_16 += 1;
    }
    let score_16 = score_16.min(10);
    let matched_head: Vec<String> = matched.iter().take(3).cloned().collect();
    out.insert(
        "16_lhb".into(),
        json!({
            "score": score_16,
            "weight": 4,
            "label": format!("近 30 天上榜 {} 次 · 识别游资 {} 位", lhb_count, matched.len()),
            "reasons_pass": if !matched.is_empty() { vec![format!("{} 席位出现", matched_head.join("/"))] } else { Vec::<String>::new() },
        }),
    );

    // ── 17 · 舆情 ─────────────────────────────────────────────
    let hot = dim_data(&dims, "17_sentiment");
    let hot_rank_len = hot
        .get("hot_rank")
        .and_then(|h| h.get("rank_history"))
        .and_then(|v| v.as_array())
        .map(|a| a.len())
        .unwrap_or(0);
    let score_17 = 6 + (hot_rank_len / 10).min(2);
    out.insert(
        "17_sentiment".into(),
        json!({
            "score": score_17,
            "weight": 3,
            "label": format!("雪球热度上榜 {} 次", hot_rank_len),
        }),
    );

    // ── 18 · 杀猪盘（safe by default） ────────────────────────
    out.insert(
        "18_trap".into(),
        json!({"score": 9, "weight": 5, "label": "🟢 未发现推广痕迹"}),
    );

    // ── 19 · 实盘赛 ───────────────────────────────────────────
    let contests = dim_data(&dims, "19_contests");
    let summary = contests.get("summary").cloned().unwrap_or(Value::Null);
    let xq_total = summary
        .get("xueqiu_cubes_total")
        .map(|v| f0(v) as i64)
        .unwrap_or(0);
    let hi = summary
        .get("high_return_cubes")
        .map(|v| f0(v) as i64)
        .unwrap_or(0);
    let score_19 = (5 + (xq_total / 5).min(3) + hi.min(2)).min(10);
    out.insert(
        "19_contests".into(),
        json!({
            "score": score_19,
            "weight": 4,
            "label": format!("雪球 {} 个组合持有 · {} 个收益 >50%", xq_total, hi),
            "reasons_pass": if xq_total != 0 { vec![format!("{} 个雪球组合持有", xq_total)] } else { Vec::<String>::new() },
        }),
    );

    // ── Overall fundamental score ─────────────────────────────
    finalize(raw, out)
}

/// Weighted average of every scored dimension (`weight` 0 entries are ignored).
fn finalize(raw: &Value, out: Map<String, Value>) -> Value {
    let mut total_weighted = 0.0f64;
    let mut total_weight = 0.0f64;
    for (_, v) in out.iter() {
        let score = f0(v.get("score").unwrap_or(&Value::Null));
        let weight = f0(v.get("weight").unwrap_or(&Value::Null));
        total_weighted += score * weight;
        total_weight += weight;
    }
    let fundamental = if total_weight != 0.0 {
        total_weighted / total_weight * 10.0
    } else {
        0.0
    };

    json!({
        "ticker": raw.get("ticker").cloned().unwrap_or(Value::Null),
        "fundamental_score": round(fundamental, 1),
        "dimensions": Value::Object(out),
    })
}

/// True when a dim carries a fetched payload (not a filled-in placeholder).
fn dim_ok(dims: &Value, key: &str) -> bool {
    let d = dims.get(key).unwrap_or(&Value::Null);
    let q = d
        .get("_pipeline")
        .and_then(|p| p.get("quality"))
        .and_then(|q| q.as_str())
        .or_else(|| d.get("quality").and_then(|q| q.as_str()))
        .unwrap_or("full");
    q != "missing" && q != "error"
}

/// Neutral entry for a dim the active depth profile never fetched.
fn not_collected(label: &str, weight: i64) -> Value {
    json!({
        "score": 5,
        "weight": weight,
        "label": format!("{label} · 当前档位未采集"),
        "reasons_pass": [],
        "reasons_fail": [],
    })
}

/// True when the snapshot is a crypto venue (`market == "C"`).
fn is_crypto_raw(raw: &Value) -> bool {
    if let Some(m) = raw
        .get("dimensions")
        .and_then(|d| d.get("0_basic"))
        .and_then(|d| d.get("data"))
        .and_then(|d| d.get("market"))
        .and_then(|m| m.as_str())
    {
        return m == "C";
    }
    raw.get("ticker")
        .and_then(|t| t.as_str())
        .map(|t| uzi_core::ticker::parse_ticker(t).market == uzi_core::ticker::CRYPTO_MARKET)
        .unwrap_or(false)
}

/// Crypto score table.
///
/// Same 19 keys as the equity table so the renderer/scorer contract holds, but
/// the drivers are crypto-native: tokenomics, market-cap rank, developer
/// activity, NVT, funding rates, fear & greed and the pump/dump risk score.
/// Dimensions with no crypto analogue (16 龙虎榜 / 19 实盘赛) carry `weight: 0`
/// so they do not dilute the weighted average.
fn score_crypto(raw: &Value) -> Value {
    let dims = raw.get("dimensions").cloned().unwrap_or(json!({}));
    let mut out = Map::new();

    // ── 1 · 代币经济 ──────────────────────────────────────────
    let fin = dim_data(&dims, "1_financials");
    let circ = f(fin.get("circulating_ratio_pct").unwrap_or(&Value::Null), 0.0);
    let fdv_to_mcap = f(fin.get("fdv_to_mcap").unwrap_or(&Value::Null), 0.0);
    let supply_model = py_str_of(fin.get("supply_model"));
    let mut score_1 = 5;
    if circ >= 80.0 {
        score_1 += 2;
    } else if circ >= 50.0 {
        score_1 += 1;
    } else if circ > 0.0 && circ < 25.0 {
        score_1 -= 2;
    }
    if fdv_to_mcap > 0.0 && fdv_to_mcap <= 1.2 {
        score_1 += 1;
    } else if fdv_to_mcap > 2.0 {
        score_1 -= 1;
    }
    let score_1 = score_1.clamp(1, 10);
    let mut reasons_pass_1: Vec<String> = Vec::new();
    let mut reasons_fail_1: Vec<String> = Vec::new();
    if circ >= 50.0 {
        reasons_pass_1.push(format!("流通率 {:.0}%", circ));
    } else if circ > 0.0 {
        reasons_fail_1.push(format!("流通率仅 {:.0}% · 解锁抛压", circ));
    }
    if fdv_to_mcap > 2.0 {
        reasons_fail_1.push(format!("FDV/市值 {:.1} · 稀释空间大", fdv_to_mcap));
    }
    out.insert(
        "1_financials".into(),
        json!({
            "score": score_1,
            "weight": 5,
            "label": format!(
                "流通率 {:.0}% · FDV/市值 {} · {}",
                circ,
                if fdv_to_mcap > 0.0 { format!("{fdv_to_mcap:.2}") } else { "—".to_string() },
                if supply_model.is_empty() { "供应模型未知" } else { supply_model.as_str() }
            ),
            "reasons_pass": reasons_pass_1,
            "reasons_fail": reasons_fail_1,
        }),
    );

    // ── 2 · K 线（与股票同一套 Wyckoff/均线口径） ─────────────
    let kline = dim_data(&dims, "2_kline");
    let stage = py_str_of(kline.get("stage"));
    let ma_align = py_str_of(kline.get("ma_align"));
    let stats = kline.get("kline_stats").cloned().unwrap_or(Value::Null);
    let mut score_2 = 5;
    if stage.contains("Stage 2") {
        score_2 += 2;
    } else if stage.contains("Stage 1") {
        score_2 += 1;
    } else if stage.contains("Stage 3") || stage.contains("Stage 4") {
        score_2 -= 2;
    }
    if ma_align.contains("多头") {
        score_2 += 1;
    }
    let dd_str = stats.get("max_drawdown").cloned().unwrap_or_else(|| json!("0%"));
    let dd = f(&dd_str, 0.0);
    if dd <= -30.0 {
        score_2 -= 1;
    }
    let score_2 = score_2.clamp(1, 10);
    let mut label_2 = format!("{} · 均线{}", stage, ma_align);
    if truthy(stats.get("ytd_return").unwrap_or(&Value::Null)) {
        label_2.push_str(&format!(
            " · YTD {}",
            uzi_core::py::py_str(&stats["ytd_return"])
        ));
    }
    if truthy(stats.get("volatility").unwrap_or(&Value::Null)) {
        label_2.push_str(&format!(" · 年化波动 {}", uzi_core::py::py_str(&stats["volatility"])));
    }
    out.insert(
        "2_kline".into(),
        json!({
            "score": score_2,
            "weight": 4,
            "label": label_2,
            "reasons_pass": if stage.contains("Stage 2") { vec![stage.clone()] } else { Vec::<String>::new() },
            "reasons_fail": if dd <= -25.0 { vec![format!("最大回撤 {:.1}%", dd)] } else { Vec::<String>::new() },
        }),
    );

    // ── 3 · 宏观 / 流动性 ─────────────────────────────────────
    let mac = dim_data(&dims, "3_macro");
    let fng = f(mac.get("fear_greed").unwrap_or(&Value::Null), 50.0);
    let mcap_chg = f(mac.get("mcap_change_24h_pct").unwrap_or(&Value::Null), 0.0);
    let btc_dom = f(mac.get("btc_dominance_pct").unwrap_or(&Value::Null), 0.0);
    let mut score_3 = 5;
    if fng <= 25.0 {
        score_3 += 2;
    } else if fng >= 75.0 {
        score_3 -= 1;
    }
    if mcap_chg > 1.0 {
        score_3 += 1;
    } else if mcap_chg < -3.0 {
        score_3 -= 1;
    }
    let score_3 = score_3.clamp(1, 10);
    out.insert(
        "3_macro".into(),
        json!({
            "score": score_3,
            "weight": 3,
            "label": format!(
                "全市场 24h {:+.1}% · BTC 占比 {:.1}% · 恐慌贪婪 {:.0}",
                mcap_chg, btc_dom, fng
            ),
            "reasons_pass": if fng <= 25.0 { vec![format!("恐慌贪婪 {:.0} · 情绪冰点", fng)] } else { Vec::<String>::new() },
            "reasons_fail": if fng >= 75.0 { vec![format!("恐慌贪婪 {:.0} · 情绪过热", fng)] } else { Vec::<String>::new() },
        }),
    );

    // ── 4 · 市值地位 ──────────────────────────────────────────
    let peers = dim_data(&dims, "4_peers");
    let rank = f(peers.get("rank").unwrap_or(&Value::Null), 0.0);
    let peer_count = peers
        .get("peer_comparison")
        .and_then(|p| p.get("peer_count"))
        .map(|v| f0(v) as i64)
        .unwrap_or(0);
    let (score_4, rank_label) = if rank > 0.0 && rank <= 3.0 {
        (9, format!("市值排名 #{}", rank as i64))
    } else if rank > 0.0 && rank <= 10.0 {
        (8, format!("市值排名 #{}", rank as i64))
    } else if rank > 0.0 && rank <= 30.0 {
        (6, format!("市值排名 #{}", rank as i64))
    } else if rank > 0.0 && rank <= 100.0 {
        (5, format!("市值排名 #{}", rank as i64))
    } else if rank > 0.0 {
        (4, format!("市值排名 #{}", rank as i64))
    } else if peer_count > 0 {
        (5, format!("同业 {} 家对比（缺市值排名）", peer_count))
    } else {
        (5, "无同业数据".to_string())
    };
    out.insert(
        "4_peers".into(),
        json!({"score": score_4, "weight": 4, "label": rank_label, "reasons_pass": [], "reasons_fail": []}),
    );

    // ── 5 · 生态/业务构成 ────────────────────────────────────
    let chain = dim_data(&dims, "5_chain");
    let breakdown = chain
        .get("main_business_breakdown")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let score_5 = if !breakdown.is_empty() { 6 } else { 5 };
    out.insert(
        "5_chain".into(),
        json!({
            "score": score_5,
            "weight": 3,
            "label": if breakdown.is_empty() { "生态分类缺失".to_string() } else { format!("生态标签 {} 个", breakdown.len()) },
            "reasons_pass": [],
            "reasons_fail": [],
        }),
    );

    // ── 6 · 开发者 / 社区活跃度 ──────────────────────────────
    let research = dim_data(&dims, "6_research");
    let commits = f(
        research.get("developer").and_then(|d| d.get("commit_count_4_weeks")).unwrap_or(&Value::Null),
        0.0,
    );
    let followers = f(
        research.get("community").and_then(|d| d.get("twitter_followers")).unwrap_or(&Value::Null),
        0.0,
    );
    let mut score_6 = 4;
    if commits >= 50.0 {
        score_6 += 3;
    } else if commits >= 15.0 {
        score_6 += 2;
    } else if commits >= 1.0 {
        score_6 += 1;
    }
    if followers >= 1_000_000.0 {
        score_6 += 2;
    } else if followers >= 100_000.0 {
        score_6 += 1;
    }
    let score_6 = score_6.clamp(1, 10);
    out.insert(
        "6_research".into(),
        json!({
            "score": score_6,
            "weight": 3,
            "label": format!("4 周提交 {:.0} 次 · 推特粉丝 {:.0}", commits, followers),
            "reasons_pass": if commits >= 15.0 { vec![format!("开发者活跃（4 周 {commits:.0} 次提交）")] } else { Vec::<String>::new() },
            "reasons_fail": if commits < 1.0 { vec!["近 4 周无代码提交".to_string()] } else { Vec::<String>::new() },
        }),
    );

    // ── 7 · 赛道地位 ─────────────────────────────────────────
    let ind = dim_data(&dims, "7_industry");
    let share = f(ind.get("market_share_pct").unwrap_or(&Value::Null), 0.0);
    let score_7 = if share >= 5.0 {
        8
    } else if share >= 1.0 {
        7
    } else if share > 0.0 {
        6
    } else {
        5
    };
    out.insert(
        "7_industry".into(),
        json!({
            "score": score_7,
            "weight": 4,
            "label": format!(
                "{} · 市值占比 {:.2}%",
                py_str_of(ind.get("industry")),
                share
            ),
        }),
    );

    // ── 8 · 生产成本（PoW 才有意义） ─────────────────────────
    out.insert(
        "8_materials".into(),
        json!({"score": 5, "weight": 1, "label": "加密网络无传统原材料成本"}),
    );

    // ── 9 · 合约资金费率 ─────────────────────────────────────
    let fut = dim_data(&dims, "9_futures");
    let funding = f(fut.get("funding_rate_pct").unwrap_or(&Value::Null), 0.0);
    let has_contract = truthy(fut.get("linked_contract").unwrap_or(&Value::Null));
    let (score_9, fut_label) = if !has_contract {
        (5, "无永续合约数据".to_string())
    } else if funding > 0.05 {
        (3, format!("资金费率 {:+.4}% · 多头拥挤", funding))
    } else if funding < -0.03 {
        (7, format!("资金费率 {:+.4}% · 空头拥挤（潜在逼空）", funding))
    } else {
        (6, format!("资金费率 {:+.4}% · 多空均衡", funding))
    };
    out.insert(
        "9_futures".into(),
        json!({"score": score_9, "weight": 2, "label": fut_label}),
    );

    // ── 10 · 估值（NVT / 区间位置 / ATH 回撤） ───────────────
    let val = dim_data(&dims, "10_valuation");
    let nvt = f(val.get("nvt_ratio").unwrap_or(&Value::Null), 0.0);
    let turnover = f(val.get("turnover_ratio").unwrap_or(&Value::Null), 0.0);
    let range_pos = f(val.get("price_range_position_pct").unwrap_or(&Value::Null), 50.0);
    let ath_dd = f(val.get("ath_drawdown_pct").unwrap_or(&Value::Null), 0.0);
    let mut score_10 = 5;
    if nvt > 0.0 {
        // Classic NVT band: < 20 cheap, > 60 expensive.
        if nvt <= 20.0 {
            score_10 += 2;
        } else if nvt >= 60.0 {
            score_10 -= 2;
        }
    }
    if range_pos <= 30.0 {
        score_10 += 1;
    } else if range_pos >= 85.0 {
        score_10 -= 1;
    }
    let score_10 = score_10.clamp(1, 10);
    out.insert(
        "10_valuation".into(),
        json!({
            "score": score_10,
            "weight": 5,
            "label": format!(
                "NVT {} · 日换手 {:.1}% · 区间位置 {:.0}% · 距 ATH {:.0}%",
                if nvt > 0.0 { format!("{nvt:.1}") } else { "—".to_string() },
                turnover * 100.0,
                range_pos,
                ath_dd
            ),
            "reasons_pass": if nvt > 0.0 && nvt <= 20.0 { vec![format!("NVT {:.1} 低于 20 · 相对网络价值便宜", nvt)] } else { Vec::<String>::new() },
            "reasons_fail": if nvt >= 60.0 { vec![format!("NVT {:.1} 偏高 · 市值透支网络活动", nvt)] } else { Vec::<String>::new() },
        }),
    );

    // ── 11 · 治理/解锁 ───────────────────────────────────────
    let gov = dim_data(&dims, "11_governance");
    let unvested = f(gov.get("unvested_supply_pct").unwrap_or(&Value::Null), 0.0);
    let score_11 = if unvested <= 10.0 {
        8
    } else if unvested <= 30.0 {
        6
    } else if unvested <= 50.0 {
        4
    } else {
        3
    };
    out.insert(
        "11_governance".into(),
        json!({
            "score": score_11,
            "weight": 2,
            "label": format!("未流通代币 {:.0}% · 无股权质押口径", unvested),
        }),
    );

    // ── 12 · 资金面 ──────────────────────────────────────────
    let cap = dim_data(&dims, "12_capital_flow");
    let vol_chg = f(cap.get("volume_change_7d_pct").unwrap_or(&Value::Null), 0.0);
    let stable_chg = f(cap.get("stablecoin_mcap_change_24h_pct").unwrap_or(&Value::Null), 0.0);
    let mut score_12 = 5;
    if vol_chg >= 30.0 {
        score_12 += 2;
    } else if vol_chg > 0.0 {
        score_12 += 1;
    } else if vol_chg <= -30.0 {
        score_12 -= 1;
    }
    if stable_chg > 0.3 {
        score_12 += 1;
    } else if stable_chg < -0.3 {
        score_12 -= 1;
    }
    let score_12 = score_12.clamp(1, 10);
    out.insert(
        "12_capital_flow".into(),
        json!({
            "score": score_12,
            "weight": 3,
            "label": format!("7 日均量 {:+.0}% · 稳定币总量 24h {:+.2}%", vol_chg, stable_chg),
            "reasons_pass": if vol_chg >= 30.0 { vec![format!("成交额 7 日放大 {:+.0}%", vol_chg)] } else { Vec::<String>::new() },
            "reasons_fail": if vol_chg <= -30.0 { vec![format!("成交额 7 日萎缩 {:+.0}%", vol_chg)] } else { Vec::<String>::new() },
        }),
    );

    // ── 13 · 监管 ────────────────────────────────────────────
    let pol = dim_data(&dims, "13_policy");
    let pol_news = pol.get("news").and_then(|v| v.as_array()).map(|a| a.len()).unwrap_or(0);
    out.insert(
        "13_policy".into(),
        json!({
            "score": 6,
            "weight": 2,
            "label": if pol_news > 0 { format!("监管相关快讯 {} 条", pol_news) } else { "监管环境中性".to_string() },
        }),
    );

    // ── 14 · 护城河（网络效应） ──────────────────────────────
    let moat = dim_data(&dims, "14_moat");
    let moat_total = f(moat.get("scores").and_then(|s| s.get("total")).unwrap_or(&Value::Null), 0.0);
    let score_14 = if moat_total > 0.0 {
        moat_total.round().clamp(1.0, 10.0) as i64
    } else {
        5
    };
    out.insert(
        "14_moat".into(),
        json!({
            "score": score_14,
            "weight": 3,
            "label": format!(
                "网络效应 {:.0}/10 · 市值占比 {:.2}%",
                moat_total,
                f(moat.get("market_share_pct").unwrap_or(&Value::Null), 0.0)
            ),
        }),
    );

    // ── 15 · 事件 ────────────────────────────────────────────
    let events = dim_data(&dims, "15_events");
    let news_len = list_len(events.get("news").unwrap_or(&Value::Null));
    let score_15 = 5 + (news_len / 10).min(3);
    out.insert(
        "15_events".into(),
        json!({
            "score": score_15,
            "weight": 3,
            "label": format!("近期加密快讯 {} 条", news_len),
        }),
    );

    // ── 16 · 龙虎榜（不适用） ────────────────────────────────
    out.insert(
        "16_lhb".into(),
        json!({"score": 5, "weight": 0, "label": "加密市场无龙虎榜（该维度不计权重）"}),
    );

    // ── 17 · 情绪 ────────────────────────────────────────────
    let sent = dim_data(&dims, "17_sentiment");
    let fng17 = f(sent.get("thermometer_value").unwrap_or(&Value::Null), 50.0);
    let up_pct = f(sent.get("positive_pct").unwrap_or(&Value::Null), 50.0);
    let trend_rank = f(sent.get("trending_rank").unwrap_or(&Value::Null), 0.0);
    let mut score_17 = 5;
    if fng17 <= 25.0 {
        score_17 += 2;
    } else if fng17 >= 75.0 {
        score_17 -= 1;
    }
    if up_pct >= 70.0 {
        score_17 += 1;
    } else if up_pct <= 30.0 && up_pct > 0.0 {
        score_17 -= 1;
    }
    if trend_rank > 0.0 && trend_rank <= 5.0 {
        score_17 += 1;
    }
    let score_17 = score_17.clamp(1, 10);
    out.insert(
        "17_sentiment".into(),
        json!({
            "score": score_17,
            "weight": 3,
            "label": format!(
                "恐慌贪婪 {:.0} · 看多占比 {:.0}%{}",
                fng17,
                up_pct,
                if trend_rank > 0.0 { format!(" · 热搜第 {} 位", trend_rank as i64) } else { String::new() }
            ),
        }),
    );

    // ── 18 · 风险（杀猪盘口径反转：分高 = 安全） ─────────────
    let trap = dim_data(&dims, "18_trap");
    let risk = f(trap.get("risk_score").unwrap_or(&Value::Null), 0.0);
    let score_18 = (10.0 - risk / 12.0).clamp(1.0, 10.0) as i64;
    let signals: Vec<String> = trap
        .get("pump_dump_signals")
        .and_then(|v| v.as_array())
        .map(|a| a.iter().map(uzi_core::py::py_str).collect())
        .unwrap_or_default();
    out.insert(
        "18_trap".into(),
        json!({
            "score": score_18,
            "weight": 5,
            "label": format!(
                "{} · 风险分 {:.0}/100",
                py_str_of(trap.get("trap_level")),
                risk
            ),
            "reasons_fail": signals.iter().take(2).cloned().collect::<Vec<String>>(),
        }),
    );

    // ── 19 · 实盘赛（不适用） ────────────────────────────────
    out.insert(
        "19_contests".into(),
        json!({"score": 5, "weight": 0, "label": "加密资产无实盘赛数据（该维度不计权重）"}),
    );

    // Dims the active depth profile never fetched (`lite` covers 7 of 20) keep a
    // neutral 5 but are labelled as "not collected" instead of showing the
    // zero-valued defaults of the branches above.
    for (key, label, weight) in [
        ("3_macro", "宏观流动性", 3),
        ("4_peers", "市值地位", 4),
        ("5_chain", "生态构成", 3),
        ("6_research", "开发者/社区", 3),
        ("7_industry", "赛道地位", 4),
        ("9_futures", "合约资金费率", 2),
        ("12_capital_flow", "资金面", 3),
        ("13_policy", "监管环境", 2),
        ("14_moat", "网络效应", 3),
        ("17_sentiment", "市场情绪", 3),
        ("18_trap", "风险扫描", 5),
    ] {
        if !dim_ok(&dims, key) {
            out.insert(key.to_string(), not_collected(label, weight));
        }
    }

    finalize(raw, out)
}

fn py_str_of(v: Option<&Value>) -> String {
    match v {
        None | Some(Value::Null) => String::new(),
        Some(Value::String(s)) => s.clone(),
        Some(other) => uzi_core::py::py_str(other),
    }
}

/// `val.get(k, "—")` semantics for f-string interpolation.
fn fmt_or_dash(v: Option<&Value>) -> String {
    match v {
        None | Some(Value::Null) => "—".to_string(),
        Some(Value::String(s)) if s.is_empty() => "—".to_string(),
        Some(other) => uzi_core::py::num_str(other),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_dimensions_still_scores_every_dimension() {
        let raw = serde_json::json!({"ticker": "X"});
        let out = score_dimensions(&raw);
        assert_eq!(out["ticker"], "X");
        let dims = out["dimensions"].as_object().unwrap();
        assert_eq!(dims.len(), 19);
        // 3/7/8/9/13/14/18 are fixed; 18_trap is the safe default of 9
        assert_eq!(dims["18_trap"]["score"], 9);
        assert_eq!(dims["3_macro"]["score"], 6);
        // 12_capital_flow: no unlock data -> +1 over the base 5
        assert_eq!(dims["12_capital_flow"]["score"], 6);
        // 11_governance base 6 + no pledge +1 = 7
        assert_eq!(dims["11_governance"]["score"], 7);
        // the weighted average itself is pinned by the `empty` golden case
        assert!(out["fundamental_score"].as_f64().unwrap() > 0.0);
    }

    #[test]
    fn first_int_extracts_leading_digits_like_re_search() {
        assert_eq!(first_int("5 年 42 分位"), Some(5));
        assert_eq!(first_int("42 分位"), Some(42));
        assert_eq!(first_int("—"), None);
    }

    #[test]
    fn int_div_floors_like_python() {
        assert_eq!(int_div(&serde_json::json!(18), 5), 3);
        assert_eq!(int_div(&serde_json::json!(4), 5), 0);
        assert_eq!(int_div(&serde_json::json!(18.0), 5), 3);
    }

    #[test]
    fn buy_count_only_counts_buy_and_overweight_ratings() {
        let ratings = serde_json::json!({"买入": 11, "增持": 5, "中性": 2, "减持": 1});
        assert_eq!(buy_count(&ratings), 16);
        assert_eq!(buy_count(&serde_json::json!({})), 0);
    }
}
