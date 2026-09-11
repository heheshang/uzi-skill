//! Port of `lib/pipeline/renderer/research.py` — 6_research section.

use super::base::{SectionRenderer, RenderContext};
use crate::pyfmt::disp;
use serde_json::Value;

pub struct ResearchRenderer;

fn first_truthy<'a>(d: &'a Value, keys: &[&str]) -> Option<&'a Value> {
    for k in keys {
        if let Some(v) = d.get(*k) {
            if uzi_core::py::truthy(v) {
                return Some(v);
            }
        }
    }
    None
}

impl SectionRenderer for ResearchRenderer {
    fn section_id(&self) -> &'static str {
        "research"
    }

    fn section_title(&self) -> &'static str {
        "📝 研究报告与评级"
    }

    fn render_full(&self, ctx: &RenderContext) -> String {
        let d = &ctx.data;
        let coverage = first_truthy(d, &["coverage", "coverage_count", "report_count"])
            .map(disp)
            .unwrap_or_else(|| "0".to_string());
        let buy_pct = first_truthy(d, &["buy_rating_pct"])
            .map(disp)
            .unwrap_or_else(|| "—".to_string());
        let tp_avg = first_truthy(d, &["target_price_avg", "target_avg"])
            .map(disp)
            .unwrap_or_else(|| "—".to_string());
        let consensus_eps = first_truthy(d, &["consensus_eps_2026"])
            .map(disp)
            .unwrap_or_else(|| "—".to_string());
        let brokers = d.get("brokers").and_then(|v| v.as_array()).cloned().unwrap_or_default();
        let recent = d
            .get("recent_reports")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();

        let coverage_truthy = first_truthy(d, &["coverage", "coverage_count", "report_count"]).is_some();
        if !coverage_truthy && brokers.is_empty() && recent.is_empty() {
            return self.render_gap(ctx, "无研报覆盖数据");
        }

        let recent_html = if !recent.is_empty() {
            let items: String = recent
                .iter()
                .take(5)
                .filter(|r| r.is_object())
                .map(|r| {
                    let broker = disp(r.get("broker").unwrap_or(&Value::String(String::new())));
                    let date = disp(r.get("date").unwrap_or(&Value::String(String::new())));
                    let title = match r.get("title") {
                        Some(v) if uzi_core::py::truthy(v) => disp(v),
                        _ => disp(r.get("rating").unwrap_or(&Value::String(String::new()))),
                    };
                    format!(r##"<li>{broker} · {date} · {title}</li>"##)
                })
                .collect();
            if items.is_empty() {
                String::new()
            } else {
                format!(
                    r##"<div><strong>近期研报</strong><ul style="font-size:12px">{items}</ul></div>"##
                )
            }
        } else {
            String::new()
        };

        format!(
            r##"<section id="research">
  <h2>📝 研究报告与评级</h2>
  <div class="research-grid" style="display:grid;grid-template-columns:repeat(4,1fr);gap:8px">
    <div><div class="label">覆盖券商</div><div class="value">{coverage}</div></div>
    <div><div class="label">买入占比</div><div class="value">{buy_pct}</div></div>
    <div><div class="label">目标价均值</div><div class="value">{tp_avg}</div></div>
    <div><div class="label">一致 EPS</div><div class="value">{consensus_eps}</div></div>
  </div>
  {recent_html}
</section>"##
        )
    }
}
