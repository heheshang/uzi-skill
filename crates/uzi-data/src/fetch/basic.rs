//! Port of `fetch_basic.py`.
//!
//! Dimension 0 · 基础信息 (name, code, industry, price, mcap, PE, PB).
//!
//! Returns either:
//! * success: `{"ticker", "market", "data", "source", "fallback": false}`
//! * name error: `{"ticker", "error": "name_not_resolved", "user_input",
//!   "suggestions": [...], "source": "name_resolver", "fallback": true}`
//! * non-stock: the `non_stock_security` early-return shape.

use serde_json::{json, Value};

use uzi_core::ticker::{classify_security_type, is_chinese_name, parse_ticker, TickerInfo};

/// `_NON_STOCK_GUIDANCE` — the per-security-type guidance dict as upstream
/// spells it (label / why / what_to_do).
fn non_stock_guidance(sec_type: &str) -> Option<(&'static str, &'static str, &'static str)> {
    match sec_type {
        "etf" => Some((
            "ETF",
            "插件的 51 评委跑 ROE / 护城河 / 管理层 / 分红 等个股财务指标，ETF 没这些字段",
            "分析该 ETF 的**前 3-5 大持仓股**（基金持仓页 / 东财 F10 可查），对每只成分股单独跑 uzi <代码>",
        )),
        "mutual_fund" => Some((
            "开放式基金",
            "开放式基金没有企业基本面字段（v3.4.3 起识别 · 之前可能误判为可转债）",
            "已自动改为循环分析该基金的前 10 大重仓股 · uzi 会二次确认",
        )),
        "lof" => Some((
            "LOF 基金",
            "基金没有企业基本面字段，不适合 51 评委流程",
            "基金评估应看：基金经理 / 规模 / 持仓集中度 / 业绩基准差 / 回撤；这些该用 /fund-analyze 类工具（本插件未覆盖）",
        )),
        "convertible_bond" => Some((
            "可转债",
            "可转债评估看的是转股价 / 溢价率 / 到期收益率 / 赎回条款，不是 ROE",
            "集思录的可转债工具 / 东财可转债专题；或直接分析**正股**",
        )),
        _ => None,
    }
}

/// `main(user_input)`.
pub fn main(user_input: &str) -> Result<Value, String> {
    let ti: TickerInfo = if is_chinese_name(user_input) {
        let r = crate::sources::resolve_chinese_name_rich(user_input);
        if r.get("resolved").map(Value::is_null).unwrap_or(true) {
            // Ambiguous or unresolvable — surface candidates for UI confirmation.
            let suggestions: Vec<Value> = r
                .get("candidates")
                .and_then(|v| v.as_array())
                .cloned()
                .unwrap_or_default()
                .into_iter()
                .take(5)
                .collect();
            let source = format!(
                "name_resolver:{}",
                r.get("source").and_then(|v| v.as_str()).unwrap_or("none")
            );
            return Ok(json!({
                "ticker": user_input,
                "market": Value::Null,
                "data": {},
                "error": "name_not_resolved",
                "user_input": user_input,
                "suggestions": suggestions,
                "source": source,
                "fallback": true,
            }));
        }
        let full = r
            .get("resolved")
            .and_then(|v| v.get("full"))
            .and_then(|v| v.as_str())
            .unwrap_or(user_input);
        parse_ticker(full)
    } else {
        parse_ticker(user_input)
    };

    // v2.9.2 · 早期拦截 ETF/LOF/可转债（插件是个股分析引擎，跑非个股标的会输出垃圾）
    let sec_type = if ti.market == "A" {
        classify_security_type(&ti.code).as_str().to_string()
    } else {
        "stock".to_string()
    };
    if let Some((label, why, what_to_do)) = non_stock_guidance(&sec_type) {
        return Ok(json!({
            "ticker": ti.full,
            "market": ti.market,
            "data": {},
            "error": "non_stock_security",
            "security_type": sec_type,
            "guidance": {"label": label, "why": why, "what_to_do": what_to_do},
            "message": format!("{} 是 {}，不是个股。\n原因: {}\n建议: {}", ti.full, label, why, what_to_do),
            "source": "market_router:classify_security_type",
            "fallback": true,
        }));
    }

    let data = crate::sources::fetch_basic(&ti);
    Ok(json!({
        "ticker": ti.full,
        "market": ti.market,
        "data": data,
        "source": format!("akshare:{}", ti.market),
        "fallback": false,
    }))
}

/// Mini-racer-safe variant. `fetch_basic` never touches the mini-racer-prone
/// AkShare endpoints `fetch_valuation.main_safe` guards against, so this simply
/// mirrors [`main`] for callers that use the `main_safe` convention uniformly.
pub fn main_safe(ticker: &str) -> Result<Value, String> {
    main(ticker)
}
