//! Port of `lib/pipeline/renderer/sentiment.py` — 17_sentiment section.

use super::base::{SectionRenderer, RenderContext};
use crate::pyfmt::{disp, num};
use serde_json::Value;

pub struct SentimentRenderer;

impl SectionRenderer for SentimentRenderer {
    fn section_id(&self) -> &'static str {
        "sentiment"
    }

    fn section_title(&self) -> &'static str {
        "🗣️ 舆情温度"
    }

    fn render_full(&self, ctx: &RenderContext) -> String {
        let data = &ctx.data;
        let heat_raw = match data.get("thermometer_value") {
            Some(v) if uzi_core::py::truthy(v) => v.clone(),
            _ => Value::Number(0.into()),
        };
        let positive_pct = match data.get("positive_pct") {
            Some(v) if uzi_core::py::truthy(v) => disp(v),
            _ => "—".to_string(),
        };
        let label = match data.get("sentiment_label") {
            Some(v) if uzi_core::py::truthy(v) => disp(v),
            _ => "—".to_string(),
        };
        let big_v = match data.get("big_v_mentions") {
            Some(v) if uzi_core::py::truthy(v) => disp(v),
            _ => "—".to_string(),
        };
        let hot_hits = match data.get("hot_trend_hit_count") {
            Some(v) if uzi_core::py::truthy(v) => disp(v),
            _ => "0".to_string(),
        };
        let news_ok = match data.get("news_sources_ok") {
            Some(v) if uzi_core::py::truthy(v) => disp(v),
            _ => "0".to_string(),
        };
        let news_hits = match data.get("news_total_hits") {
            Some(v) if uzi_core::py::truthy(v) => disp(v),
            _ => "0".to_string(),
        };

        let bar_width = if heat_raw.is_number() {
            (num(&heat_raw) as i64).clamp(0, 100)
        } else {
            0
        };
        let bar_color = if bar_width > 80 {
            "#dc2626"
        } else if bar_width > 50 {
            "#d97706"
        } else {
            "#16a34a"
        };

        format!(
            r##"<section id="sentiment">
  <h2>🗣️ 舆情温度</h2>
  <div class="sentiment-bar" style="margin:8px 0">
    <div style="font-size:12px;color:#64748b">热度 {bar_width}/100 · {label}</div>
    <div style="background:#f4f7fa;border-radius:4px;overflow:hidden;margin-top:4px">
      <div style="background:{bar_color};height:8px;width:{bar_width}%"></div>
    </div>
  </div>
  <div class="sentiment-grid" style="display:grid;grid-template-columns:repeat(2,1fr);gap:8px;margin-top:8px;font-size:12px">
    <div>正面占比 · <strong>{positive_pct}</strong></div>
    <div>大V 提及 · <strong>{big_v}</strong></div>
    <div>热榜命中 · <strong>{hot_hits}</strong></div>
    <div>新闻源 · <strong>{news_ok}/4 · {news_hits} 条</strong></div>
  </div>
</section>"##
        )
    }
}
