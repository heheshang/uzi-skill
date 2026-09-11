//! Port of `lib/pipeline/renderer/peers.py` — 4_peers section.

use super::base::{SectionRenderer, RenderContext};
use crate::global_peers::render_global_peer_comparison;
use crate::security::html_escape;
use crate::pyfmt::disp;
use serde_json::Value;

pub struct PeersRenderer;

fn esc(v: &Value) -> String {
    if v.is_null() || matches!(v, Value::String(s) if s.is_empty()) {
        return "—".to_string();
    }
    html_escape(&disp(v))
}

impl SectionRenderer for PeersRenderer {
    fn section_id(&self) -> &'static str {
        "peers"
    }

    fn section_title(&self) -> &'static str {
        "🏭 同行对比"
    }

    fn render_full(&self, ctx: &RenderContext) -> String {
        let data = &ctx.data;
        let peer_table = match data.get("peer_table") {
            Some(v) if uzi_core::py::truthy(v) => v.clone(),
            _ => Value::Array(vec![]),
        };
        let peer_comparison = match data.get("peer_comparison") {
            Some(v) if uzi_core::py::truthy(v) => v.clone(),
            _ => Value::Array(vec![]),
        };
        let global_html = render_global_peer_comparison(
            data.get("global_peer_comparison").unwrap_or(&Value::Null),
        );

        let table_empty = peer_table.as_array().map(|a| a.is_empty()).unwrap_or(true);
        let comp_empty = peer_comparison.as_array().map(|a| a.is_empty()).unwrap_or(true);
        if table_empty && comp_empty && global_html.is_empty() {
            return self.render_gap(ctx, "同行抓取失败（push2/xueqiu 反爬）");
        }

        let rows_source = if !comp_empty {
            &peer_comparison
        } else {
            &peer_table
        };
        let mut rows: Vec<String> = Vec::new();
        for p in rows_source.as_array().unwrap().iter().take(12) {
            if !p.is_object() {
                continue;
            }
            let name = match p.get("name") {
                Some(v) if uzi_core::py::truthy(v) => esc(v),
                _ => esc(p.get("code").unwrap_or(&Value::Null)),
            };
            let mcap = match p.get("market_cap") {
                Some(v) if uzi_core::py::truthy(v) => esc(v),
                _ => esc(p.get("mcap").unwrap_or(&Value::Null)),
            };
            let pe = match p.get("pe_ttm") {
                Some(v) if uzi_core::py::truthy(v) => esc(v),
                _ => esc(p.get("pe").unwrap_or(&Value::Null)),
            };
            rows.push(format!(
                r##"<tr>
  <td>{name}</td>
  <td>{mcap}</td>
  <td>{pe}</td>
  <td>{}</td>
  <td>{}</td>
</tr>"##,
                esc(p.get("pb").unwrap_or(&Value::Null)),
                esc(p.get("roe").unwrap_or(&Value::Null))
            ));
        }

        if rows.is_empty() && global_html.is_empty() {
            return self.render_gap(ctx, "同行数据为空");
        }

        let rows_html = if !rows.is_empty() {
            format!(
                r##"<table class="peers-table" style="width:100%;border-collapse:collapse"><thead><tr><th>公司</th><th>市值</th><th>PE(TTM)</th><th>PB</th><th>ROE</th></tr></thead><tbody>{}</tbody></table>"##,
                rows.concat()
            )
        } else {
            String::new()
        };

        format!(
            r##"<section id="peers">
  <h2>🏭 同行对比</h2>
  {rows_html}
  {global_html}
</section>"##
        )
    }
}
