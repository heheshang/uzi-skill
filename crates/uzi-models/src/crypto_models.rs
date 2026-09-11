//! Crypto-native replacements for the institutional dims 20–22.
//!
//! DCF / LBO / three-statement models have no meaning for a token, so the crypto
//! venue gets its own valuation layer with the **same JSON contract** (the
//! renderers and `generate_synthesis` read fixed keys):
//!
//! * dim 20 · NVT 网络价值折现（单位经济学锚）+ 同业市值对比
//! * dim 21 · 首次覆盖（评级/目标价来自 NVT 公允价值）+ 催化剂日历
//! * dim 22 · IC 备忘录 + 竞争格局（Porter + 赛道定位）
//!
//! Nothing is fabricated: when the inputs for the fair-value model are missing
//! the model reports `null` + `不适用`, and the equity-only blocks (DCF/LBO) are
//! deliberately omitted so the renderer skips them instead of mislabeling.

use serde_json::{json, Value};

use crate::{dim_data, get_or};

/// `market == "C"` — read from the features first, then `0_basic.data.market`,
/// then the ticker.
pub fn is_crypto(features: &Value, raw: &Value) -> bool {
    let m = get_or(features, "market", Value::Null);
    if m.as_str() == Some("C") {
        return true;
    }
    if get_or(dim_data(raw, "0_basic"), "market", Value::Null).as_str() == Some("C") {
        return true;
    }
    get_or(raw, "ticker", Value::Null)
        .as_str()
        .map(|t| uzi_core::ticker::parse_ticker(t).market == uzi_core::ticker::CRYPTO_MARKET)
        .unwrap_or(false)
}

/// Round to 2 decimals (local helper — `uzi_core::py::round`).
fn round2(x: f64) -> f64 {
    uzi_core::py::round(x, 2)
}

/// Owned-value convenience wrapper around [`num`].
fn numv(v: Value) -> f64 {
    num(&v)
}

fn num(v: &Value) -> f64 {
    crate::flt(v, 0.0)
}

/// Owned-value convenience wrapper around [`opt`].
fn optv(v: Value) -> Option<f64> {
    opt(&v)
}

fn opt(v: &Value) -> Option<f64> {
    match v {
        Value::Null => None,
        Value::Number(n) => n.as_f64().filter(|x| x.is_finite()),
        Value::String(s) => s.trim().replace([',', '%'], "").parse::<f64>().ok(),
        _ => None,
    }
}

fn currency_symbol(basic: &Value) -> String {
    match get_or(basic, "currency", json!("USD")).as_str().unwrap_or("USD") {
        "CNY" => "¥".to_string(),
        "HKD" => "HK$".to_string(),
        _ => "$".to_string(),
    }
}

/// The inputs the fair-value model needs, or `None` with a reason.
struct FairValue {
    price: f64,
    fair_price: f64,
    target_nvt: f64,
    current_nvt: Option<f64>,
    margin_pct: f64,
    verdict: &'static str,
    method: String,
}

/// NVT-anchored fair value: `fair_mcap = daily_volume × target_NVT`.
///
/// NVT (network value to transactions) is the market cap divided by 24h traded
/// volume; liquid majors historically trade between 20× and 60×. The model
/// inverts that band into a fair market cap and divides by circulating supply.
///
/// Returns `None` when the network has no meaningful turnover (stablecoins,
/// wrapped assets, or missing volume/supply) — the caller then emits
/// `不适用` instead of a fabricated target.
fn fair_value(raw: &Value) -> Option<FairValue> {
    let basic = dim_data(raw, "0_basic");
    let fin = dim_data(raw, "1_financials");
    let val = dim_data(raw, "10_valuation");

    // Stablecoins / wrapped assets are pegged by design — there is no network
    // value to discount. The caller reports `不适用` instead of a target price.
    let sector = get_or(&basic, "industry", json!("")).as_str().unwrap_or("").to_string();
    if sector.contains("稳定币") || sector.contains("封装") {
        return None;
    }
    let price = optv(get_or(&basic, "price", Value::Null))?;
    let mcap = optv(get_or(&basic, "market_cap_raw", Value::Null))
        .or_else(|| optv(get_or(&fin, "market_cap", Value::Null)))?;
    let vol = optv(get_or(&basic, "volume_24h", Value::Null)).or_else(|| {
        // Fall back to market cap × daily turnover when the raw volume is absent.
        let t = optv(get_or(&val, "turnover_ratio", Value::Null))?;
        if t > 0.0 {
            Some(mcap * t)
        } else {
            None
        }
    })?;
    let supply = optv(get_or(&basic, "circulating_supply", Value::Null))
        .or_else(|| optv(get_or(&fin, "circulating_supply", Value::Null)))?;
    if price <= 0.0 || mcap <= 0.0 || vol <= 0.0 || supply <= 0.0 {
        return None;
    }

    let current_nvt = mcap / vol;
    // Target NVT: liquid settlement networks historically trade in a 20–60×
    // band, so majors anchor at the 40× mid-band; younger tokens get 25×.
    let target_nvt = if sector.contains("公链") || sector.contains("L1") || sector.contains("L2") {
        40.0
    } else {
        25.0
    };
    let fair_price = vol * target_nvt / supply;
    let margin_pct = (fair_price - price) / fair_price * 100.0;
    let verdict = if margin_pct >= 25.0 {
        "低估"
    } else if margin_pct >= -25.0 {
        "合理"
    } else {
        "高估"
    };
    Some(FairValue {
        price,
        fair_price,
        target_nvt,
        current_nvt: Some(current_nvt),
        margin_pct,
        verdict,
        method: format!("NVT 网络价值折现 · 目标 NVT {target_nvt:.0}x · 24h 成交额 {:.0}", vol),
    })
}

/// dim 20 · valuation models.
pub fn dim_20(_features: &Value, raw: &Value) -> Value {
    let basic = dim_data(raw, "0_basic");
    let peers = dim_data(raw, "4_peers");
    let fv = fair_value(raw);

    let peer_table: Vec<Value> = peers
        .get("peer_table")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let peer_count = peer_table.len();
    let rank = optv(get_or(&peers, "rank", Value::Null));
    let mcap = optv(get_or(&basic, "market_cap_raw", Value::Null));

    let comps_verdict = match rank {
        Some(r) if r <= 3.0 => "赛道龙头 · 相对市值溢价可接受",
        Some(r) if r <= 10.0 => "一线资产 · 相对估值合理",
        Some(r) if r <= 50.0 => "二三线 · 需要超额叙事支撑",
        Some(_) => "长尾资产 · 流动性与生存风险折价",
        None => "无同业排名数据",
    };

    let summary = json!({
        "dcf_intrinsic": Value::Null,
        "dcf_safety_margin_pct": fv.as_ref().map(|f| crate::num_value(round2(f.margin_pct))).unwrap_or(Value::Null),
        "dcf_verdict": fv.as_ref().map(|f| json!(f.verdict)).unwrap_or(Value::Null),
        "lbo_irr_pct": Value::Null,
        "lbo_verdict": "不适用（加密资产无杠杆收购口径）",
        "comps_verdict": comps_verdict,
        "crypto_fair_price": fv.as_ref().map(|f| crate::num_value(round2(f.fair_price))).unwrap_or(Value::Null),
    });

    let valuation_model = match &fv {
        Some(f) => json!({
            "method": f.method,
            "target_nvt": crate::num_value(f.target_nvt),
            "current_nvt": f.current_nvt.map(|v| crate::num_value(round2(v))).unwrap_or(Value::Null),
            "fair_price": crate::num_value(round2(f.fair_price)),
            "market_price": crate::num_value(round2(f.price)),
            "safety_margin_pct": crate::num_value(round2(f.margin_pct)),
            "verdict": f.verdict,
        }),
        None => json!({
            "method": "NVT 网络价值折现",
            "applicable": false,
            "verdict": "不适用",
            "note": "稳定币/封装资产或缺少成交额·流通量，无法计算网络价值折现",
        }),
    };

    json!({
        "data": {
            "valuation_model": valuation_model,
            "comps": {
                "peer_count": peer_count,
                "market_cap_rank": rank.map(crate::num_value).unwrap_or(Value::Null),
                "target_market_cap": mcap.map(crate::num_value).unwrap_or(Value::Null),
                "peer_table": peer_table,
                "valuation_verdict": comps_verdict,
            },
            "summary": summary,
            "asset_class": "crypto",
        },
        "source": "compute:crypto_models (NVT 网络价值折现 + 同业市值)",
        "fallback": false,
    })
}

/// dim 21 · initiating coverage + catalysts.
pub fn dim_21(_features: &Value, raw: &Value, _d20: &Value) -> Value {
    let basic = dim_data(raw, "0_basic");
    let kline = dim_data(raw, "2_kline");
    let sent = dim_data(raw, "17_sentiment");
    let trap = dim_data(raw, "18_trap");
    let events = dim_data(raw, "15_events");
    let cur = currency_symbol(&basic);
    let fv = fair_value(raw);

    let price = optv(get_or(&basic, "price", Value::Null));
    let (rating, target, upside) = match &fv {
        Some(f) => {
            let rating = if f.margin_pct >= 30.0 {
                "买入"
            } else if f.margin_pct >= 10.0 {
                "增持"
            } else if f.margin_pct >= -10.0 {
                "持有"
            } else if f.margin_pct >= -30.0 {
                "减持"
            } else {
                "卖出"
            };
            (rating, Some(f.fair_price), Some(f.margin_pct))
        }
        None => ("未评级", None, None),
    };

    let name = get_or(&basic, "name", json!("该币种"));
    let fng = optv(get_or(&sent, "thermometer_value", Value::Null));
    let stage = get_or(&kline, "stage", json!("—"));
    let risk = numv(get_or(&trap, "risk_score", json!(0)));
    let ath_dd = optv(get_or(&basic, "ath_change_pct", Value::Null));

    let summary_text = format!(
        "{name} 现价 {cur}{}。{}  技术面 {}，恐慌贪婪 {}{}。",
        price.map(|p| format!("{p}")).unwrap_or_else(|| "—".to_string()),
        match &fv {
            Some(f) => format!(
                "按 NVT 目标 {:.0}x 推算公允价值 {cur}{:.2}，安全边际 {:+.0}%（{}）。",
                f.target_nvt, f.fair_price, f.margin_pct, f.verdict
            ),
            None => "缺少成交额/流通量，无法给出网络价值折现目标价。".to_string(),
        },
        stage.as_str().unwrap_or("—"),
        fng.map(|v| format!("{v:.0}")).unwrap_or_else(|| "—".to_string()),
        ath_dd.map(|d| format!(" · 距 ATH {d:.0}%")).unwrap_or_default(),
    );

    let mut thesis: Vec<Value> = Vec::new();
    if let Some(f) = &fv {
        thesis.push(json!({
            "pillar": "网络价值 / 成交额",
            "weight": "核心",
            "evidence": format!("当前 NVT {:.1}，目标 {:.0}x", f.current_nvt.unwrap_or(0.0), f.target_nvt),
        }));
    }
    if let Some(r) = optv(get_or(dim_data(raw, "4_peers"), "rank", Value::Null)) {
        thesis.push(json!({
            "pillar": "市值地位",
            "weight": "重要",
            "evidence": format!("全市场市值排名 #{r:.0}"),
        }));
    }
    let dev = get_or(dim_data(raw, "6_research"), "developer", json!({}));
    let commits = numv(get_or(&dev, "commit_count_4_weeks", json!(0)));
    thesis.push(json!({
        "pillar": "开发者/社区",
        "weight": "重要",
        "evidence": format!("近 4 周代码提交 {commits:.0} 次"),
    }));

    let mut risks: Vec<Value> = Vec::new();
    if risk >= 30.0 {
        risks.push(json!({
            "risk": "投机/流动性风险",
            "severity": if risk >= 60.0 { "高" } else { "中" },
            "detail": format!("本地风险评分 {risk:.0}/100"),
        }));
    }
    risks.push(json!({
        "risk": "监管不确定性",
        "severity": "中",
        "detail": "各司法辖区对加密资产的合规口径仍在变化",
    }));
    risks.push(json!({
        "risk": "解锁与通胀抛压",
        "severity": "中",
        "detail": format!("未流通代币 {}", get_or(dim_data(raw, "11_governance"), "unvested_supply_pct", json!("—"))),
    }));

    let news_events: Vec<Value> = events
        .get("news")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .take(8)
        .map(|n| {
            json!({
                "event": get_or(&n, "title", Value::Null),
                "impact": "medium",
                "date": get_or(&n, "time", Value::Null),
                "source": get_or(&n, "source", Value::Null),
            })
        })
        .collect();

    let thesis_intact = 100.0 - risk.min(100.0);

    json!({
        "data": {
            "initiating_coverage": {
                "headline": {
                    "rating": rating,
                    "target_price": target.map(|t| crate::num_value(round2(t))).unwrap_or(Value::Null),
                    "current_price": price.map(|p| crate::num_value(round2(p))).unwrap_or(Value::Null),
                    "upside_pct": upside.map(|u| crate::num_value(round2(u))).unwrap_or(Value::Null),
                },
                "executive_summary": summary_text,
                "investment_thesis": thesis,
                "key_risks": risks,
            },
            "catalyst_calendar": {"events": news_events, "next_30d": []},
            "thesis_tracker": {"thesis_intact_pct": crate::num_value(round2(thesis_intact))},
            "earnings_analysis": {
                "headline": "加密资产无季度财报 · 关注代币解锁、网络活跃度与监管节点",
                "next_report": Value::Null,
            },
            "morning_note": {
                "headline": format!("{name} 盘面速览"),
                "body": summary_text,
            },
            "idea_screens": {},
            "sector_overview": {
                "industry": get_or(&basic, "industry", json!("加密货币")),
                "market_share_pct": get_or(dim_data(raw, "7_industry"), "market_share_pct", Value::Null),
            },
            "summary": {
                "rec_rating": rating,
                "target_price": target.map(|t| crate::num_value(round2(t))).unwrap_or(Value::Null),
                "upside_pct": upside.map(|u| crate::num_value(round2(u))).unwrap_or(Value::Null),
                "thesis_intact_pct": crate::num_value(round2(thesis_intact)),
                "next_high_impact_event": news_events
                    .first()
                    .and_then(|e| e.get("event").cloned())
                    .unwrap_or(Value::Null),
                "earnings_headline": Value::Null,
                "screens_passed": 0,
            },
            "asset_class": "crypto",
        },
        "source": "compute:crypto_models (initiating coverage + catalysts)",
        "fallback": false,
    })
}

/// dim 22 · IC memo + competitive positioning.
pub fn dim_22(_features: &Value, raw: &Value, _d20: &Value, _d21: &Value) -> Value {
    let basic = dim_data(raw, "0_basic");
    let ind = dim_data(raw, "7_industry");
    let moat = dim_data(raw, "14_moat");
    let trap = dim_data(raw, "18_trap");
    let mac = dim_data(raw, "3_macro");
    let fv = fair_value(raw);

    let name = get_or(&basic, "name", json!("该资产"));
    let share = numv(get_or(&ind, "market_share_pct", json!(0)));
    let moat_scores = get_or(&moat, "scores", json!({}));
    let moat_total = numv(get_or(&moat_scores, "total", json!(0)));
    let risk = numv(get_or(&trap, "risk_score", json!(0)));
    let mcap_chg = numv(get_or(&mac, "mcap_change_24h_pct", json!(0)));
    let fng = numv(get_or(dim_data(raw, "17_sentiment"), "thermometer_value", json!(50)));

    // ── Competitive analysis (crypto reading of Porter + BCG) ──
    let new_entrants = if share > 5.0 { 2.0 } else { 4.5 };
    let substitutes = 4.0; // another L1 / another store-of-value asset
    let supplier_power = if moat_total >= 7.0 { 2.5 } else { 3.5 }; // validators/miners/MMs
    let buyer_power = 3.0;
    let rivalry = if share > 5.0 { 3.0 } else { 4.5 };
    let attractiveness = (50.0 + mcap_chg * 3.0 + share * 2.0 + (50.0 - fng) * 0.2 - risk * 0.2)
        .clamp(0.0, 100.0);
    let (bcg_cat, bcg_action) = if share >= 5.0 && mcap_chg > 0.0 {
        ("Star (明星)", "继续投入 · 巩固网络效应")
    } else if share >= 5.0 {
        ("Cash Cow (现金牛)", "维持运营 · 最大化现金流回收")
    } else if mcap_chg > 0.0 {
        ("Question Mark (问号)", "选择性投入 / 或被头部虹吸")
    } else {
        ("Dog (瘦狗)", "收缩 / 规避流动性枯竭")
    };

    let ic_rec = match &fv {
        Some(f) if f.margin_pct >= 25.0 && risk < 40.0 => format!("🟢 建议配置 · {} 公允价值 {:+.0}%", name, f.margin_pct),
        Some(f) if f.margin_pct <= -25.0 => format!("🔴 建议减持 · {} 高于公允价值 {:.0}%", name, f.margin_pct.abs()),
        Some(_) => format!("🟡 观察 · {} 接近公允价值区间", name),
        None => format!("⚪ 数据不足 · {} 无法给出 NVT 目标价", name),
    };

    let dd_completion = {
        let basic_ok = [
            get_or(&basic, "price", Value::Null),
            get_or(&basic, "market_cap_raw", Value::Null),
            get_or(&basic, "volume_24h", Value::Null),
        ]
        .iter()
        .filter(|v| opt(v).map(|x| x > 0.0).unwrap_or(false))
        .count();
        let extra_ok = [
            get_or(dim_data(raw, "1_financials"), "circulating_supply", Value::Null),
            get_or(dim_data(raw, "2_kline"), "stage", Value::Null),
            get_or(&ind, "market_share_pct", Value::Null),
            get_or(&moat, "scores", Value::Null),
            get_or(dim_data(raw, "9_futures"), "linked_contract", Value::Null),
        ]
        .iter()
        .filter(|v| !v.is_null())
        .count();
        crate::num_value(round2((basic_ok + extra_ok) as f64 / 8.0 * 100.0))
    };

    let unit_verdict = match &fv {
        Some(f) => format!(
            "NVT {:.1}（目标 {:.0}x）· 单位网络价值 {}",
            f.current_nvt.unwrap_or(0.0),
            f.target_nvt,
            f.verdict
        ),
        None => "网络单位经济不适用（稳定币/封装资产）".to_string(),
    };


    json!({
        "data": {
            "ic_memo": {
                "sections": {
                    "I_exec_summary": {"headline": ic_rec},
                    "VI_risks_mitigants": [
                        {"risk": "流动性风险", "mitigant": "只在主流交易所/深度池交易"},
                        {"risk": "监管风险", "mitigant": "关注主要辖区合规进展"},
                        {"risk": "解锁抛压", "mitigant": "跟踪流通率与解锁日历"},
                    ],
                    "VII_returns_scenarios": Value::Null,
                },
            },
            "unit_economics": {
                "verdict": unit_verdict,
                "target_nvt": fv.as_ref().map(|f| crate::num_value(f.target_nvt)).unwrap_or(Value::Null),
                "current_nvt": fv.as_ref().and_then(|f| f.current_nvt).map(|v| crate::num_value(round2(v))).unwrap_or(Value::Null),
            },
            "value_creation_plan": {
                "levers": [
                    {"lever": "网络效应扩张", "current_state": format!("市值占比 {share:.2}%"), "target_state": "提升赛道份额"},
                    {"lever": "开发者生态", "current_state": format!("护城河评分 {moat_total:.1}/10"), "target_state": "提高提交与集成数"},
                ],
                "methodology_log": ["加密资产的\"价值创造\"= 网络使用量 × 货币溢价，而非产能扩张"],
            },
            "dd_checklist": {"completion_pct": dd_completion},
            "competitive_analysis": {
                "porter_five_forces": {
                    "new_entrants_threat": {"score": crate::num_value(new_entrants)},
                    "substitutes_threat": {"score": crate::num_value(substitutes)},
                    "supplier_power": {"score": crate::num_value(supplier_power)},
                    "buyer_power": {"score": crate::num_value(buyer_power)},
                    "rivalry_intensity": {"score": crate::num_value(rivalry)},
                },
                "bcg_position": {
                    "category": bcg_cat,
                    "market_share_pct": crate::num_value(round2(share)),
                    "market_growth_pct": crate::num_value(round2(mcap_chg)),
                    "market_growth_basis": "加密大盘 24h 市值变化（赛道增长代理）",
                    "strategic_action": bcg_action,
                },
                "industry_attractiveness_pct": crate::num_value(round2(attractiveness)),
            },
            "portfolio_rebalance": Value::Null,
            "summary": {
                "ic_recommendation": ic_rec,
                "bcg_position": bcg_cat,
                "industry_attractiveness": crate::num_value(round2(attractiveness)),
                "dd_completion_pct": dd_completion,
                "value_creation_uplift_yi": Value::Null,
                "unit_economics_verdict": unit_verdict,
            },
            "asset_class": "crypto",
        },
        "source": "compute:crypto_models (IC memo + Porter/BCG)",
        "fallback": false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn btc_raw() -> Value {
        json!({
            "ticker": "BTC-USD",
            "dimensions": {
                "0_basic": {"data": {
                    "name": "Bitcoin", "price": 60000.0, "market_cap_raw": 1.18e12,
                    "volume_24h": 3.0e10, "circulating_supply": 1.97e7,
                    "industry": "L1 公链", "currency": "USD", "market": "C",
                    "ath_change_pct": -20.0,
                }},
                "1_financials": {"data": {"circulating_supply": 1.97e7, "market_cap": 1.18e12}},
                "4_peers": {"data": {"rank": 1, "peer_table": [{"name": "Bitcoin"}]}},
                "10_valuation": {"data": {"nvt_ratio": 22.0}},
            }
        })
    }

    #[test]
    fn detects_crypto_venue() {
        assert!(is_crypto(&json!({"market": "C"}), &json!({})));
        assert!(is_crypto(&json!({}), &btc_raw()));
        assert!(!is_crypto(&json!({"market": "A"}), &json!({"ticker": "600519.SH"})));
    }

    #[test]
    fn nvt_fair_value_is_computed_from_turnover() {
        let raw = btc_raw();
        let fv = fair_value(&raw).expect("bitcoin has turnover");
        assert!((fv.target_nvt - 40.0).abs() < 1e-9);
        // fair mcap = 3e10 × 40; fair price = that / 1.97e7
        let expected = 3.0e10 * 40.0 / 1.97e7;
        assert!((fv.fair_price - expected).abs() < 1.0);
        assert_eq!(fv.verdict, "合理");
    }

    #[test]
    fn dims_emit_the_shared_contract_keys() {
        let raw = btc_raw();
        let d20 = dim_20(&json!({"market": "C"}), &raw);
        assert!(d20["data"]["summary"]["dcf_safety_margin_pct"].is_number());
        assert!(d20["data"]["valuation_model"]["fair_price"].is_number());
        let d21 = dim_21(&json!({"market": "C"}), &raw, &d20["data"]);
        assert!(d21["data"]["initiating_coverage"]["headline"]["target_price"].is_number());

        let d22 = dim_22(&json!({"market": "C"}), &raw, &d20["data"], &d21["data"]);
        assert!(d22["data"]["ic_memo"]["sections"]["I_exec_summary"]["headline"].is_string());
        assert!(d22["data"]["competitive_analysis"]["industry_attractiveness_pct"].is_number());
        // Equity-only blocks stay absent so the DCF/LBO cards never render.
        assert!(d20["data"].get("dcf").is_none());
        assert!(d20["data"].get("lbo").is_none());
    }

    #[test]
    fn stablecoins_are_not_nvt_priced() {
        let mut raw = btc_raw();
        raw["dimensions"]["0_basic"]["data"]["industry"] = json!("稳定币");
        assert!(fair_value(&raw).is_none());
        let d21 = dim_21(&json!({"market": "C"}), &raw, &json!({}));
        assert_eq!(d21["data"]["initiating_coverage"]["headline"]["rating"], "未评级");
    }
}
