//! Port of `lib/pipeline/renderer/chain.py` — 5_chain section.

use super::base::{SectionRenderer, RenderContext};
use crate::pyfmt::disp;
use serde_json::Value;

pub struct ChainRenderer;

fn list_or_dash(items: Option<&Value>, limit: usize) -> String {
    let items = match items {
        Some(v) if uzi_core::py::truthy(v) => v,
        _ => return "—".to_string(),
    };
    if let Value::String(s) = items {
        return s.chars().take(200).collect();
    }
    if let Value::Array(a) = items {
        let vals: Vec<String> = a
            .iter()
            .take(limit)
            .filter(|x| uzi_core::py::truthy(x))
            .map(disp)
            .collect();
        return if vals.is_empty() {
            "—".to_string()
        } else {
            vals.join("、")
        };
    }
    disp(items).chars().take(200).collect()
}

impl SectionRenderer for ChainRenderer {
    fn section_id(&self) -> &'static str {
        "chain"
    }

    fn section_title(&self) -> &'static str {
        "🔗 产业链上下游"
    }

    fn render_full(&self, ctx: &RenderContext) -> String {
        let d = &ctx.data;
        let upstream = d.get("upstream");
        let downstream = d.get("downstream");
        let client_conc = match d.get("client_concentration") {
            Some(v) if uzi_core::py::truthy(v) => disp(v),
            _ => "—".to_string(),
        };
        let supplier_conc = match d.get("supplier_concentration") {
            Some(v) if uzi_core::py::truthy(v) => disp(v),
            _ => "—".to_string(),
        };
        let main_biz = d
            .get("main_business_breakdown")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        let products = d.get("products");

        let any = upstream.map(uzi_core::py::truthy).unwrap_or(false)
            || downstream.map(uzi_core::py::truthy).unwrap_or(false)
            || !main_biz.is_empty()
            || products.map(uzi_core::py::truthy).unwrap_or(false);
        if !any {
            return self.render_gap(ctx, "产业链数据不足");
        }

        format!(
            r##"<section id="chain">
  <h2>🔗 产业链上下游</h2>
  <div class="chain-table" style="display:grid;grid-template-columns:auto 1fr;gap:6px;font-size:13px">
    <strong>上游</strong><span>{}</span>
    <strong>下游</strong><span>{}</span>
    <strong>主要产品</strong><span>{}</span>
    <strong>客户集中度</strong><span>{client_conc}</span>
    <strong>供应商集中度</strong><span>{supplier_conc}</span>
  </div>
</section>"##,
            list_or_dash(upstream, 5),
            list_or_dash(downstream, 5),
            list_or_dash(products, 5)
        )
    }
}
