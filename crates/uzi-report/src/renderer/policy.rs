//! Port of `lib/pipeline/renderer/policy.py` — 13_policy section.

use super::base::{SectionRenderer, RenderContext};
use crate::pyfmt::disp;

pub struct PolicyRenderer;

const SENTIMENT_EMOJI: &[(&str, &str)] = &[
    ("积极", "🟢"),
    ("中性", "🟡"),
    ("收紧", "🔴"),
    ("—", "⚪"),
];

impl SectionRenderer for PolicyRenderer {
    fn section_id(&self) -> &'static str {
        "policy"
    }

    fn section_title(&self) -> &'static str {
        "📜 政策与监管"
    }

    fn render_full(&self, ctx: &RenderContext) -> String {
        let d = &ctx.data;
        let items: [(&str, String); 4] = [
            (
                "政策方向",
                match d.get("policy_dir") {
                    Some(v) if uzi_core::py::truthy(v) => disp(v),
                    _ => "—".to_string(),
                },
            ),
            (
                "补贴",
                match d.get("subsidy") {
                    Some(v) if uzi_core::py::truthy(v) => disp(v),
                    _ => "—".to_string(),
                },
            ),
            (
                "监管",
                match d.get("monitoring") {
                    Some(v) if uzi_core::py::truthy(v) => disp(v),
                    _ => "—".to_string(),
                },
            ),
            (
                "反垄断",
                match d.get("anti_trust") {
                    Some(v) if uzi_core::py::truthy(v) => disp(v),
                    _ => "—".to_string(),
                },
            ),
        ];
        let mut html = String::new();
        for (label, val) in items {
            let emoji = SENTIMENT_EMOJI
                .iter()
                .find(|(k, _)| *k == val)
                .map(|(_, e)| (*e).to_string())
                .unwrap_or_else(|| "⚪".to_string());
            html.push_str(&format!(
                r##"<div><strong>{label}</strong>：{emoji} {val}</div>"##
            ));
        }
        let year = match d.get("year") {
            Some(v) if uzi_core::py::truthy(v) => disp(v),
            _ => String::new(),
        };
        let industry = {
            let a = match d.get("industry") {
                Some(v) if uzi_core::py::truthy(v) => disp(v),
                _ => String::new(),
            };
            if !a.is_empty() {
                a
            } else {
                match ctx.meta.get("industry") {
                    Some(v) if uzi_core::py::truthy(v) => disp(v),
                    _ => String::new(),
                }
            }
        };

        format!(
            r##"<section id="policy">
  <h2>📜 政策与监管 · {industry} {year}</h2>
  <div style="font-size:13px;display:grid;grid-template-columns:1fr 1fr;gap:6px">
    {html}
  </div>
</section>"##
        )
    }
}
