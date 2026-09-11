//! Port of `lib/pipeline/renderer/valuation.py` — 10_valuation section.

use super::base::{SectionRenderer, RenderContext};
use crate::pyfmt::disp;

pub struct ValuationRenderer;

impl SectionRenderer for ValuationRenderer {
    fn section_id(&self) -> &'static str {
        "valuation"
    }

    fn section_title(&self) -> &'static str {
        "💹 估值水平"
    }

    fn render_full(&self, ctx: &RenderContext) -> String {
        let d = &ctx.data;
        let get = |k: &str| -> String {
            match d.get(k) {
                Some(v) if uzi_core::py::truthy(v) => disp(v),
                _ => "—".to_string(),
            }
        };
        let pe_ttm = get("pe_ttm");
        let pb = get("pb");
        let ps = {
            let a = get("ps_ttm");
            if a != "—" {
                a
            } else {
                get("ps")
            }
        };
        let pe_pct = get("pe_percentile");
        let pb_pct = get("pb_percentile");
        let div_yield = {
            let a = get("dividend_yield");
            if a != "—" {
                a
            } else {
                get("dividend_yield_ttm")
            }
        };

        format!(
            r##"<section id="valuation">
  <h2>💹 估值水平</h2>
  <div class="valuation-grid" style="display:grid;grid-template-columns:repeat(3,1fr);gap:8px">
    <div><div class="label">PE(TTM)</div><div class="value"><strong>{pe_ttm}</strong></div></div>
    <div><div class="label">PB</div><div class="value"><strong>{pb}</strong></div></div>
    <div><div class="label">PS(TTM)</div><div class="value">{ps}</div></div>
    <div><div class="label">PE 历史分位</div><div class="value">{pe_pct}</div></div>
    <div><div class="label">PB 历史分位</div><div class="value">{pb_pct}</div></div>
    <div><div class="label">股息率</div><div class="value">{div_yield}</div></div>
  </div>
</section>"##
        )
    }
}
