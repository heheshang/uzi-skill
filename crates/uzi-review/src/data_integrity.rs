//! Port of `lib/data_integrity.py` — post-fetch coverage validator and
//! recovery-task generator.

use crate::pyfmt;
use serde_json::{json, Map, Value};
use std::path::Path;
use std::sync::LazyLock;
use uzi_core::py::{round, truthy};

/// (dim_key, dotted data path, label, critical)
pub const CRITICAL_CHECKS: &[(&str, &str, &str, bool)] = &[
    // Dimension 0 · Basic
    ("0_basic", "name", "公司名称", true),
    ("0_basic", "price", "当前股价", true),
    ("0_basic", "industry", "所属行业", true),
    ("0_basic", "market_cap", "总市值", true),
    ("0_basic", "pe_ttm", "PE-TTM", false),
    ("0_basic", "pb", "PB", false),
    // Dimension 1 · Financials
    ("1_financials", "roe_history", "ROE 历史", true),
    ("1_financials", "revenue_history", "营收历史", false),
    ("1_financials", "net_profit_history", "净利历史", false),
    ("1_financials", "financial_health", "财务健康度", false),
    // Dimension 2 · Kline
    ("2_kline", "stage", "K 线阶段", true),
    ("2_kline", "ma_align", "均线多空", false),
    ("2_kline", "macd", "MACD", false),
    // Dimension 10 · Valuation
    ("10_valuation", "pe", "PE", false),
    ("10_valuation", "pe_quantile", "PE 5 年分位", false),
    ("10_valuation", "pb_quantile", "PB 5 年分位", false),
    // Dimension 7 · Industry
    ("7_industry", "growth", "行业增速", false),
    // Dimension 14 · Moat
    ("14_moat", "scores", "护城河评分", false),
];

/// Fetchers that provide qualitative enrichment — should have any data at all.
pub const ENRICHMENT_DIMS: &[(&str, &str)] = &[
    ("3_macro", "宏观周期"),
    ("4_peers", "同业对标"),
    ("5_chain", "上下游"),
    ("6_research", "券商研报"),
    ("7_industry", "行业景气"),
    ("8_materials", "原材料"),
    ("9_futures", "期货关联"),
    ("11_governance", "治理/减持"),
    ("12_capital_flow", "北向/两融"),
    ("13_policy", "政策环境"),
    ("14_moat", "护城河"),
    ("15_events", "事件驱动"),
    ("16_lhb", "龙虎榜/游资"),
    ("17_sentiment", "大V舆情"),
    ("18_trap", "杀猪盘"),
    ("19_contests", "实盘比赛"),
];

/// Per-field recovery hints · order matters — browser > MX > WebSearch > inference.
const RECOVERY_HINTS: &[((&str, &str), &[&str])] = &[
    (("0_basic", "name"), &["mx: '{code} 公司简称'", "browser: https://xueqiu.com/S/{code_raw}", "ws: '{code} 公司简介'"]),
    (("0_basic", "price"), &["mx: '{code} 最新价'", "browser: https://xueqiu.com/S/{code_raw}", "ws: '{code} 股价 2026'"]),
    (("0_basic", "industry"), &["mx: '{code} 所属申万行业'", "browser: https://xueqiu.com/S/{code_raw}/F10", "ws: '{code} 所属行业'"]),
    (("0_basic", "market_cap"), &["mx: '{code} 总市值'", "browser: https://quote.eastmoney.com/{eastmoney_code}.html", "ws: '{code} 市值'"]),
    (("0_basic", "pe_ttm"), &["mx: '{code} 市盈率TTM'", "browser: https://xueqiu.com/S/{code_raw}", "infer: 市值 / 归母净利润"]),
    (("0_basic", "pb"), &["mx: '{code} 市净率'", "browser: https://xueqiu.com/S/{code_raw}"]),
    (("1_financials", "roe_history"), &["mx: '{code} 最近5年ROE'", "browser: https://xueqiu.com/S/{code_raw}/F10/main"]),
    (("1_financials", "revenue_history"), &["mx: '{code} 最近5年营业收入'", "browser: https://xueqiu.com/S/{code_raw}/F10/main"]),
    (("1_financials", "net_profit_history"), &["mx: '{code} 最近5年净利润'", "browser: https://xueqiu.com/S/{code_raw}/F10/main"]),
    (("1_financials", "financial_health"), &["infer: 从 ROE/debt_ratio/fcf 综合判断", "mx: '{code} 财务健康度'"]),
    (("2_kline", "stage"), &["infer: 从 ma20/ma60 多空排列推断 Wyckoff stage", "browser: https://xueqiu.com/S/{code_raw}"]),
    (("10_valuation", "pe"), &["mx: '{code} 市盈率'", "browser: https://xueqiu.com/S/{code_raw}"]),
    (("10_valuation", "pe_quantile"), &["mx: '{code} PE 5年分位数'", "ws: '{code} PE历史分位'"]),
    (("10_valuation", "pb_quantile"), &["mx: '{code} PB 5年分位数'", "ws: '{code} PB历史分位'"]),
    (("7_industry", "growth"), &["mx: '{industry} 行业增速 2026'", "ws: '{industry} 行业规模 增速 2026'"]),
    (("14_moat", "scores"), &["agent: Porter 5 Forces + web search 护城河评分"]),
];

/// Enrichment dim recovery hints (when a whole dim is empty).
const ENRICHMENT_HINTS: &[(&str, &[&str])] = &[
    ("3_macro", &["ws: '中国 {industry} 宏观环境 利率 2026'"]),
    ("4_peers", &["mx: '{industry} 同行业公司 市值排名'", "ws: '{name} 同行业竞争者 对比'"]),
    ("5_chain", &["browser: https://xueqiu.com/S/{code_raw}/F10", "ws: '{name} 上下游产业链'"]),
    ("6_research", &["mx: '{code} 券商研报 目标价'", "ws: '{name} 最新研报 2026'"]),
    ("7_industry", &["mx: '{industry} 行业规模 TAM'", "ws: '{industry} 行业景气 2026'"]),
    ("8_materials", &["ws: '{name} 原材料 成本构成'"]),
    ("9_futures", &["ws: '{industry} 期货 相关品种'"]),
    ("11_governance", &["mx: '{code} 股东结构 高管减持'", "browser: https://quote.eastmoney.com/{eastmoney_code}.html"]),
    ("12_capital_flow", &["mx: '{code} 北向持仓 融资融券'", "browser: https://data.eastmoney.com/zlsj/{code_raw}.html"]),
    ("13_policy", &["ws: '{industry} 最新政策 2026'"]),
    ("14_moat", &["ws: '{name} 核心竞争力 技术壁垒 市场份额'"]),
    ("15_events", &["mx: '{code} 最新公告'", "ws: '{name} {code} 最新公告 中标 研发 2026'"]),
    ("16_lhb", &["mx: '{code} 龙虎榜'", "browser: https://data.eastmoney.com/stock/lhb/{code_raw}.html"]),
    ("17_sentiment", &["browser: https://xueqiu.com/S/{code_raw}", "ws: 'site:xueqiu.com {code}'"]),
    ("18_trap", &["infer: 从龙虎榜+换手率+涨跌幅综合判断"]),
    ("19_contests", &["ws: '{code} 实盘比赛 持仓'"]),
];

static EMPTY_OBJ: LazyLock<Value> = LazyLock::new(|| Value::Object(Map::new()));
static NULL: Value = Value::Null;

fn obj_or<'a>(v: Option<&'a Value>) -> &'a Value {
    match v {
        Some(x @ Value::Object(_)) => x,
        _ => &EMPTY_OBJ,
    }
}

/// Upstream `_get`: walk a dotted path, returning `None`/null on any non-dict hop.
pub(crate) fn get_path<'a>(obj: &'a Value, path: &str) -> &'a Value {
    let mut cur = obj;
    for key in path.split('.') {
        match cur {
            Value::Object(m) => cur = m.get(key).unwrap_or(&NULL),
            _ => return &NULL,
        }
    }
    cur
}

/// Upstream `_is_missing`. Note the string `"0"`/`"0.0"` count as missing, but
/// the *number* `0` does not.
pub fn is_missing(v: &Value) -> bool {
    match v {
        Value::Null => true,
        Value::String(s) => matches!(s.trim(), "" | "—" | "-" | "N/A" | "None" | "0" | "0.0"),
        Value::Array(a) => a.is_empty(),
        Value::Object(o) => o.is_empty(),
        _ => false,
    }
}

pub fn validate(raw: &Value) -> Value {
    let dims = obj_or(raw.get("dimensions"));

    let mut missing_critical: Vec<Value> = Vec::new();
    let mut missing_optional: Vec<Value> = Vec::new();
    let mut total_checks: i64 = 0;
    let mut passed_checks: i64 = 0;

    for (dim_key, path, label, critical) in CRITICAL_CHECKS {
        total_checks += 1;
        let dim = obj_or(dims.get(dim_key));
        let data = obj_or(dim.get("data"));
        let value = get_path(data, path);
        if is_missing(value) {
            let entry = json!({"dim": dim_key, "path": path, "label": label});
            if *critical {
                missing_critical.push(entry);
            } else {
                missing_optional.push(entry);
            }
        } else {
            passed_checks += 1;
        }
    }

    // Enrichment coverage
    let mut missing_enrichment: Vec<Value> = Vec::new();
    for (dim_key, label) in ENRICHMENT_DIMS {
        let dim = obj_or(dims.get(dim_key));
        let data = obj_or(dim.get("data"));
        let has_content = data
            .as_object()
            .map(|m| m.values().any(|v| !is_missing(v)))
            .unwrap_or(false);
        if !has_content {
            missing_enrichment.push(json!({"dim": dim_key, "label": label}));
        }
    }

    // Fallback dim detection
    let mut fallback_dims: Vec<Value> = Vec::new();
    if let Some(m) = dims.as_object() {
        for (k, v) in m {
            if let Some(vm) = v.as_object() {
                if vm.get("fallback") == Some(&Value::Bool(true)) {
                    let reason = match vm.get("fallback_reason") {
                        Some(r) => r.clone(),
                        None => json!("unknown"),
                    };
                    fallback_dims.push(json!({"dim": k, "reason": reason}));
                }
            }
        }
    }

    let coverage_pct = if total_checks == 0 {
        json!(0)
    } else {
        json!(round(
            passed_checks as f64 / total_checks as f64 * 100.0,
            0
        ))
    };
    let critical_missing = !missing_critical.is_empty();
    let ok = !critical_missing && missing_enrichment.len() < 7;

    let mut out = Map::new();
    out.insert("ok".into(), Value::Bool(ok));
    out.insert("critical_missing".into(), Value::Bool(critical_missing));
    out.insert("missing_critical".into(), Value::Array(missing_critical));
    out.insert("missing_optional".into(), Value::Array(missing_optional));
    out.insert("missing_enrichment".into(), Value::Array(missing_enrichment));
    out.insert("fallback_dims".into(), Value::Array(fallback_dims));
    out.insert("coverage_pct".into(), coverage_pct);
    out.insert("passed_checks".into(), json!(passed_checks));
    out.insert("total_checks".into(), json!(total_checks));
    Value::Object(out)
}

/// First truthy value among the candidates (Python `a or b or c`).
fn pick<'a>(xs: &[Option<&'a Value>]) -> Option<&'a Value> {
    for x in xs {
        if let Some(v) = x {
            if truthy(v) {
                return Some(v);
            }
        }
    }
    None
}

/// Render a Python `str.format(**ctx)` template. Returns `None` when the
/// template cannot be rendered (KeyError/IndexError → upstream keeps the
/// literal template).
fn format_named(t: &str, ctx: &Map<String, Value>) -> Option<String> {
    let mut out = String::with_capacity(t.len());
    let mut chars = t.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '{' => {
                if chars.peek() == Some(&'{') {
                    chars.next();
                    out.push('{');
                } else {
                    let mut name = String::new();
                    loop {
                        match chars.next() {
                            Some('}') => break,
                            Some('{') => return None,
                            Some(ch) => name.push(ch),
                            None => return None,
                        }
                    }
                    match ctx.get(&name) {
                        Some(v) => out.push_str(&pyfmt::str_exact(v)),
                        None => return None,
                    }
                }
            }
            '}' => {
                if chars.peek() == Some(&'}') {
                    chars.next();
                    out.push('}');
                } else {
                    return None;
                }
            }
            _ => out.push(c),
        }
    }
    Some(out)
}

fn render_actions(actions: &[String], ctx: &Map<String, Value>) -> Value {
    Value::Array(
        actions
            .iter()
            .map(|a| match format_named(a, ctx) {
                Some(s) => Value::String(s),
                None => Value::String(a.clone()),
            })
            .collect(),
    )
}

/// The upstream fallback hint `"ws: '{name} {label}'".format(name=name, label=label)`,
/// built once (label is the entry's value, baked in).
fn default_hint(name: &Value, label: &Value) -> String {
    let mut m = Map::new();
    m.insert("name".into(), name.clone());
    m.insert("label".into(), label.clone());
    format_named("ws: '{name} {label}'", &m)
        .unwrap_or_else(|| "ws: '{name} {label}'".to_string())
}

pub fn generate_recovery_tasks(raw: &Value, integrity: &Value) -> Value {
    let dims = obj_or(raw.get("dimensions"));
    let basic = obj_or(obj_or(dims.get("0_basic")).get("data"));
    let code_v = pick(&[basic.get("code"), raw.get("ticker")]).cloned();
    let code_v = code_v.unwrap_or_else(|| json!(""));
    let code_str = match &code_v {
        Value::String(s) => s.clone(),
        other => pyfmt::str_exact(other),
    };
    let name_v = pick(&[
        basic.get("name"),
        raw.get("ticker"),
        Some(&code_v),
    ])
    .cloned()
    .unwrap_or_else(|| json!(""));
    let ind7_data = obj_or(dims.get("7_industry")).get("data");
    let industry_v = pick(&[
        ind7_data.and_then(|d| d.get("industry")),
        basic.get("industry"),
    ])
    .cloned()
    .unwrap_or_else(|| json!("综合"));

    let code_raw = if code_str.contains('.') {
        code_str.split('.').next().unwrap_or("").to_string()
    } else {
        code_str.clone()
    };
    let eastmoney_code = if code_raw.is_empty() {
        String::new()
    } else {
        format!(
            "{}{}",
            if code_str.ends_with(".SH") { "1." } else { "0." },
            code_raw
        )
    };

    let mut ctx = Map::new();
    ctx.insert("code".into(), code_v);
    ctx.insert("code_raw".into(), json!(code_raw));
    ctx.insert("eastmoney_code".into(), json!(eastmoney_code));
    ctx.insert("name".into(), name_v);
    ctx.insert("industry".into(), industry_v);

    let missing_critical = integrity
        .get("missing_critical")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let missing_optional = integrity
        .get("missing_optional")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    let mut tasks: Vec<Value> = Vec::new();

    // Critical + optional missing fields
    for entry in missing_critical.iter().chain(missing_optional.iter()) {
        let dim = entry.get("dim").cloned().unwrap_or(Value::Null);
        let path = entry.get("path").cloned().unwrap_or(Value::Null);
        let label = entry.get("label").cloned().unwrap_or(Value::Null);
        let (dim_s, path_s) = (
            dim.as_str().unwrap_or("").to_string(),
            path.as_str().unwrap_or("").to_string(),
        );
        let hints: Vec<String> = RECOVERY_HINTS
            .iter()
            .find(|((d, p), _)| *d == dim_s && *p == path_s)
            .map(|(_, acts)| acts.iter().map(|s| s.to_string()).collect())
            .unwrap_or_else(|| vec![default_hint(&ctx["name"], &label)]);
        let severity = if missing_critical.iter().any(|m| m == entry) {
            "critical"
        } else {
            "optional"
        };
        let mut t = Map::new();
        t.insert("dim".into(), dim);
        t.insert("field".into(), path);
        t.insert("label".into(), label);
        t.insert("severity".into(), json!(severity));
        t.insert("suggested_actions".into(), render_actions(&hints, &ctx));
        t.insert("status".into(), json!("pending"));
        tasks.push(Value::Object(t));
    }

    // Whole-dim enrichment gaps
    for entry in integrity
        .get("missing_enrichment")
        .and_then(|v| v.as_array())
        .map(|a| a.as_slice())
        .unwrap_or(&[])
    {
        let dim = entry.get("dim").cloned().unwrap_or(Value::Null);
        let label = entry.get("label").cloned().unwrap_or(Value::Null);
        let dim_s = dim.as_str().unwrap_or("").to_string();
        let hints: Vec<String> = ENRICHMENT_HINTS
            .iter()
            .find(|(d, _)| *d == dim_s)
            .map(|(_, acts)| acts.iter().map(|s| s.to_string()).collect())
            .unwrap_or_else(|| vec![default_hint(&ctx["name"], &label)]);
        let mut t = Map::new();
        t.insert("dim".into(), dim);
        t.insert("field".into(), json!("_entire_dim"));
        t.insert("label".into(), label);
        t.insert("severity".into(), json!("enrichment"));
        t.insert("suggested_actions".into(), render_actions(&hints, &ctx));
        t.insert("status".into(), json!("pending"));
        tasks.push(Value::Object(t));
    }

    Value::Array(tasks)
}

/// Recompute integrity from the current raw snapshot and replace stale gap state.
pub fn refresh_recovery_artifact(raw: &mut Value, ticker: &str, gaps_path: &Path) -> Value {
    let integrity = validate(raw);
    if let Value::Object(m) = raw {
        m.insert("_integrity".into(), integrity.clone());
    }
    let tasks = generate_recovery_tasks(raw, &integrity);

    if tasks.as_array().map(|a| a.is_empty()).unwrap_or(true) {
        let _ = std::fs::remove_file(gaps_path);
        return integrity;
    }

    if let Some(parent) = gaps_path.parent() {
        if !parent.as_os_str().is_empty() {
            let _ = std::fs::create_dir_all(parent);
        }
    }
    let mut doc = Map::new();
    doc.insert("ticker".into(), json!(ticker));
    doc.insert(
        "generated_at".into(),
        json!(chrono::Utc::now()
            .to_rfc3339_opts(chrono::SecondsFormat::Secs, false)),
    );
    doc.insert(
        "raw_fetched_at".into(),
        raw.get("fetched_at").cloned().unwrap_or(Value::Null),
    );
    doc.insert(
        "coverage_pct".into(),
        integrity.get("coverage_pct").cloned().unwrap_or(json!(0)),
    );
    doc.insert(
        "critical_missing".into(),
        integrity
            .get("critical_missing")
            .cloned()
            .unwrap_or(json!(false)),
    );
    doc.insert("tasks".into(), tasks);

    let text = serde_json::to_string_pretty(&Value::Object(doc)).unwrap_or_else(|_| "null".into());
    let _ = std::fs::write(gaps_path, text);
    integrity
}

pub fn format_report(report: &Value) -> String {
    let status = if truthy(report.get("ok").unwrap_or(&NULL)) {
        "✅ OK"
    } else if truthy(report.get("critical_missing").unwrap_or(&NULL)) {
        "🔴 CRITICAL"
    } else {
        "🟡 WARNING"
    };
    let mut lines: Vec<String> = Vec::new();
    lines.push(format!(
        "[data_integrity] {} coverage={}% ({}/{})",
        status,
        pyfmt::str_exact(report.get("coverage_pct").unwrap_or(&NULL)),
        pyfmt::str_exact(report.get("passed_checks").unwrap_or(&NULL)),
        pyfmt::str_exact(report.get("total_checks").unwrap_or(&NULL)),
    ));

    let as_arr = |k: &str| report.get(k).and_then(|v| v.as_array());

    if let Some(mc) = as_arr("missing_critical") {
        if !mc.is_empty() {
            lines.push(format!("  🔴 critical missing ({}):", mc.len()));
            for m in mc {
                lines.push(format!(
                    "     - {} ({}.{})",
                    pyfmt::str_exact(m.get("label").unwrap_or(&NULL)),
                    pyfmt::str_exact(m.get("dim").unwrap_or(&NULL)),
                    pyfmt::str_exact(m.get("path").unwrap_or(&NULL)),
                ));
            }
        }
    }

    if let Some(mo) = as_arr("missing_optional") {
        if !mo.is_empty() {
            lines.push(format!("  🟡 optional missing ({}):", mo.len()));
            for m in mo.iter().take(8) {
                lines.push(format!(
                    "     - {} ({}.{})",
                    pyfmt::str_exact(m.get("label").unwrap_or(&NULL)),
                    pyfmt::str_exact(m.get("dim").unwrap_or(&NULL)),
                    pyfmt::str_exact(m.get("path").unwrap_or(&NULL)),
                ));
            }
        }
    }

    if let Some(me) = as_arr("missing_enrichment") {
        if !me.is_empty() {
            let labels = me
                .iter()
                .map(|m| pyfmt::str_exact(m.get("label").unwrap_or(&NULL)))
                .collect::<Vec<_>>()
                .join(", ");
            lines.push(format!(
                "  🟡 enrichment dims empty ({}): {}",
                me.len(),
                labels
            ));
        }
    }

    if let Some(fd) = as_arr("fallback_dims") {
        if !fd.is_empty() {
            let labels = fd
                .iter()
                .map(|f| pyfmt::str_exact(f.get("dim").unwrap_or(&NULL)))
                .collect::<Vec<_>>()
                .join(", ");
            lines.push(format!("  ⚠️  fallback dims: {}", labels));
        }
    }

    lines.join("\n")
}
