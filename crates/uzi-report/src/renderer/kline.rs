//! Port of `lib/pipeline/renderer/kline.py` — 2_kline section.

use super::base::{SectionRenderer, RenderContext};
use crate::pyfmt::disp;
use serde_json::Value;

pub struct KlineRenderer;

fn fmt_pct(v: Option<&Value>, default: &str) -> String {
    let v = match v {
        Some(v) if !v.is_null() && !matches!(v, Value::String(s) if s.is_empty()) => v,
        _ => return default.to_string(),
    };
    if let Value::String(s) = v {
        return s.clone();
    }
    match v.as_f64() {
        Some(x) => {
            let sign = if x > 0.0 { "+" } else { "" };
            format!("{sign}{x:.1}%")
        }
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

impl SectionRenderer for KlineRenderer {
    fn section_id(&self) -> &'static str {
        "kline"
    }

    fn section_title(&self) -> &'static str {
        "📈 K 线走势与技术指标"
    }

    fn render_full(&self, ctx: &RenderContext) -> String {
        let d = &ctx.data;
        let pc_1m = fmt_pct(d.get("price_change_1m"), "—");
        let pc_3m = fmt_pct(d.get("price_change_3m"), "—");
        let pc_6m = fmt_pct(d.get("price_change_6m"), "—");
        let pc_1y = fmt_pct(d.get("price_change_1y"), "—");
        let get = |k: &str| -> String {
            match d.get(k) {
                Some(v) if uzi_core::py::truthy(v) => disp(v),
                _ => "—".to_string(),
            }
        };
        let rsi = get("rsi");
        let ma5 = get("ma5");
        let ma20 = get("ma20");
        let ma60 = get("ma60");
        let vol_ratio = get("vol_ratio");
        let bb_status = get("bollinger_status");

        format!(
            r##"<section id="kline">
  <h2>📈 K 线走势与技术指标</h2>
  <div class="kline-grid" style="display:grid;grid-template-columns:repeat(4,1fr);gap:8px">
    <div><div class="label">近 1 月</div><div class="value">{pc_1m}</div></div>
    <div><div class="label">近 3 月</div><div class="value">{pc_3m}</div></div>
    <div><div class="label">近 6 月</div><div class="value">{pc_6m}</div></div>
    <div><div class="label">近 1 年</div><div class="value">{pc_1y}</div></div>
    <div><div class="label">RSI</div><div class="value">{rsi}</div></div>
    <div><div class="label">MA5/MA20/MA60</div><div class="value">{ma5}/{ma20}/{ma60}</div></div>
    <div><div class="label">量比</div><div class="value">{vol_ratio}</div></div>
    <div><div class="label">布林带</div><div class="value">{bb_status}</div></div>
  </div>
</section>"##
        )
    }
}
