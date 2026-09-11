//! Port of `lib/pipeline/renderer/governance.py` — 11_governance section.

use super::base::{SectionRenderer, RenderContext};
use crate::pyfmt::disp;
use serde_json::Value;

pub struct GovernanceRenderer;

fn val(d: &Value, key: &str) -> String {
    match d.get(key) {
        Some(v) if uzi_core::py::truthy(v) => disp(v),
        _ => "—".to_string(),
    }
}

impl SectionRenderer for GovernanceRenderer {
    fn section_id(&self) -> &'static str {
        "governance"
    }

    fn section_title(&self) -> &'static str {
        "⚖️ 公司治理"
    }

    fn render_full(&self, ctx: &RenderContext) -> String {
        let d = &ctx.data;
        let pledge = val(d, "pledge");
        let insider_1y = val(d, "insider_trades_1y");
        let chairman_turnover = val(d, "chairman_turnover");

        if pledge == "—" && insider_1y == "—" && chairman_turnover == "—" {
            return self.render_gap(ctx, "治理数据不足");
        }

        format!(
            r##"<section id="governance">
  <h2>⚖️ 公司治理</h2>
  <div style="font-size:13px">
    <div><strong>股权质押</strong>：{pledge}</div>
    <div><strong>近 1 年内部人交易</strong>：{insider_1y}</div>
    <div><strong>董事长变更</strong>：{chairman_turnover}</div>
  </div>
</section>"##
        )
    }
}
