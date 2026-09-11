//! Port of `lib/pipeline/renderer/moat.py` — 14_moat section.

use super::base::{SectionRenderer, RenderContext};
use crate::pyfmt::{disp, num};
use serde_json::Value;

pub struct MoatRenderer;

/// 四力评分阈值配色。
pub const SCORE_COLORS_STRONG: &str = "#16a34a";
pub const SCORE_COLORS_MEDIUM: &str = "#f59e0b";
pub const SCORE_COLORS_WEAK: &str = "#dc2626";

fn score_color(score: i64) -> &'static str {
    if score >= 7 {
        SCORE_COLORS_STRONG
    } else if score <= 3 {
        SCORE_COLORS_WEAK
    } else {
        SCORE_COLORS_MEDIUM
    }
}

impl SectionRenderer for MoatRenderer {
    fn section_id(&self) -> &'static str {
        "moat"
    }

    fn section_title(&self) -> &'static str {
        "🏰 护城河四力（intangible / switching / network / scale）"
    }

    fn render_full(&self, ctx: &RenderContext) -> String {
        let data = &ctx.data;
        let scores = match data.get("scores") {
            Some(v) if uzi_core::py::truthy(v) => v.clone(),
            _ => Value::Object(Default::default()),
        };

        let fields: [(&str, &str, String); 4] = [
            (
                "intangible",
                "无形资产",
                match data.get("intangible") {
                    Some(v) if uzi_core::py::truthy(v) => disp(v),
                    _ => "—".to_string(),
                },
            ),
            (
                "switching",
                "转换成本",
                match data.get("switching") {
                    Some(v) if uzi_core::py::truthy(v) => disp(v),
                    _ => "—".to_string(),
                },
            ),
            (
                "network",
                "网络效应",
                match data.get("network") {
                    Some(v) if uzi_core::py::truthy(v) => disp(v),
                    _ => "—".to_string(),
                },
            ),
            (
                "scale",
                "规模优势",
                match data.get("scale") {
                    Some(v) if uzi_core::py::truthy(v) => disp(v),
                    _ => "—".to_string(),
                },
            ),
        ];

        let mut items_html: Vec<String> = Vec::new();
        for (key, zh_label, text) in fields {
            let s = scores
                .get(key)
                .map(crate::pyfmt::num)
                .unwrap_or(5.0) as i64;
            let color = score_color(s);
            let body_preview: String = if !text.is_empty() && text != "—" {
                text.chars().take(200).collect()
            } else {
                "数据不足".to_string()
            };
            items_html.push(format!(
                r##"<div class="moat-item">
  <div class="moat-head">
    <strong>{zh_label}</strong>
    <span class="moat-score" style="color:{color};font-weight:700">{s}/10</span>
  </div>
  <div class="moat-body" style="font-size:12px;color:#475569;margin-top:6px">{body_preview}</div>
</div>"##
            ));
        }

        let rd_summary = match data.get("rd_summary") {
            Some(v) if uzi_core::py::truthy(v) => disp(v),
            _ => String::new(),
        };
        let rd_block = if !rd_summary.is_empty() && rd_summary != "—" {
            format!(
                r##"<div class="moat-rd" style="margin-top:12px;padding:10px;background:#f8fafc;border-left:3px solid #d97706"><strong>R&D 摘要</strong>：{}</div>"##,
                rd_summary.chars().take(300).collect::<String>()
            )
        } else {
            String::new()
        };
        let _ = num(&Value::Number(0.into()));

        format!(
            r##"<section id="moat">
  <h2>🏰 护城河四力（intangible / switching / network / scale）</h2>
  <div class="moat-grid" style="display:grid;grid-template-columns:1fr 1fr;gap:12px">
    {}
  </div>
  {rd_block}
</section>"##,
            items_html.concat()
        )
    }
}
