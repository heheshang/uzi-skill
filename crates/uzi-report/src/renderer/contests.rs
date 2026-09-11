//! Port of `lib/pipeline/renderer/contests.py` — 19_contests section.

use super::base::{SectionRenderer, RenderContext};
use crate::pyfmt::disp;
use serde_json::Value;

pub struct ContestsRenderer;

fn len_of(v: Option<&Value>) -> usize {
    match v {
        Some(Value::Array(a)) => a.len(),
        _ => 0,
    }
}

impl SectionRenderer for ContestsRenderer {
    fn section_id(&self) -> &'static str {
        "contests"
    }

    fn section_title(&self) -> &'static str {
        "🏆 实盘大赛 / 大V 持仓"
    }

    fn render_full(&self, ctx: &RenderContext) -> String {
        let d = &ctx.data;
        let xq_cubes = d.get("xueqiu_cubes").and_then(|v| v.as_array()).cloned().unwrap_or_default();
        let tgb = d.get("tgb_mentions").cloned().unwrap_or(Value::Number(0.into()));
        let ths_simu = d.get("ths_simu");
        let dpswang = d.get("dpswang");
        let summary = d.get("summary").map(disp).unwrap_or_default();

        let tgb_truthy = uzi_core::py::truthy(&tgb);
        let any = !xq_cubes.is_empty()
            || tgb_truthy
            || ths_simu.map(uzi_core::py::truthy).unwrap_or(false)
            || dpswang.map(uzi_core::py::truthy).unwrap_or(false);
        if !any && summary.is_empty() {
            return self.render_gap(ctx, "实盘 / 大V 数据不足");
        }

        let xq_html = if !xq_cubes.is_empty() {
            format!(
                r##"<div><strong>雪球组合</strong> · 检到 <strong>{}</strong> 个</div>"##,
                xq_cubes.len()
            )
        } else {
            String::new()
        };
        let summary_html = if !summary.is_empty() {
            format!(
                r##"<div style="margin-top:6px;color:#475569">{}</div>"##,
                summary.chars().take(200).collect::<String>()
            )
        } else {
            String::new()
        };

        format!(
            r##"<section id="contests">
  <h2>🏆 实盘大赛 / 大V 持仓</h2>
  <div style="font-size:13px">
    {xq_html}
    <div>淘股吧提及 · <strong>{tgb}</strong> 次</div>
    <div>同花顺实盘 · <strong>{ths}</strong> 个</div>
    <div>大盘视角王 · <strong>{dps}</strong> 条</div>
    {summary_html}
  </div>
</section>"##,
            tgb = disp(&tgb),
            ths = len_of(ths_simu),
            dps = len_of(dpswang)
        )
    }
}
