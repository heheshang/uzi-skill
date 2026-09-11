//! Port of `assemble_report.py` — assemble the final HTML report from
//! `synthesis.json` + `dimensions.json` + `panel.json` (and `raw_data.json`).
//!
//! Upstream gates HTML emission behind `lib/self_review.review_all`; the port
//! keeps the same behaviour only for the documented `UZI_SKIP_REVIEW=1` path
//! (self-review lives in another crate that this one does not depend on), and
//! always proceeds otherwise.
//!
//! `assemble` returns the standalone HTML path: it runs the upstream
//! `assemble_report.assemble` pipeline and then `inline_assets.inline_assets`.

use crate::dim_viz::{score_class, viz_for};
use crate::institutional::{
    render_data_gap_banner, render_institutional_section, render_school_lock_banner,
    render_style_chip, trap_color_emoji,
};
use crate::panel_cards::{
    li, render_chat_message, render_jury_seat, render_risks, render_top3_bears,
    render_top3_bulls, render_vote_bars,
};
use crate::pyfmt::{disp, num, pyf};
use crate::security::{escape_payload, escape_text};
use crate::segmental::render_segmental_block;
use crate::special_cards::{
    render_debate_rounds, render_friendly_layer, render_fund_managers, render_panel_insights,
    render_school_scores,
};
use anyhow::anyhow;
use serde_json::{Map, Value};
use std::path::Path;

/// Report assets shipped with this repo, at `<repo>/assets`.
///
/// Canonical resolver lives in [`uzi_core::assets`] — the self-review gate needs
/// the same answer and must not depend on this crate.
pub use uzi_core::assets::assets_dir;

fn safe(v: &Value, default: &str) -> String {
    if v.is_null() {
        return default.to_string();
    }
    if let Value::String(s) = v {
        if s.is_empty() || s == "nan" {
            return default.to_string();
        }
    }
    disp(v)
}

fn get_or<'a>(v: &'a Value, key: &str, default: &'a Value) -> &'a Value {
    v.get(key).unwrap_or(default)
}

const NULL: Value = Value::Null;

// ─── 19 维数据卡 配置 ───

struct DimMeta {
    id: &'static str,
    title: &'static str,
    en: &'static str,
    weight: i64,
    kpis: &'static [&'static str],
    kpi_labels: &'static [(&'static str, &'static str)],
}

fn dim_meta(key: &str) -> Option<DimMeta> {
    let m = match key {
        "1_financials" => DimMeta {
            id: "01",
            title: "财报扎实度",
            en: "Financials",
            weight: 5,
            kpis: &["roe", "net_margin", "revenue_growth", "fcf"],
            kpi_labels: &[
                ("roe", "ROE"),
                ("net_margin", "净利率"),
                ("revenue_growth", "营收增速"),
                ("fcf", "自由现金流"),
            ],
        },
        "2_kline" => DimMeta {
            id: "02",
            title: "K 线技术面",
            en: "Technical",
            weight: 4,
            kpis: &["stage", "ma_align", "macd", "rsi"],
            kpi_labels: &[
                ("stage", "Stage"),
                ("ma_align", "均线"),
                ("macd", "MACD"),
                ("rsi", "RSI"),
            ],
        },
        "3_macro" => DimMeta {
            id: "03",
            title: "宏观环境",
            en: "Macro",
            weight: 3,
            kpis: &["rate_cycle", "fx_trend", "geo_risk", "commodity"],
            kpi_labels: &[
                ("rate_cycle", "利率"),
                ("fx_trend", "汇率"),
                ("geo_risk", "地缘"),
                ("commodity", "大宗"),
            ],
        },
        "4_peers" => DimMeta {
            id: "04",
            title: "同行对比",
            en: "Peers",
            weight: 4,
            kpis: &["rank", "gross_margin_vs", "roe_vs", "growth_vs"],
            kpi_labels: &[
                ("rank", "行业排名"),
                ("gross_margin_vs", "毛利率vs"),
                ("roe_vs", "ROE vs"),
                ("growth_vs", "增速vs"),
            ],
        },
        "5_chain" => DimMeta {
            id: "05",
            title: "上下游产业链",
            en: "Supply Chain",
            weight: 4,
            kpis: &[
                "upstream",
                "downstream",
                "client_concentration",
                "supplier_concentration",
            ],
            kpi_labels: &[
                ("upstream", "上游"),
                ("downstream", "下游"),
                ("client_concentration", "大客户集中"),
                ("supplier_concentration", "供应商集中"),
            ],
        },
        "6_research" => DimMeta {
            id: "06",
            title: "研报观点",
            en: "Sell-side",
            weight: 3,
            kpis: &["coverage", "rating", "target_avg", "upside"],
            kpi_labels: &[
                ("coverage", "覆盖券商"),
                ("rating", "买入比例"),
                ("target_avg", "目标价均值"),
                ("upside", "上涨空间"),
            ],
        },
        "7_industry" => DimMeta {
            id: "07",
            title: "行业景气",
            en: "Industry",
            weight: 4,
            kpis: &["growth", "tam", "penetration", "lifecycle"],
            kpi_labels: &[
                ("growth", "行业增速"),
                ("tam", "TAM"),
                ("penetration", "渗透率"),
                ("lifecycle", "生命周期"),
            ],
        },
        "8_materials" => DimMeta {
            id: "08",
            title: "原材料",
            en: "Raw Materials",
            weight: 3,
            kpis: &["core_material", "price_trend", "cost_share", "import_dep"],
            kpi_labels: &[
                ("core_material", "核心材料"),
                ("price_trend", "12M趋势"),
                ("cost_share", "成本占比"),
                ("import_dep", "进口依赖"),
            ],
        },
        "9_futures" => DimMeta {
            id: "09",
            title: "期货关联",
            en: "Futures Link",
            weight: 2,
            kpis: &["linked_contract", "contract_trend"],
            kpi_labels: &[("linked_contract", "关联品种"), ("contract_trend", "走势")],
        },
        "10_valuation" => DimMeta {
            id: "10",
            title: "估值多维",
            en: "Valuation",
            weight: 5,
            kpis: &["pe", "pe_quantile", "industry_pe", "dcf"],
            kpi_labels: &[
                ("pe", "当前 PE"),
                ("pe_quantile", "PE 5年分位"),
                ("industry_pe", "行业均值"),
                ("dcf", "DCF 内在值"),
            ],
        },
        "11_governance" => DimMeta {
            id: "11",
            title: "管理层与治理",
            en: "Governance",
            weight: 4,
            kpis: &["pledge", "insider", "related_tx", "violations"],
            kpi_labels: &[
                ("pledge", "实控人质押"),
                ("insider", "近12月增减持"),
                ("related_tx", "关联交易"),
                ("violations", "违规记录"),
            ],
        },
        "12_capital_flow" => DimMeta {
            id: "12",
            title: "资金面",
            en: "Capital Flow",
            weight: 4,
            kpis: &["main_20d", "margin_trend", "holders_trend", "main_5d"],
            kpi_labels: &[
                ("main_20d", "主力资金20日"),
                ("margin_trend", "融资余额"),
                ("holders_trend", "股东户数"),
                ("main_5d", "主力5日"),
            ],
        },
        "13_policy" => DimMeta {
            id: "13",
            title: "政策与监管",
            en: "Policy",
            weight: 3,
            kpis: &["policy_dir", "subsidy", "monitoring", "anti_trust"],
            kpi_labels: &[
                ("policy_dir", "政策方向"),
                ("subsidy", "补贴税收"),
                ("monitoring", "监管动向"),
                ("anti_trust", "反垄断"),
            ],
        },
        "14_moat" => DimMeta {
            id: "14",
            title: "护城河 (5 类)",
            en: "Moat",
            weight: 3,
            kpis: &["intangible", "switching", "network", "scale"],
            kpi_labels: &[
                ("intangible", "无形资产"),
                ("switching", "转换成本"),
                ("network", "网络效应"),
                ("scale", "规模优势"),
            ],
        },
        "15_events" => DimMeta {
            id: "15",
            title: "事件驱动",
            en: "Events",
            weight: 4,
            kpis: &["recent_news", "catalyst", "earnings_preview", "warnings"],
            kpi_labels: &[
                ("recent_news", "近30天事件"),
                ("catalyst", "催化剂"),
                ("earnings_preview", "业绩预告"),
                ("warnings", "利空"),
            ],
        },
        "16_lhb" => DimMeta {
            id: "16",
            title: "龙虎榜",
            en: "Dragon-Tiger",
            weight: 4,
            kpis: &["lhb_30d", "youzi_matched", "inst_net", "youzi_net"],
            kpi_labels: &[
                ("lhb_30d", "30天上榜"),
                ("youzi_matched", "识别游资"),
                ("inst_net", "机构净买"),
                ("youzi_net", "游资净买"),
            ],
        },
        "17_sentiment" => DimMeta {
            id: "17",
            title: "舆情与大V",
            en: "Sentiment",
            weight: 3,
            kpis: &["xueqiu_heat", "guba_volume", "big_v_mentions", "positive_pct"],
            kpi_labels: &[
                ("xueqiu_heat", "雪球热度"),
                ("guba_volume", "股吧讨论"),
                ("big_v_mentions", "大V提及"),
                ("positive_pct", "正面占比"),
            ],
        },
        "18_trap" => DimMeta {
            id: "18",
            title: "杀猪盘检测",
            en: "Trap Scan",
            weight: 5,
            kpis: &["signals_hit", "trap_level", "high_risk_kw", "evidence_count"],
            kpi_labels: &[
                ("signals_hit", "命中信号"),
                ("trap_level", "风险等级"),
                ("high_risk_kw", "高危词"),
                ("evidence_count", "证据数"),
            ],
        },
        "19_contests" => DimMeta {
            id: "19",
            title: "实盘比赛持仓",
            en: "Live Contests",
            weight: 4,
            kpis: &["xq_cubes", "high_return_cubes", "tgb_mentions", "ths_simu"],
            kpi_labels: &[
                ("xq_cubes", "雪球组合"),
                ("high_return_cubes", "高收益持有"),
                ("tgb_mentions", "淘股吧"),
                ("ths_simu", "同花顺模拟"),
            ],
        },
        _ => return None,
    };
    Some(m)
}

fn cat_groups(cat: &str) -> &'static [&'static str] {
    match cat {
        "fin" => &["1_financials", "10_valuation", "14_moat"],
        "mkt" => &["2_kline", "12_capital_flow", "16_lhb"],
        "ind" => &["4_peers", "5_chain", "7_industry", "8_materials", "9_futures"],
        "co" => &["11_governance", "15_events", "6_research"],
        "env" => &["3_macro", "13_policy"],
        "saf" => &["17_sentiment", "18_trap", "19_contests"],
        _ => &[],
    }
}

fn extract_kpi_value(raw_dim_data: &Value, key: &str) -> String {
    let obj = match raw_dim_data.as_object() {
        Some(o) => o,
        None => return "—".to_string(),
    };
    if let Some(v) = obj.get(key) {
        return if v.is_null() { "—".to_string() } else { disp(v) };
    }
    for sub in obj.values() {
        if let Some(s) = sub.as_object() {
            if let Some(v) = s.get(key) {
                return if v.is_null() { "—".to_string() } else { disp(v) };
            }
        }
    }
    "—".to_string()
}

/// Render one dimension card (data-driven from `DIM_META`).
pub fn render_dim_card(dim_key: &str, dim_score: &Value, raw_dim: &Value) -> String {
    let dim_score = escape_payload(dim_score);
    let raw_dim = escape_payload(raw_dim);
    let meta = match dim_meta(dim_key) {
        Some(m) => m,
        None => return String::new(),
    };
    let score = dim_score.get("score").cloned().unwrap_or(Value::Null);
    let label = safe(get_or(&dim_score, "label", &NULL), "—");
    let pass_items = match dim_score.get("reasons_pass") {
        Some(v) if uzi_core::py::truthy(v) => v.clone(),
        _ => Value::Array(vec![]),
    };
    let fail_items = match dim_score.get("reasons_fail") {
        Some(v) if uzi_core::py::truthy(v) => v.clone(),
        _ => Value::Array(vec![]),
    };
    let weight = match dim_score.get("weight") {
        Some(v) if uzi_core::py::truthy(v) => num(v) as i64,
        _ => meta.weight,
    };
    let score_cls = score_class(&score);
    let score_pct = {
        let v = match &score {
            Value::Number(n) if n.is_i64() => Value::from(n.as_i64().unwrap_or(0) * 10),
            Value::Number(n) => Value::from(n.as_f64().unwrap_or(0.0) * 10.0),
            _ => Value::from(0),
        };
        disp(&v)
    };
    let stars = format!("{}{}", "★".repeat(weight.max(0) as usize), "☆".repeat((5 - weight).max(0) as usize));

    let raw_data = match raw_dim.get("data") {
        Some(v) if uzi_core::py::truthy(v) => v.clone(),
        _ => Value::Object(Map::new()),
    };
    let fallback = raw_dim
        .get("fallback")
        .map(uzi_core::py::truthy)
        .unwrap_or(false);
    let source = safe(get_or(&raw_dim, "source", &Value::String("—".to_string())), "—");
    let source_lower = source.to_lowercase();
    let source_label = if !fallback && !source.is_empty() && !source_lower.contains("web_search") {
        "官方接口"
    } else if source_lower.contains("web_search") && !fallback {
        "官方接口"
    } else if fallback {
        "web_search"
    } else {
        "官方接口"
    };

    let viz_html = match viz_for(dim_key) {
        Some(f) => {
            let data = raw_data.clone();
            match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| f(&data))) {
                Ok(html) => format!(r##"<div class="dim-viz">{html}</div>"##),
                Err(_) => {
                    r##"<div class="dim-viz" style="color:#dc2626;font-size:11px">viz error: render failed</div>"##
                        .to_string()
                }
            }
        }
        None => String::new(),
    };

    let kpi_html = if viz_html.is_empty() {
        let mut kpi_cells: Vec<String> = Vec::new();
        for k in meta.kpis {
            let v = extract_kpi_value(&raw_data, k);
            if v != "—" {
                let label_k = meta
                    .kpi_labels
                    .iter()
                    .find(|(key, _)| key == k)
                    .map(|(_, l)| (*l).to_string())
                    .unwrap_or_else(|| (*k).to_string());
                kpi_cells.push(format!(
                    r##"<div class="kpi"><div class="k">{label_k}</div><div class="v">{v}</div></div>"##
                ));
            }
        }
        if kpi_cells.is_empty() {
            String::new()
        } else {
            format!(
                r##"<div class="dim-kpis">{}</div>"##,
                kpi_cells.concat()
            )
        }
    } else {
        String::new()
    };

    let pf_html = if uzi_core::py::truthy(&pass_items) || uzi_core::py::truthy(&fail_items) {
        let mut s = r##"<div class="dim-pass-fail">"##.to_string();
        if uzi_core::py::truthy(&pass_items) {
            s.push_str(&format!(
                r##"<div class="pass"><ul>{}</ul></div>"##,
                li(&pass_items)
            ));
        }
        if uzi_core::py::truthy(&fail_items) {
            s.push_str(&format!(
                r##"<div class="fail"><ul>{}</ul></div>"##,
                li(&fail_items)
            ));
        }
        s.push_str("</div>");
        s
    } else {
        String::new()
    };

    let badge_cls = if fallback { "fallback" } else { "live" };
    let badge_text = if fallback { "公开信息" } else { source_label };

    let mut raw_dump = uzi_core::json::to_pretty(&raw_data);
    if raw_dump.chars().count() > 1500 {
        raw_dump = format!("{}\n... (truncated)", raw_dump.chars().take(1500).collect::<String>());
    }
    let raw_dump = escape_text(&Value::String(raw_dump));

    let score_display = if score.is_null() {
        "—".to_string()
    } else {
        disp(&score)
    };

    format!(
        r##"<div class="dim-card" data-dim="{id}">
  <div class="dim-head">
    <div>
      <div class="dim-num">DIM {id} · WEIGHT {stars}</div>
      <div class="dim-title">{title}</div>
      <div class="dim-en">{en}</div>
    </div>
    <div class="dim-score">
      <div class="num {score_cls}">{score_display}</div>
    </div>
  </div>
  <div class="dim-bar"><div class="fill {score_cls}" style="width: {score_pct}%"></div></div>
  <div class="dim-label">{label}</div>
  {viz_html}
  {kpi_html}
  {pf_html}
  <div class="dim-source">数据来源: <span class="badge {badge_cls}">{badge_text}</span></div>
  <details>
    <summary>查看原始数据 ▼</summary>
    <pre>{raw_dump}</pre>
  </details>
</div>"##,
        id = meta.id,
        title = meta.title,
        en = meta.en,
    )
}

/// Render all cards in one category.
pub fn render_dim_category(cat: &str, dimensions: &Value, raw: &Value) -> String {
    let raw_dims = if uzi_core::py::truthy(raw) {
        raw.get("dimensions").cloned().unwrap_or(Value::Object(Map::new()))
    } else {
        Value::Object(Map::new())
    };
    let dim_scores = if uzi_core::py::truthy(dimensions) {
        dimensions
            .get("dimensions")
            .cloned()
            .unwrap_or(Value::Object(Map::new()))
    } else {
        Value::Object(Map::new())
    };
    let mut cards: Vec<String> = Vec::new();
    for key in cat_groups(cat) {
        cards.push(render_dim_card(
            key,
            get_or(&dim_scores, key, &Value::Object(Map::new())),
            get_or(&raw_dims, key, &Value::Object(Map::new())),
        ));
    }
    cards.join("\n")
}

fn render_pipeline_fallback_banner(ticker: &str) -> String {
    let safe_ticker: String = ticker
        .chars()
        .map(|ch| {
            if ch.is_alphanumeric() || matches!(ch, '.' | '-' | '_') {
                ch
            } else {
                '_'
            }
        })
        .take(80)
        .collect();
    let name = if safe_ticker.is_empty() {
        "unknown".to_string()
    } else {
        safe_ticker
    };
    let marker = uzi_core::cache::cache_root()
        .join(name)
        .join("_pipeline_fallback.json");
    if !marker.exists() {
        return String::new();
    }
    let payload = match std::fs::read_to_string(&marker)
        .ok()
        .and_then(|t| serde_json::from_str::<Value>(&t).ok())
    {
        Some(p) => p,
        None => return String::new(),
    };
    let error_type = escape_text(get_or(&payload, "error_type", &Value::String("PipelineError".into())));
    let message = escape_text(get_or(&payload, "error", &Value::String("未知错误".into())));
    let created_at = escape_text(get_or(&payload, "created_at", &Value::String(String::new())));
    format!(
        r##"<div class="pipeline-fallback-banner" style="margin:12px 0;padding:12px 16px;border:1px solid #f59e0b;background:#fffbeb;color:#92400e;font-size:12px">
  <strong>执行路径降级：</strong>本报告由 legacy 流程生成，pipeline 未完整执行。
  <span style="margin-left:8px">{error_type}: {message}</span>
  <span style="margin-left:8px;color:#a16207">{created_at}</span>
</div>"##
    )
}

fn get_plugin_version() -> String {
    let root = assets_dir();
    let deep = root.parent().unwrap_or(Path::new(""));
    let repo = deep.parent().unwrap_or(Path::new("")).parent().unwrap_or(Path::new(""));
    let manifest = repo.join(".claude-plugin").join("plugin.json");
    if let Ok(text) = std::fs::read_to_string(&manifest) {
        if let Ok(v) = serde_json::from_str::<Value>(&text) {
            if let Some(ver) = v.get("version").and_then(|x| x.as_str()) {
                return ver.to_string();
            }
        }
    }
    "?".to_string()
}

fn copy_dir(src: &Path, dst: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let ty = entry.file_type()?;
        let target = dst.join(entry.file_name());
        if ty.is_dir() {
            copy_dir(&entry.path(), &target)?;
        } else {
            std::fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
}

/// Assemble and inline the report; returns the standalone HTML path.
///
/// Before rendering, runs the mechanical self-review gate exactly like upstream
/// `assemble_report.assemble`: a critical issue count blocks report generation
/// (set `UZI_SKIP_REVIEW=1` to bypass, for development only); warnings are
/// recorded and printed but still produce the HTML.
pub fn assemble(ticker: &str) -> anyhow::Result<String> {
    let syn_opt = uzi_core::cache::read_task_output(ticker, "synthesis");
    let raw_opt = uzi_core::cache::read_task_output(ticker, "raw_data");
    let panel_opt = uzi_core::cache::read_task_output(ticker, "panel");
    if syn_opt.is_none() || raw_opt.is_none() || panel_opt.is_none() {
        return Err(anyhow!(
            "Missing prerequisite cache for {ticker}. Run Tasks 1-4 first."
        ));
    }

    // v2.9 · mechanical self-review gate, run BEFORE any HTML is produced.
    if std::env::var("UZI_SKIP_REVIEW").map(|v| v != "1").unwrap_or(true) {
        let review = uzi_review::self_review::review_all(ticker, None);
        uzi_review::self_review::write_review(ticker, &review);
        let crit = review
            .get("critical_count")
            .and_then(|v| v.as_i64())
            .unwrap_or(0);
        let warn = review
            .get("warning_count")
            .and_then(|v| v.as_i64())
            .unwrap_or(0);
        if crit > 0 {
            println!("{}", uzi_review::self_review::format_human(&review));
            anyhow::bail!(
                "⛔ BLOCKED by self-review: {} 有 {} 个 critical 问题待修。\n→ 读 .cache/{}/_review_issues.json\n→ 对每条 critical issue 执行 suggested_fix（agent 补数据 / 写 agent_analysis）\n→ 全部修完后重跑 uzi <ticker> --stage2。\n→ 如需强制跳过（仅调试）：export UZI_SKIP_REVIEW=1",
                ticker,
                crit,
                ticker
            );
        }
        if warn > 0 {
            println!("{}", uzi_review::self_review::format_human(&review));
            println!(
                "⚠  {}: {} warning 已记录，继续生成 HTML",
                ticker, warn
            );
        }
    }

    let syn = escape_payload(&syn_opt.unwrap());
    let raw = escape_payload(&raw_opt.unwrap());
    let panel = escape_payload(&panel_opt.unwrap());

    let basic = {
        let d = raw
            .get("dimensions")
            .and_then(|x| x.get("0_basic"))
            .and_then(|x| x.get("data"))
            .cloned()
            .unwrap_or(Value::Null);
        if d.is_object() {
            d
        } else {
            Value::Object(Map::new())
        }
    };
    let mkt = {
        let a = raw.get("market").filter(|v| uzi_core::py::truthy(v));
        match a {
            Some(v) => disp(v),
            None => match basic.get("market") {
                Some(v) if uzi_core::py::truthy(v) => disp(v),
                _ => "A".to_string(),
            },
        }
    };
    let currency_symbol = match mkt.as_str() {
        "H" => "HK$",
        "U" => "$",
        _ => "¥",
    };

    let debate = get_or(&syn, "debate", &NULL).clone();
    let divide = get_or(&syn, "great_divide", &NULL).clone();
    let dashboard = get_or(&syn, "dashboard", &NULL).clone();
    let dp = get_or(&dashboard, "data_perspective", &NULL).clone();
    let intel = get_or(&dashboard, "intelligence", &NULL).clone();
    let bp = get_or(&dashboard, "battle_plan", &NULL).clone();
    let zones = get_or(&syn, "buy_zones", &NULL).clone();
    let trap = raw
        .get("dimensions")
        .and_then(|d| d.get("18_trap"))
        .and_then(|d| d.get("data"))
        .cloned()
        .unwrap_or(Value::Null);
    let trap_level = match trap.get("trap_level") {
        Some(v) if uzi_core::py::truthy(v) => disp(v),
        _ => "🟢 安全".to_string(),
    };
    let (trap_color, trap_emoji) = trap_color_emoji(&trap_level);

    let bull = get_or(&debate, "bull", &NULL).clone();
    let bear = get_or(&debate, "bear", &NULL).clone();
    let last_round = debate
        .get("rounds")
        .and_then(|v| v.as_array())
        .and_then(|a| a.last())
        .cloned()
        .unwrap_or(Value::Null);

    let investors = get_or(&panel, "investors", &Value::Array(vec![])).clone();
    let investors_arr = investors.as_array().cloned().unwrap_or_default();
    let mut chat_ordered = investors_arr.clone();
    chat_ordered.sort_by(|a, b| {
        let rank = |v: &Value| -> i32 {
            match v.get("signal").and_then(|s| s.as_str()).unwrap_or("neutral") {
                "bullish" => 0,
                "bearish" => 1,
                "neutral" => 2,
                _ => 3,
            }
        };
        let ca = num(a.get("confidence").unwrap_or(&Value::Number(0.into())));
        let cb = num(b.get("confidence").unwrap_or(&Value::Number(0.into())));
        rank(a)
            .cmp(&rank(b))
            .then_with(|| (-ca).partial_cmp(&(-cb)).unwrap_or(std::cmp::Ordering::Equal))
    });

    let sig_dist = get_or(&panel, "signal_distribution", &NULL).clone();
    let bull_count = sig_dist.get("bullish").cloned().unwrap_or(Value::Number(0.into()));
    let bear_count = sig_dist.get("bearish").cloned().unwrap_or(Value::Number(0.into()));
    let neut_count = sig_dist.get("neutral").cloned().unwrap_or(Value::Number(0.into()));

    let template_path = assets_dir().join("report-template.html");
    let mut template = std::fs::read_to_string(&template_path)
        .map_err(|e| anyhow!("{}: {e}", template_path.display()))?;

    let market_state = uzi_core::cache::market_status(&mkt, None);

    let change_pct = basic.get("change_pct").cloned().unwrap_or(Value::Null);
    let change_pct_str = if change_pct.is_null() {
        "—".to_string()
    } else {
        format!("{:+.2}%", num(&change_pct))
    };
    let change_dir = if num(&change_pct) >= 0.0 { "up" } else { "down" };
    let intel_risks: Vec<String> = intel
        .get("risks")
        .and_then(|v| v.as_array())
        .map(|a| a.iter().map(disp).collect())
        .unwrap_or_default();
    let intel_cats: Vec<String> = intel
        .get("catalysts")
        .and_then(|v| v.as_array())
        .map(|a| a.iter().map(disp).collect())
        .unwrap_or_default();
    let zone_price = |name: &str| -> String {
        let z = get_or(&zones, name, &NULL);
        let p = get_or(z, "price", &NULL);
        if p.is_null() {
            "—".to_string()
        } else {
            disp(p)
        }
    };
    let zone_rationale = |name: &str| -> String {
        let z = get_or(&zones, name, &NULL);
        safe(get_or(z, "rationale", &NULL), "—")
    };
    let tag_of = |inv: &Value| -> String {
        let g = get_or(inv, "group", &NULL);
        let mapped = if uzi_core::py::truthy(g) {
            crate::panel_cards::GROUP_LABELS
                .iter()
                .find(|(k, _)| *k == disp(g))
                .map(|(_, v)| (*v).to_string())
        } else {
            None
        };
        match mapped {
            Some(m) => m,
            None => safe(get_or(inv, "tagline", &NULL), ""),
        }
    };
    let bull_tag = tag_of(&bull);
    let bear_tag = tag_of(&bear);
    let signal_cn = |v: &Value| -> String {
        match disp(v).as_str() {
            "bullish" => "看多",
            "neutral" => "中性",
            "bearish" => "看空",
            _ => "",
        }
        .to_string()
    };
    let bull_signal = get_or(&divide, "bull_signal", &Value::String(String::new())).clone();
    let bear_signal = get_or(&divide, "bear_signal", &Value::String(String::new())).clone();
    let bull_signal_cn = {
        let s = signal_cn(&bull_signal);
        if s.is_empty() { "看多".to_string() } else { s }
    };
    let bear_signal_cn = {
        let s = signal_cn(&bear_signal);
        if s.is_empty() { "看空".to_string() } else { s }
    };
    let fetched_at: String = disp(get_or(&raw, "fetched_at", &Value::String(String::new())))
        .chars()
        .take(19)
        .collect::<String>()
        .replace('T', " ");
    let verdict_label = {
        let base = safe(get_or(&syn, "verdict_label", &NULL), "—");
        match syn.get("verdict_detail") {
            Some(v) if uzi_core::py::truthy(v) => format!("{base} · {}", disp(v)),
            _ => base,
        }
    };
    let punchline = {
        let a = get_or(&divide, "punchline", &NULL);
        if uzi_core::py::truthy(a) {
            safe(a, "—")
        } else {
            safe(get_or(&debate, "punchline", &NULL), "—")
        }
    };

    let two_level = |primary: &Value, fallback: &Value, key: &str| -> Value {
        if uzi_core::py::truthy(primary) {
            primary.clone()
        } else {
            get_or(fallback, key, &NULL).clone()
        }
    };
    let replacements: Vec<(&str, String)> = vec![
        ("{{NAME}}", safe(&two_level(get_or(&syn, "name", &NULL), &basic, "name"), "—")),
        ("{{TICKER}}", safe(&two_level(get_or(&syn, "ticker", &NULL), &basic, "code"), "—")),
        ("{{CURRENCY}}", currency_symbol.to_string()),
        (
            "{{ONE_LINER}}",
            {
                let a = get_or(&basic, "one_liner", &NULL);
                let v = if uzi_core::py::truthy(a) {
                    a.clone()
                } else {
                    get_or(&basic, "industry", &Value::String(String::new())).clone()
                };
                safe(&v, "")
            },
        ),
        ("{{PRICE}}", safe(get_or(&basic, "price", &NULL), "—")),
        ("{{CHANGE_PCT}}", change_pct_str),
        ("{{CHANGE_DIR}}", change_dir.to_string()),
        ("{{MCAP}}", safe(get_or(&basic, "market_cap", &NULL), "—")),
        ("{{PE}}", safe(get_or(&basic, "pe_ttm", &NULL), "—")),
        ("{{PB}}", safe(get_or(&basic, "pb", &NULL), "—")),
        ("{{INDUSTRY}}", safe(get_or(&basic, "industry", &NULL), "—")),
        ("{{OVERALL_SCORE}}", disp(get_or(&syn, "overall_score", &Value::Number(0.into())))),
        (
            "{{OVERALL_SCORE_INT}}",
            format!("{}", num(get_or(&syn, "overall_score", &Value::Number(0.into()))) as i64),
        ),
        ("{{VERDICT_LABEL}}", verdict_label),
        ("{{TRAP_LEVEL}}", trap_level.clone()),
        ("{{TRAP_COLOR}}", trap_color.to_string()),
        ("{{TRAP_EMOJI}}", trap_emoji.to_string()),
        (
            "{{TRAP_RECOMMENDATION}}",
            safe(
                get_or(&trap, "recommendation", &NULL),
                "数据正常，未发现异常推广痕迹",
            ),
        ),
        (
            "{{CORE_CONCLUSION}}",
            safe(get_or(&dashboard, "core_conclusion", &NULL), "—"),
        ),
        ("{{DP_TREND}}", safe(get_or(&dp, "trend", &NULL), "—")),
        ("{{DP_PRICE}}", safe(get_or(&dp, "price", &NULL), "—")),
        ("{{DP_VOLUME}}", safe(get_or(&dp, "volume", &NULL), "—")),
        ("{{DP_CHIPS}}", safe(get_or(&dp, "chips", &NULL), "—")),
        ("{{INTEL_NEWS}}", safe(get_or(&intel, "news", &NULL), "—")),
        ("{{INTEL_RISKS}}", safe(&Value::String(intel_risks.join(", ")), "—")),
        ("{{INTEL_CATALYSTS}}", safe(&Value::String(intel_cats.join(", ")), "—")),
        ("{{BP_ENTRY}}", safe(get_or(&bp, "entry", &NULL), "—")),
        ("{{BP_POSITION}}", safe(get_or(&bp, "position", &NULL), "—")),
        ("{{BP_STOP}}", safe(get_or(&bp, "stop", &NULL), "—")),
        ("{{BP_TARGET}}", safe(get_or(&bp, "target", &NULL), "—")),
        (
            "{{BULL_ID}}",
            safe(get_or(&bull, "investor_id", &NULL), "_placeholder"),
        ),
        ("{{BULL_NAME}}", safe(get_or(&bull, "name", &NULL), "（未选出）")),
        ("{{BULL_SCORE}}", disp(get_or(&divide, "bull_score", &Value::Number(0.into())))),
        (
            "{{BULL_LAST_SAY}}",
            safe(get_or(&last_round, "bull_say", &NULL), "—"),
        ),
        (
            "{{BEAR_ID}}",
            safe(get_or(&bear, "investor_id", &NULL), "_placeholder"),
        ),
        ("{{BEAR_NAME}}", safe(get_or(&bear, "name", &NULL), "（未选出）")),
        ("{{BEAR_SCORE}}", disp(get_or(&divide, "bear_score", &Value::Number(0.into())))),
        (
            "{{BEAR_LAST_SAY}}",
            safe(get_or(&last_round, "bear_say", &NULL), "—"),
        ),
        ("{{PUNCHLINE}}", punchline.clone()),
        ("{{ZONE_VALUE_PRICE}}", zone_price("value")),
        ("{{ZONE_VALUE_RATIONALE}}", zone_rationale("value")),
        ("{{ZONE_GROWTH_PRICE}}", zone_price("growth")),
        ("{{ZONE_GROWTH_RATIONALE}}", zone_rationale("growth")),
        ("{{ZONE_TECH_PRICE}}", zone_price("technical")),
        ("{{ZONE_TECH_RATIONALE}}", zone_rationale("technical")),
        ("{{ZONE_YOUZI_PRICE}}", zone_price("youzi")),
        ("{{ZONE_YOUZI_RATIONALE}}", zone_rationale("youzi")),
        (
            "{{GENERATED_AT}}",
            chrono::Local::now().format("%Y-%m-%d %H:%M").to_string(),
        ),
        ("{{BULL_COUNT}}", disp(&bull_count)),
        ("{{BEAR_COUNT}}", disp(&bear_count)),
        ("{{NEUT_COUNT}}", disp(&neut_count)),
        (
            "{{CONSENSUS_PCT}}",
            format!("{:.0}", num(get_or(&panel, "panel_consensus", &Value::Number(0.into())))),
        ),
        ("{{BULL_TAG}}", bull_tag),
        ("{{BEAR_TAG}}", bear_tag),
        ("{{BULL_SIGNAL_CN}}", bull_signal_cn),
        ("{{BEAR_SIGNAL_CN}}", bear_signal_cn),
        ("{{TOTAL_COUNT}}", investors_arr.len().to_string()),
        ("{{MARKET_STATUS}}", market_state.label.clone()),
        (
            "{{MARKET_STATUS_CLASS}}",
            if market_state.is_open { "open" } else { "closed" }.to_string(),
        ),
        ("{{DATA_FETCHED_AT}}", fetched_at),
        ("{{PLUGIN_VERSION}}", get_plugin_version()),
    ];
    // Sanitize the two `safe(...).as_str().pipe_value()` helpers below by using
    // the plain `disp` of the original value instead (kept explicit for parity).
    for (k, v) in replacements {
        template = template.replace(k, &escape_text(&Value::String(v)));
    }

    template = template.replace(
        "<!-- INJECT_JURY_SEATS -->",
        &investors_arr
            .iter()
            .map(render_jury_seat)
            .collect::<Vec<_>>()
            .join("\n"),
    );
    template = template.replace(
        "<!-- INJECT_CHAT_MESSAGES -->",
        &chat_ordered
            .iter()
            .map(render_chat_message)
            .collect::<Vec<_>>()
            .join("\n"),
    );
    template = template.replace(
        "<!-- INJECT_VOTE_BARS -->",
        &render_vote_bars(get_or(&panel, "vote_distribution", &Value::Object(Map::new()))),
    );
    template = template.replace("<!-- INJECT_TOP3_BULLS -->", &render_top3_bulls(&investors));
    template = template.replace("<!-- INJECT_TOP3_BEARS -->", &render_top3_bears(&investors));
    template = template.replace(
        uzi_core::assets::PANEL_INSIGHTS_MARKER,
        &render_panel_insights(&syn, &panel),
    );

    let school_html = render_school_scores(&syn, &panel);
    if !school_html.is_empty() {
        if template.contains("<!-- INJECT_SCHOOL_SCORES -->") {
            template = template.replace("<!-- INJECT_SCHOOL_SCORES -->", &school_html);
        } else {
            let anchored = template.replacen(
                "</div>\n        <!-- Top 3 Bears",
                &format!("</div>\n        {school_html}\n        <!-- Top 3 Bears"),
                1,
            );
            template = anchored;
            if !template.contains(&school_html) {
                template = template.replacen(
                    "<!-- INJECT_DEBATE_ROUNDS -->",
                    &format!("{school_html}\n<!-- INJECT_DEBATE_ROUNDS -->"),
                    1,
                );
            }
        }
    }
    template = template.replace(
        "<!-- INJECT_RISKS -->",
        &render_risks(get_or(&syn, "risks", &Value::Array(vec![]))),
    );
    template = template.replace("<!-- INJECT_DEBATE_ROUNDS -->", &render_debate_rounds(&debate));
    template = template.replace(
        "<!-- INJECT_FRIENDLY_LAYER -->",
        &render_friendly_layer(&syn, &raw),
    );

    let fund_managers = {
        let a = get_or(&syn, "fund_managers", &NULL);
        if uzi_core::py::truthy(a) {
            a.clone()
        } else {
            get_or(&raw, "fund_managers", &Value::Array(vec![])).clone()
        }
    };
    template = template.replace(
        "<!-- INJECT_FUND_MANAGERS -->",
        &render_fund_managers(&fund_managers),
    );

    let dimensions = escape_payload(
        &uzi_core::cache::read_task_output(ticker, "dimensions").unwrap_or(Value::Object(Map::new())),
    );
    template = template.replace(
        "<!-- INJECT_DIM_FINANCIAL -->",
        &render_dim_category("fin", &dimensions, &raw),
    );
    template = template.replace(
        "<!-- INJECT_DIM_MARKET -->",
        &render_dim_category("mkt", &dimensions, &raw),
    );
    template = template.replace(
        "<!-- INJECT_DIM_INDUSTRY -->",
        &render_dim_category("ind", &dimensions, &raw),
    );
    template = template.replace(
        "<!-- INJECT_DIM_COMPANY -->",
        &render_dim_category("co", &dimensions, &raw),
    );
    template = template.replace(
        "<!-- INJECT_DIM_ENV -->",
        &render_dim_category("env", &dimensions, &raw),
    );
    template = template.replace(
        "<!-- INJECT_DIM_SAFETY -->",
        &render_dim_category("saf", &dimensions, &raw),
    );

    let mut inst_html = render_institutional_section(&raw);
    if currency_symbol != "¥" {
        inst_html = inst_html.replace('¥', currency_symbol);
    }
    template = template.replace("<!-- INJECT_INSTITUTIONAL_MODELING -->", &inst_html);

    let seg_html = render_segmental_block(ticker);
    template = template.replace("<!-- INJECT_SEGMENTAL -->", &seg_html);

    let school_lock_html = render_school_lock_banner(&syn);
    let data_gap_html = render_data_gap_banner(
        get_or(&syn, "data_gaps", &NULL),
        &raw,
        &syn,
    );
    let pipeline_fallback_html = render_pipeline_fallback_banner(ticker);
    template = template.replace(
        "<!-- INJECT_DATA_GAP_BANNER -->",
        &format!("{pipeline_fallback_html}{school_lock_html}{data_gap_html}"),
    );

    template = template.replace("<!-- INJECT_STYLE_CHIP -->", &render_style_chip(&syn));

    if currency_symbol != "¥" {
        template = template.replace('¥', currency_symbol);
    }

    let date = chrono::Local::now().format("%Y%m%d").to_string();
    let out_dir = crate::inline::reports_dir().join(format!("{ticker}_{date}"));
    std::fs::create_dir_all(&out_dir)?;
    let out_file = out_dir.join("full-report.html");
    std::fs::write(&out_file, &template)?;

    let out_avatars = out_dir.join("avatars");
    if !out_avatars.exists() {
        let avatars_src = assets_dir().join("avatars");
        let _ = copy_dir(&avatars_src, &out_avatars);
    }

    let long_active = {
        let a = get_or(&panel, "long_active", &NULL).clone();
        if uzi_core::py::truthy(&a) {
            num(&a)
        } else {
            ["bullish", "neutral", "bearish"]
                .iter()
                .map(|k| num(get_or(&sig_dist, k, &Value::Number(0.into()))))
                .sum()
        }
    };
    let one_liner = format!(
        "{name} 体检结果：{score} 分，{verdict}。\n{long_active} 位多头评委里 {bullish} 人喊买。\n💬 {punchline}\n{trap_emoji} {trap_level}\n全文 → {out}\n",
        name = disp(get_or(&syn, "name", &NULL)),
        score = num(get_or(&syn, "overall_score", &Value::Number(0.into()))) as i64,
        verdict = disp(get_or(&syn, "verdict_label", &NULL)),
        long_active = pyf(long_active),
        bullish = disp(get_or(&sig_dist, "bullish", &Value::Number(0.into()))),
        punchline = punchline,
        out = out_file.display(),
    );
    let _ = std::fs::write(out_dir.join("one-liner.txt"), one_liner);

    let standalone = crate::inline::inline_assets(ticker)?;
    Ok(standalone.to_string_lossy().to_string())
}

