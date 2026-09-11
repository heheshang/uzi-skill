//! Port of `lib/pipeline/renderer/macro.py` — 3_macro section.

use super::base::{SectionRenderer, RenderContext};
use crate::pyfmt::disp;
use serde_json::Value;

pub struct MacroRenderer;

impl SectionRenderer for MacroRenderer {
    fn section_id(&self) -> &'static str {
        "macro"
    }

    fn section_title(&self) -> &'static str {
        "🌏 宏观环境"
    }

    fn render_full(&self, ctx: &RenderContext) -> String {
        let d = &ctx.data;
        let rows: [(&str, Option<&Value>); 5] = [
            ("利率周期", d.get("rate_cycle")),
            ("汇率走势", d.get("fx_trend")),
            ("地缘风险", d.get("geo_risk")),
            ("大宗商品", d.get("commodity")),
            ("成长动能", d.get("growth_momentum")),
        ];
        let items: String = rows
            .iter()
            .filter(|(_, v)| v.map(uzi_core::py::truthy).unwrap_or(false))
            .map(|(label, v)| {
                format!(
                    r##"<div class="macro-item"><strong>{label}</strong>：{}</div>"##,
                    disp(v.unwrap())
                )
            })
            .collect();
        if items.is_empty() {
            return self.render_gap(ctx, "宏观数据不足");
        }
        format!(
            r##"<section id="macro"><h2>🌏 宏观环境</h2><div class="macro-grid">{items}</div></section>"##
        )
    }
}
