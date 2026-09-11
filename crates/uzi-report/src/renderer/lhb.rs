//! Port of `lib/pipeline/renderer/lhb.py` — 16_lhb section.

use super::base::{SectionRenderer, RenderContext};
use crate::pyfmt::disp;
use serde_json::Value;

pub struct LhbRenderer;

fn or_empty(v: &Value, k: &str) -> Value {
    match v.get(k) {
        Some(x) if uzi_core::py::truthy(x) => x.clone(),
        _ => Value::Array(vec![]),
    }
}

impl SectionRenderer for LhbRenderer {
    fn section_id(&self) -> &'static str {
        "lhb"
    }

    fn section_title(&self) -> &'static str {
        "🐲 龙虎榜"
    }

    fn render_full(&self, ctx: &RenderContext) -> String {
        let d = &ctx.data;
        let count_30d = match d.get("lhb_count_30d") {
            Some(v) if uzi_core::py::truthy(v) => v.clone(),
            _ => Value::Number(0.into()),
        };
        let records = or_empty(d, "lhb_records");
        let matched_youzi = or_empty(d, "matched_youzi");
        let inst_vs_youzi = match d.get("inst_vs_youzi") {
            Some(v) if uzi_core::py::truthy(v) => v.clone(),
            _ => Value::Object(Default::default()),
        };

        let count_zero = crate::pyfmt::num(&count_30d) == 0.0;
        let records_empty = records.as_array().map(|a| a.is_empty()).unwrap_or(true);
        let matched_empty = matched_youzi.as_array().map(|a| a.is_empty()).unwrap_or(true);
        if count_zero && records_empty && matched_empty {
            return self.render_gap(ctx, "近 30 日未上龙虎榜");
        }

        let youzi_html = if !matched_empty {
            let names: Vec<String> = matched_youzi
                .as_array()
                .unwrap()
                .iter()
                .take(5)
                .map(|m| {
                    if m.is_string() {
                        disp(m)
                    } else {
                        disp(m.get("name").unwrap_or(&Value::String(String::new())))
                    }
                })
                .filter(|n| !n.is_empty())
                .collect();
            format!(r##"<div><strong>游资身影</strong>：{}</div>"##, names.join("、"))
        } else {
            String::new()
        };

        let inst_net = if inst_vs_youzi.is_object() {
            inst_vs_youzi.get("institutional_net").cloned().unwrap_or(Value::Null)
        } else {
            Value::Null
        };
        let youzi_buy = if inst_vs_youzi.is_object() {
            inst_vs_youzi.get("youzi_buy").cloned().unwrap_or(Value::Null)
        } else {
            Value::Null
        };
        let show = |v: &Value| -> String {
            if uzi_core::py::truthy(v) {
                disp(v)
            } else {
                "—".to_string()
            }
        };

        format!(
            r##"<section id="lhb">
  <h2>🐲 龙虎榜</h2>
  <div style="font-size:13px">
    <div>近 30 日上榜 · <strong>{}</strong> 次</div>
    {youzi_html}
    <div>机构净买入：<strong>{}</strong> · 游资净买入：<strong>{}</strong></div>
  </div>
</section>"##,
            disp(&count_30d),
            show(&inst_net),
            show(&youzi_buy)
        )
    }
}
