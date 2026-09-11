//! Port of `lib/pipeline/renderer/fund.py` — 6_fund_holders section.

use super::base::{SectionRenderer, RenderContext};
use crate::pyfmt::{disp, num};
use serde_json::{Map, Value};

pub struct FundRenderer;

/// `fund_code → manager_name` fallback when eastmoney SSL blocks the lookup.
pub const FUND_CODE_TO_MANAGER: &[(&str, &str)] = &[
    ("161005", "朱少醒"),
    ("003494", "朱少醒"),
    ("022645", "朱少醒"),
    ("005827", "张坤"),
    ("118001", "张坤"),
    ("110011", "张坤"),
    ("163406", "谢治宇"),
    ("340007", "谢治宇"),
    ("012001", "田瑀"),
    ("012002", "田瑀"),
    ("260108", "刘彦春"),
    ("162605", "刘彦春"),
    ("003095", "葛兰"),
    ("003096", "葛兰"),
    ("100056", "朱少醒"),
    ("519069", "劳杰男"),
    ("007119", "傅鹏博"),
    ("110022", "萧楠"),
];

/// Local avatar slugs that exist under `assets/avatars`.
pub const MANAGER_AVATAR_SLUG: &[(&str, &str)] = &[
    ("张坤", "zhangkun"),
    ("谢治宇", "xiezhiyu"),
    ("朱少醒", "zhushaoxing"),
    ("冯柳", "fengliu"),
    ("邓晓峰", "dengxiaofeng"),
];

/// Resolve a manager name from the fund code fallback map.
pub fn resolve_manager(fund_code: &str, current_name: &str) -> String {
    let known = !current_name.is_empty()
        && !matches!(current_name, "—" | "-" | "n/a");
    if known {
        return current_name.to_string();
    }
    FUND_CODE_TO_MANAGER
        .iter()
        .find(|(c, _)| *c == fund_code)
        .map(|(_, n)| (*n).to_string())
        .unwrap_or_else(|| "—".to_string())
}

/// Resolve the avatar slug for a manager name ("" → text placeholder).
pub fn resolve_avatar(manager_name: &str) -> String {
    if manager_name.is_empty() || matches!(manager_name, "—" | "-") {
        return String::new();
    }
    MANAGER_AVATAR_SLUG
        .iter()
        .find(|(n, _)| *n == manager_name)
        .map(|(_, s)| (*s).to_string())
        .unwrap_or_default()
}

/// Enrich one fund manager row with resolved name + avatar.
pub fn enrich_manager(m: &Value) -> Value {
    let mut out: Map<String, Value> = m.as_object().cloned().unwrap_or_default();
    let fund_code = disp(out.get("fund_code").unwrap_or(&Value::String(String::new())));
    let current_name = disp(out.get("name").unwrap_or(&Value::String(String::new())));
    let resolved = resolve_manager(&fund_code, &current_name);
    if resolved != current_name {
        out.insert("name".to_string(), Value::String(resolved.clone()));
        out.insert(
            "_name_resolved_by".to_string(),
            Value::String("fund_code_map".to_string()),
        );
    }
    if !resolved.is_empty() && resolved != "—" && resolved != "-" {
        let avatar = resolve_avatar(&resolved);
        let has_avatar = out.get("avatar").map(uzi_core::py::truthy).unwrap_or(false);
        if !avatar.is_empty() && !has_avatar {
            out.insert("avatar".to_string(), Value::String(avatar));
        }
    }
    Value::Object(out)
}

fn dedupe_by_code(mgrs: &[Value]) -> Vec<Value> {
    let mut sorted = mgrs.to_vec();
    sorted.sort_by(|a, b| {
        let pa = num(a.get("position_pct").unwrap_or(&Value::Number(0.into())));
        let pb = num(b.get("position_pct").unwrap_or(&Value::Number(0.into())));
        (-pa).partial_cmp(&(-pb)).unwrap_or(std::cmp::Ordering::Equal)
    });
    let mut by_code: Vec<(String, Value)> = Vec::new();
    for m in sorted {
        let code = m.get("fund_code").cloned().unwrap_or(Value::Null);
        if !uzi_core::py::truthy(&code) {
            continue;
        }
        let code_s = disp(&code);
        if !by_code.iter().any(|(c, _)| *c == code_s) {
            by_code.push((code_s, m));
        }
    }
    by_code.into_iter().map(|(_, v)| v).collect()
}

impl FundRenderer {
    const INITIAL_FULL_CAP: usize = 6;
    const LITE_CAP: usize = 30;

    fn render_header(&self, total: usize, full_count: usize, lite_count: usize, overflow: usize) -> String {
        let total_display = total + overflow;
        if lite_count > 0 {
            let overflow_note = if overflow > 0 {
                format!("（另有 {overflow} 家未列 · 点基金链接自行查）")
            } else {
                String::new()
            };
            return format!(
                r##"<div class="fund-mgr-header">✨ <strong>{total_display} 家公募基金</strong>持有本股 · 头部 <strong>{full_count}</strong> 家有完整 5Y 业绩，其余 <strong>{lite_count}</strong> 家按持仓占比列出{overflow_note}</div>"##
            );
        }
        format!(
            r##"<div class="fund-mgr-header">✨ <strong>{full_count} 位公募基金经理</strong>持有本股 · 按 5 年累计收益排序 · 你可以直接"抄作业"</div>"##
        )
    }

    fn render_full_card(&self, m: &Value) -> String {
        let name = match m.get("name") {
            Some(v) if uzi_core::py::truthy(v) => disp(v),
            _ => "—".to_string(),
        };
        let fund_name = match m.get("fund_name") {
            Some(v) if uzi_core::py::truthy(v) => disp(v),
            _ => "—".to_string(),
        };
        let avatar = disp(m.get("avatar").unwrap_or(&Value::String(String::new())));
        let position = disp(m.get("position_pct").unwrap_or(&Value::Number(0.into())));
        let rank = disp(m.get("rank_in_fund").unwrap_or(&Value::Number(0.into())));
        let quarters = disp(m.get("holding_quarters").unwrap_or(&Value::Number(0.into())));
        let trend = m.get("position_trend").and_then(|v| v.as_str()).unwrap_or("持平");
        let (trend_icon, trend_color) = match trend {
            "加仓" => ("📈", "#16a34a"),
            "减仓" => ("📉", "#dc2626"),
            _ => ("➡️", "#64748b"),
        };

        let ret_5y = num(m.get("return_5y").unwrap_or(&Value::Number(0.into())));
        let ann_5y = num(m.get("annualized_5y").unwrap_or(&Value::Number(0.into())));
        let max_dd = num(m.get("max_drawdown").unwrap_or(&Value::Number(0.into())));
        let sharpe = num(m.get("sharpe").unwrap_or(&Value::Number(0.into())));
        let peer_rank = num(m.get("peer_rank_pct").unwrap_or(&Value::Number(50.into())));

        let ret_color = if ret_5y > 0.0 { "#16a34a" } else { "#dc2626" };
        let dd_color = if max_dd > -20.0 {
            "#16a34a"
        } else if max_dd > -40.0 {
            "#f59e0b"
        } else {
            "#dc2626"
        };
        let sharpe_color = if sharpe > 1.0 {
            "#16a34a"
        } else if sharpe > 0.5 {
            "#f59e0b"
        } else {
            "#dc2626"
        };

        let avatar_html = if !avatar.is_empty() {
            format!(
                r##"<img src="avatars/{avatar}.svg" style="width:54px;height:54px;image-rendering:pixelated;border:2px solid #d97706;border-radius:8px;background:#fff;flex-shrink:0">"##
            )
        } else {
            let initial = if !name.is_empty() && name != "—" {
                name.chars().next().map(|c| c.to_string()).unwrap_or_default()
            } else {
                "?".to_string()
            };
            format!(
                r##"<div style="width:54px;height:54px;background:#fef3c7;border:2px solid #d97706;border-radius:8px;display:flex;align-items:center;justify-content:center;font-family:Fira Sans;font-size:20px;font-weight:900;color:#d97706;flex-shrink:0">{initial}</div>"##
            )
        };

        let stars_n = (((100.0 - peer_rank) / 20.0) as i64 + 1).clamp(1, 5) as usize;
        let stars = "⭐".repeat(stars_n);
        let fund_code = disp(m.get("fund_code").unwrap_or(&Value::String(String::new())));
        let fund_url = match m.get("fund_url") {
            Some(v) if uzi_core::py::truthy(v) => disp(v),
            _ => format!("https://fund.eastmoney.com/{fund_code}.html"),
        };

        format!(
            r##"<div class="fund-card">
  <div class="fund-header" style="display:flex;gap:12px;align-items:center">
    {avatar_html}
    <div style="flex:1;min-width:0">
      <div class="fund-manager-name"><strong>{name}</strong> <span class="fund-stars">{stars}</span></div>
      <div class="fund-name">{fund_name}</div>
      <div class="fund-meta">持本股 {quarters} 季 · 位列第 {rank} 大 · 占基金 {position}% · <span style="color:{trend_color};font-weight:700">{trend_icon} {trend}</span></div>
    </div>
  </div>
  <div class="fund-metrics-grid">
    <div class="fund-metric"><div class="fm-label">5 年累计</div><div class="fm-value" style="color:{ret_color}">{ret_sign}{ret_5y:.1}%</div></div>
    <div class="fund-metric"><div class="fm-label">年化</div><div class="fm-value">{ann_sign}{ann_5y:.1}%</div></div>
    <div class="fund-metric"><div class="fm-label">最大回撤</div><div class="fm-value" style="color:{dd_color}">{max_dd:.1}%</div></div>
    <div class="fund-metric"><div class="fm-label">夏普比率</div><div class="fm-value" style="color:{sharpe_color}">{sharpe:.2}</div></div>
  </div>
  <a href="{fund_url}" target="_blank" rel="noopener" class="fund-link">查看基金 →</a>
</div>"##,
            ret_sign = if ret_5y > 0.0 { "+" } else { "" },
            ann_sign = if ann_5y > 0.0 { "+" } else { "" },
        )
    }

    fn render_compact_row(&self, m: &Value, rank: usize) -> String {
        let name = match m.get("name") {
            Some(v) if uzi_core::py::truthy(v) => disp(v),
            _ => "—".to_string(),
        };
        let fund_name = match m.get("fund_name") {
            Some(v) if uzi_core::py::truthy(v) => disp(v),
            _ => "—".to_string(),
        };
        let fund_code = disp(m.get("fund_code").unwrap_or(&Value::String(String::new())));
        let avatar = disp(m.get("avatar").unwrap_or(&Value::String(String::new())));
        let position_pct = num(m.get("position_pct").unwrap_or(&Value::Number(0.into())));
        let fund_url = match m.get("fund_url") {
            Some(v) if uzi_core::py::truthy(v) => disp(v),
            _ => format!("https://fund.eastmoney.com/{fund_code}.html"),
        };

        let badge_style = if rank <= 3 {
            "background:linear-gradient(135deg,#f59e0b,#d97706);color:#fff"
        } else if rank <= 10 {
            "background:#e2e8f0;color:#475569"
        } else {
            "background:#f1f5f9;color:#64748b"
        };

        let avatar_html = if !avatar.is_empty() {
            format!(r##"<img src="avatars/{avatar}.svg" class="fc-avatar" alt="">"##)
        } else {
            let initial = if !name.is_empty() && name != "—" && name != "-" {
                name.chars().next().map(|c| c.to_string()).unwrap_or_default()
            } else if !fund_name.is_empty() && fund_name != "—" && fund_name != "-" {
                fund_name.chars().next().map(|c| c.to_string()).unwrap_or_default()
            } else {
                "?".to_string()
            };
            format!(r##"<div class="fc-avatar fc-avatar-ph">{initial}</div>"##)
        };

        let metric_html = format!(
            r##"<span class="fc-return" style="color:#64748b">持仓 {position_pct:.2}%</span><span class="fc-rank-pct" style="color:#94a3b8;font-size:10px">点→查业绩</span>"##
        );
        let name_display = if name != "—" && name != "-" {
            name.clone()
        } else {
            fund_name.clone()
        };
        let fund_display = if name != "—" && name != "-" {
            format!("代码 {fund_code}")
        } else {
            String::new()
        };

        format!(
            r##"<div class="fund-compact-row">
  <span class="fc-rank" style="{badge_style}">{rank}</span>
  {avatar_html}
  <div class="fc-info">
    <div class="fc-name">{name_display}</div>
    <div class="fc-fund">{fund_display}</div>
  </div>
  {metric_html}
  <a href="{fund_url}" target="_blank" rel="noopener" class="fc-link" title="查看基金详情">→</a>
</div>"##
        )
    }
}

impl SectionRenderer for FundRenderer {
    fn section_id(&self) -> &'static str {
        "fund_managers"
    }

    fn section_title(&self) -> &'static str {
        "大佬抄作业 · 公募基金持仓"
    }

    fn render_full(&self, ctx: &RenderContext) -> String {
        let managers = ctx
            .data
            .get("fund_managers")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        if managers.is_empty() {
            return self.render_gap(ctx, "无公募基金持仓数据");
        }
        let enriched: Vec<Value> = managers.iter().map(enrich_manager).collect();

        let is_full = |m: &Value| {
            m.get("_row_type").and_then(|v| v.as_str()) == Some("full")
                && m.get("return_5y").map(|v| !v.is_null()).unwrap_or(false)
        };
        let is_lite = |m: &Value| {
            m.get("_row_type").and_then(|v| v.as_str()) == Some("lite")
                || m.get("return_5y").map(|v| v.is_null()).unwrap_or(true)
        };
        let full_mgrs: Vec<Value> = enriched.iter().filter(|m| is_full(m)).cloned().collect();
        let lite_mgrs: Vec<Value> = enriched.iter().filter(|m| is_lite(m)).cloned().collect();
        let lite_deduped = dedupe_by_code(&lite_mgrs);
        let lite_capped: Vec<Value> = lite_deduped.iter().take(Self::LITE_CAP).cloned().collect();
        let lite_overflow = lite_deduped.len().saturating_sub(Self::LITE_CAP);

        let full_cards: Vec<String> = full_mgrs
            .iter()
            .take(Self::INITIAL_FULL_CAP)
            .map(|m| self.render_full_card(m))
            .collect();
        let compact_rows: String = lite_capped
            .iter()
            .enumerate()
            .map(|(i, m)| self.render_compact_row(m, i + 1 + full_cards.len()))
            .collect();

        let header = self.render_header(
            enriched.len(),
            full_mgrs.len(),
            lite_deduped.len(),
            lite_overflow,
        );
        let grid = if full_cards.is_empty() {
            String::new()
        } else {
            format!(r##"<div class="fund-mgr-grid">{}</div>"##, full_cards.concat())
        };
        let compact_html = if compact_rows.is_empty() {
            String::new()
        } else {
            format!(
                r##"<div class="fund-compact-list"><div class="fund-compact-head"><span class="fc-h-rank">#</span><span class="fc-h-avatar"></span><span class="fc-h-name">基金经理 / 基金</span><span class="fc-h-metric">持仓</span><span class="fc-h-link"></span></div>{compact_rows}</div>"##
            )
        };

        format!(r##"<section id="fund_managers">{header}{grid}{compact_html}</section>"##)
    }
}
