//! Port of `lib/investor_evaluator.py` — the rule-engine executor that turns
//! `(investor_id, features)` into a quantified verdict.
//!
//! Three layers, exactly like upstream:
//!   1. reality check (`investor_knowledge.reality_check`)
//!   2. rule engine (`investor_criteria.INVESTOR_RULES`)
//!   3. composite: holding bonus + affinity adjustment

use crate::pyhelp::{self as h, Missing};
use crate::{criteria, db, knowledge, profile, seat_db};
use serde_json::{Map, Value};
use uzi_core::py;

/// score ≥ 65 → bullish.
pub const BULLISH_THRESHOLD: f64 = 65.0;
/// score < 35 → bearish.
pub const BEARISH_THRESHOLD: f64 = 35.0;

/// v3.5.0 · 流派标签 · `--school` locks a single school and skips the rest.
pub fn school_labels(key: &str) -> Option<&'static str> {
    Some(match key {
        "A" => "价值派",
        "B" => "成长派",
        "C" => "宏观派",
        "D" => "技术派",
        "E" => "中国价投",
        "F" => "A 股游资",
        "G" => "量化",
        "H" => "科技领袖派",
        "I" => "AI 卡位/瓶颈猎手",
        _ => return None,
    })
}

/// `investor_evaluator.get_locked_school` — `UZI_SCHOOL` env, uppercase, valid
/// single letter or `""`.
pub fn get_locked_school() -> String {
    let raw = std::env::var("UZI_SCHOOL").unwrap_or_default();
    let raw = raw.trim().to_uppercase();
    if school_labels(&raw).is_some() {
        raw
    } else {
        String::new()
    }
}

fn group_of(investor_id: &str) -> String {
    db::investor_by_id(investor_id)
        .and_then(|i| i.get("group"))
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string()
}

fn name_of(investor_id: &str) -> String {
    db::investor_by_id(investor_id)
        .and_then(|i| i.get("name"))
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string()
}

/// Python `f.get(key, default)` as a display string (numbers via `str()`).
fn str_default(f: &Value, key: &str, default: &str) -> String {
    match f.get(key) {
        None => default.to_string(),
        Some(v) => py::num_str(v),
    }
}

/// `float(v)` best effort for `market_cap_yi` (Python `float()` accepts strings).
fn to_float(v: &Value) -> f64 {
    match v {
        Value::Number(n) => n.as_f64().unwrap_or(0.0),
        Value::Bool(b) => {
            if *b {
                1.0
            } else {
                0.0
            }
        }
        Value::String(s) => py::parse_float(s).unwrap_or(0.0),
        _ => 0.0,
    }
}

/// v2.13.3 · F 组游资射程前置检查 (v3.4.5 LHB 反查覆盖).
fn is_youzi_out_of_range(investor_id: &str, features: &Value) -> (bool, String) {
    if group_of(investor_id) != "F" {
        return (false, String::new());
    }
    let nickname = name_of(investor_id);
    if nickname.is_empty() || !seat_db::seats().contains_key(&nickname) {
        return (false, String::new());
    }

    // `mc = features.get("market_cap") or 0` then the 亿 → 元 fallback
    let mut mc = match features.get("market_cap") {
        Some(v) if py::truthy(v) => to_float(v),
        _ => 0.0,
    };
    if mc == 0.0 {
        if let Some(yi) = features.get("market_cap_yi") {
            if py::truthy(yi) {
                mc = to_float(yi) * 1e8;
            }
        }
    }

    let mut probe = features.clone();
    if let Some(obj) = probe.as_object_mut() {
        obj.insert("market_cap".into(), Value::from(mc));
    } else {
        probe = Value::Object(Map::new());
        probe["market_cap"] = Value::from(mc);
    }
    if seat_db::is_in_range(&nickname, &probe) {
        return (false, String::new());
    }

    // v3.4.5 · out of range but the LHB shows the seat actually traded → keep scoring
    let matched = features.get("matched_youzi").and_then(Value::as_array);
    if let Some(matched) = matched {
        if matched.iter().any(|m| m.as_str() == Some(nickname.as_str())) {
            return (false, String::new());
        }
    }

    let mc_yi = if mc != 0.0 { mc / 1e8 } else { 0.0 };
    (true, format!("市值 {:.0} 亿不在 {} 射程", mc_yi, nickname))
}

/// `investor_evaluator._fmt_msg` — f-string over the feature dict; unknown or
/// null placeholders render as `?`; a template that cannot be formatted is
/// returned verbatim.
pub fn fmt_msg(template: &str, features: &Value) -> String {
    if template.is_empty() {
        return String::new();
    }
    h::format_map(
        template,
        &|key| match features.get(key) {
            None | Some(Value::Null) => None,
            Some(v) => Some(v.clone()),
        },
        Missing::Literal("?"),
    )
    .unwrap_or_else(|_| template.to_string())
}

fn rule_entry(rule: &criteria::Rule, msg: String) -> Value {
    let mut m = Map::new();
    m.insert("rule_id".into(), Value::String(rule.rule_id.clone()));
    m.insert("name".into(), Value::String(rule.name.clone()));
    m.insert("weight".into(), Value::from(rule.weight));
    m.insert("msg".into(), Value::String(msg));
    Value::Object(m)
}

/// `investor_evaluator._build_headline`.
fn build_headline(signal: &str, pass_list: &[Value], fail_list: &[Value]) -> String {
    let msg = |v: &Value| v["msg"].as_str().unwrap_or("").to_string();
    if signal == "bullish" && !pass_list.is_empty() {
        return format!("看多核心：{}", msg(&pass_list[0]));
    }
    if signal == "bearish" && !fail_list.is_empty() {
        return format!("看空核心：{}", msg(&fail_list[0]));
    }
    if !pass_list.is_empty() && !fail_list.is_empty() {
        return format!("观望：{}；但 {}", msg(&pass_list[0]), msg(&fail_list[0]));
    }
    if !pass_list.is_empty() {
        return format!("中性：{}", msg(&pass_list[0]));
    }
    if !fail_list.is_empty() {
        return format!("中性：{}", msg(&fail_list[0]));
    }
    "数据不足，暂无判断".to_string()
}

/// `investor_evaluator._build_rationale`.
fn build_rationale(pass_list: &[Value], fail_list: &[Value]) -> String {
    let mut lines: Vec<String> = Vec::new();
    if !pass_list.is_empty() {
        lines.push("✅ 符合标准：".into());
        for r in pass_list.iter().take(4) {
            lines.push(format!(
                "  • [权{}] {}",
                r["weight"].as_i64().unwrap_or(0),
                r["msg"].as_str().unwrap_or("")
            ));
        }
    }
    if !fail_list.is_empty() {
        lines.push("❌ 未达标准：".into());
        for r in fail_list.iter().take(4) {
            lines.push(format!(
                "  • [权{}] {}",
                r["weight"].as_i64().unwrap_or(0),
                r["msg"].as_str().unwrap_or("")
            ));
        }
    }
    if lines.is_empty() {
        "无有效规则命中".to_string()
    } else {
        lines.join("\n")
    }
}

fn profile_fields(investor_id: &str) -> (Value, Value, Value) {
    let p = profile::get_profile(investor_id, &group_of(investor_id));
    (
        p["time_horizon"].clone(),
        p["position_sizing"].clone(),
        p["what_would_change_my_mind"].clone(),
    )
}

/// `investor_evaluator._skip_result`.
fn skip_result(investor_id: &str, reason: &str) -> Value {
    let (th, ps, ww) = profile_fields(investor_id);
    let mut m = Map::new();
    m.insert("investor_id".into(), Value::String(investor_id.into()));
    m.insert("score".into(), Value::from(-1));
    m.insert("signal".into(), Value::String("skip".into()));
    m.insert("confidence".into(), Value::from(0));
    m.insert("weight_pass".into(), Value::from(0));
    m.insert("weight_total".into(), Value::from(0));
    m.insert("pass_count".into(), Value::from(0));
    m.insert("fail_count".into(), Value::from(0));
    m.insert("pass_rules".into(), Value::Array(vec![]));
    m.insert("fail_rules".into(), Value::Array(vec![]));
    m.insert("headline".into(), Value::String(format!("不适合 — {}", reason)));
    m.insert(
        "rationale".into(),
        Value::String(format!("该投资者{}，不对此股票发表意见。", reason)),
    );
    m.insert("skip_reason".into(), Value::String(reason.into()));
    m.insert("time_horizon".into(), th);
    m.insert("position_sizing".into(), ps);
    m.insert("what_would_change_my_mind".into(), ww);
    Value::Object(m)
}

/// `investor_evaluator._unknown_result`.
fn unknown_result(investor_id: &str) -> Value {
    let (th, ps, ww) = profile_fields(investor_id);
    let mut m = Map::new();
    m.insert("investor_id".into(), Value::String(investor_id.into()));
    m.insert("score".into(), Value::from(50.0));
    m.insert("signal".into(), Value::String("neutral".into()));
    m.insert("confidence".into(), Value::from(30));
    m.insert("weight_pass".into(), Value::from(0));
    m.insert("weight_total".into(), Value::from(0));
    m.insert("pass_count".into(), Value::from(0));
    m.insert("fail_count".into(), Value::from(0));
    m.insert("pass_rules".into(), Value::Array(vec![]));
    m.insert("fail_rules".into(), Value::Array(vec![]));
    m.insert("headline".into(), Value::String("该投资者暂无量化评估规则".into()));
    m.insert("rationale".into(), Value::String("此投资者未配置规则库，使用默认中性判断。".into()));
    m.insert("time_horizon".into(), th);
    m.insert("position_sizing".into(), ps);
    m.insert("what_would_change_my_mind".into(), ww);
    Value::Object(m)
}

/// Crypto-facing identity replaces the equity persona while preserving the
/// stable internal investor id used by overrides and cache joins. These names
/// are style lenses, not attributed recommendations or simulated quotations.
fn crypto_identity(investor_id: &str) -> (String, String, String) {
    let identity = match investor_id {
        // A · monetary value and network valuation
        "buffett" => ("中本聪", "Satoshi Nakamoto", "货币属性与去中心化价值"),
        "graham" => ("尼克·萨博", "Nick Szabo", "密码学货币与稀缺性"),
        "fisher" => ("林·奥尔登", "Lyn Alden", "货币网络与宏观价值"),
        "munger" => ("维杰·博亚帕蒂", "Vijay Boyapati", "货币网络采用周期"),
        "templeton" => ("尼克·卡特", "Nic Carter", "网络安全与货币化"),
        "klarman" => ("克里斯·伯尼斯克", "Chris Burniske", "加密资产估值与周期安全边际"),
        // B · protocol growth and adoption
        "lynch" => ("维塔利克·布特林", "Vitalik Buterin", "协议采用与开发者生态"),
        "oneill" => ("阿纳托利·雅科文科", "Anatoly Yakovenko", "高性能网络与应用增长"),
        "thiel" => ("巴拉吉·斯里尼瓦桑", "Balaji Srinivasan", "网络效应与开放协议"),
        "wood" => ("克里斯·迪克森", "Chris Dixon", "开放网络创新周期"),
        "andreessen" => ("弗雷德·埃尔萨姆", "Fred Ehrsam", "协议投资与基础设施"),
        "gurley" => ("凯尔·萨马尼", "Kyle Samani", "加密应用与基础设施增长"),
        "naval" => ("琳达·谢", "Linda Xie", "无需许可创新与长期采用"),
        "gerstner" => ("瑞安·塞尔基斯", "Ryan Selkis", "协议基本面与市场采用"),
        "chamath" => ("阿里·保罗", "Ari Paul", "数字资产组合与非对称收益"),
        // C · liquidity, reflexivity and skeptical short book
        "soros" => ("亚瑟·海斯", "Arthur Hayes", "美元流动性与加密反身性"),
        "dalio" => ("拉乌尔·帕尔", "Raoul Pal", "全球流动性与加密周期"),
        "marks" => ("杰夫·多曼", "Jeff Dorman", "数字资产周期与风险溢价"),
        "druck" => ("保罗·都铎·琼斯", "Paul Tudor Jones", "宏观趋势与比特币周期"),
        "robertson" => ("乔迪·亚历山大", "Jordi Alexander", "衍生品结构与系统风险"),
        "burry" => ("莫莉·怀特", "Molly White", "加密市场风险与反方审视"),
        "chanos" => ("斯蒂芬·迪尔", "Stephen Diehl", "代币价值与反方审视"),
        // D · crypto-native market structure
        "livermore" => ("彼得·勃兰特", "Peter Brandt", "价格结构与趋势交易"),
        "minervini" => ("CryptoCred", "CryptoCred", "市场结构与趋势跟随"),
        "darvas" => ("DonAlt", "DonAlt", "区间突破与动量"),
        "gann" => ("Rekt Capital", "Rekt Capital", "周期结构与关键价位"),
        // E · protocol quality and token design
        "duan" => ("哈苏", "Hasu", "协议机制与代币价值"),
        "zhangkun" => ("马特·胡根", "Matt Hougan", "数字资产质量与组合配置"),
        "zhushaoxing" => ("塔伦·奇特拉", "Tarun Chitra", "协议风险与机制设计"),
        "xiezhiyu" => ("丹·罗宾逊", "Dan Robinson", "DeFi 协议设计与竞争壁垒"),
        "fengliu" => ("大卫·霍夫曼", "David Hoffman", "协议经济与生态采用"),
        "dengxiaofeng" => ("卢卡斯·努齐", "Lucas Nuzzi", "链上数据与网络质量"),
        "zhang_lei" => ("奥拉夫·卡尔森-威", "Olaf Carlson-Wee", "长期协议投资与网络效应"),
        // G · systematic crypto research
        "simons" => ("乔·王", "Qiao Wang", "链上系统研究与市场结构"),
        "thorp" => ("亚历克斯·埃文斯", "Alex Evans", "协议数据与风险定价"),
        "shaw" => ("叶夫根尼·盖沃伊", "Evgeny Gaevoy", "做市流动性与微观结构"),
        "asness" => ("克拉拉·梅达利", "Clara Medalie", "加密市场数据与因子研究"),
        // H/I · open networks and AI-crypto infrastructure
        "jensen_huang" => ("杰克·多尔西", "Jack Dorsey", "开放货币网络与支付基础设施"),
        "musk" => ("加文·伍德", "Gavin Wood", "多链协议与开放网络"),
        "altman" => ("胡安·贝内特", "Juan Benet", "去中心化存储与网络基础设施"),
        "saylor" => ("迈克尔·塞勒", "Michael Saylor", "比特币货币化与储备资产"),
        "serenity" => ("Serenity", "Serenity Crypto AI", "AI 与加密基础设施卡位"),
        _ => {
            let seat = match investor_id {
                "zhang_mz" => 1, "sun_ge" => 2, "zhao_lg" => 3, "fs_wyj" => 4,
                "yangjia" => 5, "chen_xq" => 6, "hu_jl" => 7, "fang_xx" => 8,
                "zuoshou" => 9, "xiao_ey" => 10, "jiao_yy" => 11, "mao_lb" => 12,
                "xiao_xian" => 13, "lasa" => 14, "chengdu" => 15, "sunan" => 16,
                "ningbo_st" => 17, "liuyi_zl" => 18, "liu_sh" => 19, "gu_bl" => 20,
                "bj_cj" => 21, "wang_zr" => 22, "xin_dd" => 23, "ghzw" => 24,
                _ => 0,
            };
            return (
                format!("A股游资席位（不参与）·{seat:02}"),
                format!("Excluded A-share Seat {seat:02}"),
                "市场范围排除".to_string(),
            );
        }
    };
    (identity.0.to_string(), identity.1.to_string(), identity.2.to_string())
}

/// Crypto-native evaluation: each school uses only on-chain, tokenomics,
/// market-structure, liquidity, and protocol-adoption evidence.
fn evaluate_crypto_investor(investor_id: &str, features: &Value) -> Value {
    let group = group_of(investor_id);
    if group == "F" {
        let (name, en, role) = crypto_identity(investor_id);
        let mut result = skip_result(investor_id, "A 股游资不覆盖加密市场");
        if let Some(obj) = result.as_object_mut() {
            obj.insert("name".into(), Value::String(name));
            obj.insert("en".into(), Value::String(en));
            obj.insert("role".into(), Value::String(role));
        }
        return result;
    }
    let num = |key: &str| -> Option<f64> {
        features.get(key).and_then(|v| v.as_f64().or_else(|| {
            v.as_str().and_then(|s| s.trim_end_matches('%').parse().ok())
        }))
    };
    let show = |key: &str, precision: usize| -> String {
        num(key).map(|v| format!("{v:.precision$}")).unwrap_or_else(|| "—".to_string())
    };
    let rank = num("market_cap_rank");
    let nvt = num("nvt_ratio");
    let mcap_fdv = num("mcap_to_fdv");
    let circulating = num("circulating_ratio_pct");
    let drawdown = num("max_drawdown_1y");
    let volatility = num("volatility_1y");
    let change_30d = num("change_30d_pct");
    let fear_greed = num("fear_greed");
    let funding = num("funding_rate_pct");
    let volume = num("volume_24h");
    let tvl_ratio = num("mcap_to_tvl_ratio");
    let network = num("market_share_pct");
    let ai_hit = features.get("ai_chain_hit").and_then(Value::as_bool).unwrap_or(false);
    let trend = features.get("ma_align").and_then(Value::as_str).filter(|s| *s != "—" && *s != "None").map(|s| s.contains("多头"));
    let rsi = num("rsi");
    let (score_raw, rationale, period, sizing): (f64, String, &str, &str) = match group.as_str() {
        "A" => (50.0 + if rank.is_some_and(|v| v <= 10.0) { 10.0 } else { 0.0 } + if nvt.is_some_and(|v| v > 0.0 && v < 35.0) { 15.0 } else if nvt.is_some_and(|v| v > 90.0) { -15.0 } else { 0.0 } + if circulating.is_some_and(|v| v >= 90.0) { 10.0 } else if circulating.is_some_and(|v| v < 60.0) { -10.0 } else { 0.0 } + if drawdown.is_some_and(|v| v.abs() >= 50.0) && fear_greed.is_some_and(|v| v <= 35.0) { 10.0 } else { 0.0 }, format!("关注市值排名、NVT、流通率与回撤：排名 #{}，NVT {}，流通率 {}%", show("market_cap_rank", 0), show("nvt_ratio", 1), show("circulating_ratio_pct", 1)), "2-5 年", "按网络价值与回撤分批建仓"),
        "B" => (50.0 + if network.is_some_and(|v| v >= 10.0) { 12.0 } else { 0.0 } + if tvl_ratio.is_some_and(|v| v < 5.0) { 10.0 } else if tvl_ratio.is_some_and(|v| v > 20.0) { -8.0 } else { 0.0 } + if change_30d.is_some_and(|v| v >= 15.0) { 10.0 } else if change_30d.is_some_and(|v| v <= -20.0) { -8.0 } else { 0.0 } + if mcap_fdv.is_some_and(|v| v >= 0.8) { 8.0 } else if mcap_fdv.is_some_and(|v| v > 0.0 && v < 0.5) { -12.0 } else { 0.0 }, format!("验证网络份额、采用增长与稀释压力：网络份额 {}%，市值/TVL {}，30日涨跌 {}%", show("market_share_pct", 1), show("mcap_to_tvl_ratio", 2), show("change_30d_pct", 1)), "2-5 年", "围绕采用曲线控制稀释风险"),
        "C" => (50.0 + if fear_greed.is_some_and(|v| v <= 25.0) { 12.0 } else if fear_greed.is_some_and(|v| v >= 80.0) { -15.0 } else { 0.0 } + if funding.is_some_and(|v| v.abs() > 0.05) { -10.0 } else if funding.is_some() { 5.0 } else { 0.0 } + if change_30d.is_some_and(|v| v > 20.0) && fear_greed.is_some_and(|v| v > 70.0) { -10.0 } else { 0.0 }, format!("先看风险偏好和杠杆：恐慌贪婪 {}，资金费率 {}%，30日涨跌 {}%", show("fear_greed", 0), show("funding_rate_pct", 4), show("change_30d_pct", 1)), "数周到数月", "按周期与流动性窗口动态调整"),
        "D" => (50.0 + match trend { Some(true) => 15.0, Some(false) => -10.0, None => 0.0 } + if change_30d.is_some_and(|v| v > 10.0) { 10.0 } else if change_30d.is_some_and(|v| v < -15.0) { -10.0 } else { 0.0 } + if rsi.is_some_and(|v| v > 75.0) { -8.0 } else if rsi.is_some_and(|v| v < 30.0) { 5.0 } else { 0.0 }, format!("只交易可验证的结构：MA多头排列 {}，RSI {}，30日涨跌 {}%", match trend { Some(true) => "是", Some(false) => "否", None => "—" }, show("rsi", 1), show("change_30d_pct", 1)), "数天到数月", "破坏结构即止损，绝不摊平"),
        "E" => (50.0 + if rank.is_some_and(|v| v <= 20.0) { 10.0 } else if rank.is_some() { -5.0 } else { 0.0 } + if circulating.is_some_and(|v| v >= 80.0) { 8.0 } else if circulating.is_some() { -8.0 } else { 0.0 } + if network.is_some_and(|v| v >= 5.0) { 8.0 } else { 0.0 } + if nvt.is_some_and(|v| v < 60.0) { 6.0 } else if nvt.is_some() { -6.0 } else { 0.0 }, format!("质量来自网络地位与供给纪律：排名 #{}，流通率 {}%，网络份额 {}%，NVT {}", show("market_cap_rank", 0), show("circulating_ratio_pct", 1), show("market_share_pct", 1), show("nvt_ratio", 1)), "3-7 年", "集中于供给透明、网络效应强的协议"),
        "G" => (50.0 + match trend { Some(true) => 8.0, Some(false) => -5.0, None => 0.0 } + if volatility.is_some_and(|v| v < 60.0) { 10.0 } else if volatility.is_some_and(|v| v > 100.0) { -12.0 } else { 0.0 } + if volume.is_some_and(|v| v > 0.0) { 5.0 } else if volume.is_some() { -15.0 } else { 0.0 } + if funding.is_some_and(|v| v.abs() < 0.03) { 5.0 } else if funding.is_some() { -5.0 } else { 0.0 }, format!("量化评估趋势、波动与可交易性：年化波动 {}%，24小时成交额 {}，资金费率 {}%", show("volatility_1y", 1), show("volume_24h", 0), show("funding_rate_pct", 4)), "数天到数周", "风险平价，按波动率和流动性限仓"),
        "H" => (50.0 + if ai_hit { 15.0 } else { 0.0 } + if network.is_some_and(|v| v >= 10.0) { 10.0 } else { 0.0 } + if rank.is_some_and(|v| v <= 50.0) { 5.0 } else if rank.is_some() { -5.0 } else { 0.0 }, format!("寻找真实的开放网络与应用扩散：AI/基础设施卡位 {}，网络份额 {}%，市值排名 #{}", if ai_hit { "命中" } else { "未命中" }, show("market_share_pct", 1), show("market_cap_rank", 0)), "5-10 年", "重仓清晰的技术范式，路线被绕过则退出"),
        "I" => (50.0 + if ai_hit { 20.0 } else { -10.0 } + if network.is_some_and(|v| v > 5.0) { 8.0 } else { 0.0 } + if mcap_fdv.is_some_and(|v| v >= 0.7) { 5.0 } else if mcap_fdv.is_some() { -5.0 } else { 0.0 }, format!("只接受可验证的 AI 加密卡位：关键词命中 {}，网络份额 {}%，市值/FDV {}", if ai_hit { "是" } else { "否" }, show("market_share_pct", 1), show("mcap_to_fdv", 2)), "6-24 个月", "高信念小仓试错，卡位证伪立即清仓"),
        _ => (50.0, "缺少可归类的加密研究 mandate".to_string(), "—", "观望"),
    };
    let score = score_raw.clamp(0.0, 100.0);
    let signal = if score >= 65.0 { "bullish" } else if score < 35.0 { "bearish" } else { "neutral" };
    let verdict = if signal == "bullish" { "买入" } else if signal == "bearish" { "回避" } else { "观望" };
    let headline = format!("{}：评分 {:.0}/100；{}", if signal == "bullish" { "看多" } else if signal == "bearish" { "看空" } else { "中性" }, score, rationale);
    let (name, en, role) = crypto_identity(investor_id);
    serde_json::json!({"investor_id": investor_id, "name": name, "en": en, "group": group, "role": role, "mandate": "long", "signal": signal, "confidence": (55.0 + (score - 50.0).abs() * 1.5).clamp(0.0, 100.0), "score": score.round() as i64, "verdict": verdict, "reasoning": rationale, "comment": headline, "headline": headline, "pass": [], "fail": [], "weight_pass": 0, "weight_total": 0, "ideal_price": null, "period": period, "time_horizon": period, "position_sizing": sizing, "what_would_change_my_mind": "代币经济、网络采用、流动性或市场结构出现结构性恶化", "skip_reason": null})
}

/// `investor_evaluator.evaluate`.
pub fn evaluate_investor(investor_id: &str, features: &Value) -> Value {
    if features
        .get("market")
        .and_then(Value::as_str)
        .is_some_and(|m| m.eq_ignore_ascii_case("C"))
    {
        return evaluate_crypto_investor(investor_id, features);
    }
    // v3.5.0 · 用户锁定单一流派视角
    let locked = get_locked_school();
    if !locked.is_empty() {
        if group_of(investor_id) != locked {
            let label = school_labels(&locked).unwrap_or(&locked);
            return skip_result(
                investor_id,
                &format!("用户锁定 {} 派视角 · 非该派评委不参与", label),
            );
        }
    }

    // ─── Layer 1: Reality Check ───
    let market = str_default(features, "market", "A");
    let ticker = str_default(features, "ticker", "");
    let name = str_default(features, "name", "");
    let industry = str_default(features, "industry", "");
    let rc = knowledge::reality_check(investor_id, &market, &ticker, &name, &industry);

    if !py::truthy(&rc["should_evaluate"]) {
        let reason = rc["skip_reason"]
            .as_str()
            .filter(|s| !s.is_empty())
            .unwrap_or("不在能力圈");
        return skip_result(investor_id, reason);
    }

    let (out_of_range, range_reason) = is_youzi_out_of_range(investor_id, features);
    if out_of_range {
        return skip_result(investor_id, &range_reason);
    }

    let Some(rules) = criteria::rules_for(investor_id) else {
        return unknown_result(investor_id);
    };
    if rules.is_empty() {
        return unknown_result(investor_id);
    }

    // ─── Layer 2: Rule Engine ───
    let mut pass_list: Vec<Value> = Vec::new();
    let mut fail_list: Vec<Value> = Vec::new();
    let mut weight_pass: i64 = 0;
    let mut weight_total: i64 = 0;

    for rule in rules {
        let Some(ok) = criteria::safe_check(rule, features) else {
            continue; // data missing → rule skipped, weight not counted
        };
        weight_total += rule.weight;
        if ok {
            weight_pass += rule.weight;
            let template = if rule.pass_msg.is_empty() { &rule.name } else { &rule.pass_msg };
            pass_list.push(rule_entry(rule, fmt_msg(template, features)));
        } else {
            let fallback = format!("未达{}", rule.name);
            let template = if rule.fail_msg.is_empty() { fallback.as_str() } else { &rule.fail_msg };
            fail_list.push(rule_entry(rule, fmt_msg(template, features)));
        }
    }

    // ─── Layer 3: Reality Adjustment ───
    let affinity_adj = rc["affinity_adjust"].as_f64().unwrap_or(0.0);
    let holding_match = rc["holding_match"].as_array().cloned();

    if let Some(hm) = &holding_match {
        let attitude = hm.first().and_then(Value::as_str).unwrap_or("");
        let note = hm.get(1).and_then(Value::as_str).unwrap_or("");
        if attitude == "held" || attitude == "bullish_known" {
            pass_list.insert(
                0,
                {
                    let mut m = Map::new();
                    m.insert("rule_id".into(), Value::String("known_holding".into()));
                    m.insert("name".into(), Value::String("实际持仓 / 公开看好".into()));
                    m.insert("weight".into(), Value::from(6));
                    m.insert("msg".into(), Value::String(format!("📌 {}", note)));
                    Value::Object(m)
                },
            );
            weight_pass += 6;
            weight_total += 6;
        }
    }

    let score = if weight_total > 0 {
        py::round(
            (weight_pass as f64 / weight_total as f64) * 100.0 + affinity_adj,
            1,
        )
    } else {
        py::round(50.0 + affinity_adj, 1)
    };
    let score = score.max(0.0).min(100.0);

    let override_signal = rc["override_signal"].as_str().filter(|s| !s.is_empty());
    let signal = if let Some(s) = override_signal {
        s.to_string()
    } else if score >= BULLISH_THRESHOLD {
        "bullish".to_string()
    } else if score < BEARISH_THRESHOLD {
        "bearish".to_string()
    } else {
        "neutral".to_string()
    };

    let n_rules = rules.len() as f64 + if holding_match.is_some() { 1.0 } else { 0.0 };
    let base_conf = (50.0 + n_rules * 8.0).min(100.0);
    let extremeness = (score - 50.0).abs() * 0.6;
    let confidence = py::round((base_conf * 0.6 + 40.0 + extremeness * 0.4).min(100.0), 0);

    pass_list.sort_by(|a, b| {
        b["weight"].as_i64().cmp(&a["weight"].as_i64())
    });
    fail_list.sort_by(|a, b| {
        b["weight"].as_i64().cmp(&a["weight"].as_i64())
    });

    let headline = build_headline(&signal, &pass_list, &fail_list);
    let rationale = build_rationale(&pass_list, &fail_list);
    let (th, ps, ww) = profile_fields(investor_id);

    let mut m = Map::new();
    m.insert("investor_id".into(), Value::String(investor_id.into()));
    m.insert("score".into(), Value::from(score));
    m.insert("signal".into(), Value::String(signal));
    m.insert("confidence".into(), Value::from(confidence));
    m.insert("weight_pass".into(), Value::from(weight_pass));
    m.insert("weight_total".into(), Value::from(weight_total));
    m.insert("pass_count".into(), Value::from(pass_list.len()));
    m.insert("fail_count".into(), Value::from(fail_list.len()));
    m.insert("pass_rules".into(), Value::Array(pass_list));
    m.insert("fail_rules".into(), Value::Array(fail_list));
    m.insert("headline".into(), Value::String(headline));
    m.insert("rationale".into(), Value::String(rationale));
    m.insert("time_horizon".into(), th);
    m.insert("position_sizing".into(), ps);
    m.insert("what_would_change_my_mind".into(), ww);
    Value::Object(m)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn fmt_msg_handles_missing_and_null_placeholders() {
        let f = json!({"pe": 18.5, "name": null});
        assert_eq!(fmt_msg("PE {pe:.1f} 在 {pe_quantile_5y} 分位", &f), "PE 18.5 在 ? 分位");
        assert_eq!(fmt_msg("{name}", &f), "?"); // present null → ?
        assert_eq!(fmt_msg("{unknown} {pe}", &f), "? 18.5");
        assert_eq!(fmt_msg("no placeholders", &f), "no placeholders");
        assert_eq!(fmt_msg("", &f), "");
        // unsupported spec → raw template (Python ValueError fallback)
        assert_eq!(fmt_msg("{pe:.1f} {name:.2f}", &json!({"pe": 1.0, "name": "x"})), "{pe:.1f} {name:.2f}");
    }

    #[test]
    fn signals_follow_the_thresholds() {
        let features = json!({
            "market": "A",
            "name": "测试",
            "industry": "白酒",
            "pe": 10, "pe_quantile_5y": 5, "pb": 1.0, "pe_x_pb": 10,
            "net_margin": 30, "debt_ratio": 20, "moat_total": 30,
            "consecutive_dividend_years": 8, "roe_5y_above_15": 5, "roe_5y_min": 18,
            "fcf_known": true, "fcf_positive": true, "fcf_margin": 12, "current_ratio": 3,
            "consecutive_profit_years": 8, "is_safe": true,
        });
        let r = evaluate_investor("buffett", &features);
        assert_eq!(r["signal"], "bullish");
        assert_eq!(r["weight_pass"], r["weight_total"]);
        assert!(r["score"].as_f64().unwrap() >= 65.0);
        assert_eq!(r["pass_rules"].as_array().unwrap().len(), 7);
        assert!(r["headline"].as_str().unwrap().starts_with("看多核心："));
    }

    #[test]
    fn youzi_out_of_range_is_a_skip_with_the_reason() {
        let features = json!({"market": "A", "name": "宁德时代", "industry": "电池", "market_cap_yi": 9000});
        let r = evaluate_investor("zhao_lg", &features);
        assert_eq!(r["signal"], "skip");
        assert_eq!(r["score"], -1);
        assert_eq!(r["skip_reason"], "市值 9000 亿不在 赵老哥 射程");
        assert_eq!(r["headline"], "不适合 — 市值 9000 亿不在 赵老哥 射程");

        // LHB override: the seat actually traded → evaluate anyway
        let mut with_lhb = features.clone();
        with_lhb["matched_youzi"] = json!(["赵老哥"]);
        let r2 = evaluate_investor("zhao_lg", &with_lhb);
        assert_ne!(r2["signal"], "skip");
    }

    #[test]
    fn market_scope_skips_youzi_outside_a_shares() {
        let r = evaluate_investor("zhao_lg", &json!({"market": "US", "name": "Apple"}));
        assert_eq!(r["signal"], "skip");
        assert_eq!(r["skip_reason"], "不看US市场");
    }

    #[test]
    fn output_key_order_matches_the_documented_schema() {
        let r = evaluate_investor("buffett", &json!({"market": "A", "name": "x"}));
        let keys: Vec<&str> = r.as_object().unwrap().keys().map(String::as_str).collect();
        assert_eq!(
            keys,
            vec![
                "investor_id",
                "score",
                "signal",
                "confidence",
                "weight_pass",
                "weight_total",
                "pass_count",
                "fail_count",
                "pass_rules",
                "fail_rules",
                "headline",
                "rationale",
                "time_horizon",
                "position_sizing",
                "what_would_change_my_mind",
            ]
        );
        let rule_keys: Vec<&str> = r["pass_rules"]
            .as_array()
            .and_then(|a| a.first())
            .and_then(Value::as_object)
            .map(|o| o.keys().map(String::as_str).collect())
            .unwrap_or_default();
        if !rule_keys.is_empty() {
            assert_eq!(rule_keys, vec!["rule_id", "name", "weight", "msg"]);
        }
    }

    #[test]
    fn holding_bonus_adds_a_virtual_rule() {
        let r = evaluate_investor("buffett", &json!({"market": "US", "ticker": "AAPL", "name": "苹果", "industry": "消费电子"}));
        assert_eq!(r["signal"], "bullish"); // override_signal from the known holding
        let first = &r["pass_rules"][0];
        assert_eq!(first["rule_id"], "known_holding");
        assert_eq!(first["weight"], 6);
        assert!(first["msg"].as_str().unwrap().starts_with("📌 "));
    }
}
