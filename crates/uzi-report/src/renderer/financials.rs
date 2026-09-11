//! Port of `lib/pipeline/renderer/financials.py` — 1_financials section.

use super::base::{SectionRenderer, RenderContext};
use crate::pyfmt::disp;
use serde_json::Value;

pub struct FinancialsRenderer;

fn fmt_pct(v: Option<&Value>, default: &str) -> String {
    let v = match v {
        Some(v) if !v.is_null() && !matches!(v, Value::String(s) if s.is_empty()) => v,
        _ => return default.to_string(),
    };
    if let Value::String(s) = v {
        return s.clone();
    }
    match v.as_f64() {
        Some(x) => format!("{x:.1}%"),
        None => {
            let s = disp(v);
            if s.is_empty() {
                default.to_string()
            } else {
                s
            }
        }
    }
}

fn coerce(v: Option<&Value>) -> Option<&Value> {
    match v {
        Some(v) if uzi_core::py::truthy(v) => Some(v),
        _ => None,
    }
}

impl SectionRenderer for FinancialsRenderer {
    fn section_id(&self) -> &'static str {
        "financials"
    }

    fn section_title(&self) -> &'static str {
        "📊 财务指标"
    }

    fn render_full(&self, ctx: &RenderContext) -> String {
        let data = &ctx.data;
        let roe = fmt_pct(coerce(data.get("roe")).or_else(|| coerce(data.get("roe_ttm"))), "—");
        let net_margin = fmt_pct(coerce(data.get("net_margin")), "—");
        let gross_margin = fmt_pct(coerce(data.get("gross_margin")), "—");
        let rev_growth = fmt_pct(
            coerce(data.get("revenue_growth")).or_else(|| coerce(data.get("revenue_growth_yoy"))),
            "—",
        );
        let debt_ratio = fmt_pct(
            coerce(data.get("debt_ratio")).or_else(|| coerce(data.get("asset_liability_ratio"))),
            "—",
        );
        let current_ratio = match coerce(data.get("current_ratio")) {
            Some(v) => disp(v),
            None => "—".to_string(),
        };

        format!(
            r##"<section id="financials">
  <h2>📊 财务指标</h2>
  <div class="fin-grid" style="display:grid;grid-template-columns:repeat(3,1fr);gap:8px">
    <div class="fin-metric"><div class="label">ROE</div><div class="value"><strong>{roe}</strong></div></div>
    <div class="fin-metric"><div class="label">净利率</div><div class="value"><strong>{net_margin}</strong></div></div>
    <div class="fin-metric"><div class="label">毛利率</div><div class="value">{gross_margin}</div></div>
    <div class="fin-metric"><div class="label">营收增速</div><div class="value">{rev_growth}</div></div>
    <div class="fin-metric"><div class="label">资产负债率</div><div class="value">{debt_ratio}</div></div>
    <div class="fin-metric"><div class="label">流动比率</div><div class="value">{current_ratio}</div></div>
  </div>
</section>"##
        )
    }
}
