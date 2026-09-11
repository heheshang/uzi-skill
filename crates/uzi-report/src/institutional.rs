//! Port of `lib/report/institutional.py` — institutional modeling blocks
//! (DCF / Comps / LBO / initiating coverage / IC memo / catalyst calendar /
//! competitive analysis) plus data-gap and school-lock banners.

use crate::pyfmt::{disp, pyf, signed};
use crate::security::{escape_payload, escape_text};
use crate::svg::{svg_radar, svg_sparkline};
use serde_json::Value;

fn safe(v: &Value, default: &str) -> String {
    if v.is_null() {
        return default.to_string();
    }
    if let Value::String(s) = v {
        if s.is_empty() || s == "—" {
            return default.to_string();
        }
    }
    disp(v)
}

/// Python `float(v)` with a fallback.
fn number(v: &Value, default: Option<f64>) -> Option<f64> {
    let parsed = match v {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.trim().parse::<f64>().ok(),
        Value::Bool(b) => Some(if *b { 1.0 } else { 0.0 }),
        _ => None,
    };
    parsed.or(default)
}

fn num0(v: &Value) -> f64 {
    number(v, Some(0.0)).unwrap_or(0.0)
}

fn numeric_series(values: &Value) -> Vec<f64> {
    match values.as_array() {
        Some(a) => a
            .iter()
            .filter_map(|v| number(v, None))
            .collect(),
        None => Vec::new(),
    }
}

/// Risk level → (color key, emoji).
pub fn trap_color_emoji(level: &str) -> (&'static str, &'static str) {
    if level.contains('🟢') || level.contains("安全") {
        return ("green", "🟢");
    }
    if level.contains('🟡') || level.contains("注意") {
        return ("yellow", "🟡");
    }
    if level.contains('🟠') || level.contains("警惕") {
        return ("orange", "🟠");
    }
    ("red", "🔴")
}

const MISSING_BLOCK: &str = r##"<div class="dcf-block"><p class="muted">DCF 数据缺失</p></div>"##;

pub fn render_dcf_block(dim20: &Value) -> String {
    let dim20 = escape_payload(dim20);
    let dcf = dim20.get("dcf").cloned().unwrap_or(Value::Null);
    if !uzi_core::py::truthy(&dcf) || dcf.get("intrinsic_per_share").is_none() {
        return MISSING_BLOCK.to_string();
    }
    if dcf.get("intrinsic_per_share").map(|v| v.is_null()).unwrap_or(false) {
        let verdict = escape_text(&Value::String(safe(
            dcf.get("verdict").unwrap_or(&Value::Null),
            "亏损期自由现金流为负，DCF 无法收敛",
        )));
        return format!(
            r##"<div class="dcf-block"><p class="muted">DCF 暂不可用：{verdict}。亏损期请结合 PB 分位、LBO 与 Comps 估值交叉判断。</p></div>"##
        );
    }

    let wacc_info = dcf.get("wacc_breakdown").cloned().unwrap_or(Value::Null);
    let wacc_info = if wacc_info.is_object() { wacc_info } else { Value::Null };
    let wacc_pct = num0(wacc_info.get("wacc").unwrap_or(&Value::Null)) * 100.0;
    let ke_pct = num0(wacc_info.get("cost_of_equity").unwrap_or(&Value::Null)) * 100.0;
    let kd_pct = num0(wacc_info.get("after_tax_kd").unwrap_or(&Value::Null)) * 100.0;

    let intrinsic = num0(dcf.get("intrinsic_per_share").unwrap_or(&Value::Null));
    let cur_px = num0(dcf.get("current_price").unwrap_or(&Value::Null));
    let sm = num0(dcf.get("safety_margin_pct").unwrap_or(&Value::Null));
    let verdict = disp(dcf.get("verdict").unwrap_or(&Value::String(String::new())));

    let log_items: String = match dcf.get("methodology_log").and_then(|v| v.as_array()) {
        Some(a) => a.iter().take(7).map(|l| format!("<li>{}</li>", disp(l))).collect(),
        None => String::new(),
    };

    let sens = dcf.get("sensitivity_table").cloned().unwrap_or(Value::Null);
    let wacc_axis = match sens.get("wacc_axis").and_then(|v| v.as_array()) {
        Some(a) => a.clone(),
        None => Vec::new(),
    };
    let g_axis = match sens.get("g_axis").and_then(|v| v.as_array()) {
        Some(a) => a.clone(),
        None => Vec::new(),
    };
    let values = match sens.get("values_per_share").and_then(|v| v.as_array()) {
        Some(a) => a.clone(),
        None => Vec::new(),
    };

    let mut heat_rows = String::new();
    if !values.is_empty() && !wacc_axis.is_empty() && !g_axis.is_empty() {
        let mut header = String::from("<tr><th></th>");
        for g in &g_axis {
            header.push_str(&format!("<th>g={}</th>", disp(g)));
        }
        header.push_str("</tr>");
        let mut body = String::new();
        for (i, row) in values.iter().enumerate() {
            let mut cells = String::new();
            for val in row.as_array().cloned().unwrap_or_default() {
                let numeric_val = number(&val, None);
                let (color, fg, display_val) = match numeric_val {
                    None => ("#e7ecf2", "#64748b", "—".to_string()),
                    Some(n) if cur_px > 0.0 => {
                        let ratio = n / cur_px;
                        let (c, f) = if ratio >= 1.3 {
                            ("#065f46", "#fff")
                        } else if ratio >= 1.1 {
                            ("#059669", "#fff")
                        } else if ratio >= 0.9 {
                            ("#e7ecf2", "#111")
                        } else if ratio >= 0.7 {
                            ("#f97316", "#fff")
                        } else {
                            ("#b91c1c", "#fff")
                        };
                        (c, f, pyf(n))
                    }
                    Some(n) => ("#e7ecf2", "#111", pyf(n)),
                };
                let value_prefix = if display_val == "—" { "" } else { "¥" };
                cells.push_str(&format!(
                    r##"<td style="background:{color};color:{fg};padding:6px 10px;text-align:center;font-weight:700">{value_prefix}{display_val}</td>"##
                ));
            }
            let axis_label = wacc_axis
                .get(i)
                .map(disp)
                .unwrap_or_else(|| "—".to_string());
            body.push_str(&format!(
                r##"<tr><th style="padding:6px 8px;background:#f4f7fa;font-size:12px">WACC {axis_label}</th>{cells}</tr>"##
            ));
        }
        heat_rows = format!(
            r##"<table class="sens-heatmap" style="border-collapse:collapse;margin:12px 0;font-size:13px">{header}{body}</table>"##
        );
    }

    let sm_color = if sm > 10.0 {
        "#059669"
    } else if sm > -10.0 {
        "#d97706"
    } else {
        "#dc2626"
    };

    let tv_pct = safe(dcf.get("tv_pct_of_ev").unwrap_or(&Value::Null), "—");

    format!(
        r##"
    <div class="dcf-block" style="background:#fff;border:1px solid #e7ecf2;border-radius:12px;padding:20px;margin:16px 0;box-shadow:0 1px 3px rgba(16,24,40,0.06)">
      <div class="dcf-head" style="display:flex;justify-content:space-between;align-items:baseline;border-bottom:2px solid #3b82f6;padding-bottom:8px;margin-bottom:14px">
        <div>
          <span style="background:#3b82f6;color:#fff;padding:4px 10px;border-radius:4px;font-size:11px;font-weight:700;letter-spacing:1px">DCF VALUATION</span>
          <span style="margin-left:12px;font-size:14px;color:#64748b">2-Stage FCF + Gordon Growth Terminal</span>
        </div>
        <div style="font-size:11px;color:#94a3b8">dim 20.dcf</div>
      </div>
      <div class="dcf-summary" style="display:grid;grid-template-columns:repeat(4,1fr);gap:16px;margin-bottom:16px">
        <div><div style="font-size:11px;color:#64748b">WACC</div><div style="font-size:22px;font-weight:800;color:#111">{wacc_pct:.2}%</div><div style="font-size:10px;color:#94a3b8">k_e {ke_pct:.1}% · k_d {kd_pct:.1}%</div></div>
        <div><div style="font-size:11px;color:#64748b">内在价值 / 股</div><div style="font-size:22px;font-weight:800;color:#111">¥{intrinsic}</div><div style="font-size:10px;color:#94a3b8">vs 当前 ¥{cur_px}</div></div>
        <div><div style="font-size:11px;color:#64748b">安全边际</div><div style="font-size:22px;font-weight:800;color:{sm_color}">{sm}</div><div style="font-size:10px;color:#94a3b8">{verdict}</div></div>
        <div><div style="font-size:11px;color:#64748b">终值占 EV</div><div style="font-size:22px;font-weight:800;color:#111">{tv_pct}%</div><div style="font-size:10px;color:#94a3b8">高度依赖 g</div></div>
      </div>
      <details style="margin-bottom:14px">
        <summary style="cursor:pointer;color:#2563eb;font-weight:600;font-size:13px">📐 计算推导（7 步）</summary>
        <ol style="margin:10px 0 0 20px;color:#475569;font-size:13px;line-height:1.8">{log_items}</ol>
      </details>
      <div>
        <div style="font-size:12px;color:#64748b;margin-bottom:6px">📊 5×5 敏感性表（WACC × 终值 g）· 中心 = 基础案例</div>
        {heat_rows}
      </div>
    </div>
    "##,
        intrinsic = pyf(intrinsic),
        cur_px = pyf(cur_px),
        sm = signed(sm, 1),
        wacc_pct = wacc_pct,
        ke_pct = ke_pct,
        kd_pct = kd_pct,
    )
}

pub fn render_comps_block(dim20: &Value) -> String {
    let dim20 = escape_payload(dim20);
    let comps = dim20.get("comps").cloned().unwrap_or(Value::Null);
    if !uzi_core::py::truthy(&comps) || comps.get("peer_stats").is_none() {
        return r##"<div class="comps-block"><p class="muted">Comps 同行数据缺失</p></div>"##.to_string();
    }
    let stats = comps.get("peer_stats").cloned().unwrap_or(Value::Null);
    let target_pct = comps.get("target_percentile").cloned().unwrap_or(Value::Null);
    let verdict = disp(comps.get("valuation_verdict").unwrap_or(&Value::String("—".to_string())));
    let implied = comps.get("implied_price").cloned().unwrap_or(Value::Null);

    let pct_color = |p: f64| -> &'static str {
        if p <= 25.0 {
            "#059669"
        } else if p <= 50.0 {
            "#3b82f6"
        } else if p <= 75.0 {
            "#d97706"
        } else {
            "#dc2626"
        }
    };

    let mut metric_rows = String::new();
    for m in ["pe", "pb", "ps", "ev_ebitda", "roe", "net_margin"] {
        let s = stats.get(m).cloned().unwrap_or(Value::Null);
        if !uzi_core::py::truthy(&s) {
            continue;
        }
        let pct = num0(target_pct.get(m).unwrap_or(&Value::Number(50.into()))).clamp(0.0, 100.0);
        let bar = format!(
            r##"<div style="background:#e7ecf2;height:6px;border-radius:3px;overflow:hidden"><div style="background:{};height:100%;width:{}%"></div></div>"##,
            pct_color(pct),
            pyf(pct)
        );
        metric_rows.push_str(&format!(
            r##"
        <tr>
          <td style="padding:8px;font-weight:600">{metric}</td>
          <td style="padding:8px;text-align:right">{min}</td>
          <td style="padding:8px;text-align:right">{median}</td>
          <td style="padding:8px;text-align:right">{max}</td>
          <td style="padding:8px;text-align:center"><span style="color:{color};font-weight:700">{pct:.0}%</span><br>{bar}</td>
        </tr>"##,
            metric = m.to_uppercase().replace('_', "-"),
            min = disp(s.get("min").unwrap_or(&Value::String("—".to_string()))),
            median = disp(s.get("median").unwrap_or(&Value::String("—".to_string()))),
            max = disp(s.get("max").unwrap_or(&Value::String("—".to_string()))),
            color = pct_color(pct),
            pct = pct
        ));
    }

    let implied_rows = match implied.as_object() {
        Some(o) if !o.is_empty() => o
            .iter()
            .map(|(k, v)| {
                format!(
                    r##"<div style="display:inline-block;margin-right:20px"><span style="color:#64748b;font-size:11px">{k}</span><div style="font-size:20px;font-weight:800">¥{}</div></div>"##,
                    disp(v)
                )
            })
            .collect::<String>(),
        _ => r##"<span class="muted">—</span>"##.to_string(),
    };

    format!(
        r##"
    <div class="comps-block" style="background:#fff;border:1px solid #e7ecf2;border-radius:12px;padding:20px;margin:16px 0;box-shadow:0 1px 3px rgba(16,24,40,0.06)">
      <div style="display:flex;justify-content:space-between;align-items:baseline;border-bottom:2px solid #8b5cf6;padding-bottom:8px;margin-bottom:14px">
        <div>
          <span style="background:#8b5cf6;color:#fff;padding:4px 10px;border-radius:4px;font-size:11px;font-weight:700;letter-spacing:1px">COMPS</span>
          <span style="margin-left:12px;font-size:14px;color:#64748b">同行对标 · 分位分析</span>
        </div>
        <div style="font-size:14px;font-weight:700">{verdict}</div>
      </div>
      <table style="width:100%;border-collapse:collapse;font-size:13px">
        <thead style="background:#f7f9fc;color:#64748b;font-size:11px;letter-spacing:0.5px">
          <tr><th style="padding:8px;text-align:left">METRIC</th><th style="padding:8px;text-align:right">MIN</th><th style="padding:8px;text-align:right">MEDIAN</th><th style="padding:8px;text-align:right">MAX</th><th style="padding:8px;text-align:center">目标分位</th></tr>
        </thead>
        <tbody>{metric_rows}</tbody>
      </table>
      <div style="margin-top:14px;padding-top:12px;border-top:1px dashed #e7ecf2">
        <div style="font-size:11px;color:#64748b;margin-bottom:6px">隐含每股价（基于同行中位数倍数）</div>
        {implied_rows}
      </div>
    </div>
    "##
    )
}

pub fn render_lbo_block(dim20: &Value) -> String {
    let dim20 = escape_payload(dim20);
    let lbo = dim20.get("lbo").cloned().unwrap_or(Value::Null);
    if !uzi_core::py::truthy(&lbo) {
        return String::new();
    }
    let irr = num0(lbo.get("irr_pct").unwrap_or(&Value::Null));
    let moic = safe(lbo.get("moic").unwrap_or(&Value::Null), "—");
    let verdict = disp(lbo.get("verdict").unwrap_or(&Value::String(String::new())));
    let debt_sched = numeric_series(lbo.get("debt_schedule").unwrap_or(&Value::Null));
    let ebitda_path = numeric_series(lbo.get("ebitda_path").unwrap_or(&Value::Null));
    let irr_color = if irr >= 20.0 {
        "#059669"
    } else if irr >= 15.0 {
        "#d97706"
    } else {
        "#dc2626"
    };

    let ebitda_sparks = if ebitda_path.is_empty() {
        String::new()
    } else {
        let vals: Vec<Value> = ebitda_path.iter().map(|v| Value::from(*v)).collect();
        svg_sparkline(&vals, 220, 40, "#3b82f6", true)
    };
    let debt_sparks = if debt_sched.is_empty() {
        String::new()
    } else {
        let vals: Vec<Value> = debt_sched.iter().map(|v| Value::from(*v)).collect();
        svg_sparkline(&vals, 220, 40, "#dc2626", true)
    };

    format!(
        r##"
    <div class="lbo-block" style="background:#fff;border:1px solid #e7ecf2;border-radius:12px;padding:20px;margin:16px 0;box-shadow:0 1px 3px rgba(16,24,40,0.06)">
      <div style="display:flex;justify-content:space-between;align-items:baseline;border-bottom:2px solid #d97706;padding-bottom:8px;margin-bottom:14px">
        <div>
          <span style="background:#d97706;color:#fff;padding:4px 10px;border-radius:4px;font-size:11px;font-weight:700;letter-spacing:1px">QUICK LBO</span>
          <span style="margin-left:12px;font-size:14px;color:#64748b">PE 买方视角 · 5 年退出</span>
        </div>
      </div>
      <div style="display:grid;grid-template-columns:repeat(4,1fr);gap:16px;margin-bottom:16px">
        <div><div style="font-size:11px;color:#64748b">入场 EBITDA</div><div style="font-size:20px;font-weight:800">{entry} 亿</div><div style="font-size:10px;color:#94a3b8">EV {ev} 亿</div></div>
        <div><div style="font-size:11px;color:#64748b">杠杆倍数</div><div style="font-size:20px;font-weight:800">{lev}x</div><div style="font-size:10px;color:#94a3b8">债 {debt} 亿</div></div>
        <div><div style="font-size:11px;color:#64748b">退出 IRR</div><div style="font-size:24px;font-weight:900;color:{irr_color}">{irr_s}%</div><div style="font-size:10px;color:#94a3b8">MOIC {moic}x</div></div>
        <div><div style="font-size:11px;color:#64748b">结论</div><div style="font-size:14px;font-weight:700;color:{irr_color}">{verdict}</div></div>
      </div>
      <div class="lbo-spark-grid" style="display:grid;grid-template-columns:1fr 1fr;gap:20px">
        <div style="min-width:0"><div style="font-size:11px;color:#64748b;margin-bottom:4px">5 年 EBITDA 路径</div>{ebitda_sparks}</div>
        <div style="min-width:0"><div style="font-size:11px;color:#64748b;margin-bottom:4px">债务偿还进度</div>{debt_sparks}</div>
      </div>
    </div>
    "##,
        entry = disp(lbo.get("entry_ebitda_yi").unwrap_or(&Value::Number(0.into()))),
        ev = disp(lbo.get("entry_ev_yi").unwrap_or(&Value::Number(0.into()))),
        lev = disp(lbo.get("leverage_turns").unwrap_or(&Value::Number(0.into()))),
        debt = disp(lbo.get("entry_debt_yi").unwrap_or(&Value::Number(0.into()))),
        irr_s = pyf(irr),
    )
}

pub fn render_initiating_coverage(dim21: &Value) -> String {
    let dim21 = escape_payload(dim21);
    let ic = dim21.get("initiating_coverage").cloned().unwrap_or(Value::Null);
    if !uzi_core::py::truthy(&ic) {
        return String::new();
    }
    let head = ic.get("headline").cloned().unwrap_or(Value::Null);
    let rating = safe(head.get("rating").unwrap_or(&Value::Null), "—");
    let tp = head.get("target_price").cloned().unwrap_or(Value::Number(0.into()));
    let cur = disp(head.get("current_price").unwrap_or(&Value::Number(0.into())));
    let ups = head.get("upside_pct").cloned().unwrap_or(Value::Number(0.into()));

    let is_rated = !rating.contains("未评级");
    let rating_color = if !is_rated {
        "#64748b"
    } else if rating.contains("买入") || rating.contains("增持") {
        "#059669"
    } else if rating.contains("持有") {
        "#d97706"
    } else {
        "#dc2626"
    };
    let target_display = if is_rated && num0(&tp) > 0.0 {
        format!("¥{}", disp(&tp))
    } else {
        "—".to_string()
    };
    let upside_display = if is_rated {
        format!("{}%", signed(num0(&ups), 1))
    } else {
        "—".to_string()
    };
    let pillars = match ic.get("investment_thesis").and_then(|v| v.as_array()) {
        Some(a) => a.clone(),
        None => Vec::new(),
    };
    let risks = match ic.get("key_risks").and_then(|v| v.as_array()) {
        Some(a) => a.clone(),
        None => Vec::new(),
    };

    let pillar_html: String = pillars
        .iter()
        .take(5)
        .map(|p| {
            format!(
                r##"<li style="margin-bottom:8px"><strong>{}</strong> <span style="background:#eef4ff;color:#3730a3;padding:2px 6px;border-radius:3px;font-size:10px;margin-left:4px">{}</span><br><span style="color:#64748b;font-size:12px">{}</span></li>"##,
                safe(p.get("pillar").unwrap_or(&Value::Null), "—"),
                disp(p.get("weight").unwrap_or(&Value::String(String::new()))),
                disp(p.get("evidence").unwrap_or(&Value::String(String::new())))
            )
        })
        .collect();
    let risk_html: String = risks
        .iter()
        .take(5)
        .map(|r| {
            format!(
                r##"<li style="margin-bottom:6px"><span style="color:#dc2626">●</span> <strong>{}</strong> <span style="color:#94a3b8;font-size:11px">({})</span><br><span style="color:#64748b;font-size:12px">{}</span></li>"##,
                safe(r.get("risk").unwrap_or(&Value::Null), "—"),
                disp(r.get("severity").unwrap_or(&Value::String(String::new()))),
                disp(r.get("detail").unwrap_or(&Value::String(String::new())))
            )
        })
        .collect();

    format!(
        r##"
    <div class="initiating-block" style="background:#fff;border:1px solid #e7ecf2;border-radius:12px;padding:20px;margin:16px 0;box-shadow:0 1px 3px rgba(16,24,40,0.06)">
      <div style="display:flex;justify-content:space-between;align-items:baseline;border-bottom:2px solid #2563eb;padding-bottom:8px;margin-bottom:14px">
        <div>
          <span style="background:#2563eb;color:#fff;padding:4px 10px;border-radius:4px;font-size:11px;font-weight:700;letter-spacing:1px">INITIATING COVERAGE</span>
          <span style="margin-left:12px;font-size:14px;color:#64748b">机构首次覆盖 · JPM/GS/MS 格式</span>
        </div>
      </div>
      <div style="display:flex;gap:24px;margin-bottom:14px;padding:12px;background:#f7f9fc;border-radius:8px">
        <div><div style="font-size:11px;color:#64748b">RATING</div><div style="font-size:18px;font-weight:800;color:{rating_color}">{rating}</div></div>
        <div><div style="font-size:11px;color:#64748b">TARGET</div><div style="font-size:18px;font-weight:800">{target_display}</div></div>
        <div><div style="font-size:11px;color:#64748b">CURRENT</div><div style="font-size:18px;font-weight:800">¥{cur}</div></div>
        <div><div style="font-size:11px;color:#64748b">UPSIDE</div><div style="font-size:18px;font-weight:800;color:{rating_color}">{upside_display}</div></div>
      </div>
      <div style="padding:10px;background:#eef4ff;border-left:3px solid #2563eb;margin-bottom:14px;font-size:13px;line-height:1.6">{summary}</div>
      <div style="display:grid;grid-template-columns:1fr 1fr;gap:20px">
        <div>
          <div style="font-size:11px;color:#64748b;font-weight:700;margin-bottom:8px">💪 INVESTMENT THESIS</div>
          <ul style="margin:0;padding-left:18px;font-size:13px">{pillar_html}</ul>
        </div>
        <div>
          <div style="font-size:11px;color:#64748b;font-weight:700;margin-bottom:8px">⚠️ KEY RISKS</div>
          <ul style="margin:0;padding-left:18px;font-size:13px">{risk_html}</ul>
        </div>
      </div>
    </div>
    "##,
        summary = disp(ic.get("executive_summary").unwrap_or(&Value::String(String::new())))
    )
}

pub fn render_ic_memo(dim22: &Value) -> String {
    let dim22 = escape_payload(dim22);
    let ic = dim22.get("ic_memo").cloned().unwrap_or(Value::Null);
    let sections = ic.get("sections").cloned().unwrap_or(Value::Null);
    if !uzi_core::py::truthy(&sections) {
        return String::new();
    }
    let exec_sum = sections.get("I_exec_summary").cloned().unwrap_or(Value::Null);
    let scenarios = match sections.get("VII_returns_scenarios").and_then(|v| v.as_array()) {
        Some(a) => a.clone(),
        None => Vec::new(),
    };
    let risks = match sections.get("VI_risks_mitigants").and_then(|v| v.as_array()) {
        Some(a) => a.clone(),
        None => Vec::new(),
    };

    let headline = safe(exec_sum.get("headline").unwrap_or(&Value::Null), "—");
    let rec_color = if headline.contains('🟢') {
        "#059669"
    } else if headline.contains('🟡') {
        "#d97706"
    } else if headline.contains('⚪') {
        "#64748b"
    } else {
        "#dc2626"
    };

    let mut scen_html = String::new();
    for s in scenarios {
        let ret = num0(s.get("return_pct").unwrap_or(&Value::Null));
        let ret_color = if ret > 0.0 { "#059669" } else { "#dc2626" };
        scen_html.push_str(&format!(
            r##"
        <div style="border:1px solid #e7ecf2;border-radius:8px;padding:10px">
          <div style="font-size:11px;color:#64748b;font-weight:700">{scenario} · p={prob}%</div>
          <div style="font-size:20px;font-weight:800;margin:4px 0">¥{target}</div>
          <div style="font-size:13px;font-weight:700;color:{ret_color}">{ret}</div>
          <div style="font-size:10px;color:#94a3b8;margin-top:4px">{assumptions}</div>
        </div>"##,
            scenario = safe(s.get("scenario").unwrap_or(&Value::Null), "—"),
            prob = safe(s.get("probability_pct").unwrap_or(&Value::Null), "—"),
            target = safe(s.get("price_target").unwrap_or(&Value::Null), "—"),
            assumptions = disp(s.get("assumptions").unwrap_or(&Value::String(String::new()))),
            ret = signed(ret, 1)
        ));
    }

    let risk_html: String = risks
        .iter()
        .take(5)
        .map(|r| {
            format!(
                r##"<li style="margin-bottom:6px"><strong>{}</strong> <span style="color:#dc2626;font-size:10px">({})</span><br><span style="color:#64748b;font-size:12px">{}</span> · <span style="color:#059669;font-size:11px">缓解：{}</span></li>"##,
                safe(r.get("risk").unwrap_or(&Value::Null), "—"),
                disp(r.get("severity").unwrap_or(&Value::String(String::new()))),
                disp(r.get("detail").unwrap_or(&Value::String(String::new()))),
                safe(r.get("mitigant").unwrap_or(&Value::Null), "—")
            )
        })
        .collect();

    format!(
        r##"
    <div class="ic-memo-block" style="background:#fff;border:1px solid #e7ecf2;border-radius:12px;padding:20px;margin:16px 0;box-shadow:0 1px 3px rgba(16,24,40,0.06)">
      <div style="display:flex;justify-content:space-between;align-items:baseline;border-bottom:2px solid #be123c;padding-bottom:8px;margin-bottom:14px">
        <div>
          <span style="background:#be123c;color:#fff;padding:4px 10px;border-radius:4px;font-size:11px;font-weight:700;letter-spacing:1px">IC MEMO</span>
          <span style="margin-left:12px;font-size:14px;color:#64748b">投委会备忘录 · 8 章节</span>
        </div>
      </div>
      <div style="padding:14px;background:#fef3f2;border-left:4px solid {rec_color};margin-bottom:14px">
        <div style="font-size:11px;color:#64748b;font-weight:700;margin-bottom:4px">RECOMMENDATION</div>
        <div style="font-size:18px;font-weight:800;color:{rec_color}">{headline}</div>
      </div>
      <div style="margin-bottom:14px">
        <div style="font-size:11px;color:#64748b;font-weight:700;margin-bottom:8px">📊 三情景回报分析</div>
        <div style="display:grid;grid-template-columns:1fr 1fr 1fr;gap:12px">{scen_html}</div>
      </div>
      <div>
        <div style="font-size:11px;color:#64748b;font-weight:700;margin-bottom:8px">⚠️ 核心风险 + 缓解</div>
        <ul style="margin:0;padding-left:18px;font-size:13px">{risk_html}</ul>
      </div>
    </div>
    "##
    )
}

pub fn render_catalyst_calendar(dim21: &Value) -> String {
    let dim21 = escape_payload(dim21);
    let cat = dim21.get("catalyst_calendar").cloned().unwrap_or(Value::Null);
    let events = match cat.get("events").and_then(|v| v.as_array()) {
        Some(a) => a.clone(),
        None => Vec::new(),
    };
    if events.is_empty() {
        return String::new();
    }
    let impact_color = |imp: &str| -> &'static str {
        match imp {
            "high" => "#dc2626",
            "medium" => "#d97706",
            "low" => "#94a3b8",
            "past" => "#64748b",
            _ => "#94a3b8",
        }
    };

    let mut items = String::new();
    for ev in events.iter().take(12) {
        let imp = ev
            .get("impact")
            .and_then(|v| v.as_str())
            .unwrap_or("low");
        let event_date: String = safe(ev.get("date").unwrap_or(&Value::Null), "—")
            .chars()
            .take(10)
            .collect();
        let expectation = ev.get("expectation").cloned().unwrap_or(Value::Null);
        let expectation_html = if uzi_core::py::truthy(&expectation) {
            format!(
                r##"<div style="font-size:11px;color:#94a3b8">{}</div>"##,
                disp(&expectation)
            )
        } else {
            String::new()
        };
        items.push_str(&format!(
            r##"
        <div style="display:flex;padding:10px;border-bottom:1px solid #f4f7fa">
          <div style="min-width:90px;font-size:12px;color:#64748b;font-family:Menlo,monospace">{event_date}</div>
          <div style="width:8px;height:8px;border-radius:50%;background:{color};margin:6px 10px 0 0"></div>
          <div style="flex:1"><div style="font-size:13px;color:#111">{event}</div>
            {expectation_html}
          </div>
          <div style="font-size:10px;color:{color};font-weight:700;text-transform:uppercase">{imp}</div>
        </div>"##,
            color = impact_color(imp),
            event = disp(ev.get("event").unwrap_or(&Value::String("—".to_string())))
        ));
    }

    format!(
        r##"
    <div class="catalyst-block" style="background:#fff;border:1px solid #e7ecf2;border-radius:12px;padding:20px;margin:16px 0;box-shadow:0 1px 3px rgba(16,24,40,0.06)">
      <div style="display:flex;justify-content:space-between;align-items:baseline;border-bottom:2px solid #059669;padding-bottom:8px;margin-bottom:10px">
        <div>
          <span style="background:#059669;color:#fff;padding:4px 10px;border-radius:4px;font-size:11px;font-weight:700;letter-spacing:1px">CATALYST CALENDAR</span>
          <span style="margin-left:12px;font-size:14px;color:#64748b">催化剂日历 · 影响分级</span>
        </div>
        <div style="font-size:11px;color:#94a3b8">共 {n} 条 · {high} 高影响</div>
      </div>
      <div>{items}</div>
    </div>
    "##,
        n = events.len(),
        high = disp(cat.get("high_impact_count").unwrap_or(&Value::Number(0.into())))
    )
}

pub fn render_competitive_analysis(dim22: &Value) -> String {
    let dim22 = escape_payload(dim22);
    let ca = dim22.get("competitive_analysis").cloned().unwrap_or(Value::Null);
    let porter = ca.get("porter_five_forces").cloned().unwrap_or(Value::Null);
    let bcg = ca.get("bcg_position").cloned().unwrap_or(Value::Null);
    let attr = num0(ca.get("industry_attractiveness_pct").unwrap_or(&Value::Null));
    if !uzi_core::py::truthy(&porter) {
        return String::new();
    }

    let force_labels = ["新进入者", "替代品", "供应商", "买方", "现有竞争"];
    let force_keys = [
        "new_entrants_threat",
        "substitutes_threat",
        "supplier_power",
        "buyer_power",
        "rivalry_intensity",
    ];
    let force_values: Vec<f64> = force_keys
        .iter()
        .map(|k| {
            let s = porter.get(*k).cloned().unwrap_or(Value::Null);
            number(s.get("score").unwrap_or(&Value::Null), Some(3.0)).unwrap_or(3.0)
        })
        .collect();
    let labels: Vec<String> = force_labels.iter().map(|s| s.to_string()).collect();
    let radar = svg_radar(&labels, &force_values, 5.0, 200.0);

    let bcg_cat = bcg
        .get("category")
        .and_then(|v| v.as_str())
        .unwrap_or("—");
    let bcg_color = match bcg_cat {
        "Star (明星)" => "#059669",
        "Cash Cow (现金牛)" => "#3b82f6",
        "Question Mark (问号)" => "#d97706",
        "Dog (瘦狗)" => "#94a3b8",
        _ => "#94a3b8",
    };

    format!(
        r##"
    <div class="competitive-block" style="background:#fff;border:1px solid #e7ecf2;border-radius:12px;padding:20px;margin:16px 0;box-shadow:0 1px 3px rgba(16,24,40,0.06)">
      <div style="display:flex;justify-content:space-between;align-items:baseline;border-bottom:2px solid #7c3aed;padding-bottom:8px;margin-bottom:14px">
        <div>
          <span style="background:#7c3aed;color:#fff;padding:4px 10px;border-radius:4px;font-size:11px;font-weight:700;letter-spacing:1px">COMPETITIVE</span>
          <span style="margin-left:12px;font-size:14px;color:#64748b">Porter 5 Forces + BCG Matrix</span>
        </div>
        <div style="font-size:12px;color:#64748b">行业吸引力 <strong style="color:#111">{attr}</strong>%</div>
      </div>
      <div style="display:grid;grid-template-columns:1fr 1fr;gap:20px;align-items:center">
        <div style="text-align:center">{radar}</div>
        <div>
          <div style="font-size:11px;color:#64748b;margin-bottom:6px">BCG 矩阵定位</div>
          <div style="font-size:22px;font-weight:800;color:{bcg_color};margin-bottom:8px">{bcg_cat}</div>
          <div style="font-size:12px;color:#475569;margin-bottom:4px">市场份额 {ms}% · 市场增速 {mg}%</div>
          <div style="padding:10px;background:#faf5ff;border-left:3px solid {bcg_color};font-size:12px">战略建议：{action}</div>
        </div>
      </div>
    </div>
    "##,
        attr = pyf(attr),
        ms = safe(bcg.get("market_share_pct").unwrap_or(&Value::Null), "—"),
        mg = safe(bcg.get("market_growth_pct").unwrap_or(&Value::Null), "—"),
        action = disp(bcg.get("strategic_action").unwrap_or(&Value::String("—".to_string())))
    )
}

pub fn render_style_chip(syn: &Value) -> String {
    let syn = escape_payload(syn);
    let style = syn.get("detected_style").cloned().unwrap_or(Value::Null);
    if !uzi_core::py::truthy(&style) {
        return String::new();
    }
    let label = match syn.get("style_label_cn") {
        Some(v) if uzi_core::py::truthy(v) => disp(v),
        _ => disp(&style),
    };
    let explanation = match syn.get("style_explanation") {
        Some(v) if uzi_core::py::truthy(v) => disp(v),
        _ => String::new(),
    };
    let diag = syn.get("style_diagnostics").cloned().unwrap_or(Value::Null);
    let fund_old = number(diag.get("raw_fund_old").unwrap_or(&Value::Number(0.into())), Some(0.0));
    let fund_new = number(
        syn.get("fundamental_score").unwrap_or(&Value::Number(0.into())),
        Some(0.0),
    );
    let cons_old = number(diag.get("raw_consensus_old").unwrap_or(&Value::Number(0.into())), Some(0.0));
    let cons_new = number(
        syn.get("panel_consensus").unwrap_or(&Value::Number(0.into())),
        Some(0.0),
    );

    fn delta(old: Option<f64>, new: Option<f64>) -> String {
        let (old, new) = match (old, new) {
            (Some(o), Some(n)) => (o, n),
            _ => return String::new(),
        };
        let d = new - old;
        if d.abs() < 0.05 {
            return String::new();
        }
        let cls = if d > 0.0 { "delta-up" } else { "delta-down" };
        let sign = if d > 0.0 { "+" } else { "" };
        format!(r##" <span class="{cls}">({sign}{d:.1})</span>"##)
    }

    let compare = format!(
        "fund {fo:.1}→<strong>{fnv:.1}</strong>{fd} · panel {co:.1}→<strong>{cnv:.1}</strong>{cd}",
        fo = fund_old.unwrap_or(0.0),
        fnv = fund_new.unwrap_or(0.0),
        fd = delta(fund_old, fund_new),
        co = cons_old.unwrap_or(0.0),
        cnv = cons_new.unwrap_or(0.0),
        cd = delta(cons_old, cons_new)
    );

    format!(
        r##"<div class="style-chip-wrap">
  <span class="icon">🎯</span>
  <span class="label">本股识别为</span>
  <span class="value">{label}</span>
  <span class="hint">{explanation}</span>
  <span class="compare">{compare}</span>
</div>"##
    )
}

pub fn render_data_gap_banner(data_gaps: &Value, raw: &Value, syn: &Value) -> String {
    let data_gaps = escape_payload(data_gaps);
    let raw = escape_payload(raw);
    let syn = escape_payload(syn);
    if !data_gaps.is_object() || !uzi_core::py::truthy(data_gaps.get("tasks").unwrap_or(&Value::Null)) {
        return String::new();
    }

    let tasks = data_gaps.get("tasks").and_then(|v| v.as_array()).cloned().unwrap_or_default();
    let total = tasks.len();
    let unresolved = match data_gaps.get("unresolved") {
        Some(v) if uzi_core::py::truthy(v) => crate::pyfmt::num(v) as i64,
        _ => total as i64,
    };
    let ack = total as i64 - unresolved;
    let cov = crate::pyfmt::num(data_gaps.get("coverage_pct").unwrap_or(&Value::Number(0.into())));

    let mut is_low_confidence = false;
    let mut fund_score_for_msg = 0.0_f64;
    if syn.is_object() {
        fund_score_for_msg = crate::pyfmt::num(syn.get("fundamental_score").unwrap_or(&Value::Number(60.into())));
        if fund_score_for_msg < 50.0 && cov < 60.0 {
            is_low_confidence = true;
        }
    }

    let mut is_fund_like = false;
    let mut fund_label: Option<String> = None;
    if uzi_core::py::truthy(&raw) {
        let basic = raw
            .get("dimensions")
            .and_then(|d| d.get("0_basic"))
            .and_then(|b| b.get("data"))
            .cloned()
            .unwrap_or(Value::Null);
        let sec_type = match basic.get("security_type") {
            Some(v) if uzi_core::py::truthy(v) => disp(v),
            _ => match raw.get("security_type") {
                Some(v) if uzi_core::py::truthy(v) => disp(v),
                _ => String::new(),
            },
        };
        let ticker = raw
            .get("ticker")
            .map(disp)
            .unwrap_or_default();
        let label_for = |st: &str| -> Option<String> {
            match st {
                "etf" => Some("ETF".to_string()),
                "lof" => Some("LOF 基金".to_string()),
                "mutual_fund" => Some("开放式基金".to_string()),
                _ => None,
            }
        };
        if matches!(sec_type.as_str(), "etf" | "lof" | "mutual_fund") {
            is_fund_like = true;
            fund_label = label_for(&sec_type);
        } else if !ticker.is_empty() {
            let ti = uzi_core::ticker::parse_ticker(&ticker);
            if ti.market == "A" {
                let st = uzi_core::ticker::classify_security_type(&ti.code);
                let st = st.as_str();
                if matches!(st, "etf" | "lof" | "mutual_fund") {
                    is_fund_like = true;
                    fund_label = label_for(st);
                }
            }
        }
    }

    let mut sorted_tasks = tasks.clone();
    let order = |sev: Option<&str>| -> i32 {
        match sev {
            Some("critical") => 0,
            Some("optional") => 1,
            Some("enrichment") => 2,
            _ => 9,
        }
    };
    sorted_tasks.sort_by(|a, b| {
        let sa = order(a.get("severity").and_then(|v| v.as_str()));
        let sb = order(b.get("severity").and_then(|v| v.as_str()));
        sa.cmp(&sb).then_with(|| {
            let da = disp(a.get("dim").unwrap_or(&Value::String(String::new())));
            let db = disp(b.get("dim").unwrap_or(&Value::String(String::new())));
            da.cmp(&db)
        })
    });

    let mut chips_html: Vec<String> = Vec::new();
    for t in sorted_tasks.iter().take(20) {
        let mut cls = "chip".to_string();
        if t.get("status").and_then(|v| v.as_str()) == Some("acknowledged") {
            cls.push_str(" ack");
        }
        chips_html.push(format!(
            r##"<span class="{cls}">{label} · {dim}</span>"##,
            label = disp(t.get("label").unwrap_or(&Value::String("?".to_string()))),
            dim = disp(t.get("dim").unwrap_or(&Value::String("?".to_string())))
        ));
    }
    let chips_block = chips_html.join("\n      ");
    let overflow = if sorted_tasks.len() > 20 {
        format!(
            r##"<span class="chip">+{} 更多</span>"##,
            sorted_tasks.len() - 20
        )
    } else {
        String::new()
    };

    let (banner_class, title, subtitle, hint) = if is_fund_like {
        let fl = fund_label.unwrap_or_else(|| "基金".to_string());
        (
            "data-gap-banner fund-type".to_string(),
            format!("⚠️ FUND-TYPE NOTE · {fl} 缺个股财务字段属预期"),
            format!(
                "<strong>{fl}</strong>本身没有 ROE / 营收 / 净利率 / PE / 公司名 等个股财务字段 · 所以数据覆盖率 <strong>{cov}%</strong> 是<strong>预期偏低</strong>·不影响分析可信度. 如果你想看具体业绩 · v3.4.0+ 起会自动询问是否循环分析<strong>前 10 大持仓股</strong>（如 ETF 沪深 300 → 茅台 / 宁德等成分股）·每只持仓股都有完整 22 维报告."
            ),
            "📌 这不是数据采集失败 · 是基金类型本身的字段差异. 对基金的核心评估应看持仓集中度 / 跟踪误差 / 历史回撤 (UZI 暂不直接评 · 走持仓循环代替).".to_string(),
        )
    } else if is_low_confidence {
        (
            "data-gap-banner low-confidence".to_string(),
            "🚨 LOW CONFIDENCE · 规则引擎评分可能失真".to_string(),
            format!(
                "<strong>规则引擎给出 fundamental_score = {fs:.1}</strong> · 但数据覆盖率仅 <strong>{cov}%</strong> · <strong>{total}</strong> 个核心字段缺失 · 当多个维度数据空缺时 · 规则引擎默认给中性 5-6 分 · 会人为<strong>拉低 fund_score</strong> · 不一定真实反映基本面.",
                fs = fund_score_for_msg
            ),
            "📌 强烈建议：以 <strong>agent 重评估</strong>（基于全 22 维 + DCF + 同行 + 流派分歧）为准 · 而不是看 fund_score / 评委 0 看多 / 24 看空 这种规则引擎结论. 下方流派 consensus 也受数据缺失影响 · 仅作参考.".to_string(),
        )
    } else {
        let mut sub = format!(
            "数据覆盖率 <strong>{cov}%</strong> · 共 <strong>{total}</strong> 个字段未从脚本采集到"
        );
        if ack > 0 {
            sub.push_str(&format!("（其中 <strong>{ack}</strong> 已由 agent 确认"));
            sub.push_str("真的拿不到）");
        }
        (
            "data-gap-banner".to_string(),
            "DATA QUALITY · 本报告存在已知数据缺口".to_string(),
            sub,
            "Agent 已尝试浏览器抓取 / MX API / WebSearch / 逻辑推导；划线字段为已确认无法补齐，其余字段显示为 “—”。".to_string(),
        )
    };

    format!(
        r##"<div class="{banner_class}" role="alert">
  <div class="icon">⚠️</div>
  <div class="body">
    <div class="title">{title}</div>
    <div class="subtitle">{subtitle}</div>
    <div class="list">
      {chips_block}
      {overflow}
    </div>
    <div class="hint">{hint}</div>
  </div>
</div>"##
    )
}

pub fn render_school_lock_banner(syn: &Value) -> String {
    let syn = escape_payload(syn);
    if !syn.is_object() {
        return String::new();
    }
    let lock = syn.get("school_lock").cloned().unwrap_or(Value::Null);
    if !lock.is_object() || !uzi_core::py::truthy(lock.get("group").unwrap_or(&Value::Null)) {
        return String::new();
    }
    let group = disp(lock.get("group").unwrap_or(&Value::Null));
    let label = match lock.get("label") {
        Some(v) if uzi_core::py::truthy(v) => disp(v),
        _ => group.clone(),
    };

    let themes: &[(&str, &str, &str, &str, &str)] = &[
        ("A", "#065f46", "rgba(5,150,105,0.10)", "🛡️", "巴菲特 / 格雷厄姆 / 费雪 / 芒格 / 邓普顿 / 卡拉曼"),
        ("B", "#1e40af", "rgba(59,130,246,0.10)", "🚀", "彼得·林奇 / 木头姐 / Andreessen (a16z) / Gurley / Naval / Gerstner / Chamath"),
        ("C", "#7c2d12", "rgba(217,119,6,0.10)", "🌍", "索罗斯 / 达里奥 / Druckenmiller / Burry / Chanos"),
        ("D", "#9d174d", "rgba(219,39,119,0.10)", "📈", "利弗莫尔 / 米内尔维尼 / 达瓦斯 / 江恩"),
        ("E", "#7c3aed", "rgba(139,92,246,0.10)", "🇨🇳", "段永平 / 张坤 / 冯柳 / 邓晓峰 / 张磊 (高瓴)"),
        ("F", "#dc2626", "rgba(220,38,38,0.10)", "⚡", "赵老哥 / 孙哥 / 章盟主 / 葛卫东 / 炒股养家"),
        ("G", "#2563eb", "rgba(37,99,235,0.10)", "🤖", "Renaissance (Simons) / Ed Thorp / DE Shaw / AQR (Asness)"),
        ("H", "#b45309", "rgba(217,119,6,0.10)", "👑", "黄仁勋 / 马斯克 / Sam Altman / Saylor · 科技领袖派"),
        ("I", "#4338ca", "rgba(99,102,241,0.10)", "🔗", "Serenity · AI 供应链卡脖子/瓶颈猎手"),
    ];
    let (fg, bg, icon, members_hint) = themes
        .iter()
        .find(|(g, ..)| *g == group)
        .map(|(_, fg, bg, ic, mh)| (*fg, *bg, *ic, *mh))
        .unwrap_or(("#475569", "rgba(100,116,139,0.10)", "🎯", ""));
    let members_html = if !members_hint.is_empty() {
        format!(
            r##"<div style="margin-top:4px;color:#64748b;font-size:11px">代表评委 · {members_hint}</div>"##
        )
    } else {
        String::new()
    };

    format!(
        r##"<div class="school-lock-banner" style="margin:16px 0;padding:14px 20px;background:{bg};border-left:5px solid {fg};border-radius:8px;display:flex;align-items:center;gap:14px;font-size:13px;line-height:1.5">  <div style="font-size:22px">{icon}</div>  <div style="flex:1">    <div style="font-size:11px;letter-spacing:2px;color:{fg};font-weight:700;margin-bottom:3px">      SCHOOL LOCK · 已锁定单一流派视角    </div>    <div style="color:#1e293b">      本次分析仅由 <strong style="color:{fg}">{group} · {label}</strong> 的评委参与评分 · 其他流派的评委已 skip · 报告里"评委打分板 / 流派分数 / 多空辩论"均限于该派内.    </div>    {members_html}  </div></div>"##
    )
}

pub fn render_institutional_section(raw: &Value) -> String {
    let raw = escape_payload(raw);
    let dims = raw.get("dimensions").cloned().unwrap_or(Value::Null);
    let dims = if dims.is_object() { dims } else { Value::Null };
    let d20 = dims
        .get("20_valuation_models")
        .and_then(|v| v.get("data"))
        .cloned()
        .unwrap_or(Value::Null);
    let d21 = dims
        .get("21_research_workflow")
        .and_then(|v| v.get("data"))
        .cloned()
        .unwrap_or(Value::Null);
    let d22 = dims
        .get("22_deep_methods")
        .and_then(|v| v.get("data"))
        .cloned()
        .unwrap_or(Value::Null);

    if !(uzi_core::py::truthy(&d20) || uzi_core::py::truthy(&d21) || uzi_core::py::truthy(&d22)) {
        return r##"<div class="muted" style="padding:20px;text-align:center;color:#94a3b8">Task 1.5 机构建模数据缺失 · 请运行 compute_deep_methods</div>"##.to_string();
    }

    [
        render_dcf_block(&d20),
        render_comps_block(&d20),
        render_lbo_block(&d20),
        render_initiating_coverage(&d21),
        render_ic_memo(&d22),
        render_catalyst_calendar(&d21),
        render_competitive_analysis(&d22),
    ]
    .concat()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // Mirrors upstream tests/test_html_escape_boundary.py::
    //   test_institutional_renderers_escape_agent_and_upstream_text
    // Python: PAYLOAD='<img src=x onerror="alert(1)">';
    //   catalyst = _render_catalyst_calendar({"catalyst_calendar": {"events": [{"date": "2026-01-01", "event": PAYLOAD}]}})
    //   memo = _render_ic_memo({"ic_memo": {"sections": {"I_exec_summary": {"headline": PAYLOAD}, "VII_returns_scenarios": [], "VI_risks_mitigants": []}}})
    //   assert PAYLOAD not in catalyst; "&lt;img" in catalyst; same for memo
    const PAYLOAD: &str = "<img src=x onerror=\"alert(1)\">";

    #[test]
    fn institutional_renderers_escape_agent_and_upstream_text() {
        let catalyst = render_catalyst_calendar(&json!({
            "catalyst_calendar": {"events": [{"date": "2026-01-01", "event": PAYLOAD}]}
        }));
        let memo = render_ic_memo(&json!({
            "ic_memo": {"sections": {
                "I_exec_summary": {"headline": PAYLOAD},
                "VII_returns_scenarios": [],
                "VI_risks_mitigants": [],
            }}
        }));
        assert!(!catalyst.contains(PAYLOAD));
        assert!(catalyst.contains("&lt;img"));
        assert!(!memo.contains(PAYLOAD));
        assert!(memo.contains("&lt;img"));
    }
}
