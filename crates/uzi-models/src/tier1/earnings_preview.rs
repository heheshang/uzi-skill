//! Port of `lib/tier1/earnings_preview.py` — pre-earnings preview
//! (A-share / HK / US adaptation).

use crate::{dim_data, get_or, pnum, py_str_py};
use serde_json::{json, Map, Value};
use uzi_core::py::round;

use chrono::Datelike;

use crate::clock;

/// Sector → watch-metric mapping (A-share local dimensions included).
const SECTOR_METRICS: &[(&[&str], &[&str])] = &[
    (
        &["软件", "saas", "云", "互联网", "信息技术", "应用"],
        &["ARR / 经常性收入", "净留存率 NRR", "RPO 在手合同", "付费客户数", "云收入占比"],
    ),
    (
        &["零售", "消费", "商超", "电商", "连锁", "餐饮"],
        &["同店销售 SSSG", "客流量", "客单价", "线上占比", "存货周转"],
    ),
    (
        &["工业", "机械", "设备", "制造", "工程", "军工"],
        &["在手订单 / backlog", "book-to-bill", "量 vs 价拆分", "产能利用率"],
    ),
    (
        &["银行", "保险", "证券", "金融", "信托"],
        &["净息差 NIM", "不良率 / 拨备", "信贷增速", "中间业务收入", "AUM"],
    ),
    (
        &["医药", "生物", "医疗", "制药", "疫苗", "器械"],
        &["核心品种放量", "处方量 / 入院", "集采影响", "在研管线进度"],
    ),
    (
        &["白酒", "酒", "食品饮料", "调味"],
        &["动销 / 终端动销", "渠道库存", "吨价 / 提价", "预收款（合同负债）"],
    ),
    (
        &["光模块", "光通信", "光器件", "cpo", "硅光", "光芯片"],
        &["800G/1.6T 出货量", "高端产品占比", "毛利率（供给紧→提价）", "大客户订单能见度"],
    ),
    (
        &["新能源", "光伏", "储能", "锂电", "风电", "电池"],
        &["装机 / 出货量 GW", "单位盈利（元/W·Wh）", "产能利用率", "原材料价格传导"],
    ),
    (
        &["半导体", "芯片", "晶圆", "封装", "材料"],
        &["产能利用率", "ASP / 提价", "库存周期位置", "先进制程占比"],
    ),
    (
        &["汽车", "整车", "零部件", "新能源车"],
        &["销量 / 交付量", "单车 ASP", "毛利率", "新车型周期"],
    ),
];

const DEFAULT_METRICS: &[&str] = &["营收 vs 共识", "毛利率趋势", "经营性现金流", "前瞻指引 vs 共识"];

/// `_sector_watch_metrics`.
fn sector_watch_metrics(industry: &str, name: &str) -> (String, Vec<String>) {
    let blob = format!("{} {}", industry, name).to_lowercase();
    for (kws, metrics) in SECTOR_METRICS {
        if kws.iter().any(|kw| blob.contains(kw)) {
            let label = if industry.is_empty() { "通用" } else { industry };
            return (
                label.to_string(),
                metrics.iter().map(|m| (*m).to_string()).collect(),
            );
        }
    }
    let label = if industry.is_empty() { "通用" } else { industry };
    (
        label.to_string(),
        DEFAULT_METRICS.iter().map(|m| (*m).to_string()).collect(),
    )
}

/// `_build_consensus_table`.
fn build_consensus_table(features: &Value, research: &Value) -> (Value, Vec<String>) {
    let mut rows: Vec<Value> = Vec::new();
    let mut notes: Vec<String> = Vec::new();

    // EPS consensus
    let cons_eps = {
        let a = pnum(uzi_core::py::get(features, "consensus_eps_2026"), 0.0);
        if a != 0.0 {
            a
        } else {
            pnum(uzi_core::py::get(research, "consensus_eps"), 0.0)
        }
    };
    let eps_now = pnum(uzi_core::py::get(features, "eps"), 0.0);
    if cons_eps > 0.0 {
        let yoy = if eps_now > 0.0 {
            json!(round((cons_eps / eps_now - 1.0) * 100.0, 1))
        } else {
            Value::Null
        };
        rows.push(json!({
            "metric": "EPS（市场一致预期）",
            "consensus": crate::num_value(round(cons_eps, 3)),
            "yoy_pct": yoy,
            "source": "6_research 一致预期",
        }));
    } else {
        rows.push(json!({
            "metric": "EPS（市场一致预期）",
            "consensus": null, "yoy_pct": null,
            "source": "⚠️ 需 web 补充（一致预期）",
        }));
        notes.push("EPS 一致预期缺失 → 需 web 搜索补充".to_string());
    }

    // Revenue consensus
    let cons_rev = pnum(uzi_core::py::get(research, "consensus_rev_yi"), 0.0);
    let rev_latest = pnum(uzi_core::py::get(features, "revenue_latest_yi"), 0.0);
    let cagr = pnum(uzi_core::py::get(features, "revenue_growth_3y_cagr"), 0.0);
    if cons_rev > 0.0 {
        let yoy = if rev_latest > 0.0 {
            json!(round((cons_rev / rev_latest - 1.0) * 100.0, 1))
        } else {
            Value::Null
        };
        rows.push(json!({
            "metric": "营收（亿，一致预期）",
            "consensus": crate::num_value(round(cons_rev, 1)),
            "yoy_pct": yoy,
            "source": "6_research 一致预期",
        }));
    } else if rev_latest > 0.0 {
        let proxy = round(rev_latest * (1.0 + cagr / 100.0), 1);
        rows.push(json!({
            "metric": "营收（亿，估算）",
            "consensus": crate::num_value(proxy),
            "yoy_pct": crate::num_value(round(cagr, 1)),
            "source": "⚠️ 无一致预期 → 用 3yCAGR 外推，需 web 补充",
        }));
        notes.push("营收一致预期缺失 → 用历史 3 年 CAGR 外推占位，建议 web 核对".to_string());
    }

    // Target price / rating distribution
    let tp = pnum(uzi_core::py::get(features, "target_price_avg"), 0.0);
    if tp > 0.0 {
        let px = pnum(uzi_core::py::get(features, "price"), 0.0);
        rows.push(json!({
            "metric": "卖方平均目标价",
            "consensus": crate::num_value(round(tp, 2)),
            "yoy_pct": if px > 0.0 { json!(round((tp / px - 1.0) * 100.0, 1)) } else { Value::Null },
            "source": format!(
                "6_research（覆盖 {} 家 / 买入率 {:.0}%）",
                pnum(uzi_core::py::get(features, "research_coverage"), 0.0) as i64,
                pnum(uzi_core::py::get(features, "buy_rating_pct"), 0.0)
            ),
        }));
    }

    (Value::Array(rows), notes)
}

/// `_build_scenarios` — bull / base / bear.
fn build_scenarios(features: &Value, _market: &str) -> Value {
    let rev_latest = pnum(uzi_core::py::get(features, "revenue_latest_yi"), 0.0);
    let cagr = pnum(uzi_core::py::get(features, "revenue_growth_3y_cagr"), 0.0);
    let last_growth = pnum(uzi_core::py::get(features, "revenue_growth_latest"), 0.0);
    let eps_now = pnum(uzi_core::py::get(features, "eps"), 0.0);
    let cons_eps = pnum(uzi_core::py::get(features, "consensus_eps_2026"), 0.0);
    let base_eps = if cons_eps > 0.0 {
        cons_eps
    } else if eps_now > 0.0 {
        eps_now
    } else {
        0.0
    };
    let gm = pnum(uzi_core::py::get(features, "gross_margin"), 0.0);

    let base_g = if cagr != 0.0 || last_growth != 0.0 {
        round((cagr + last_growth) / 2.0, 1)
    } else {
        0.0
    };
    let bull_g = round(base_g + 8.0, 1);
    let bear_g = round(base_g - 8.0, 1);

    let rev = |g: f64| -> Value {
        if rev_latest > 0.0 {
            crate::num_value(round(rev_latest * (1.0 + g / 100.0), 1))
        } else {
            Value::Null
        }
    };
    let eps = |mult: f64| -> Value {
        if base_eps > 0.0 {
            crate::num_value(round(base_eps * mult, 3))
        } else {
            Value::Null
        }
    };

    let vol = pnum(uzi_core::py::get(features, "volatility_1y"), 0.0);
    let hist_move = if vol > 0.0 {
        Some(round(vol / 16.0, 1))
    } else {
        None
    };
    let hm_str = hist_move.map(|v| py_str_py(&crate::num_value(v)));

    let react_note = format!(
        "（历史财报日股价反应可 web 核对：搜 \"{}\" earnings reaction）",
        py_str_py(&get_or(features, "name", json!("该股")))
    );

    let mut scenarios = vec![
        json!({
            "scenario": "bull",
            "label": "🟢 Bull 乐观",
            "revenue_yi": rev(bull_g),
            "revenue_growth_pct": crate::num_value(bull_g),
            "eps": eps(1.12),
            "gross_margin_assumption": if gm != 0.0 {
                format!("毛利率扩张（基准 {:.1}% → 提价/规模效应）", gm)
            } else {
                "毛利率扩张".to_string()
            },
            "triggers": [
                "营收 / 核心运营指标显著超共识（量价齐升）",
                "管理层上修全年指引",
                "高毛利产品占比提升、费用率下降",
            ],
            "expected_stock_reaction": match &hm_str {
                Some(h) => format!("+{}% 量级（参考历史财报日波动）", h),
                None => "上行，幅度参考历史财报日波动".to_string(),
            },
        }),
        json!({
            "scenario": "base",
            "label": "⚪ Base 中性",
            "revenue_yi": rev(base_g),
            "revenue_growth_pct": crate::num_value(base_g),
            "eps": eps(1.0),
            "gross_margin_assumption": if gm != 0.0 {
                format!("毛利率持平（约 {:.1}%）", gm)
            } else {
                "毛利率持平".to_string()
            },
            "triggers": [
                "营收 / EPS 基本符合一致预期（±2%）",
                "指引维持不变",
                "无重大叙事变化",
            ],
            "expected_stock_reaction": match &hm_str {
                Some(h) => format!("±{}% 区间内震荡", h),
                None => "窄幅波动".to_string(),
            },
        }),
        json!({
            "scenario": "bear",
            "label": "🔴 Bear 悲观",
            "revenue_yi": rev(bear_g),
            "revenue_growth_pct": crate::num_value(bear_g),
            "eps": eps(0.85),
            "gross_margin_assumption": if gm != 0.0 {
                format!("毛利率收缩（基准 {:.1}% → 竞争/成本压力）", gm)
            } else {
                "毛利率收缩".to_string()
            },
            "triggers": [
                "营收 / 核心指标不及共识，或环比走弱",
                "管理层下修指引 / 谨慎措辞",
                "毛利率受成本或价格战拖累",
            ],
            "expected_stock_reaction": match &hm_str {
                Some(h) => format!("-{}% 量级（参考历史财报日波动）", h),
                None => "下行，幅度参考历史财报日波动".to_string(),
            },
        }),
    ];
    for s in scenarios.iter_mut() {
        if let Value::Object(m) = s {
            m.insert("reaction_note".into(), Value::String(react_note.clone()));
        }
    }
    Value::Array(scenarios)
}

/// `_build_catalyst_checklist`.
fn build_catalyst_checklist(features: &Value, watch_metrics: &[String]) -> Value {
    let mut checklist: Vec<Value> = Vec::new();
    checklist.push(json!({
        "item": "营收 / EPS vs 一致预期（及 whisper number）",
        "why": "超预期/不及是财报日股价首要驱动；buy-side whisper 常比公开共识更相关（可 web 补充）",
        "importance": "high",
    }));
    checklist.push(json!({
        "item": "前瞻指引 vs 共识（全年营收/利润、capex）",
        "why": "买方更看下一季/全年指引，而非当期数字本身",
        "importance": "high",
    }));
    if !watch_metrics.is_empty() {
        checklist.push(json!({
            "item": format!("行业核心运营指标：{}", watch_metrics[0]),
            "why": "该指标拐点最先反映需求真伪，领先于利润表",
            "importance": "high",
        }));
    }
    checklist.push(json!({
        "item": "毛利率方向（扩张 / 收缩）+ 管理层归因",
        "why": "毛利率趋势决定盈利弹性，叙事比单点数值更影响估值",
        "importance": "medium",
    }));
    if uzi_core::py::truthy(uzi_core::py::get(features, "has_positive_catalyst"))
        || uzi_core::py::truthy(uzi_core::py::get(features, "has_negative_catalyst"))
    {
        checklist.push(json!({
            "item": "战略 / 叙事变化（并购、回购、新品、产能、诉讼）",
            "why": "近期事件流显示存在叙事变量，可能盖过财务数字",
            "importance": "medium",
        }));
    }
    Value::Array(checklist.into_iter().take(5).collect())
}

/// `_build_implied_move`.
fn build_implied_move(features: &Value, market: &str) -> Value {
    let vol = pnum(uzi_core::py::get(features, "volatility_1y"), 0.0);
    let hist_daily = if vol > 0.0 {
        Some(round(vol / 16.0, 1))
    } else {
        None
    };
    let hd_str = hist_daily.map(|v| py_str_py(&crate::num_value(v)));
    if market == "A" {
        return json!({
            "method": "历史财报日波动代替（A 股个股无期权）",
            "options_available": false,
            "implied_move_pct": null,
            "historical_proxy_pct": hist_daily.map(crate::num_value).unwrap_or(Value::Null),
            "note": match &hd_str {
                Some(h) => format!(
                    "A 股无个股期权 → 用历史财报日 ±波动估计预期波幅；年化波动 {:.0}% → 单日近似 ±{}%",
                    vol, h
                ),
                None => "A 股无个股期权，且历史波动数据不足，需 web 补充历史财报日反应".to_string(),
            },
        });
    }
    json!({
        "method": "期权隐含波动（at-the-money straddle）",
        "options_available": true,
        "implied_move_pct": null,
        "historical_proxy_pct": hist_daily.map(crate::num_value).unwrap_or(Value::Null),
        "note": match &hd_str {
            Some(h) => format!(
                "美股/港股可用财报到期 ATM straddle 报价反推 implied move（需 web 补充期权链）；历史波动近似单日 ±{}% 作为下限参考",
                h
            ),
            None => "可用财报到期 ATM straddle 反推 implied move（需 web 补充期权链）".to_string(),
        },
    })
}

/// `build_earnings_preview` — pre-earnings preview, multi-market.
pub fn build_earnings_preview(features: &Value, raw_data: &Value) -> Value {
    let features = features.clone();
    let features = &features;
    let raw_data = raw_data.clone();
    let raw_data = &raw_data;
    let research = dim_data(raw_data, "6_research");

    let name = get_or(features, "name", json!("—"));
    let industry = get_or(features, "industry", json!("—"));
    let market = get_or(features, "market", json!("A"));
    let market = market.as_str().unwrap_or("A").to_string();
    let now = clock::now();

    let q_month = (now.month() - 1) / 3 + 1;
    let report_quarter = format!("{} Q{}", now.year(), q_month);

    let (consensus_table, cons_notes) = build_consensus_table(features, research);
    let industry_str = industry.as_str().unwrap_or("—");
    let name_str = name.as_str().unwrap_or("—");
    let (sector_label, watch_metrics) = sector_watch_metrics(industry_str, name_str);
    let scenarios = build_scenarios(features, &market);
    let catalyst_checklist = build_catalyst_checklist(features, &watch_metrics);
    let implied_move = build_implied_move(features, &market);

    let market_name = match market.as_str() {
        "A" => "A股".to_string(),
        "HK" => "港股".to_string(),
        "US" => "美股".to_string(),
        other => other.to_string(),
    };

    let scenario_growth = |i: usize| -> Value {
        uzi_core::py::get(&scenarios[i], "revenue_growth_pct").clone()
    };
    let methodology_log = json!([
        format!(
            "Step 1 · 公司={}（{}）· 行业={} · 预览季度≈{}",
            py_str_py(&name),
            market_name,
            py_str_py(&industry),
            report_quarter
        ),
        format!(
            "Step 2 · 一致预期对照：{} 行{}",
            consensus_table.as_array().map(|a| a.len()).unwrap_or(0),
            if cons_notes.is_empty() {
                "（数据齐备）".to_string()
            } else {
                format!("（{} 项需 web 补充）", cons_notes.len())
            }
        ),
        format!(
            "Step 3 · 观察指标按行业分类 → {}：{} 项",
            sector_label,
            watch_metrics.len()
        ),
        format!(
            "Step 4 · 三情景：Bull {}% / Base {}% / Bear {}% 营收增速",
            py_str_py(&scenario_growth(0)),
            py_str_py(&scenario_growth(1)),
            py_str_py(&scenario_growth(2))
        ),
        format!(
            "Step 5 · 催化剂清单 {} 项；隐含波动 → {}",
            catalyst_checklist.as_array().map(|a| a.len()).unwrap_or(0),
            py_str_py(uzi_core::py::get(&implied_move, "method"))
        ),
    ]);

    let mut company = Map::new();
    company.insert("name".into(), name);
    company.insert("code".into(), get_or(features, "code", Value::Null));
    company.insert("industry".into(), industry);
    company.insert("market".into(), Value::String(market.clone()));

    json!({
        "method": "Earnings Preview (pre-earnings)",
        "company": Value::Object(company),
        "report_quarter": report_quarter,
        "generated_at": clock::date_str(&now),
        "consensus_table": consensus_table,
        "consensus_notes": Value::Array(cons_notes.into_iter().map(Value::String).collect()),
        "watch_metrics": {"sector": sector_label, "metrics": watch_metrics},
        "scenarios": scenarios,
        "catalyst_checklist": catalyst_checklist,
        "implied_move": implied_move,
        "methodology_log": methodology_log,
    })
}
