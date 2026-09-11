//! Port of `lib/daily_screen/personas.py` — F-school tape patterns plus the
//! cross-market Serenity evaluation.
//!
//! Every string (including the reasoning template) is reproduced verbatim: the
//! reasoning text is part of the rendered report.

use serde_json::{Map, Value};

use super::events::{filter_evidence, is_business_evidence};
use super::models::{PersonaVerdict, StockSnapshot};

/// `PersonaSpec` dataclass.
pub struct PersonaSpec {
    pub investor_id: &'static str,
    pub name: &'static str,
    pub style: &'static str,
    pub must_have: &'static [&'static str],
    pub positive_patterns: &'static [&'static str],
    pub veto_rules: &'static [&'static str],
    pub entry_condition: &'static str,
    pub invalidation: &'static str,
    pub horizon: &'static str,
}

/// `F_PERSONAS` — order is the report's evaluator column order.
pub const F_PERSONAS: &[PersonaSpec] = &[
    PersonaSpec {
        investor_id: "zhang_mz",
        name: "章盟主",
        style: "容量趋势",
        must_have: &["capacity", "trend_up"],
        positive_patterns: &["leader", "theme_hot"],
        veto_rules: &["illiquid", "isolated"],
        entry_condition: "主线核心维持放量承接",
        invalidation: "跌破上午均价且板块转弱",
        horizon: "1-3d",
    },
    PersonaSpec {
        investor_id: "sun_ge",
        name: "孙哥",
        style: "板块引导",
        must_have: &["theme_hot", "leader"],
        positive_patterns: &["breadth", "active"],
        veto_rules: &["isolated"],
        entry_condition: "板块宽度继续扩散",
        invalidation: "个股强而板块不响应",
        horizon: "1-3d",
    },
    PersonaSpec {
        investor_id: "zhao_lg",
        name: "赵老哥",
        style: "一二板定龙头",
        must_have: &["strong", "leader"],
        positive_patterns: &["fresh_theme", "active"],
        veto_rules: &["extended", "laggard"],
        entry_condition: "前排换手确认且题材保持新鲜",
        invalidation: "后排化或题材快速退潮",
        horizon: "1-3d",
    },
    PersonaSpec {
        investor_id: "fs_wyj",
        name: "佛山无影脚",
        style: "超跌反核",
        must_have: &["small_cap", "reversal"],
        positive_patterns: &["active"],
        veto_rules: &["large_cap", "illiquid"],
        entry_condition: "反核后承接不破启动位",
        invalidation: "冲高无承接或退出流动性下降",
        horizon: "1-3d",
    },
    PersonaSpec {
        investor_id: "yangjia",
        name: "炒股养家",
        style: "情绪周期",
        must_have: &["theme_hot"],
        positive_patterns: &["breadth", "strong"],
        veto_rules: &["risk_off", "climax"],
        entry_condition: "赚钱效应继续向同题材扩散",
        invalidation: "高位一致转分歧且亏钱效应扩散",
        horizon: "1-3d",
    },
    PersonaSpec {
        investor_id: "chen_xq",
        name: "陈小群",
        style: "龙头全周期",
        must_have: &["leader", "strong"],
        positive_patterns: &["reversal", "hard_event"],
        veto_rules: &["laggard", "priced_in"],
        entry_condition: "总龙分歧转一致并带动板块",
        invalidation: "后排跟风或消息兑现后失去承接",
        horizon: "1-3d",
    },
    PersonaSpec {
        investor_id: "hu_jl",
        name: "呼家楼",
        style: "板块平铺协同",
        must_have: &["breadth", "theme_hot"],
        positive_patterns: &["capacity", "active"],
        veto_rules: &["isolated"],
        entry_condition: "多标的同步放量",
        invalidation: "协同消失并退化为单票脉冲",
        horizon: "1-3d",
    },
    PersonaSpec {
        investor_id: "fang_xx",
        name: "方新侠",
        style: "大成交趋势",
        must_have: &["capacity", "trend_up"],
        positive_patterns: &["near_high", "leader"],
        veto_rules: &["illiquid"],
        entry_condition: "大成交趋势保持在均价上方",
        invalidation: "放量跌破趋势承接位",
        horizon: "1-3d",
    },
    PersonaSpec {
        investor_id: "zuoshou",
        name: "作手新一",
        style: "主线接力",
        must_have: &["theme_hot", "leader"],
        positive_patterns: &["reversal", "active"],
        veto_rules: &["laggard", "competition"],
        entry_condition: "弱转强或回封确认",
        invalidation: "竞争板抢位且自身地位下降",
        horizon: "1-3d",
    },
    PersonaSpec {
        investor_id: "xiao_ey",
        name: "小鳄鱼",
        style: "基本面盘面共振",
        must_have: &["trend_up", "fundamental_proxy"],
        positive_patterns: &["theme_hot", "hard_event"],
        veto_rules: &["governance_risk"],
        entry_condition: "业务证据与盘面同时增强",
        invalidation: "纯概念冲高或基本面证据被证伪",
        horizon: "1-3d",
    },
    PersonaSpec {
        investor_id: "jiao_yy",
        name: "交易猿",
        style: "容量龙头加速",
        must_have: &["capacity", "leader"],
        positive_patterns: &["strong", "near_high"],
        veto_rules: &["small_cap", "failed_acceleration"],
        entry_condition: "渡劫后再次放量确认",
        invalidation: "加速失败并跌破上午均价",
        horizon: "1-3d",
    },
    PersonaSpec {
        investor_id: "mao_lb",
        name: "毛老板",
        style: "AI 主线重仓",
        must_have: &["ai_chain", "capacity"],
        positive_patterns: &["hard_event", "leader"],
        veto_rules: &["fake_ai"],
        entry_condition: "AI 硬催化和容量承接同时成立",
        invalidation: "仅关键词无订单或产业证据",
        horizon: "1-3d",
    },
    PersonaSpec {
        investor_id: "xiao_xian",
        name: "消闲派",
        style: "超预期加速",
        must_have: &["hard_event", "strong"],
        positive_patterns: &["leader", "near_high"],
        veto_rules: &["priced_in", "laggard"],
        entry_condition: "预期上修后强度可持续",
        invalidation: "低于预期或补涨末端",
        horizon: "1-3d",
    },
    PersonaSpec {
        investor_id: "lasa",
        name: "拉萨天团",
        style: "散户拥挤反向",
        must_have: &["crowded"],
        positive_patterns: &["active"],
        veto_rules: &["climax"],
        entry_condition: "只作为拥挤观察信号",
        invalidation: "拥挤继续上升时禁止追涨",
        horizon: "1-3d",
    },
    PersonaSpec {
        investor_id: "chengdu",
        name: "成都帮",
        style: "底部点火",
        must_have: &["low_position", "active"],
        positive_patterns: &["reversal", "hard_event"],
        veto_rules: &["extended"],
        entry_condition: "低位直线放量后回踩不破",
        invalidation: "高位接力或消息无发酵",
        horizon: "1-3d",
    },
    PersonaSpec {
        investor_id: "sunan",
        name: "苏南帮",
        style: "低价小盘联动",
        must_have: &["small_cap", "breadth"],
        positive_patterns: &["low_price", "active"],
        veto_rules: &["large_cap"],
        entry_condition: "区域/题材小票同步",
        invalidation: "大市值或只有单票异动",
        horizon: "1-3d",
    },
    PersonaSpec {
        investor_id: "ningbo_st",
        name: "宁波桑田路",
        style: "连板接力",
        must_have: &["strong", "leader"],
        positive_patterns: &["active", "theme_hot"],
        veto_rules: &["failed_acceleration"],
        entry_condition: "梯队高度和换手继续确认",
        invalidation: "断板修复弱或板块高度下降",
        horizon: "1-3d",
    },
    PersonaSpec {
        investor_id: "liuyi_zl",
        name: "六一中路",
        style: "题材龙头接力",
        must_have: &["theme_hot", "leader"],
        positive_patterns: &["hard_event", "active"],
        veto_rules: &["stale_theme", "laggard"],
        entry_condition: "主线回流且龙头确认",
        invalidation: "旧题材无回流或后排套利",
        horizon: "1-3d",
    },
    PersonaSpec {
        investor_id: "liu_sh",
        name: "流沙河",
        style: "分歧低吸",
        must_have: &["trend_up", "not_extended"],
        positive_patterns: &["reversal", "active"],
        veto_rules: &["breakdown", "climax"],
        entry_condition: "分歧低吸后出现承接",
        invalidation: "一致缩量或破位无承接",
        horizon: "1-3d",
    },
    PersonaSpec {
        investor_id: "gu_bl",
        name: "古北路",
        style: "活跃容量",
        must_have: &["active", "capacity"],
        positive_patterns: &["theme_hot", "leader"],
        veto_rules: &["sample_low"],
        entry_condition: "滚动活跃且板块启动",
        invalidation: "样本不足或板块未启动",
        horizon: "1-3d",
    },
    PersonaSpec {
        investor_id: "bj_cj",
        name: "北京炒家",
        style: "首板",
        must_have: &["mid_cap", "strong"],
        positive_patterns: &["active", "fresh_theme"],
        veto_rules: &["extended", "illiquid"],
        entry_condition: "上午放量首板代理条件成立",
        invalidation: "午后被动跟风或流动性下降",
        horizon: "1-3d",
    },
    PersonaSpec {
        investor_id: "wang_zr",
        name: "瑞鹤仙",
        style: "强势题材",
        must_have: &["strong", "active"],
        positive_patterns: &["theme_hot", "leader"],
        veto_rules: &["illiquid", "climax"],
        entry_condition: "强势题材保持辨识度",
        invalidation: "次日追高拥挤或交易活跃度下降",
        horizon: "1-3d",
    },
    PersonaSpec {
        investor_id: "xin_dd",
        name: "鑫多多",
        style: "困境反转",
        must_have: &["low_position", "reversal"],
        positive_patterns: &["hard_event", "small_cap"],
        veto_rules: &["extended", "priced_in"],
        entry_condition: "低位反转催化获盘口确认",
        invalidation: "高位纯接力或预期已兑现",
        horizon: "1-3d",
    },
    PersonaSpec {
        investor_id: "ghzw",
        name: "股海贼王",
        style: "主线接力与格局票",
        must_have: &["theme_hot", "leader"],
        positive_patterns: &["active", "capacity", "hard_event"],
        veto_rules: &["laggard", "failed_acceleration"],
        entry_condition: "涨停原因、板块地位与大盘环境同时成立",
        invalidation: "非主线、承接差或高位竞争失败",
        horizon: "1-3d",
    },
];

const AI_TERMS: &[&str] = &[
    "AI",
    "人工智能",
    "算力",
    "光模块",
    "CPO",
    "半导体",
    "芯片",
    "PCB",
    "铜箔",
    "机器人",
    "液冷",
    "电源",
    "数据中心",
];

/// `build_features` result: an ordered key → `bool | None` map.
pub struct Features {
    entries: Vec<(&'static str, Option<bool>)>,
}

impl Features {
    /// `features.get(key)` (`None` = JSON `null`).
    pub fn get(&self, key: &str) -> Option<bool> {
        self.entries
            .iter()
            .find(|(k, _)| *k == key)
            .and_then(|(_, v)| *v)
    }

    /// `json`-friendly view, in upstream literal order.
    pub fn to_value(&self) -> Value {
        let mut out = Map::new();
        for (key, value) in &self.entries {
            out.insert(
                (*key).to_string(),
                match value {
                    Some(b) => Value::Bool(*b),
                    None => Value::Null,
                },
            );
        }
        Value::Object(out)
    }
}

/// `build_features(stock, theme, evidence)`.
pub fn build_features(stock: &StockSnapshot, theme: &Value, evidence: Option<&[Value]>) -> Features {
    let evidence = evidence.unwrap_or(&[]);
    let (evidence, _) = filter_evidence(evidence, &stock.observed_at);
    let cap = stock.market_cap.unwrap_or(0.0);
    let has_hard_event = evidence.iter().any(is_business_evidence);
    let haystack = format!("{} {}", stock.name, stock.industry).to_lowercase();
    let ai_chain = AI_TERMS
        .iter()
        .any(|term| haystack.contains(&term.to_lowercase()));
    let near_high = matches!(stock.high, Some(high) if high != 0.0 && stock.price >= high * 0.985);
    let reversal = matches!(stock.open_price, Some(open) if open != 0.0)
        && matches!(stock.prev_close, Some(prev) if prev != 0.0)
        && stock.open_price.unwrap_or(0.0) < stock.prev_close.unwrap_or(0.0)
        && stock.change_pct > 1.0;
    let theme_num = |key: &str, default: f64| -> f64 {
        match theme.get(key) {
            Some(v) if uzi_core::py::truthy(v) => uzi_core::py::f0(v),
            _ => default,
        }
    };
    let leader_rank = theme_num("leader_rank", 999.0);
    let theme_rank = theme_num("theme_rank", 999.0);
    let breadth_pct = theme_num("breadth_pct", 0.0);
    let turnover = stock.turnover_rate.unwrap_or(0.0);
    let volume_ratio = stock.volume_ratio.unwrap_or(0.0);

    let b = |v: bool| Some(v);
    let entries: Vec<(&'static str, Option<bool>)> = vec![
        ("illiquid", b(stock.amount < 2e8)),
        ("capacity", b(stock.amount >= 10e8)),
        ("active", b(turnover >= 3.0 || volume_ratio >= 1.2)),
        ("trend_up", None), // A trend requires historical bars.
        ("strong", b(stock.change_pct >= 4.0)),
        (
            "extended",
            b(stock.change_pct >= if stock.market == "A" { 9.5 } else { 12.0 }),
        ),
        ("not_extended", b(stock.change_pct < 7.0)),
        ("near_high", b(near_high)),
        ("reversal", b(reversal)),
        ("leader", b(leader_rank <= 2.0)),
        ("laggard", b(leader_rank > 5.0)),
        ("theme_hot", b(theme_rank <= 5.0 && breadth_pct >= 55.0)),
        ("breadth", b(breadth_pct >= 65.0)),
        ("isolated", b(breadth_pct < 40.0)),
        ("fresh_theme", None), // A single snapshot cannot establish theme age.
        ("stale_theme", None),
        ("small_cap", b(cap > 0.0 && cap <= 100e8)),
        ("mid_cap", b(cap >= 20e8 && cap <= 800e8)),
        ("large_cap", b(cap > 800e8)),
        ("low_price", b(stock.price <= 20.0)),
        ("low_position", None), // Requires price history, not today's return.
        ("crowded", b(turnover >= 12.0 || stock.change_pct >= 9.0)),
        ("climax", b(stock.change_pct >= 9.5)),
        ("hard_event", b(has_hard_event)),
        ("priced_in", b(stock.change_pct >= 8.0 && has_hard_event)),
        ("ai_chain", b(ai_chain)),
        ("fake_ai", b(ai_chain && !has_hard_event)),
        ("fundamental_proxy", b(has_hard_event)),
        (
            "governance_risk",
            b(evidence
                .iter()
                .any(|item| uzi_core::py::get(item, "risk").as_str() == Some("governance"))),
        ),
        (
            "competition",
            b(breadth_pct > 80.0 && leader_rank > 2.0),
        ),
        (
            "failed_acceleration",
            b(stock.change_pct < 2.0 && !near_high),
        ),
        ("breakdown", b(stock.change_pct < -2.0)),
        ("risk_off", None),
        (
            "sample_low",
            b(!evidence
                .iter()
                .any(|item| uzi_core::py::get(item, "kind").as_str() == Some("lhb"))),
        ),
    ];
    Features { entries }
}

fn join_keys(keys: &[String]) -> String {
    if keys.is_empty() {
        "无".to_string()
    } else {
        keys.join(", ")
    }
}

/// `evaluate_f_personas(stock, theme, evidence)`.
pub fn evaluate_f_personas(
    stock: &StockSnapshot,
    theme: &Value,
    evidence: Option<&[Value]>,
) -> Vec<PersonaVerdict> {
    if stock.market != "A" {
        return F_PERSONAS
            .iter()
            .map(|spec| PersonaVerdict {
                investor_id: spec.investor_id.to_string(),
                name: spec.name.to_string(),
                style: spec.style.to_string(),
                eligible: false,
                signal: "skip".to_string(),
                confidence: 0,
                matched_patterns: Vec::new(),
                vetoes: Vec::new(),
                reasoning_summary:
                    "港股不适用 A 股涨停板、T+1 与龙虎榜人物生态。".to_string(),
                entry_condition: "不适用".to_string(),
                invalidation: "不适用".to_string(),
                horizon: spec.horizon.to_string(),
            })
            .collect();
    }

    let features = build_features(stock, theme, evidence);
    let mut verdicts = Vec::with_capacity(F_PERSONAS.len());
    for spec in F_PERSONAS {
        let mut matched: Vec<String> = Vec::new();
        for key in spec.must_have.iter().chain(spec.positive_patterns.iter()) {
            if features.get(key) == Some(true) {
                matched.push((*key).to_string());
            }
        }
        let missing: Vec<String> = spec
            .must_have
            .iter()
            .filter(|key| features.get(key) != Some(true))
            .map(|key| (*key).to_string())
            .collect();
        let vetoes: Vec<String> = spec
            .veto_rules
            .iter()
            .filter(|key| features.get(key) == Some(true))
            .map(|key| (*key).to_string())
            .collect();
        let unknown_vetoes: Vec<String> = spec
            .veto_rules
            .iter()
            .filter(|key| features.get(key).is_none())
            .map(|key| (*key).to_string())
            .collect();

        let must_ratio = (spec.must_have.len() - missing.len()) as f64
            / std::cmp::max(1, spec.must_have.len()) as f64;
        let raw = 38.0 + must_ratio * 34.0 + matched.len() as f64 * 5.0
            - vetoes.len() as f64 * 16.0;
        let confidence = uzi_core::py::round0(raw.min(92.0)).max(0.0) as i64;

        let signal = if !vetoes.is_empty() {
            "bearish"
        } else if missing.is_empty()
            && unknown_vetoes.is_empty()
            && matched.len() >= spec.must_have.len() + 1
        {
            "bullish"
        } else {
            "neutral"
        };

        let mut reasoning = format!(
            "规则初筛（非 Agent 研判）· {}：命中 {}",
            spec.style,
            join_keys(&matched)
        );
        if !missing.is_empty() {
            reasoning.push_str(&format!("；缺少 {}", missing.join(", ")));
        }
        if !vetoes.is_empty() {
            reasoning.push_str(&format!("；否决 {}", vetoes.join(", ")));
        }
        if !unknown_vetoes.is_empty() {
            reasoning.push_str(&format!("；风险待核验 {}", unknown_vetoes.join(", ")));
        }

        verdicts.push(PersonaVerdict {
            investor_id: spec.investor_id.to_string(),
            name: spec.name.to_string(),
            style: spec.style.to_string(),
            eligible: true,
            signal: signal.to_string(),
            confidence,
            matched_patterns: matched,
            vetoes,
            reasoning_summary: reasoning,
            entry_condition: spec.entry_condition.to_string(),
            invalidation: spec.invalidation.to_string(),
            horizon: spec.horizon.to_string(),
        });
    }
    verdicts
}

/// `evaluate_serenity(stock, theme, evidence)`.
pub fn evaluate_serenity(
    stock: &StockSnapshot,
    theme: &Value,
    evidence: Option<&[Value]>,
) -> PersonaVerdict {
    let features = build_features(stock, theme, evidence);
    let hard = features.get("hard_event") == Some(true);
    let ai = features.get("ai_chain") == Some(true);
    let (signal, confidence, reasoning) = if ai && hard {
        (
            "bullish",
            78,
            "AI 供应链位置与 A/B 级业务证据同时命中，进入卡位候选。",
        )
    } else if ai {
        (
            "neutral",
            56,
            "命中 AI 供应链关键词，但缺订单、认证、量产或财报贡献等硬证据，只能观察。",
        )
    } else {
        (
            "bearish",
            35,
            "未识别到可验证的 AI 供应链瓶颈位置，不进入 Serenity 核心池。",
        )
    };
    PersonaVerdict {
        investor_id: "serenity".to_string(),
        name: "Serenity".to_string(),
        style: "AI 供应链卡位".to_string(),
        eligible: true,
        signal: signal.to_string(),
        confidence,
        matched_patterns: if ai {
            vec!["ai_chain".to_string()]
        } else {
            Vec::new()
        },
        vetoes: if hard {
            Vec::new()
        } else {
            vec!["hard_evidence_missing".to_string()]
        },
        reasoning_summary: reasoning.to_string(),
        entry_condition: "等待客户验证、订单或供需紧张证据与价格承接共振".to_string(),
        invalidation: "出现可替代方案、供给转松或估值已完全反映".to_string(),
        horizon: "1-4q".to_string(),
    }
}
