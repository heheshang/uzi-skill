//! Port of `lib/pipeline/renderer/futures.py` — 9_futures section.

use super::base::{SectionRenderer, RenderContext};
use crate::pyfmt::disp;

pub struct FuturesRenderer;

impl SectionRenderer for FuturesRenderer {
    fn section_id(&self) -> &'static str {
        "futures"
    }

    fn section_title(&self) -> &'static str {
        "📦 期货联动"
    }

    fn render_full(&self, ctx: &RenderContext) -> String {
        let d = &ctx.data;
        let contract = match d.get("linked_contract") {
            Some(v) if uzi_core::py::truthy(v) => disp(v),
            _ => "—".to_string(),
        };
        let price_trend = match d.get("price_trend") {
            Some(v) if uzi_core::py::truthy(v) => disp(v),
            _ => "—".to_string(),
        };
        let inventory = match d.get("inventory") {
            Some(v) if uzi_core::py::truthy(v) => disp(v),
            _ => "—".to_string(),
        };

        if contract.contains("无直接") || contract == "—" {
            return self.render_gap(ctx, "非大宗商品相关行业");
        }

        format!(
            r##"<section id="futures">
  <h2>📦 期货联动</h2>
  <div style="font-size:13px">
    <div><strong>关联合约</strong>：{contract}</div>
    <div><strong>价格走势</strong>：{price_trend}</div>
    <div><strong>库存</strong>：{inventory}</div>
  </div>
</section>"##
        )
    }
}
