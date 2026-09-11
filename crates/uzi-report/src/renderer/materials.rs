//! Port of `lib/pipeline/renderer/materials.py` — 8_materials section.

use super::base::{SectionRenderer, RenderContext};
use crate::pyfmt::disp;

pub struct MaterialsRenderer;

impl SectionRenderer for MaterialsRenderer {
    fn section_id(&self) -> &'static str {
        "materials"
    }

    fn section_title(&self) -> &'static str {
        "⚙️ 原材料与成本"
    }

    fn render_full(&self, ctx: &RenderContext) -> String {
        let d = &ctx.data;
        let get = |k: &str| -> String {
            match d.get(k) {
                Some(v) if uzi_core::py::truthy(v) => disp(v),
                _ => "—".to_string(),
            }
        };
        let core = get("core_material");
        let price_trend = get("price_trend");
        let cost_share = get("cost_share");
        let import_dep = get("import_dep");
        let details = d
            .get("materials_detail")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();

        if core == "—" && price_trend == "—" && details.is_empty() {
            return self.render_gap(ctx, "原材料数据不足");
        }

        let detail_html = if !details.is_empty() {
            let mut items: Vec<String> = Vec::new();
            for m in details.iter().take(5) {
                if !m.is_object() {
                    continue;
                }
                let name = match m.get("name") {
                    Some(v) if uzi_core::py::truthy(v) => disp(v),
                    _ => "—".to_string(),
                };
                let trend = match m.get("price_change") {
                    Some(v) if uzi_core::py::truthy(v) => disp(v),
                    _ => match m.get("trend") {
                        Some(v) if uzi_core::py::truthy(v) => disp(v),
                        _ => "—".to_string(),
                    },
                };
                items.push(format!(
                    r##"<li><strong>{name}</strong> · {trend}</li>"##
                ));
            }
            if items.is_empty() {
                String::new()
            } else {
                format!(
                    r##"<ul style="font-size:12px">{}</ul>"##,
                    items.concat()
                )
            }
        } else {
            String::new()
        };

        format!(
            r##"<section id="materials">
  <h2>⚙️ 原材料与成本</h2>
  <div style="font-size:13px">
    <div><strong>核心原材料</strong>：{core}</div>
    <div><strong>价格走势</strong>：{price_trend}</div>
    <div><strong>成本占比</strong>：{cost_share}</div>
    <div><strong>进口依赖度</strong>：{import_dep}</div>
  </div>
  {detail_html}
</section>"##
        )
    }
}
