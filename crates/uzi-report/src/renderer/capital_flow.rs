//! Port of `lib/pipeline/renderer/capital_flow.py` — 12_capital_flow section.

use super::base::{SectionRenderer, RenderContext};
use crate::pyfmt::disp;
use serde_json::Value;

pub struct CapitalFlowRenderer;

fn arr(v: &Value, k: &str) -> Vec<Value> {
    match v.get(k) {
        Some(Value::Array(a)) => a.clone(),
        _ => Vec::new(),
    }
}

impl SectionRenderer for CapitalFlowRenderer {
    fn section_id(&self) -> &'static str {
        "capital_flow"
    }

    fn section_title(&self) -> &'static str {
        "💰 资金流向"
    }

    fn render_full(&self, ctx: &RenderContext) -> String {
        let d = &ctx.data;
        let main_20d = arr(d, "main_fund_flow_20d");
        let north = d.get("northbound").cloned().unwrap_or(Value::Null);
        let margin = arr(d, "margin_recent");
        let holder_hist = arr(d, "holder_count_history");
        let inst_hist = d.get("institutional_history").cloned().unwrap_or(Value::Null);
        let block_trades = arr(d, "block_trades_recent");
        let unlock = arr(d, "unlock_recent");

        let any = !main_20d.is_empty()
            || uzi_core::py::truthy(&north)
            || !margin.is_empty()
            || !holder_hist.is_empty()
            || uzi_core::py::truthy(&inst_hist)
            || !block_trades.is_empty();
        if !any {
            return self.render_gap(ctx, "资金流向数据不足");
        }

        let mut sections: Vec<String> = Vec::new();
        if !main_20d.is_empty() {
            sections.push(format!(
                r##"<div>主力资金 20 日流向 · <strong>{}</strong> 条记录</div>"##,
                main_20d.len()
            ));
        }
        if uzi_core::py::truthy(&north) {
            let total = match north.get("net_20d") {
                Some(v) if uzi_core::py::truthy(v) => disp(v),
                _ => match north.get("total") {
                    Some(v) if uzi_core::py::truthy(v) => disp(v),
                    _ => "—".to_string(),
                },
            };
            sections.push(format!(r##"<div>北向 · {total}</div>"##));
        }
        if !margin.is_empty() {
            sections.push(format!(
                r##"<div>融资 · <strong>{}</strong> 条记录</div>"##,
                margin.len()
            ));
        }
        if !holder_hist.is_empty() {
            sections.push(format!(
                r##"<div>股东户数历史 · <strong>{}</strong> 期</div>"##,
                holder_hist.len()
            ));
        }
        if !block_trades.is_empty() {
            sections.push(format!(
                r##"<div>近期大宗交易 · <strong>{}</strong> 笔</div>"##,
                block_trades.len()
            ));
        }
        if !unlock.is_empty() {
            sections.push(format!(
                r##"<div>解禁日历 · <strong>{}</strong> 条</div>"##,
                unlock.len()
            ));
        }

        format!(
            r##"<section id="capital_flow">
  <h2>💰 资金流向</h2>
  <div style="font-size:13px;display:grid;grid-template-columns:1fr 1fr;gap:6px">
    {}
  </div>
</section>"##,
            sections.concat()
        )
    }
}
