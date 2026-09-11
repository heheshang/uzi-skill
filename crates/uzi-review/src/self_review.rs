//! Port of `lib/self_review.py` — mechanical self-check gate (~17 checks +
//! runner + human formatter).

use crate::data_integrity::{get_path as di_get, is_missing, CRITICAL_CHECKS};
use crate::pyfmt;
use serde_json::{json, Map, Value};
use std::collections::BTreeSet;
use std::sync::LazyLock;
use uzi_core::py::{f0, round, truthy};

static EMPTY_OBJ: LazyLock<Value> = LazyLock::new(|| Value::Object(Map::new()));
static NULL: Value = Value::Null;

fn obj_or<'a>(v: Option<&'a Value>) -> &'a Value {
    match v {
        Some(x @ Value::Object(_)) => x,
        _ => &EMPTY_OBJ,
    }
}

/// `(ctx["dims"].get(key) or {}).get("data") or {}`.
fn get_dim<'a>(ctx: &'a Value, key: &str) -> &'a Value {
    let dim = ctx
        .get("dims")
        .and_then(|d| d.get(key))
        .filter(|v| truthy(v));
    let data = dim.and_then(|d| d.get("data")).filter(|v| truthy(v));
    data.unwrap_or(&EMPTY_OBJ)
}

fn issue(
    sev: &str,
    cat: &str,
    dim: &str,
    issue: String,
    evidence: String,
    fix: String,
) -> Value {
    let mut m = Map::new();
    m.insert("severity".into(), Value::String(sev.into()));
    m.insert("category".into(), Value::String(cat.into()));
    m.insert("dim".into(), Value::String(dim.into()));
    m.insert("issue".into(), Value::String(issue));
    m.insert("evidence".into(), Value::String(evidence));
    m.insert("suggested_fix".into(), Value::String(fix));
    Value::Object(m)
}

fn issues_vec(v: Vec<Value>) -> Value {
    Value::Array(v)
}

fn truncate_chars(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

fn env_eq(k: &str, v: &str) -> bool {
    std::env::var(k).map(|x| x == v).unwrap_or(false)
}

/// `check_*` gate strictness / CLI-direct-run detection.
fn is_cli_only() -> bool {
    env_eq("UZI_DEPTH", "lite")
        || env_eq("UZI_LITE", "1")
        || env_eq("UZI_CLI_ONLY", "1")
        || env_eq("CI", "true")
}

const CORE_FETCHERS: &[&str] = &[
    "0_basic",
    "1_financials",
    "2_kline",
    "10_valuation",
    "11_governance",
    "15_events",
    "16_lhb",
];
const ALL_FETCHERS: &[&str] = &[
    "0_basic", "1_financials", "2_kline", "3_macro", "4_peers", "5_chain", "6_research", "7_industry",
    "8_materials", "9_futures", "10_valuation", "11_governance", "12_capital_flow", "13_policy",
    "14_moat", "15_events", "16_lhb", "17_sentiment", "18_trap", "19_contests",
];

/// `lib.analysis_profile.get_profile().fetchers_enabled` — `Err` mirrors the
/// upstream `except Exception` fallback (unknown depth → full 20 dims).
fn profile_fetcher_keys() -> Result<&'static [&'static str], ()> {
    let depth = match std::env::var("UZI_DEPTH") {
        Ok(v) => v,
        Err(_) => {
            let lite = std::env::var("UZI_LITE")
                .unwrap_or_else(|_| "auto".into())
                .to_lowercase();
            if matches!(lite.as_str(), "1" | "true" | "yes" | "on") {
                "lite".to_string()
            } else {
                "medium".to_string()
            }
        }
    };
    let depth = if depth.is_empty() {
        "medium".to_string()
    } else {
        depth.to_lowercase()
    };
    match depth.as_str() {
        "lite" => Ok(CORE_FETCHERS),
        "medium" | "deep" => Ok(ALL_FETCHERS),
        _ => Err(()),
    }
}

fn key_num(k: &str) -> Option<i64> {
    let first = k.chars().next()?;
    if !first.is_ascii_digit() {
        return None;
    }
    let prefix = k.split('_').next().unwrap_or("");
    if !prefix.is_empty() && prefix.chars().all(|c| c.is_ascii_digit()) {
        prefix.parse::<i64>().ok()
    } else {
        None
    }
}

fn profile_enabled_nums() -> Result<BTreeSet<i64>, ()> {
    Ok(profile_fetcher_keys()?
        .iter()
        .filter_map(|k| key_num(k))
        .collect())
}

fn num_is_zero(v: &Value) -> bool {
    match v {
        Value::Number(n) => n.as_f64() == Some(0.0),
        Value::Bool(b) => !b,
        _ => false,
    }
}

fn num_gt(v: &Value, bound: f64) -> bool {
    match v {
        Value::Number(n) => n.as_f64().map(|x| x > bound).unwrap_or(false),
        Value::Bool(b) => {
            if *b {
                1.0 > bound
            } else {
                0.0 > bound
            }
        }
        _ => false,
    }
}

fn py_in_none_zero_dash(v: &Value) -> bool {
    match v {
        Value::Null => true,
        Value::String(s) => s == "—",
        Value::Number(n) => n.as_f64() == Some(0.0),
        Value::Bool(b) => !b,
        _ => false,
    }
}

fn contains_ci(haystack: &str, needle: &str) -> bool {
    haystack.to_lowercase().contains(&needle.to_lowercase())
}

// ═══════════════════════════════════════════════════════════════
// 检查注册表
// ═══════════════════════════════════════════════════════════════

fn check_industry_mapping_sanity(ctx: &Value) -> Value {
    let mut out: Vec<Value> = Vec::new();
    let basic = get_dim(ctx, "0_basic");
    let ind = basic.get("industry").unwrap_or(&NULL);
    let ind_metrics = obj_or(get_dim(ctx, "7_industry").get("cninfo_metrics"));
    let matched = ind_metrics.get("industry_name_match").unwrap_or(&NULL);
    let ind_s = ind.as_str().unwrap_or("");
    let matched_s = matched.as_str().unwrap_or("");

    const COLLISION_REDFLAGS: &[(&str, &str, &str)] = &[
        ("工业金属", "农副食品", "有色金属"),
        ("工业母机", "农副食品", "专用设备"),
        ("工业机械", "农副食品", "通用设备"),
        ("白酒", "农副食品", "酒、饮料和精制茶"),
    ];
    for (sw, wrong, right) in COLLISION_REDFLAGS {
        if ind_s.contains(sw) && matched_s.contains(wrong) {
            out.push(issue(
                "critical",
                "industry",
                "7_industry",
                format!(
                    "BUG#R10 class regression: 申万行业 {} 被误映射到证监会 {}",
                    pyfmt::repr(ind),
                    pyfmt::repr(matched)
                ),
                format!(
                    "industry={}, matched={}",
                    pyfmt::repr(ind),
                    pyfmt::repr(matched)
                ),
                format!(
                    "检查 lib/industry_mapping.SW_TO_CSRC_INDUSTRY[{}] 是否指向含 {} 的证监会名；必要时清 cache 重跑",
                    pyfmt::repr(&json!(sw)),
                    pyfmt::repr(&json!(right))
                ),
            ));
        }
    }
    issues_vec(out)
}

fn check_all_dims_exist(ctx: &Value) -> Value {
    let mut out: Vec<Value> = Vec::new();
    let dims = obj_or(ctx.get("dims"));

    let mut required: BTreeSet<i64> = (0..20).collect();
    if let Ok(nums) = profile_enabled_nums() {
        if !nums.is_empty() {
            required = nums;
        }
    }

    let present: BTreeSet<i64> = dims
        .as_object()
        .map(|m| m.keys().filter_map(|k| key_num(k)).collect())
        .unwrap_or_default();
    let missing: Vec<i64> = required.difference(&present).copied().collect();
    for num in missing {
        out.push(issue(
            "critical",
            "data",
            &format!("{}_", num),
            format!("应跑的维度 {} 完全缺失（fetcher 从未运行或崩溃）", num),
            format!("dims 里没有 key 以 {}_ 开头", num),
            "重跑 run.py <ticker> --no-resume 或手动 fetch_X".to_string(),
        ));
    }
    issues_vec(out)
}

fn check_empty_dims(ctx: &Value) -> Value {
    let mut out: Vec<Value> = Vec::new();
    let dims = obj_or(ctx.get("dims"));
    let enabled = profile_enabled_nums().ok();

    let mut entries: Vec<(&String, &Value)> = dims
        .as_object()
        .map(|m| m.iter().collect())
        .unwrap_or_default();
    entries.sort_by(|a, b| a.0.cmp(b.0));

    for (k, v) in entries {
        if !v.is_object() {
            continue;
        }
        if let Some(en) = &enabled {
            if let Some(num) = key_num(k) {
                if !en.contains(&num) {
                    continue;
                }
            }
        }
        let data = v.get("data");
        let data_empty = match data {
            None | Some(Value::Null) => true,
            Some(Value::Object(m)) => m.is_empty(),
            Some(Value::Array(a)) => a.is_empty(),
            _ => false,
        };
        if !data_empty {
            continue;
        }
        let is_timeout = v.get("_timeout").map(truthy).unwrap_or(false);
        let err = v.get("error").unwrap_or(&NULL);
        let err_truthy = truthy(err);
        let sev = if is_timeout || err_truthy {
            "warning"
        } else {
            "critical"
        };
        let suffix = if is_timeout {
            " (timeout)"
        } else if err_truthy {
            " (crash)"
        } else {
            ""
        };
        let err_s = err.as_str().unwrap_or("");
        out.push(issue(
            sev,
            "data",
            k,
            format!("维度 {} data 为空{}", k, suffix),
            format!(
                "_timeout={}, error={}",
                if is_timeout { "True" } else { "False" },
                truncate_chars(err_s, 60)
            ),
            "agent 用 WebSearch / mx_api / 手工查权威源补齐，写入 agent_analysis.dim_commentary".to_string(),
        ));
    }
    issues_vec(out)
}

fn check_hk_kline_populated(ctx: &Value) -> Value {
    let mut out: Vec<Value> = Vec::new();
    if ctx.get("market").and_then(|m| m.as_str()) != Some("H") {
        return issues_vec(out);
    }
    let kline = get_dim(ctx, "2_kline");
    let count = kline.get("kline_count").cloned().unwrap_or(json!(0));
    let stage = kline.get("stage").cloned().unwrap_or(json!("—"));
    if num_is_zero(&count) {
        out.push(issue(
            "critical",
            "hk",
            "2_kline",
            "港股 kline_count=0，技术面维度不可用".to_string(),
            format!(
                "kline_count={}, stage={}",
                pyfmt::str_exact(&count),
                pyfmt::repr(&stage)
            ),
            "检查 _kline_hk_chain 三层 fallback 是否都失败（东财→新浪→yfinance）；可能需手动重跑".to_string(),
        ));
    } else if stage.as_str() == Some("—") && num_gt(&count, 60.0) {
        out.push(issue(
            "warning",
            "hk",
            "2_kline",
            "港股有 kline 数据但 stage 未分类".to_string(),
            format!(
                "kline_count={}, stage={}",
                pyfmt::str_exact(&count),
                pyfmt::repr(&stage)
            ),
            "indicators.stage 计算失败，查 _stage() 函数是否遇到 ma200=None".to_string(),
        ));
    }
    issues_vec(out)
}

fn check_hk_financials_populated(ctx: &Value) -> Value {
    let mut out: Vec<Value> = Vec::new();
    if ctx.get("market").and_then(|m| m.as_str()) != Some("H") {
        return issues_vec(out);
    }
    let fin = get_dim(ctx, "1_financials");
    if !truthy(fin) {
        out.push(issue(
            "critical",
            "hk",
            "1_financials",
            "港股 1_financials 完全空，ROE/营收/净利全缺".to_string(),
            "data={}".to_string(),
            "检查 fetch_financials._fetch_hk 是否调了 stock_financial_hk_analysis_indicator_em".to_string(),
        ));
    } else if !truthy(fin.get("roe_history").unwrap_or(&NULL)) {
        let keys: Vec<Value> = fin
            .as_object()
            .map(|m| m.keys().take(5).map(|k| json!(k)).collect())
            .unwrap_or_default();
        out.push(issue(
            "warning",
            "hk",
            "1_financials",
            "港股 roe_history 缺失（6 年 ROE 历史是评委依赖字段）".to_string(),
            format!("keys={}", pyfmt::repr(&Value::Array(keys))),
            "agent 用 mx_api 或 hkexnews 补齐".to_string(),
        ));
    }
    issues_vec(out)
}

fn check_panel_non_empty(ctx: &Value) -> Value {
    let mut out: Vec<Value> = Vec::new();
    let panel = obj_or(ctx.get("panel"));
    let investors = panel.get("investors").unwrap_or(&NULL);
    if !truthy(investors) {
        out.push(issue(
            "critical",
            "panel",
            "panel",
            "panel.json 无 investors".to_string(),
            String::new(),
            "重跑 generate_panel()".to_string(),
        ));
        return issues_vec(out);
    }
    let Some(list) = investors.as_array() else {
        return issues_vec(out);
    };
    let sigs: Vec<&Value> = list.iter().map(|i| i.get("signal").unwrap_or(&NULL)).collect();
    let skip = sigs.iter().filter(|s| s.as_str() == Some("skip")).count();
    let n = list.len();
    let skip_rate = skip as f64 / n as f64;
    if skip_rate > 0.5 {
        out.push(issue(
            "warning",
            "panel",
            "panel",
            format!(
                "{:.0}% 评委 skip（可能是不在其能力圈的股票，也可能是 bug）",
                skip_rate * 100.0
            ),
            format!("{}/{} skip", skip, n),
            "确认是否是港股/美股/ST 股，否则查 investor_knowledge.reality_check".to_string(),
        ));
    }

    let sum: f64 = list
        .iter()
        .filter_map(|i| i.get("score"))
        .filter(|s| s.is_number())
        .map(|s| s.as_f64().unwrap_or(0.0))
        .sum();
    let avg = sum / n as f64;
    if avg == 0.0 || avg > 100.0 {
        out.push(issue(
            "critical",
            "panel",
            "panel",
            format!("panel 分数异常 (avg={:.1})", avg),
            format!("avg={}", pyfmt::str_exact(&json!(avg))),
            "查 investor_evaluator 或 rules 是否传入了错误 features".to_string(),
        ));
    }
    issues_vec(out)
}

fn check_coverage_threshold(ctx: &Value) -> Value {
    let mut out: Vec<Value> = Vec::new();
    let raw = ctx.get("raw").unwrap_or(&NULL);
    let integrity = obj_or(raw.get("_integrity"));
    let mut pct_v = integrity
        .get("coverage_pct")
        .cloned()
        .unwrap_or(json!(100));
    let mut mcl: Vec<Value> = integrity
        .get("missing_critical")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    if let Ok(keys) = profile_fetcher_keys() {
        let raw_dims = obj_or(raw.get("dimensions"));
        let mut filtered_total = 0i64;
        let mut filtered_passed = 0i64;
        for (dim_key, path, _label, _crit) in CRITICAL_CHECKS {
            if !keys.contains(dim_key) {
                continue;
            }
            filtered_total += 1;
            let dim = obj_or(raw_dims.get(dim_key));
            let data = obj_or(dim.get("data"));
            if !is_missing(di_get(data, path)) {
                filtered_passed += 1;
            }
        }
        if filtered_total > 0 {
            pct_v = json!(round(
                filtered_passed as f64 / filtered_total as f64 * 100.0,
                0
            ));
            mcl.retain(|m| {
                m.get("dim")
                    .and_then(|d| d.as_str())
                    .map(|d| keys.contains(&d))
                    .unwrap_or(false)
            });
        }
    }

    let pct = f0(&pct_v);
    if pct < 60.0 {
        let mut severity = if pct < 40.0 { "critical" } else { "warning" };
        let note;
        if severity == "critical" && is_cli_only() {
            severity = "warning";
            note = "（lite/CLI 直跑模式降级为 warning，仍出报告供参考）";
        } else {
            note = "";
        }
        let head: Vec<Value> = mcl.iter().take(3).cloned().collect();
        out.push(issue(
            severity,
            "data",
            "overall",
            format!("数据完整性仅 {:.0}%（< 60% 不该出报告）{}", pct, note),
            format!(
                "coverage_pct={}, missing_critical={}",
                pyfmt::str_exact(&pct_v),
                pyfmt::repr(&Value::Array(head))
            ),
            "agent 用 WebSearch / mx_api 补齐 missing_critical 维度，重跑 stage2".to_string(),
        ));
    }
    issues_vec(out)
}

fn check_placeholder_strings(ctx: &Value) -> Value {
    let mut out: Vec<Value> = Vec::new();
    let syn = obj_or(ctx.get("syn"));
    let dim_comm = obj_or(syn.get("dim_commentary"));
    const BAD_MARKERS: &[&str] = &[
        "[脚本占位]",
        "[TODO]",
        "PLACEHOLDER",
        "占位符",
        "[未实现]",
        "placeholder",
    ];
    if let Some(m) = dim_comm.as_object() {
        for (dim, text) in m {
            let Some(text) = text.as_str() else { continue };
            for marker in BAD_MARKERS {
                if contains_ci(text, marker) {
                    out.push(issue(
                        "critical",
                        "consistency",
                        dim,
                        format!("dim_commentary[{}] 含占位符 {}", dim, pyfmt::repr(&json!(marker))),
                        truncate_chars(text, 100),
                        format!(
                            "agent 写真实 commentary 覆盖该维度；检查 _auto_summarize_dim 是否漏了 {}",
                            dim
                        ),
                    ));
                }
            }
        }
    }
    issues_vec(out)
}

fn check_valuation_sanity(ctx: &Value) -> Value {
    let mut out: Vec<Value> = Vec::new();
    let vm = get_dim(ctx, "20_valuation_models");
    if !truthy(vm) {
        return issues_vec(out);
    }
    let dcf = obj_or(vm.get("dcf"));
    let iv = dcf
        .get("intrinsic_per_share")
        .cloned()
        .unwrap_or_else(|| {
            dcf.get("intrinsic_value_per_share")
                .cloned()
                .unwrap_or(json!(0))
        });
    if py_in_none_zero_dash(&iv) {
        out.push(issue(
            "warning",
            "valuation",
            "20_valuation_models",
            "DCF 内在价值为 0/None（可能负 FCF 或假设异常）".to_string(),
            format!("intrinsic_per_share={}", pyfmt::str_exact(&iv)),
            "检查 fetch_financials.net_profit_history 最新值是否 > 0".to_string(),
        ));
    }

    let comps = obj_or(vm.get("comps"));
    let target_price = comps.get("implied_price").cloned().unwrap_or_else(|| {
        comps
            .get("target_price_implied")
            .cloned()
            .unwrap_or(Value::Null)
    });
    if py_in_none_zero_dash(&target_price) {
        out.push(issue(
            "info",
            "valuation",
            "20_valuation_models",
            "Comps 隐含目标价缺失".to_string(),
            format!("implied_price={}", pyfmt::str_exact(&target_price)),
            "检查 fetch_peers 是否返回足够同行样本".to_string(),
        ));
    }
    issues_vec(out)
}

fn check_industry_data_coverage(ctx: &Value) -> Value {
    let mut out: Vec<Value> = Vec::new();
    let ind = get_dim(ctx, "7_industry");
    if !truthy(ind) {
        return issues_vec(out);
    }
    if truthy(ind.get("needs_web_search").unwrap_or(&NULL))
        && !truthy(ind.get("agent_populated").unwrap_or(&NULL))
    {
        let queries = ind.get("web_search_queries").unwrap_or(&NULL);
        if truthy(queries) {
            let market_part = match ctx.get("market") {
                Some(Value::Object(m)) => pyfmt::str_exact(m.get("industry").unwrap_or(&NULL)),
                _ => String::new(),
            };
            let qlen = queries
                .as_array()
                .map(|a| a.len())
                .or_else(|| queries.as_object().map(|o| o.len()))
                .or_else(|| queries.as_str().map(|s| s.chars().count()))
                .unwrap_or(0);
            let fix = queries
                .as_array()
                .map(|a| {
                    a.iter()
                        .take(2)
                        .map(pyfmt::str_exact)
                        .collect::<Vec<_>>()
                        .join("; ")
                })
                .unwrap_or_default();
            out.push(issue(
                "warning",
                "data",
                "7_industry",
                format!(
                    "行业景气度字段需要 agent 用 search_trusted 补齐（{} 不在硬编码表里）",
                    market_part
                ),
                format!("needs_web_search=True, {} 条建议查询未执行", qlen),
                format!("agent 执行: {}", fix),
            ));
        }
    }
    issues_vec(out)
}

fn check_metals_materials_populated(ctx: &Value) -> Value {
    let mut out: Vec<Value> = Vec::new();
    let basic = get_dim(ctx, "0_basic");
    let ind_v = basic.get("industry").cloned().unwrap_or(json!(""));
    let ind = ind_v.as_str().unwrap_or("");
    const METAL_IND: &[&str] = &[
        "工业金属", "有色金属", "贵金属", "能源金属", "小金属", "稀有金属", "钢铁", "普钢", "特钢",
        "煤炭开采",
    ];
    if !METAL_IND.iter().any(|k| ind.contains(k)) {
        return issues_vec(out);
    }
    let mat = get_dim(ctx, "8_materials");
    let core = mat.get("core_material").cloned().unwrap_or(json!("—"));
    if core.as_str() == Some("—") || !truthy(mat.get("materials_detail").unwrap_or(&NULL)) {
        out.push(issue(
            "warning",
            "data",
            "8_materials",
            format!("金属类行业 {} 但 materials 无原材料数据", pyfmt::repr(&ind_v)),
            format!("core_material={}", pyfmt::repr(&core)),
            "检查 INDUSTRY_MATERIALS 是否覆盖该细分；必要时走 search_trusted".to_string(),
        ));
    }
    issues_vec(out)
}

fn check_agent_analysis_exists(ctx: &Value) -> Value {
    let mut out: Vec<Value> = Vec::new();
    let ag = ctx.get("ag").unwrap_or(&NULL);
    let cli = is_cli_only();

    if ag.is_null() {
        let severity = if cli { "warning" } else { "critical" };
        let note = if cli { "（lite/CLI 直跑模式可接受）" } else { "" };
        out.push(issue(
            severity,
            "self-check",
            "agent_analysis",
            format!("agent_analysis.json 不存在{}", note),
            "file not found".to_string(),
            "CLI 直跑无 agent 环境可以忽略此项；若走 Claude Code/Codex/Cursor 则 agent 必须读 panel.json + raw_data.json 后写 agent_analysis.json".to_string(),
        ));
        return issues_vec(out);
    }
    if !truthy(ag.get("agent_reviewed").unwrap_or(&NULL)) {
        out.push(issue(
            if cli { "warning" } else { "critical" },
            "self-check",
            "agent_analysis",
            "agent_analysis.agent_reviewed != True".to_string(),
            format!(
                "agent_reviewed={}",
                pyfmt::str_exact(ag.get("agent_reviewed").unwrap_or(&NULL))
            ),
            "agent 核查完内容后必须显式设置 agent_reviewed: true".to_string(),
        ));
    }
    let dc = obj_or(ag.get("dim_commentary"));
    if dc.as_object().map(|m| m.len()).unwrap_or(0) < 15 {
        let keys: Vec<Value> = dc
            .as_object()
            .map(|m| m.keys().map(|k| json!(k)).collect())
            .unwrap_or_default();
        out.push(issue(
            "warning",
            "self-check",
            "agent_analysis",
            format!(
                "agent 仅覆盖 {}/22 维 dim_commentary（建议 ≥ 15）",
                dc.as_object().map(|m| m.len()).unwrap_or(0)
            ),
            format!("covered_dims={}", pyfmt::repr(&Value::Array(keys))),
            "agent 补写更多维度的 dim_commentary，尤其是 14_moat / 13_policy / 7_industry 定性维度".to_string(),
        ));
    }
    issues_vec(out)
}

fn check_factcheck_redflags(ctx: &Value) -> Value {
    let mut out: Vec<Value> = Vec::new();
    let ag = obj_or(ctx.get("ag"));
    let syn = obj_or(ctx.get("syn"));
    let basic = get_dim(ctx, "0_basic");
    let mb = match basic.get("main_business") {
        Some(v) if truthy(v) => pyfmt::str_exact(v),
        _ => String::new(),
    };
    let ind = match basic.get("industry") {
        Some(v) if truthy(v) => pyfmt::str_exact(v),
        _ => String::new(),
    };
    let main_business = format!("{}{}", mb, ind);

    let mut all_text = String::new();
    for container in [ag, syn] {
        let dc = obj_or(container.get("dim_commentary"));
        if let Some(m) = dc.as_object() {
            for text in m.values() {
                if let Some(s) = text.as_str() {
                    all_text.push_str(s);
                    all_text.push(' ');
                }
            }
        }
    }

    const REDFLAGS: &[(&str, &[&str], &str)] = &[
        ("苹果|Apple", &["光学", "镜头", "屏幕", "代工", "结构件", "精密"], "苹果产业链"),
        ("特斯拉|Tesla", &["电池", "零部件", "车身", "锂电"], "特斯拉供应链"),
    ];
    for (claim_pattern, justify_kws, label) in REDFLAGS {
        let matched = claim_pattern
            .split('|')
            .any(|alt| contains_ci(&all_text, alt));
        if matched && !justify_kws.iter().any(|k| main_business.contains(k)) {
            out.push(issue(
                "warning",
                "consistency",
                "synthesis",
                format!("commentary 提到 {} 但 main_business 未见相关业务", label),
                format!(
                    "claim mentions {}, main_business={}",
                    claim_pattern,
                    pyfmt::repr(&json!(truncate_chars(&main_business, 80)))
                ),
                "在 raw_data.dimensions['5_chain'] 里找到明确证据，否则删除该关联".to_string(),
            ));
        }
    }
    issues_vec(out)
}

fn check_consensus_formula_sanity(ctx: &Value) -> Value {
    let mut out: Vec<Value> = Vec::new();
    let panel = obj_or(ctx.get("panel"));
    let cf = obj_or(panel.get("consensus_formula"));
    let cons_v = panel.get("panel_consensus").cloned().unwrap_or(json!(-1));
    if !cons_v.is_number() {
        return issues_vec(out);
    }
    let cons = f0(&cons_v);
    if cons < 0.0 {
        return issues_vec(out);
    }
    let version = cf.get("version").cloned().unwrap_or(json!(""));
    let version_s = version.as_str().unwrap_or("");
    let is_current = ["v2.15.5", "v2.11", "v2.9.1", "bullish + 0.5", "polarize"]
        .iter()
        .any(|t| version_s.contains(t));
    if truthy(&cf) && !is_current {
        out.push(issue(
            "warning",
            "panel",
            "panel",
            "consensus_formula 不是 v2.15.5 混合公式，可能是旧 cache".to_string(),
            format!("version={}", pyfmt::repr(&version)),
            "清 cache 重跑或直接 stage2() 重新合成".to_string(),
        ));
    }
    let sig = obj_or(panel.get("signal_distribution"));
    let sm_v = cf.get("score_mean").cloned().unwrap_or(json!(50));
    if f0(sig.get("bullish").unwrap_or(&NULL)) == 0.0 && f0(&sm_v) < 30.0 && cons > 30.0 {
        out.push(issue(
            "critical",
            "panel",
            "panel",
            "panel_consensus 公式异常：bullish=0 且 score_mean<30 但 consensus>30".to_string(),
            format!(
                "consensus={}, bullish={}, neutral={}, score_mean={}",
                pyfmt::str_exact(&cons_v),
                pyfmt::str_exact(sig.get("bullish").unwrap_or(&json!(0))),
                pyfmt::str_exact(sig.get("neutral").unwrap_or(&json!(0))),
                pyfmt::str_exact(&sm_v)
            ),
            "检查 generate_panel 的 consensus 公式".to_string(),
        ));
    }
    issues_vec(out)
}

fn check_panel_hollow_verdicts(ctx: &Value) -> Value {
    let panel = obj_or(ctx.get("panel"));
    if truthy(panel.get("consensus_valid").unwrap_or(&json!(true))) {
        return issues_vec(Vec::new());
    }
    let hv = panel.get("hollow_verdicts").cloned().unwrap_or(json!(0));
    let hp = panel.get("hollow_pct").cloned().unwrap_or(json!(0));
    let pc = panel.get("panel_consensus").cloned().unwrap_or(Value::Null);
    let ids = panel
        .get("hollow_ids")
        .unwrap_or(&NULL)
        .as_array()
        .map(|a| {
            a.iter()
                .take(8)
                .map(pyfmt::str_exact)
                .collect::<Vec<_>>()
                .join(", ")
        })
        .unwrap_or_default();
    issues_vec(vec![issue(
        "critical",
        "panel",
        "panel",
        format!("共识分包含 {} 个无证据空判", pyfmt::str_exact(&hv)),
        format!(
            "hollow_pct={}% · panel_consensus={} · ids={}",
            pyfmt::str_exact(&hp),
            pyfmt::str_exact(&pc),
            ids
        ),
        "补齐数据后重跑，或由 agent 用可追溯证据覆盖空判；不要引用当前共识分。".to_string(),
    )])
}

fn check_panel_insights_rendered(_ctx: &Value) -> Value {
    let mut out: Vec<Value> = Vec::new();
    let ar = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../uzi-report/src/assemble_report.rs");
    if ar.exists() {
        if let Ok(src) = std::fs::read_to_string(&ar) {
            // The Rust port may keep the upstream snake_case symbol; accept the
            // field-name spelling too so a renamed helper is not a false alarm.
            if !(src.contains("render_panel_insights") || src.contains("panel_insights")) {
                out.push(issue(
                    "critical",
                    "self-check",
                    "report",
                    "v2.9.1 regression: assemble_report 缺 render_panel_insights".to_string(),
                    "grep 失败".to_string(),
                    "恢复 render_panel_insights 函数 + INJECT_PANEL_INSIGHTS 替换".to_string(),
                ));
            }
        }
    }
    issues_vec(out)
}

fn check_debate_bull_bear_populated(ctx: &Value) -> Value {
    let mut out: Vec<Value> = Vec::new();
    let syn = obj_or(ctx.get("syn"));
    let debate = obj_or(syn.get("debate"));
    let bull = obj_or(debate.get("bull"));
    let bear = obj_or(debate.get("bear"));
    if !truthy(bull.get("investor_id").unwrap_or(&NULL)) {
        out.push(issue(
            "warning",
            "panel",
            "debate",
            "debate.bull 未选出 bullish 代表（可能全 skip 或全 bearish）".to_string(),
            format!("bull={}", pyfmt::str_exact(bull)),
            "确认 panel 有非 skip 投资者，或 agent 用 great_divide_override 指定".to_string(),
        ));
    }
    if !truthy(bear.get("investor_id").unwrap_or(&NULL)) {
        out.push(issue(
            "warning",
            "panel",
            "debate",
            "debate.bear 未选出 bearish 代表".to_string(),
            format!("bear={}", pyfmt::str_exact(bear)),
            "同上".to_string(),
        ));
    }
    let bull_id = bull.get("investor_id").unwrap_or(&NULL);
    let bear_id = bear.get("investor_id").unwrap_or(&NULL);
    if truthy(bull_id) && bull_id == bear_id {
        out.push(issue(
            "critical",
            "panel",
            "debate",
            "debate bull 和 bear 是同一人".to_string(),
            format!("both={}", pyfmt::repr(bull_id)),
            "generate_synthesis 选 bull/bear 逻辑应排除同人".to_string(),
        ));
    }
    issues_vec(out)
}

type CheckFn = fn(&Value) -> Value;

static CHECKS: &[(&str, CheckFn)] = &[
    ("check_industry_mapping_sanity", check_industry_mapping_sanity),
    ("check_all_dims_exist", check_all_dims_exist),
    ("check_empty_dims", check_empty_dims),
    ("check_hk_kline_populated", check_hk_kline_populated),
    ("check_hk_financials_populated", check_hk_financials_populated),
    ("check_panel_non_empty", check_panel_non_empty),
    ("check_coverage_threshold", check_coverage_threshold),
    ("check_placeholder_strings", check_placeholder_strings),
    ("check_valuation_sanity", check_valuation_sanity),
    ("check_industry_data_coverage", check_industry_data_coverage),
    ("check_metals_materials_populated", check_metals_materials_populated),
    ("check_agent_analysis_exists", check_agent_analysis_exists),
    ("check_factcheck_redflags", check_factcheck_redflags),
    ("check_consensus_formula_sanity", check_consensus_formula_sanity),
    ("check_panel_hollow_verdicts", check_panel_hollow_verdicts),
    ("check_panel_insights_rendered", check_panel_insights_rendered),
    ("check_debate_bull_bear_populated", check_debate_bull_bear_populated),
];

pub fn checks() -> &'static [(&'static str, fn(&Value) -> Value)] {
    CHECKS
}

fn panic_message(e: Box<dyn std::any::Any + Send>) -> String {
    if let Some(s) = e.downcast_ref::<&str>() {
        (*s).to_string()
    } else if let Some(s) = e.downcast_ref::<String>() {
        s.clone()
    } else {
        "unknown".to_string()
    }
}

fn read_or_empty(ticker: &str, name: &str) -> Value {
    match uzi_core::cache::read_task_output(ticker, name) {
        Some(v) if truthy(&v) => v,
        _ => Value::Object(Map::new()),
    }
}

pub fn review_all(ticker: &str, _cache_root: Option<&str>) -> Value {
    let raw = read_or_empty(ticker, "raw_data");
    let syn = read_or_empty(ticker, "synthesis");
    let panel = read_or_empty(ticker, "panel");
    let ag = uzi_core::cache::read_task_output(ticker, "agent_analysis").unwrap_or(Value::Null);

    let dims = obj_or(raw.get("dimensions")).clone();
    let market = raw.get("market").cloned().unwrap_or(json!("A"));

    let mut ctx = Map::new();
    ctx.insert("ticker".into(), json!(ticker));
    ctx.insert("market".into(), market.clone());
    ctx.insert("raw".into(), raw);
    ctx.insert("syn".into(), syn);
    ctx.insert("panel".into(), panel);
    ctx.insert("ag".into(), ag);
    ctx.insert("dims".into(), dims);
    let ctx = Value::Object(ctx);

    let mut all_issues: Vec<Value> = Vec::new();
    for (name, f) in CHECKS {
        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| f(&ctx))) {
            Ok(Value::Array(a)) => all_issues.extend(a),
            Ok(other) => {
                if truthy(&other) {
                    all_issues.push(other);
                }
            }
            Err(e) => {
                let msg = truncate_chars(&panic_message(e), 100);
                all_issues.push(issue(
                    "warning",
                    "self-check",
                    "review-engine",
                    format!("check {} 自己炸了: PanicException: {}", name, msg),
                    String::new(),
                    String::new(),
                ));
            }
        }
    }

    let count = |sev: &str| {
        all_issues
            .iter()
            .filter(|i| i.get("severity").and_then(|s| s.as_str()) == Some(sev))
            .count() as i64
    };
    let crit = count("critical");
    let warn = count("warning");
    let info = count("info");

    let checks_run: Vec<Value> = CHECKS.iter().map(|(n, _)| json!(n)).collect();

    let mut report = Map::new();
    report.insert("ticker".into(), json!(ticker));
    report.insert("market".into(), market);
    report.insert(
        "reviewed_at".into(),
        json!(chrono::Local::now()
            .format("%Y-%m-%dT%H:%M:%S")
            .to_string()),
    );
    report.insert("critical_count".into(), json!(crit));
    report.insert("warning_count".into(), json!(warn));
    report.insert("info_count".into(), json!(info));
    report.insert("passed".into(), Value::Bool(crit == 0));
    report.insert("issues".into(), Value::Array(all_issues));
    report.insert("checks_run".into(), Value::Array(checks_run));
    Value::Object(report)
}

pub fn write_review(ticker: &str, report: &Value) -> std::path::PathBuf {
    match uzi_core::cache::write_task_output(ticker, "_review_issues", report) {
        Ok(p) => p,
        Err(_) => std::path::PathBuf::from(format!(".cache/{}/_review_issues.json", ticker)),
    }
}

pub fn format_human(report: &Value) -> String {
    let mut lines: Vec<String> = Vec::new();
    let passed = truthy(report.get("passed").unwrap_or(&NULL));
    let mark = if passed { "✓" } else { "✗" };
    lines.push(format!(
        "{} Self-Review · {} ({})",
        mark,
        pyfmt::str_exact(report.get("ticker").unwrap_or(&NULL)),
        pyfmt::str_exact(report.get("market").unwrap_or(&NULL))
    ));
    lines.push(format!(
        "  critical={} warning={} info={}",
        pyfmt::str_exact(report.get("critical_count").unwrap_or(&NULL)),
        pyfmt::str_exact(report.get("warning_count").unwrap_or(&NULL)),
        pyfmt::str_exact(report.get("info_count").unwrap_or(&NULL))
    ));
    lines.push(format!(
        "  reviewed_at={}",
        pyfmt::str_exact(report.get("reviewed_at").unwrap_or(&NULL))
    ));

    if let Some(arr) = report.get("issues").and_then(|v| v.as_array()) {
        if !arr.is_empty() {
            lines.push(String::new());
            for sev in ["critical", "warning", "info"] {
                let sev_issues: Vec<&Value> = arr
                    .iter()
                    .filter(|i| i.get("severity").and_then(|s| s.as_str()) == Some(sev))
                    .collect();
                if sev_issues.is_empty() {
                    continue;
                }
                let icon = match sev {
                    "critical" => "🔴",
                    "warning" => "🟡",
                    _ => "🔵",
                };
                lines.push(format!("  {} {} ({}):", icon, sev.to_uppercase(), sev_issues.len()));
                for i in sev_issues {
                    lines.push(format!(
                        "    [{}] {}",
                        pyfmt::str_exact(i.get("dim").unwrap_or(&NULL)),
                        pyfmt::str_exact(i.get("issue").unwrap_or(&NULL))
                    ));
                    let ev = i.get("evidence").unwrap_or(&NULL);
                    if truthy(ev) {
                        lines.push(format!(
                            "      evidence: {}",
                            truncate_chars(&pyfmt::str_exact(ev), 120)
                        ));
                    }
                    let fx = i.get("suggested_fix").unwrap_or(&NULL);
                    if truthy(fx) {
                        lines.push(format!(
                            "      fix: {}",
                            truncate_chars(&pyfmt::str_exact(fx), 200)
                        ));
                    }
                }
            }
        }
    }
    lines.join("\n")
}
