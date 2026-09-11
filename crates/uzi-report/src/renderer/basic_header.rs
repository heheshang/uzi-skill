//! Port of `lib/pipeline/renderer/basic_header.py` — 0_basic section.

use super::base::SectionRenderer;
use super::base::RenderContext;
use crate::pyfmt::disp;
use serde_json::Value;

pub struct BasicHeaderRenderer;

fn field(data: &Value, key: &str) -> String {
    match data.get(key) {
        Some(v) if uzi_core::py::truthy(v) => disp(v),
        _ => "—".to_string(),
    }
}

impl SectionRenderer for BasicHeaderRenderer {
    fn section_id(&self) -> &'static str {
        "basic_header"
    }

    fn render_full(&self, ctx: &RenderContext) -> String {
        let data = &ctx.data;
        let name = {
            let n = field(data, "name");
            if n != "—" {
                n
            } else if !ctx.name.is_empty() {
                ctx.name.clone()
            } else {
                "—".to_string()
            }
        };
        let full_name = field(data, "full_name");
        let industry = field(data, "industry");
        let price = field(data, "price");
        let market_cap = field(data, "market_cap");
        let pe_ttm = field(data, "pe_ttm");
        let pe_static = field(data, "pe_static");
        let pb = field(data, "pb");
        let eps = field(data, "eps");
        let listed = field(data, "listed_date");
        let actual = field(data, "actual_controller");
        let main_business = field(data, "main_business");
        let actual30: String = actual.chars().take(30).collect();
        let mb200: String = main_business.chars().take(200).collect();

        format!(
            r##"<section id="basic_header">
  <div class="basic-header">
    <h1>{name} <small style="color:#64748b;font-size:14px">{ticker}</small></h1>
    <div style="color:#475569;font-size:12px">{full_name} · {industry}</div>
    <div class="basic-grid" style="display:grid;grid-template-columns:repeat(4,1fr);gap:8px;margin-top:12px">
      <div><div class="label">现价</div><div class="value"><strong>¥{price}</strong></div></div>
      <div><div class="label">市值</div><div class="value">{market_cap}</div></div>
      <div><div class="label">PE(TTM)</div><div class="value">{pe_ttm}</div></div>
      <div><div class="label">PB</div><div class="value">{pb}</div></div>
      <div><div class="label">EPS</div><div class="value">{eps}</div></div>
      <div><div class="label">PE(静)</div><div class="value">{pe_static}</div></div>
      <div><div class="label">上市</div><div class="value">{listed}</div></div>
      <div><div class="label">实控人</div><div class="value" style="font-size:11px">{actual30}</div></div>
    </div>
    <div style="margin-top:8px;font-size:12px;color:#475569">
      <strong>主营</strong>：{mb200}
    </div>
  </div>
</section>"##,
            ticker = ctx.ticker
        )
    }
}
