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
        // `tgb_mentions` is the raw array (may contain `{"error": ...}` rows from a
        // failed crawl). The count display must use the data layer's already-filtered
        // `tgb_mentions_count` — dumping `disp(array)` into "N 次" leaked the error
        // strings into the report (§5.2) and made `truthy` treat an error-only array
        // as "有数据".
        let tgb = d.get("tgb_mentions_count").cloned().unwrap_or(Value::Number(0.into()));
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // 回归守卫 §5.2：错误串不得再泄漏进用户可见报告。
    // 旧的实现直接 `disp(&tgb_mentions)`——一个只含 `{"error": ...}` 的非空数组
    // 会被 `truthy` 判为"有数据"，且被 `disp` 整段 dump 进"淘股吧提及 · N 次"。
    #[test]
    fn error_only_tgb_renders_gap_not_data() {
        let data = json!({
            "tgb_mentions": [{"error": "tgb fetch failed: invalid peer certificate"}],
            "tgb_mentions_count": 0, // 数据层已正确过滤错误行 → 0
            "xueqiu_cubes": [],
            "ths_simu": [],
            "dpswang": [],
            "summary": {},
        });
        let ctx = RenderContext::new("002273.SZ", "水晶光电").with_data(data);
        let html = ContestsRenderer.render_full(&ctx);
        // 错误串不得出现在报告里
        assert!(!html.contains("invalid peer certificate"), "错误串泄漏进报告");
        assert!(!html.contains("['{"), "数组被 dump 进计数字段");
        // 过滤后计数为 0 → "淘股吧提及 · 0 次"，绝不能再渲染数组的 JSON 表示
        assert!(
            html.contains("淘股吧提及 · <strong>0</strong> 次"),
            "error-only 时计数应显示 0（数字），而不是数组 repr: {html}"
        );
    }

    // 有效计数应显示为数字，而不是数组的 JSON 表示。
    #[test]
    fn valid_tgb_count_displays_number() {
        let data = json!({
            "tgb_mentions": [{"title": "a"}, {"title": "b"}],
            "tgb_mentions_count": 2,
            "xueqiu_cubes": [{"name": "x"}],
            "ths_simu": [],
            "dpswang": [],
            "summary": {},
        });
        let ctx = RenderContext::new("002273.SZ", "水晶光电").with_data(data);
        let html = ContestsRenderer.render_full(&ctx);
        assert!(html.contains("淘股吧提及 · <strong>2</strong> 次"), "计数字段显示数字: {html}");
        assert!(!html.contains("淘股吧提及 · <strong>[{"), "不应 dump 数组");
    }
}
