//! Port of `lib/pipeline/renderer/events.py` — 15_events section.

use super::base::{SectionRenderer, RenderContext};
use crate::pyfmt::disp;
use serde_json::Value;

pub struct EventsRenderer;

impl SectionRenderer for EventsRenderer {
    fn section_id(&self) -> &'static str {
        "events"
    }

    fn section_title(&self) -> &'static str {
        "📅 事件驱动 · 近期催化与风险"
    }

    fn render_full(&self, ctx: &RenderContext) -> String {
        let data = &ctx.data;
        let timeline = data
            .get("event_timeline")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        let catalysts = match data.get("catalyst") {
            Some(v) if uzi_core::py::truthy(v) => v.clone(),
            _ => data.get("catalysts").cloned().unwrap_or(Value::Null),
        };
        let catalysts = catalysts.as_array().cloned().unwrap_or_default();
        let warnings = data
            .get("warnings")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        let news = data
            .get("recent_news")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();

        if timeline.is_empty() && catalysts.is_empty() && warnings.is_empty() && news.is_empty() {
            return self.render_gap(ctx, "无事件数据");
        }

        let timeline_html = if !timeline.is_empty() {
            let items: String = timeline
                .iter()
                .take(8)
                .filter(|ev| ev.is_string())
                .map(|ev| format!("<li>{}</li>", disp(ev)))
                .collect();
            format!(
                r##"<div><h3>时间线</h3><ul style="font-size:12px">{items}</ul></div>"##
            )
        } else {
            String::new()
        };

        let catalyst_html = if !catalysts.is_empty() {
            let mut items: Vec<String> = Vec::new();
            for c in catalysts.iter().take(5) {
                if !c.is_object() {
                    continue;
                }
                let date = disp(c.get("date").unwrap_or(&Value::String("—".to_string())));
                let event = match c.get("event") {
                    Some(v) if uzi_core::py::truthy(v) => disp(v),
                    _ => disp(c.get("title").unwrap_or(&Value::String(String::new()))),
                };
                items.push(format!(r##"<li><strong>{date}</strong> · {event}</li>"##));
            }
            if items.is_empty() {
                String::new()
            } else {
                format!(
                    r##"<div><h3>🟢 催化剂</h3><ul style="font-size:12px">{}</ul></div>"##,
                    items.concat()
                )
            }
        } else {
            String::new()
        };

        let warning_html = if !warnings.is_empty() {
            let items: String = warnings
                .iter()
                .take(5)
                .filter(|w| w.is_string())
                .map(|w| format!(r##"<li style="color:#dc2626">{}</li>"##, disp(w)))
                .collect();
            format!(
                r##"<div><h3>🔴 警示</h3><ul style="font-size:12px">{items}</ul></div>"##
            )
        } else {
            String::new()
        };

        format!(
            r##"<section id="events">
  <h2>📅 事件驱动 · 近期催化与风险</h2>
  {catalyst_html}
  {warning_html}
  {timeline_html}
</section>"##
        )
    }
}
