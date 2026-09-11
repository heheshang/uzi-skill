//! Port of `lib/pipeline/renderer/industry.py` — 7_industry section.

use super::base::{SectionRenderer, RenderContext};
use crate::pyfmt::{disp, num};
use serde_json::Value;

pub struct IndustryRenderer;

fn or_dash(d: &Value, key: &str) -> String {
    match d.get(key) {
        Some(v) if uzi_core::py::truthy(v) => disp(v),
        _ => "—".to_string(),
    }
}

impl SectionRenderer for IndustryRenderer {
    fn section_id(&self) -> &'static str {
        "industry"
    }

    fn section_title(&self) -> &'static str {
        "🏢 行业景气"
    }

    fn render_full(&self, ctx: &RenderContext) -> String {
        let data = &ctx.data;
        let industry = {
            let v = or_dash(data, "industry");
            if v != "—" {
                v
            } else {
                let m = or_dash(&ctx.meta, "industry");
                if m != "—" {
                    m
                } else {
                    "—".to_string()
                }
            }
        };
        let growth = or_dash(data, "growth");
        let tam = or_dash(data, "tam");
        let penetration = or_dash(data, "penetration");
        let ind_pe = or_dash(data, "industry_pe");
        let ind_pb = or_dash(data, "industry_pb");

        let cninfo = data.get("cninfo_metrics").cloned().unwrap_or(Value::Null);
        let pe_weighted = match cninfo.get("industry_pe_weighted") {
            Some(v) if uzi_core::py::truthy(v) => disp(v),
            _ => "—".to_string(),
        };
        let total_mcap = cninfo.get("total_mcap_yi").filter(|v| uzi_core::py::truthy(v)).map(num);
        let company_count = match cninfo.get("company_count") {
            Some(v) if uzi_core::py::truthy(v) => disp(v),
            _ => "—".to_string(),
        };
        let mcap_display = match total_mcap {
            Some(v) => format!("{v:.0} 亿"),
            None => "—".to_string(),
        };

        format!(
            r##"<section id="industry">
  <h2>🏢 行业景气 · {industry}</h2>
  <div class="industry-grid" style="display:grid;grid-template-columns:repeat(3,1fr);gap:8px">
    <div class="industry-metric"><div class="label">行业增速</div><div class="value">{growth}</div></div>
    <div class="industry-metric"><div class="label">TAM 市场空间</div><div class="value">{tam}</div></div>
    <div class="industry-metric"><div class="label">渗透率</div><div class="value">{penetration}</div></div>
    <div class="industry-metric"><div class="label">行业 PE</div><div class="value">{ind_pe}</div></div>
    <div class="industry-metric"><div class="label">行业 PB</div><div class="value">{ind_pb}</div></div>
    <div class="industry-metric"><div class="label">PE 加权（cninfo）</div><div class="value">{pe_weighted}</div></div>
  </div>
  <div style="margin-top:8px;color:#64748b;font-size:12px">
    行业公司数：{company_count} · 总市值：{mcap_display}
  </div>
</section>"##
        )
    }
}
