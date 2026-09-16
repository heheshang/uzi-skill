//! Port of `lib/pipeline/score_fns.py::generate_panel` — the 66-investor rule
//! engine, consensus aggregation, and per-school scoreboard.

use serde_json::{json, Map, Value};
use uzi_core::py::{f0, round, truthy};
use uzi_investors::{crypto_persona_comment, evaluate_investor, investors, persona_comment};

const NEUTRAL_WEIGHT: f64 = 0.6;
const SCORE_WEIGHT: f64 = 0.65;
const VOTE_WEIGHT: f64 = 0.35;
const POLARIZE_K: f64 = 1.30;

/// Python `int(x)` — truncates toward zero.
fn py_int(v: &Value) -> i64 {
    match v {
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                i
            } else {
                n.as_f64().unwrap_or(0.0).trunc() as i64
            }
        }
        Value::Bool(b) => {
            if *b {
                1
            } else {
                0
            }
        }
        Value::String(s) => uzi_core::py::parse_float(s)
            .map(|x| x.trunc() as i64)
            .unwrap_or(0),
        _ => 0,
    }
}

/// `d.get(key, default)` — defaults only when the key is ABSENT (a JSON `null`
/// stays `null`, matching Python's `dict.get` with a default).
fn get_or<'a>(v: &'a Value, key: &str, default: &'a Value) -> &'a Value {
    v.get(key).unwrap_or(default)
}

fn score_to_verdict(score: i64, signal: &str) -> &'static str {
    if signal == "bullish" && score >= 80 {
        return "强烈买入";
    }
    if signal == "bullish" {
        return "买入";
    }
    if signal == "bearish" && score <= 20 {
        return "回避";
    }
    if signal == "bearish" {
        return "观望";
    }
    if score >= 50 {
        "关注"
    } else {
        "观望"
    }
}

/// 极化拉伸 · 50 为中心 · 距离 × k · 裁剪到 [0, 100].
fn polarize(c: f64, k: f64) -> f64 {
    (50.0 + (c - 50.0) * k).clamp(0.0, 100.0)
}

fn consensus_to_verdict(c: f64) -> &'static str {
    if c >= 80.0 {
        "重仓"
    } else if c >= 65.0 {
        "买入"
    } else if c >= 50.0 {
        "关注"
    } else if c >= 35.0 {
        "谨慎"
    } else {
        "回避"
    }
}

/// Group display metadata, in upstream `GROUP_META` key order.
const GROUP_META: &[(&str, &str, &str)] = &[
    ("A", "经典价值派", "巴菲特 / 格雷厄姆 / 费雪 / 芒格 一脉"),
    ("B", "成长派", "彼得·林奇 / 欧奈尔 / 蒂尔 / 伍德 一脉"),
    ("C", "宏观派", "索罗斯 / 达利欧 / 马克斯 一脉"),
    ("D", "技术派", "利弗莫尔 / Minervini / 达瓦斯 一脉"),
    ("E", "中式价投", "段永平 / 张坤 / 朱少醒 / 冯柳 一脉"),
    ("F", "A 股游资", "龙虎榜顶流 23 位·章盟主/孙哥/赵老哥为代表"),
    ("G", "量化派", "Simons / Thorp / Shaw 一脉"),
    ("H", "科技领袖派", "黄仁勋 / 马斯克 / Altman / Saylor 一脉"),
    ("I", "AI 卡位/瓶颈猎手", "Serenity · AI 供应链卡脖子/瓶颈点"),
];
const CRYPTO_GROUP_META: &[(&str, &str, &str)] = &[
    ("A", "加密价值派", "货币属性 / 链上价值 / 网络安全边际"),
    ("B", "协议成长派", "代币增长 / 协议垄断 / 开放网络创新"),
    ("C", "加密宏观派", "反身性 / 流动性 / 周期与风险"),
    ("D", "链上技术派", "趋势 / 突破 / 市场结构"),
    ("E", "网络长期派", "协议质量 / 代币价值 / 网络复利"),
    ("F", "加密资金派", "仅保留具备加密市场能力圈的资金席位"),
    ("G", "加密量化派", "统计套利 / 风险定价 / 系统交易"),
    ("H", "加密科技派", "算力生态 / 开放协议 / 应用扩散"),
    ("I", "AI 加密卡位派", "AI 与加密基础设施的关键位置"),
];

/// Rule-engine panel: every investor's verdict cites the specific criteria that
/// were hit or missed, then consensus is aggregated continuously + discretely.
pub fn generate_panel(_dims_scored: &Value, raw: &Value) -> Value {
    let dims = raw.get("dimensions").cloned().unwrap_or(json!({}));
    let features = uzi_features::extract_features(raw, &dims);

    let empty = json!({});
    let basic_ctx = dims
        .get("0_basic")
        .and_then(|d| d.get("data"))
        .unwrap_or(&empty);
    let kline_ctx = dims
        .get("2_kline")
        .and_then(|d| d.get("data"))
        .unwrap_or(&empty);
    let fin_ctx = dims
        .get("1_financials")
        .and_then(|d| d.get("data"))
        .unwrap_or(&empty);

    let mut investors_out: Vec<Value> = Vec::new();
    let mut vote_dist: Map<String, Value> = [
        "strongly_buy",
        "buy",
        "watch",
        "wait",
        "avoid",
        "n_a",
        "skip",
    ]
    .iter()
    .map(|k| (k.to_string(), json!(0)))
    .collect();
    let mut sig_dist: Map<String, Value> = ["bullish", "neutral", "bearish", "skip"]
        .iter()
        .map(|k| (k.to_string(), json!(0)))
        .collect();

    let is_crypto = features.get("market").and_then(Value::as_str) == Some("C");
    for inv in investors() {
        let inv_id = inv.get("id").and_then(|v| v.as_str()).unwrap_or("");
        let mandate = inv.get("mandate").and_then(|v| v.as_str()).unwrap_or("long");
        let group = inv.get("group").cloned().unwrap_or(Value::Null);

        let verdict_obj = evaluate_investor(inv_id, &features);
        let display_name = if is_crypto && verdict_obj.get("signal").and_then(Value::as_str) != Some("skip") {
            verdict_obj.get("name").cloned().unwrap_or_else(|| inv.get("name").cloned().unwrap_or(Value::Null))
        } else {
            inv.get("name").cloned().unwrap_or(Value::Null)
        };
        let sig = verdict_obj
            .get("signal")
            .and_then(|v| v.as_str())
            .unwrap_or("neutral")
            .to_string();
        let mut score = py_int(&verdict_obj["score"]).max(0);
        let mut confidence = py_int(&verdict_obj["confidence"]);

        let (verdict, headline, comment, reasoning);
        if sig == "skip" {
            verdict = "不适合".to_string();
            score = 0;
            confidence = 0;
            let skip_reason = verdict_obj
                .get("skip_reason")
                .and_then(|v| v.as_str())
                .unwrap_or("不在能力圈")
                .to_string();
            headline = format!("不适合 — {}", skip_reason);
            comment = format!("不在能力圈范围内，不做评价。\n{}", headline);
            reasoning = verdict_obj
                .get("rationale")
                .or_else(|| verdict_obj.get("reasoning"))
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
        } else {
            verdict = if mandate == "short" && sig == "bearish" {
                "做空候选".to_string()
            } else if mandate == "short" {
                "无明确做空逻辑".to_string()
            } else {
                score_to_verdict(score, &sig).to_string()
            };
            let persona_line = if is_crypto {
                let null = json!(null);
                let ctx = json!({
                    "market_cap_rank": get_or(&features, "market_cap_rank", &null),
                    "nvt_ratio": get_or(&features, "nvt_ratio", &null),
                    "circulating_ratio_pct": get_or(&features, "circulating_ratio_pct", &null),
                    "market_share_pct": get_or(&features, "market_share_pct", &null),
                    "mcap_to_tvl_ratio": get_or(&features, "mcap_to_tvl_ratio", &null),
                    "change_30d_pct": get_or(&features, "change_30d_pct", &null),
                    "fear_greed": get_or(&features, "fear_greed", &null),
                    "funding_rate_pct": get_or(&features, "funding_rate_pct", &null),
                    "ma_align": get_or(&features, "ma_align", &null),
                    "rsi": get_or(&features, "rsi", &null),
                    "volatility_1y": get_or(&features, "volatility_1y", &null),
                    "volume_24h": get_or(&features, "volume_24h", &null),
                    "mcap_to_fdv": get_or(&features, "mcap_to_fdv", &null),
                    "ai_chain_hit": features.get("ai_chain_hit").cloned().unwrap_or(json!(false)),
                });
                crypto_persona_comment(inv_id, &sig, &ctx)
            } else {
                let roe_hist = fin_ctx
                    .get("roe_history")
                    .and_then(|v| v.as_array())
                    .cloned()
                    .unwrap_or_default();
                let roe = match roe_hist.last() {
                    Some(v) => uzi_core::py::num_str(v),
                    None => "—".to_string(),
                };
                let ctx = json!({
                    "name": get_or(basic_ctx, "name", &json!("这只票")),
                    "industry": get_or(basic_ctx, "industry", &json!("该行业")),
                    "price": get_or(basic_ctx, "price", &json!("—")),
                    "pe": get_or(basic_ctx, "pe_ttm", &json!("—")),
                    "roe": roe,
                    "stage": get_or(kline_ctx, "stage", &json!("—")),
                    "growth": get_or(fin_ctx, "revenue_growth", &json!("—")),
                });
                persona_comment(inv_id, &sig, &ctx)
            };
            headline = verdict_obj
                .get("headline")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            comment = if persona_line.is_empty() {
                headline.clone()
            } else {
                format!("{}\n{}", persona_line, headline)
            };
            reasoning = verdict_obj
                .get("rationale")
                .or_else(|| verdict_obj.get("reasoning"))
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
        }

        let v_key = match verdict.as_str() {
            "强烈买入" => "strongly_buy",
            "买入" => "buy",
            "关注" => "watch",
            "观望" => "wait",
            "回避" => "avoid",
            "不适合" => "skip",
            _ => "n_a",
        };
        if mandate != "short" {
            let entry = vote_dist.entry(v_key.to_string()).or_insert(json!(0));
            *entry = json!(entry.as_i64().unwrap_or(0) + 1);
            let entry = sig_dist.entry(sig.clone()).or_insert(json!(0));
            *entry = json!(entry.as_i64().unwrap_or(0) + 1);
        }

        let shorten = |rules: &Value| -> Value {
            let Some(arr) = rules.as_array() else {
                return json!([]);
            };
            Value::Array(
                arr.iter()
                    .take(4)
                    .map(|r| {
                        json!({
                            "name": r.get("name").cloned().unwrap_or(Value::Null),
                            "msg": r.get("msg").cloned().unwrap_or(Value::Null),
                            "weight": r.get("weight").cloned().unwrap_or(Value::Null),
                        })
                    })
                    .collect(),
            )
        };
        let pass = shorten(verdict_obj.get("pass_rules").unwrap_or(&json!([])));
        let fail = shorten(verdict_obj.get("fail_rules").unwrap_or(&json!([])));

        let group_str = group.as_str().unwrap_or("");
        investors_out.push(json!({
            "investor_id": inv_id,
            "name": display_name,
            "group": group,
            "mandate": mandate,
            "avatar": format!("avatars/{}.svg", inv_id),
            "signal": sig,
            "confidence": confidence,
            "score": score,
            "verdict": verdict,
            "reasoning": reasoning,
            "comment": comment,
            "headline": headline,
            "pass": pass,
            "fail": fail,
            "weight_pass": verdict_obj.get("weight_pass").cloned().unwrap_or(Value::Null),
            "weight_total": verdict_obj.get("weight_total").cloned().unwrap_or(Value::Null),
            "ideal_price": Value::Null,
            "period": if matches!(group_str, "A" | "B" | "E") { "中长线" } else { "短线" },
            "time_horizon": verdict_obj.get("time_horizon").cloned().unwrap_or_else(|| json!("—")),
            "position_sizing": verdict_obj.get("position_sizing").cloned().unwrap_or_else(|| json!("—")),
            "what_would_change_my_mind": verdict_obj.get("what_would_change_my_mind").cloned().unwrap_or_else(|| json!("—")),
        }));
    }

    // ── Consensus: 0.65 × continuous score mean + 0.35 × weighted votes ──
    let sig_snapshot = sig_dist.clone();
    let bullish = sig_snapshot["bullish"].as_i64().unwrap_or(0);
    let neutral = sig_snapshot["neutral"].as_i64().unwrap_or(0);
    let bearish = sig_snapshot["bearish"].as_i64().unwrap_or(0);
    let active_count = bullish + neutral + bearish;

    let active_scores: Vec<i64> = investors_out
        .iter()
        .filter(|m| {
            m.get("mandate").and_then(|v| v.as_str()) != Some("short")
                && m.get("signal").and_then(|v| v.as_str()) != Some("skip")
        })
        .map(|m| py_int(&m["score"]))
        .collect();
    let score_mean = if active_scores.is_empty() {
        50.0
    } else {
        active_scores.iter().sum::<i64>() as f64 / active_scores.len() as f64
    };
    let vote_weighted =
        (bullish as f64 + NEUTRAL_WEIGHT * neutral as f64) / active_count.max(1) as f64 * 100.0;
    let consensus_raw = SCORE_WEIGHT * score_mean + VOTE_WEIGHT * vote_weighted;
    let consensus = polarize(consensus_raw, POLARIZE_K);

    // ── Short book ──
    let short_book: Vec<&Value> = investors_out
        .iter()
        .filter(|m| m.get("mandate").and_then(|v| v.as_str()) == Some("short"))
        .collect();
    let short_active: Vec<&&Value> = short_book
        .iter()
        .filter(|m| m.get("signal").and_then(|v| v.as_str()) != Some("skip"))
        .collect();
    let short_scores: Vec<i64> = short_active.iter().map(|m| py_int(&m["score"])).collect();
    let mut sorted_short: Vec<&&Value> = short_active.clone();
    sorted_short.sort_by_key(|m| py_int(&m["score"]));
    let short_consensus = json!({
        "total": short_book.len(),
        "active": short_active.len(),
        "skip": short_book.len() - short_active.len(),
        "short_candidates": short_active.iter().filter(|m| m.get("signal").and_then(|v| v.as_str()) == Some("bearish")).count(),
        "no_short_thesis": short_active.iter().filter(|m| matches!(m.get("signal").and_then(|v| v.as_str()), Some("bullish") | Some("neutral"))).count(),
        "avg_score": if short_scores.is_empty() { 50.0 } else { round(short_scores.iter().sum::<i64>() as f64 / short_scores.len() as f64, 1) },
        "top_short_candidates": Value::Array(sorted_short.iter().take(5).map(|m| json!({
            "id": m.get("investor_id").cloned().unwrap_or(Value::Null),
            "name": m.get("name").cloned().unwrap_or(Value::Null),
            "score": m.get("score").cloned().unwrap_or(Value::Null),
            "headline": m.get("headline").cloned().unwrap_or(Value::Null),
        })).collect()),
    });

    // ── Per-school scores ──
    let mut by_group: std::collections::BTreeMap<String, Vec<&Value>> =
        std::collections::BTreeMap::new();
    for inv in investors_out.iter() {
        let g = inv
            .get("group")
            .and_then(|v| v.as_str())
            .unwrap_or("?")
            .to_string();
        by_group.entry(g).or_default().push(inv);
    }

    let mut school_scores = Map::new();
    for (g, all_members) in by_group.iter() {
        let members: Vec<&&Value> = all_members
            .iter()
            .filter(|m| m.get("mandate").and_then(|v| v.as_str()) != Some("short"))
            .collect();
        let n_members = members.len();
        let active_m: Vec<&&&Value> = members
            .iter()
            .filter(|m| m.get("signal").and_then(|v| v.as_str()) != Some("skip"))
            .collect();
        let n_active = active_m.len();
        let g_bull = active_m
            .iter()
            .filter(|m| m.get("signal").and_then(|v| v.as_str()) == Some("bullish"))
            .count();
        let g_neu = active_m
            .iter()
            .filter(|m| m.get("signal").and_then(|v| v.as_str()) == Some("neutral"))
            .count();
        let g_bear = active_m
            .iter()
            .filter(|m| m.get("signal").and_then(|v| v.as_str()) == Some("bearish"))
            .count();
        let g_skip = members
            .iter()
            .filter(|m| m.get("signal").and_then(|v| v.as_str()) == Some("skip"))
            .count();

        let (s_score_mean, s_vote, s_consensus) = if n_active > 0 {
            let mean = active_m
                .iter()
                .map(|m| f0(&m["score"]))
                .sum::<f64>()
                / n_active as f64;
            let vote = (g_bull as f64 + NEUTRAL_WEIGHT * g_neu as f64) / n_active as f64 * 100.0;
            let raw = SCORE_WEIGHT * mean + VOTE_WEIGHT * vote;
            (mean, vote, polarize(raw, POLARIZE_K))
        } else {
            (0.0, 0.0, 0.0)
        };

        let dominant = if n_active > 0 {
            let mut best = ("bullish", g_bull);
            if g_neu > best.1 {
                best = ("neutral", g_neu);
            }
            if g_bear > best.1 {
                best = ("bearish", g_bear);
            }
            best.0
        } else {
            "skip"
        };

        let meta_table = if is_crypto { CRYPTO_GROUP_META } else { GROUP_META };
        let meta = meta_table
            .iter()
            .find(|(key, _, _)| key == g)
            .map(|(_, label, desc)| (*label, *desc))
            .unwrap_or((g.as_str(), ""));
        school_scores.insert(
            g.clone(),
            json!({
                "group": g,
                "label": meta.0,
                "desc": meta.1,
                "n_members": n_members,
                "n_active": n_active,
                "short_excluded": all_members.len() - n_members,
                "consensus": round(s_consensus, 1),
                "avg_score": round(s_score_mean, 1),
                "vote_consensus": round(s_vote, 1),
                "score_mean": round(s_score_mean, 1),
                "verdict": if n_active > 0 { consensus_to_verdict(s_consensus) } else { "不适合" },
                "bullish": g_bull,
                "neutral": g_neu,
                "bearish": g_bear,
                "skip": g_skip,
                "dominant_signal": dominant,
            }),
        );
    }

    // ── Hollow-verdict guard ──
    let active_long: Vec<&Value> = investors_out
        .iter()
        .filter(|i| {
            i.get("mandate").and_then(|v| v.as_str()) != Some("short")
                && i.get("signal").and_then(|v| v.as_str()) != Some("skip")
        })
        .collect();
    let hollow_ids: Vec<String> = active_long
        .iter()
        .filter(|i| {
            py_int(&i["score"]) == 0 && !truthy(&i["pass"]) && !truthy(&i["fail"])
        })
        .filter_map(|i| {
            i.get("investor_id")
                .and_then(|v| v.as_str())
                .map(str::to_string)
        })
        .collect();
    let hollow_pct = if active_long.is_empty() {
        0.0
    } else {
        round(hollow_ids.len() as f64 / active_long.len() as f64 * 100.0, 0)
    };
    let consensus_valid = hollow_pct < 20.0;
    let consensus_warning = if consensus_valid {
        Value::Null
    } else {
        json!(format!(
            "共识分不可采信：{}/{} 位多头评委（{:.0}%）没有任何有效规则证据。",
            hollow_ids.len(),
            active_long.len(),
            hollow_pct
        ))
    };

    json!({
        "ticker": raw.get("ticker").cloned().unwrap_or(Value::Null),
        "panel_consensus": round(consensus, 1),
        "consensus_valid": consensus_valid,
        "hollow_verdicts": hollow_ids.len(),
        "hollow_pct": hollow_pct,
        "hollow_ids": hollow_ids,
        "consensus_warning": consensus_warning,
        "vote_distribution": Value::Object(vote_dist),
        "signal_distribution": Value::Object(sig_dist),
        "investors": investors_out,
        "school_scores": Value::Object(school_scores),
        "long_active": active_count,
        "short_consensus": short_consensus,
        "consensus_formula": {
            "version": "v2.15.5 · polarize(0.65*score_mean + 0.35*vote_weighted, k=1.3)",
            "score_weight": SCORE_WEIGHT,
            "vote_weight": VOTE_WEIGHT,
            "neutral_weight": NEUTRAL_WEIGHT,
            "polarize_k": POLARIZE_K,
            "score_mean": round(score_mean, 2),
            "vote_weighted": round(vote_weighted, 2),
            "consensus_raw": round(consensus_raw, 2),
            "consensus_final": round(consensus, 2),
            "bullish": bullish,
            "neutral_weighted": round(neutral as f64 * NEUTRAL_WEIGHT, 2),
            "bearish": sig_snapshot["bearish"],
            "skip": sig_snapshot["skip"],
            "active": active_count,
            "short_excluded": short_book.len(),
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verdict_and_polarize_boundaries() {
        assert_eq!(score_to_verdict(80, "bullish"), "强烈买入");
        assert_eq!(score_to_verdict(79, "bullish"), "买入");
        assert_eq!(score_to_verdict(100, "bearish"), "观望");
        assert_eq!(score_to_verdict(20, "bearish"), "回避");
        assert_eq!(score_to_verdict(21, "bearish"), "观望");
        assert_eq!(score_to_verdict(50, "neutral"), "关注");
        assert_eq!(score_to_verdict(49, "neutral"), "观望");

        assert_eq!(polarize(50.0, 1.3), 50.0);
        assert_eq!(polarize(0.0, 1.3), 0.0);
        assert_eq!(polarize(100.0, 1.3), 100.0);
        assert!((polarize(70.0, 1.3) - 76.0).abs() < 1e-9);

        assert_eq!(consensus_to_verdict(80.0), "重仓");
        assert_eq!(consensus_to_verdict(64.9), "关注");
        assert_eq!(consensus_to_verdict(34.9), "回避");
    }

    #[test]
    fn py_int_truncates_toward_zero() {
        assert_eq!(py_int(&json!(3.9)), 3);
        assert_eq!(py_int(&json!(-3.9)), -3);
        assert_eq!(py_int(&json!(7)), 7);
        assert_eq!(py_int(&json!(null)), 0);
    }

    #[test]
    fn get_or_defaults_only_on_absent_keys() {
        let v = json!({"a": null, "b": 1});
        assert_eq!(get_or(&v, "a", &json!("fallback")), &Value::Null);
        assert_eq!(get_or(&v, "zz", &json!("fallback")), &json!("fallback"));
    }
}
