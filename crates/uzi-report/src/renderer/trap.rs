//! Port of `lib/pipeline/renderer/trap.py` — 18_trap section.

use super::base::{SectionRenderer, RenderContext};
use crate::pyfmt::{disp, num};
use serde_json::Value;

pub struct TrapRenderer;

impl SectionRenderer for TrapRenderer {
    fn section_id(&self) -> &'static str {
        "trap"
    }

    fn section_title(&self) -> &'static str {
        "⚠️ 杀猪盘排查"
    }

    fn render_full(&self, ctx: &RenderContext) -> String {
        let d = &ctx.data;
        let risk_score = match d.get("risk_score") {
            Some(v) if uzi_core::py::truthy(v) => v.clone(),
            _ => Value::Number(0.into()),
        };
        let pump_signals = d
            .get("pump_dump_signals")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        let warning_flags = d
            .get("warning_flags")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        let trap_likelihood = match d.get("trap_likelihood") {
            Some(v) if uzi_core::py::truthy(v) => disp(v),
            _ => "—".to_string(),
        };

        let rs = num(&risk_score);
        let (color, level) = if risk_score.is_number() && rs > 60.0 {
            ("#dc2626", "高风险")
        } else if risk_score.is_number() && rs > 30.0 {
            ("#d97706", "中风险")
        } else {
            ("#16a34a", "低风险")
        };

        let flags_html = if !warning_flags.is_empty() {
            let items: String = warning_flags
                .iter()
                .take(8)
                .filter(|f| f.is_string())
                .map(|f| format!(r##"<li style="color:#dc2626">{}</li>"##, disp(f)))
                .collect();
            if items.is_empty() {
                String::new()
            } else {
                format!(
                    r##"<div><strong>警示标记</strong><ul style="font-size:12px">{items}</ul></div>"##
                )
            }
        } else {
            String::new()
        };

        let signals_html = if !pump_signals.is_empty() {
            let items: String = pump_signals
                .iter()
                .take(5)
                .filter(|s| s.is_string())
                .map(|s| format!("<li>{}</li>", disp(s)))
                .collect();
            if items.is_empty() {
                String::new()
            } else {
                format!(
                    r##"<div><strong>拉升信号</strong><ul style="font-size:12px">{items}</ul></div>"##
                )
            }
        } else {
            String::new()
        };

        format!(
            r##"<section id="trap">
  <h2>⚠️ 杀猪盘排查</h2>
  <div style="font-size:13px">
    <div>风险分 · <span style="color:{color};font-weight:700">{risk}/100</span> · {level}</div>
    <div>杀猪盘可能性 · {trap_likelihood}</div>
    {signals_html}
    {flags_html}
  </div>
</section>"##,
            risk = disp(&risk_score)
        )
    }
}
