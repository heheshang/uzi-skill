//! Port of `lib/daily_screen/renderer.py` — the dense responsive HTML decision
//! terminal for daily screening.
//!
//! The markup below is an exact transcription of the upstream f-string: for
//! identical input the produced bytes are identical. `main_style` is embedded
//! verbatim (the `<style>…</style>` slice the upstream renderer copies out of
//! `assets/report-template.html`).
//!
//! `screen_style.txt` and `report_template_style.txt` are frozen copies of the
//! two upstream stylesheets. Regenerate `screen_style.txt` from the upstream
//! renderer with `{{`→`{` / `}}`→`}` un-doubling, and
//! `report_template_style.txt` with the `<style>` slice shown above; both are
//! covered byte-for-byte by `tests/golden_daily_screen.rs` (the rendered HTML
//! equals the Python output).

use anyhow::Result;
use serde_json::Value;
use std::path::{Path, PathBuf};

/// `ACTION_LABELS`.
pub const ACTION_LABELS: &[(&str, &str)] = &[
    ("buyable", "数据条件通过，待成交确认"),
    ("wait_pullback", "等分歧/回踩"),
    ("wait_reseal", "等回封确认"),
    ("watch_only", "只看不追"),
    ("unbuyable", "当前不可成交"),
    ("avoid", "回避"),
];

fn action_label(action: &str) -> &str {
    ACTION_LABELS
        .iter()
        .find(|(key, _)| *key == action)
        .map(|(_, label)| *label)
        .unwrap_or(action)
}

/// `_money(value, market)`.
fn money(value: f64, market: &str) -> String {
    let symbol = if market == "H" { "HK$" } else { "¥" };
    format!("{symbol}{:.1}亿", value / 1e8)
}

/// Python `x.get(key, default)` + `or "—"`.
fn dash_or(value: &Value) -> String {
    if uzi_core::py::truthy(value) {
        uzi_core::py::py_display(value)
    } else {
        "—".to_string()
    }
}

/// `value.get(key, default)`.
fn get_or<'a>(value: &'a Value, key: &str, default: &'a str) -> String {
    match value.get(key) {
        Some(v) => uzi_core::py::py_display(v),
        None => default.to_string(),
    }
}

/// The `<style>` block upstream slices out of `assets/report-template.html`.
const MAIN_STYLE: &str = include_str!("../report_template_style.txt");

/// The screening terminal's own stylesheet (upstream renderer f-string body).
const SCREEN_STYLE: &str = include_str!("screen_style.txt");

/// `render_report(report, output, avatars_dir)`.
pub fn render_report(report: &Value, output: &Path, avatars_dir: &Path) -> Result<PathBuf> {
    let report = uzi_report::security::escape_payload(report);
    let picks = report
        .get("picks")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let mut rows: Vec<String> = Vec::new();
    for (index, pick) in picks.iter().enumerate() {
        let rank = index + 1;
        let stock = uzi_core::py::get(pick, "snapshot");
        let verdicts: Vec<Value> = match pick.get("persona_verdicts").and_then(|v| v.as_array()) {
            Some(list) => list.clone(),
            None => Vec::new(),
        };
        let bulls: Vec<&Value> = verdicts
            .iter()
            .filter(|item| uzi_core::py::get(item, "signal").as_str() == Some("bullish"))
            .collect();
        let mut avatars = String::new();
        for item in bulls.iter().take(6) {
            avatars.push_str(&format!(
                "<span class=\"avatar\" title=\"{} · {}\"><img src=\"avatars/{}.svg\" alt=\"\"></span>",
                get_or(item, "name", "—"),
                get_or(item, "style", "—"),
                uzi_report::security::safe_asset_id_default(uzi_core::py::get(item, "investor_id"))
            ));
        }
        let serenity = match pick.get("serenity") {
            Some(v) if v.is_object() => v.clone(),
            _ => Value::Object(Default::default()),
        };
        if uzi_core::py::get(&serenity, "signal").as_str() == Some("bullish") {
            avatars.push_str(
                "<span class=\"avatar serenity\" title=\"Serenity · AI 卡位\"><img src=\"avatars/serenity.svg\" alt=\"\"></span>",
            );
        }
        let mut persona_rows = String::new();
        for item in verdicts
            .iter()
            .filter(|item| matches!(uzi_core::py::get(item, "eligible"), Value::Bool(true)))
        {
            persona_rows.push_str(&format!(
                "<tr><td><img src=\"avatars/{}.svg\" alt=\"\">{}</td><td>{}</td><td><span class=\"signal {}\">{}</span></td><td>{}</td><td>{}</td></tr>",
                uzi_report::security::safe_asset_id_default(uzi_core::py::get(item, "investor_id")),
                get_or(item, "name", "—"),
                get_or(item, "style", "—"),
                get_or(item, "signal", ""),
                get_or(item, "signal", ""),
                get_or(item, "confidence", ""),
                get_or(item, "reasoning_summary", ""),
            ));
        }
        let evidence: String = pick
            .get("evidence")
            .and_then(|v| v.as_array())
            .map(|items| {
                items
                    .iter()
                    .map(|item| {
                        format!(
                            "<li><b>{}</b> <a href=\"{}\" target=\"_blank\" rel=\"noopener noreferrer\">{}</a> <span>{} · {}</span></li>",
                            get_or(item, "grade", "—"),
                            uzi_report::security::safe_url_default(uzi_core::py::get(item, "url")),
                            get_or(item, "title", "—"),
                            get_or(item, "source", ""),
                            get_or(item, "published_at", "—"),
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("")
            })
            .unwrap_or_default();
        let extra = match stock.get("extra") {
            Some(v) if v.is_object() => v.clone(),
            _ => Value::Object(Default::default()),
        };
        let intraday = match extra.get("intraday") {
            Some(v) if v.is_object() => v.clone(),
            _ => Value::Object(Default::default()),
        };
        let quote = match intraday.get("quote") {
            Some(v) if v.is_object() => v.clone(),
            _ => Value::Object(Default::default()),
        };
        let industry_source = match extra.get("industry_source") {
            Some(v) if v.is_object() => v.clone(),
            _ => Value::Object(Default::default()),
        };
        let bars_len = intraday
            .get("bars")
            .and_then(|v| v.as_array())
            .map(|a| a.len())
            .unwrap_or(0);
        let source_errors = match intraday.get("source_errors").and_then(|v| v.as_array()) {
            Some(list) if !list.is_empty() => list
                .iter()
                .map(uzi_core::py::py_display)
                .collect::<Vec<_>>()
                .join(", "),
            _ => "无已记录错误".to_string(),
        };
        let source_details = format!(
            "<h3>数据对齐</h3><p>行业：{}<br>\n盘口：{} · 报价时间 {}<br>\n买一 / 卖一：{} / {} · VWAP {}<br>\n分钟线：{} · {} 条<br>\n源诊断：{}</p>",
            get_or(&industry_source, "source", "—"),
            get_or(&quote, "source", "—"),
            get_or(&quote, "quote_at", "—"),
            get_or(&quote, "bid", "—"),
            get_or(&quote, "ask", "—"),
            get_or(&quote, "vwap", "—"),
            get_or(&intraday, "minute_source", "—"),
            bars_len,
            source_errors,
        );
        let mut risk_items: Vec<String> = Vec::new();
        for key in ["risk_flags", "data_gaps"] {
            if let Some(list) = pick.get(key).and_then(|v| v.as_array()) {
                risk_items.extend(list.iter().map(uzi_core::py::py_display));
            }
        }
        let risks = if risk_items.is_empty() {
            "<li>无额外已识别风险；不代表风险已排除</li>".to_string()
        } else {
            risk_items
                .iter()
                .map(|item| format!("<li>{item}</li>"))
                .collect::<Vec<_>>()
                .join("")
        };
        let support_block = if avatars.is_empty() {
            "<span class=\"none\">暂无强看多角色</span>".to_string()
        } else {
            avatars
        };
        let evidence_block = if evidence.is_empty() {
            "<li>当前仅有行情横截面证据</li>".to_string()
        } else {
            evidence
        };
        rows.push(format!(
            "<article class=\"pick-row\">\n  <div class=\"rank\">{rank:02}</div>\n  <div class=\"identity\"><strong>{}</strong><span>{} · {}股</span></div>\n  <div class=\"quote\"><strong>{:+.2}%</strong><span>{}</span></div>\n  <div class=\"theme\"><strong>{}</strong><span>主题 #{} · 个股 #{}</span></div>\n  <div class=\"action\"><span class=\"action-pill {}\">{}</span><small>规则分 {:.1}</small></div>\n  <div class=\"supporters\">{}</div>\n  <div class=\"thesis\"><span>{}</span><small>失效：{}</small></div>\n  <details><summary>证据与完整评委矩阵</summary><div class=\"detail-grid\"><section><h3>进入条件</h3><p>{}</p>{}<h3>证据</h3><ul>{}</ul><h3>风险</h3><ul>{}</ul></section><section class=\"matrix\"><table><thead><tr><th>评委</th><th>战法</th><th>信号</th><th>置信</th><th>依据</th></tr></thead><tbody>{}</tbody></table></section></div></details>\n</article>",
            get_or(stock, "name", "—"),
            get_or(stock, "code", "—"),
            get_or(stock, "market", ""),
            uzi_core::py::f0(uzi_core::py::get(stock, "change_pct")),
            money(
                uzi_core::py::f0(uzi_core::py::get(stock, "amount")),
                &get_or(stock, "market", "")
            ),
            get_or(stock, "industry", "—"),
            dash_or(uzi_core::py::get(pick, "theme_rank")),
            dash_or(uzi_core::py::get(pick, "leader_rank")),
            get_or(pick, "action", ""),
            action_label(&get_or(pick, "action", "")),
            uzi_core::py::f0(uzi_core::py::get(pick, "research_confidence")),
            support_block,
            get_or(pick, "why_now", ""),
            get_or(pick, "invalidation", ""),
            get_or(pick, "entry_condition", ""),
            source_details,
            evidence_block,
            risks,
            persona_rows,
        ));
    }
    let empty = "<div class=\"empty\"><strong>当前数据不足以形成入榜候选</strong><span>未补足证据或未达到规则阈值，不凑数。</span></div>";
    let actions = match report.get("action_summary") {
        Some(v) if v.is_object() => v.clone(),
        _ => Value::Object(Default::default()),
    };
    let filters = match report.get("filters") {
        Some(v) if v.is_object() => v.clone(),
        _ => Value::Object(Default::default()),
    };
    let top_n = match filters.get("top_n") {
        Some(v) => uzi_core::py::py_display(v),
        None => "10".to_string(),
    };
    let rows_html = if rows.is_empty() {
        empty.to_string()
    } else {
        rows.join("")
    };
    let html = format!(
        "<!doctype html><html lang=\"zh-CN\"><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width,initial-scale=1\"><title>UZI 每日观察榜</title><style>\n{SCREEN_STYLE}\n</style></head><body><main><header><div><h1>UZI 每日观察榜</h1><div class=\"sub\">A+港股 · 游资 F 组 + Serenity · 最多 {top_n} 只，不凑数</div><div class=\"meta\">生成 {} · 快照观测 {}</div></div><div class=\"summary\"><span>条件通过 {}</span><span>等回踩 {}</span><span>等确认 {}</span><span>只观察 {}</span><span>不可成交 {}</span></div></header><div class=\"screen-head\"><span>#</span><span>股票</span><span>盘面</span><span>主线地位</span><span>行动</span><span>规则匹配角色</span><span>为什么现在 / 失效</span></div>{rows_html}<footer>研究辅助，不构成买卖建议。当前为规则初筛，尚非 Agent 角色研判；规则分未经概率校准，不是胜率。缺消息、行业或分时成交验证时仅供观察。</footer></main></body></html>",
        get_or(&report, "generated_at", ""),
        uzi_core::py::py_display(uzi_core::py::get(&report, "as_of_by_market")),
        action_count(&actions, "buyable"),
        action_count(&actions, "wait_pullback"),
        action_count(&actions, "wait_reseal"),
        action_count(&actions, "watch_only"),
        action_count(&actions, "unbuyable"),
    );
    let _ = MAIN_STYLE; // versus/portfolio renderers consume it

    if let Some(parent) = output.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = output.with_extension("html.tmp");
    std::fs::write(&tmp, html)?;
    std::fs::rename(&tmp, output)?;
    let target_avatars = output.parent().map(|p| p.join("avatars"));
    if let Some(target) = target_avatars {
        if !target.exists() && avatars_dir.exists() {
            copy_dir(avatars_dir, &target)?;
        }
    }
    Ok(output.to_path_buf())
}

fn action_count(actions: &Value, key: &str) -> String {
    match actions.get(key) {
        Some(v) => uzi_core::py::py_display(v),
        None => "0".to_string(),
    }
}

/// `shutil.copytree(src, dst)` (recursive; upstream only copies when the target
/// is absent).
fn copy_dir(src: &Path, dst: &Path) -> Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let target = dst.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir(&entry.path(), &target)?;
        } else {
            std::fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
}

/// Exposed so the versus/portfolio renderers share one copy of the template CSS.
pub(crate) fn main_style() -> &'static str {
    MAIN_STYLE
}
