//! Port of `lib/report/segmental.py` — segmental revenue build-up block,
//! projection table, segment donut and projection chart.

use crate::pyfmt::{disp, group_f, num, pyf};
use crate::svg::*;
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

/// Main entry: reads `segmental_model.json` for the ticker (empty when absent).
pub fn render_segmental_block(ticker: &str) -> String {
    let model = uzi_core::cache::read_task_output(ticker, "segmental_model").unwrap_or(Value::Null);
    if !uzi_core::py::truthy(&model)
        || !model.get("segments").map(uzi_core::py::truthy).unwrap_or(false)
    {
        return String::new();
    }

    let validation = uzi_core::cache::read_task_output(ticker, "segmental_validation")
        .unwrap_or(Value::Null);
    let summary = validation.get("summary").cloned().unwrap_or(Value::Null);
    let synthesis = uzi_core::cache::read_task_output(ticker, "synthesis").unwrap_or(Value::Null);

    let segments = model
        .get("segments")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let thesis = match model.get("core_thesis") {
        Some(v) if uzi_core::py::truthy(v) => disp(v),
        _ => match model.get("thesis") {
            Some(v) if uzi_core::py::truthy(v) => disp(v),
            _ => "—".to_string(),
        },
    };
    let total_rev = num(model.get("total_revenue_latest_yi").unwrap_or(&Value::Number(0.into())));
    let rev_hist = match model.get("total_revenue_history_yi").and_then(|v| v.as_array()) {
        Some(a) => a.clone(),
        None => Vec::new(),
    };
    let currency = match model.get("currency") {
        Some(v) if uzi_core::py::truthy(v) => disp(v),
        _ => "CNY".to_string(),
    };

    let gap = num(summary.get("reconciliation_gap_pct").unwrap_or(&Value::Number(0.into())));
    let base_3y = num(summary.get("base_3y_total_growth_pct").unwrap_or(&Value::Number(0.into())));
    let passed = validation
        .get("passed")
        .map(uzi_core::py::truthy)
        .unwrap_or(true);
    let badge_color = if passed && gap < 5.0 {
        "#059669"
    } else if passed {
        "#d97706"
    } else {
        "#dc2626"
    };
    let badge_icon = if passed { "✓" } else { "✗" };
    let badge_html = format!(
        r##"<span style="background:{badge_color};color:#fff;padding:4px 10px;border-radius:999px;font-size:11px;font-weight:700;letter-spacing:1px">{badge_icon} 对账 gap {gap:.1}%</span>"##
    );
    let base_3y_cagr = {
        let base = 1.0 + base_3y / 100.0;
        if base >= 0.0 {
            (base.powf(1.0 / 3.0) - 1.0) * 100.0
        } else {
            0.0
        }
    };
    let growth_badge = format!(
        r##"<span style="background:#2563eb;color:#fff;padding:4px 10px;border-radius:999px;font-size:11px;font-weight:700">📈 Bottom-Up Base 3Y 总增速 {b:+.1}% (CAGR {c:+.1}%)</span>"##,
        b = base_3y,
        c = base_3y_cagr
    );

    // DCF cross-check
    let _inst = synthesis.get("institutional_modeling").cloned().unwrap_or(Value::Null);
    let dcf_d = synthesis
        .get("raw_data")
        .and_then(|v| v.get("dimensions"))
        .and_then(|d| d.get("20_valuation_models"))
        .cloned()
        .unwrap_or(Value::Null);
    let d20 = dcf_d.get("data").cloned().unwrap_or(Value::Null);
    let dcf_obj = d20.get("dcf").cloned().unwrap_or(Value::Null);
    let assumptions = match dcf_obj
        .get("wacc_breakdown")
        .and_then(|w| w.get("assumptions"))
    {
        Some(v) if uzi_core::py::truthy(v) => v.clone(),
        _ => dcf_obj.get("assumptions").cloned().unwrap_or(Value::Null),
    };
    let dcf_g1 = assumptions
        .get("stage1_growth")
        .filter(|v| uzi_core::py::truthy(v))
        .or_else(|| assumptions.get("growth_5y").filter(|v| uzi_core::py::truthy(v)));
    let dcf_cagr = dcf_g1.and_then(|v| {
        if let Value::Number(_) = v {
            Some(num(v) * 100.0)
        } else if let Value::String(s) = v {
            s.trim().parse::<f64>().ok().map(|x| x * 100.0)
        } else {
            None
        }
    });

    let cross_check_badge = match dcf_cagr {
        None => String::new(),
        Some(cagr) => {
            let diff = (base_3y_cagr - cagr).abs();
            let (ic_color, ic_icon, ic_verdict) = if diff < 3.0 {
                ("#059669", "✓", "一致")
            } else if diff < 6.0 {
                ("#d97706", "⚠", "小分歧")
            } else {
                ("#dc2626", "✗", "严重打架")
            };
            format!(
                r##"<span style="background:{ic_color};color:#fff;padding:4px 10px;border-radius:999px;font-size:11px;font-weight:700">🔀 {ic_icon} vs DCF 自上而下 {cagr:.1}% · {ic_verdict}</span>"##
            )
        }
    };

    let donut_svg = svg_segment_donut(&segments, &currency, 220);
    let line_svg = svg_segment_projection(&segments, &rev_hist, 420, 220);
    let projection_table = render_segmental_projection_table(&segments, &currency);

    let thesis_icons: &[(&str, &str, &str, &str)] = &[
        ("cash_cow", "💰", "#059669", "稳定现金牛"),
        ("growth_engine", "🚀", "#2563eb", "成长引擎"),
        ("declining", "📉", "#dc2626", "衰退中"),
        ("cyclical", "🔄", "#d97706", "周期波动"),
        ("turnaround", "🔁", "#7c3aed", "困境反转"),
        ("stable_cash_cow", "💰", "#059669", "稳定现金牛"),
        ("", "❓", "#94a3b8", "未分类"),
    ];
    let mut segment_cards = String::new();
    for (i, s) in segments.iter().enumerate() {
        let _ = i;
        let name = safe(s.get("name").unwrap_or(&Value::Null), &format!("分段{}", i + 1));
        let rev = num(s.get("latest_revenue_yi").unwrap_or(&Value::Number(0.into())));
        let share = num(s.get("latest_share_pct").unwrap_or(&Value::Number(0.into())));
        let drivers = match s.get("drivers").and_then(|v| v.as_array()) {
            Some(a) => a.clone(),
            None => Vec::new(),
        };
        let tag = s.get("thesis_tag").and_then(|v| v.as_str()).unwrap_or("");
        let bull = s.get("bull_growth_3y_cagr").filter(|v| !v.is_null()).map(num);
        let base = s.get("base_growth_3y_cagr").filter(|v| !v.is_null()).map(num);
        let bear = s.get("bear_growth_3y_cagr").filter(|v| !v.is_null()).map(num);
        let note = match s.get("agent_note") {
            Some(v) if uzi_core::py::truthy(v) => disp(v),
            _ => String::new(),
        };
        let gm = s.get("gross_margin_pct").filter(|v| !v.is_null()).map(num);
        let profit_share = s.get("profit_share_pct").filter(|v| !v.is_null()).map(num);
        let hist_rev = match s.get("revenue_history_yi").and_then(|v| v.as_array()) {
            Some(a) => a.clone(),
            None => Vec::new(),
        };
        let hist_periods: Vec<String> = match s.get("history_periods").and_then(|v| v.as_array()) {
            Some(a) => a.iter().map(disp).collect(),
            None => Vec::new(),
        };

        let (icon, color, tag_cn) = thesis_icons
            .iter()
            .find(|(k, ..)| *k == tag)
            .map(|(_, ic, c, t)| (*ic, *c, *t))
            .unwrap_or_else(|| {
                let (_, ic, c, t) = thesis_icons[thesis_icons.len() - 1];
                (ic, c, t)
            });
        let drivers_html = if !drivers.is_empty() {
            drivers
                .iter()
                .take(5)
                .map(|d| format!(r##"<span class="seg-driver">{}</span>"##, disp(d)))
                .collect::<String>()
        } else {
            r##"<span class="seg-driver muted">（agent 未填 drivers）</span>"##.to_string()
        };

        let mut margin_badges = String::new();
        if let Some(gm) = gm {
            let margin_color = if gm >= 40.0 {
                "#059669"
            } else if gm >= 20.0 {
                "#d97706"
            } else {
                "#dc2626"
            };
            margin_badges.push_str(&format!(
                r##"<span class="seg-metric" style="color:{margin_color}">毛利率 <strong>{gm:.1}%</strong></span>"##
            ));
        }
        if let Some(profit_share) = profit_share {
            if share != 0.0 {
                let delta = profit_share - share;
                if delta.abs() >= 2.0 {
                    let sign = if delta > 0.0 { "+" } else { "" };
                    let delta_color = if delta > 0.0 { "#059669" } else { "#dc2626" };
                    margin_badges.push_str(&format!(
                        r##"<span class="seg-metric" style="color:{delta_color}">利润贡献 {profit_share:.1}% <small>({sign}{delta:.1}pp vs 营收)</small></span>"##
                    ));
                }
            }
        }

        let mut sparkline_html = String::new();
        if hist_rev.len() >= 3 {
            let first: String = hist_periods
                .first()
                .map(|p| p.chars().take(7).collect())
                .unwrap_or_default();
            let last: String = hist_periods
                .last()
                .map(|p| p.chars().take(7).collect())
                .unwrap_or_default();
            sparkline_html = format!(
                r##"<div class="seg-spark"><span class="spark-lbl">{first} → {last}</span>{spark}<span class="spark-val">{val:.0}亿</span></div>"##,
                spark = svg_sparkline(&hist_rev, 160, 32, color, true),
                val = num(&hist_rev[hist_rev.len() - 1])
            );
        }

        let cagr_row = match (bull, base, bear) {
            (Some(bull), Some(base), Some(bear)) => {
                let bull_end = rev * (1.0 + bull / 100.0).powi(3);
                let base_end = rev * (1.0 + base / 100.0).powi(3);
                let bear_end = rev * (1.0 + bear / 100.0).powi(3);
                format!(
                    r##"<div class="seg-cagr"><div class="cagr-cell bull">  <span class="lbl">Bull CAGR</span>  <span class="val">{bull:+.1}%</span>  <span class="end">→ {be}</span></div><div class="cagr-cell base">  <span class="lbl">Base CAGR</span>  <span class="val">{base:+.1}%</span>  <span class="end">→ {bae}</span></div><div class="cagr-cell bear">  <span class="lbl">Bear CAGR</span>  <span class="val">{bear:+.1}%</span>  <span class="end">→ {bre}</span></div></div>"##,
                    be = group_f(bull_end, 0),
                    bae = group_f(base_end, 0),
                    bre = group_f(bear_end, 0)
                )
            }
            _ => r##"<div class="muted" style="font-size:11px">（agent 未填 3 情景 CAGR）</div>"##.to_string(),
        };

        let note_html = if note.is_empty() {
            String::new()
        } else {
            format!(r##"<div class="seg-note">💡 {note}</div>"##)
        };
        let metrics_html = if margin_badges.is_empty() {
            String::new()
        } else {
            format!(r##"<div class="seg-metrics-row">{margin_badges}</div>"##)
        };

        let color_bg = format!("{color}20");
        segment_cards.push_str(&format!(
            r##"<div class="seg-card">  <div class="seg-head">    <span class="seg-icon" style="color:{color}">{icon}</span>    <span class="seg-name">{name}</span>    <span class="seg-tag" style="background:{color_bg};color:{color}">{tag_cn}</span>    <span class="seg-share">{share:.1}%</span>  </div>  <div class="seg-rev">{currency} <strong>{rev_group}</strong> 亿</div>  {metrics_html}  {sparkline_html}  <div class="seg-drivers">{drivers_html}</div>  {cagr_row}  {note_html}</div>"##,
            rev_group = group_f(rev, 1)
        ));
    }

    let source_notes = match model.get("source_notes").and_then(|v| v.as_array()) {
        Some(a) => a.iter().map(disp).collect::<Vec<_>>(),
        None => Vec::new(),
    };
    let src_line = source_notes.join(" · ");
    let warnings = match validation.get("warnings").and_then(|v| v.as_array()) {
        Some(a) => a.iter().map(disp).collect::<Vec<_>>(),
        None => Vec::new(),
    };
    let warn_line = if warnings.is_empty() {
        String::new()
    } else {
        format!(
            r##"<div class="seg-warnings">⚠ {}</div>"##,
            warnings
                .iter()
                .take(3)
                .cloned()
                .collect::<Vec<_>>()
                .join(" · ")
        )
    };

    format!(
        r##"
<div class="segmental-section">
  <div class="seg-section-header">
    <div class="seg-section-title">
      <div class="section-tag">SEGMENTAL · 分业务建模</div>
      <h3>{name} · 分业务收入 Build-Up</h3>
    </div>
    <div class="seg-badges">{badge_html} {growth_badge} {cross_check_badge}</div>
  </div>

  <div class="seg-thesis">
    <span class="lbl">CORE THESIS</span>
    <span class="txt">{thesis}</span>
  </div>

  <div class="seg-charts-grid">
    <div class="seg-chart-cell">
      <div class="seg-chart-title">当前营收构成 · {currency} {total_rev} 亿</div>
      {donut_svg}
    </div>
    <div class="seg-chart-cell">
      <div class="seg-chart-title">历史 + 3 情景预测</div>
      {line_svg}
    </div>
  </div>

  {projection_table}

  <div class="seg-cards-grid">{segment_cards}</div>

  {warn_line}
  <div class="seg-source">数据来源 · {src_line}</div>
</div>
"##,
        name = safe(model.get("name").unwrap_or(&Value::Null), "—"),
        total_rev = group_f(total_rev, 1)
    )
}

/// 3-scenario × 3-year projection table.
pub fn render_segmental_projection_table(segments: &[Value], currency: &str) -> String {
    if segments.is_empty() {
        return String::new();
    }
    let mut rows_html = String::new();
    let mut totals_bull = [0.0_f64; 3];
    let mut totals_base = [0.0_f64; 3];
    let mut totals_bear = [0.0_f64; 3];
    for s in segments {
        let name = disp(s.get("name").unwrap_or(&Value::String(String::new())));
        let rev = num(s.get("latest_revenue_yi").unwrap_or(&Value::Number(0.into())));
        let bull = s.get("bull_growth_3y_cagr").filter(|v| !v.is_null()).map(num);
        let base = s.get("base_growth_3y_cagr").filter(|v| !v.is_null()).map(num);
        let bear = s.get("bear_growth_3y_cagr").filter(|v| !v.is_null()).map(num);
        let (bull, base, bear) = match (bull, base, bear) {
            (Some(b), Some(a), Some(c)) => (b, a, c),
            _ => continue,
        };
        let mut projections: [Vec<f64>; 3] = [Vec::new(), Vec::new(), Vec::new()];
        for (sc, cagr) in [(0usize, bull), (1, base), (2, bear)] {
            for yr in 1..=3 {
                let val = rev * (1.0 + cagr / 100.0).powi(yr);
                projections[sc].push(val);
                match sc {
                    0 => totals_bull[(yr - 1) as usize] += val,
                    1 => totals_base[(yr - 1) as usize] += val,
                    _ => totals_bear[(yr - 1) as usize] += val,
                }
            }
        }
        rows_html.push_str(&format!(
            r##"<tr><td class="seg-tbl-name">{name}</td><td class="seg-tbl-cur">{rev}</td>{cells}</tr>"##,
            rev = group_f(rev, 0),
            cells = ["bull", "base", "bear"]
                .iter()
                .enumerate()
                .flat_map(|(sc, scn)| {
                    projections[sc]
                        .iter()
                        .map(move |v| {
                            format!(
                                r##"<td class="seg-tbl-val {scn}">{}</td>"##,
                                group_f(*v, 0)
                            )
                        })
                })
                .collect::<String>()
        ));
    }

    let any_base = totals_base.iter().any(|v| *v != 0.0);
    if !any_base {
        return String::new();
    }
    let total_latest: f64 = segments
        .iter()
        .map(|s| num(s.get("latest_revenue_yi").unwrap_or(&Value::Number(0.into()))))
        .sum();
    let mut total_cells = String::new();
    for (sc, scn) in ["bull", "base", "bear"].iter().enumerate() {
        let arr = match sc {
            0 => totals_bull,
            1 => totals_base,
            _ => totals_bear,
        };
        for v in arr.iter() {
            total_cells.push_str(&format!(
                r##"<td class="seg-tbl-val {scn}"><strong>{}</strong></td>"##,
                group_f(*v, 0)
            ));
        }
    }
    let total_row = format!(
        r##"<tr class="seg-tbl-total"><td class="seg-tbl-name"><strong>合计</strong></td><td class="seg-tbl-cur"><strong>{latest}</strong></td>{total_cells}</tr>"##,
        latest = group_f(total_latest, 0)
    );

    if rows_html.is_empty() {
        return String::new();
    }

    format!(
        r##"<div class="seg-projection-table-wrap"><div class="seg-chart-title">3 情景 × 3 年营收预测 · 单位 {currency} 亿</div><table class="seg-projection-table"><thead><tr><th rowspan="2" class="seg-tbl-name">业务线</th><th rowspan="2" class="seg-tbl-cur">当前</th><th colspan="3" class="bull">Bull 🚀</th><th colspan="3" class="base">Base 📊</th><th colspan="3" class="bear">Bear 📉</th></tr><tr>{heads}</tr></thead><tbody>{rows_html}{total_row}</tbody></table></div>"##,
        heads = ["bull", "base", "bear"]
            .iter()
            .flat_map(|sc| (1..=3).map(move |y| format!(r##"<th class="{sc}">Y+{y}</th>"##)))
            .collect::<String>()
    )
}

/// Donut chart of revenue share per segment.
pub fn svg_segment_donut(segments: &[Value], _currency: &str, size: i64) -> String {
    if segments.is_empty() {
        return format!(r##"<svg width="{size}" height="{size}"></svg>"##);
    }
    let palette = [
        "#2563eb", "#d97706", "#059669", "#7c3aed", "#dc2626", "#db2777", "#64748b",
    ];
    let cx = size as f64 / 2.0;
    let cy = cx;
    let r_outer = size as f64 / 2.0 - 12.0;
    let r_inner = r_outer - 28.0;

    let total_share: f64 = segments
        .iter()
        .map(|s| num(s.get("latest_share_pct").unwrap_or(&Value::Number(0.into()))))
        .sum();
    let total_share = if total_share == 0.0 { 100.0 } else { total_share };
    let mut paths = Vec::new();
    let mut labels = Vec::new();
    let mut start_angle = -90.0_f64;
    for (i, s) in segments.iter().enumerate() {
        let share = num(s.get("latest_share_pct").unwrap_or(&Value::Number(0.into())));
        let angle = share / total_share * 360.0;
        let end_angle = start_angle + angle;
        let color = palette[i % palette.len()];
        let rad_start = start_angle.to_radians();
        let rad_end = end_angle.to_radians();
        let x1 = cx + r_outer * rad_start.cos();
        let y1 = cy + r_outer * rad_start.sin();
        let x2 = cx + r_outer * rad_end.cos();
        let y2 = cy + r_outer * rad_end.sin();
        let x3 = cx + r_inner * rad_end.cos();
        let y3 = cy + r_inner * rad_end.sin();
        let x4 = cx + r_inner * rad_start.cos();
        let y4 = cy + r_inner * rad_start.sin();
        let large = if angle > 180.0 { 1 } else { 0 };
        let (ro, ri) = (pyf(r_outer), pyf(r_inner));
        let path = format!(
            "M {x1:.1} {y1:.1} A {ro} {ro} 0 {large} 1 {x2:.1} {y2:.1} L {x3:.1} {y3:.1} A {ri} {ri} 0 {large} 0 {x4:.1} {y4:.1} Z"
        );
        paths.push(format!(
            r##"<path d="{path}" fill="{color}" opacity="0.88"><title>{} · {share:.1}%</title></path>"##,
            disp(s.get("name").unwrap_or(&Value::Null))
        ));
        if share >= 5.0 {
            let mid = ((start_angle + end_angle) / 2.0).to_radians();
            let lx = cx + (r_outer + 8.0) * mid.cos();
            let ly = cy + (r_outer + 8.0) * mid.sin();
            let anchor = if mid.cos() > 0.1 {
                "start"
            } else if mid.cos() < -0.1 {
                "end"
            } else {
                "middle"
            };
            let name: String = disp(s.get("name").unwrap_or(&Value::Null)).chars().take(8).collect();
            labels.push(format!(
                r##"<text x="{lx:.1}" y="{ly:.1}" text-anchor="{anchor}" font-size="10" fill="#475569" font-weight="600">{name}</text>"##
            ));
        }
        start_angle = end_angle;
    }

    let center_text = format!(
        r##"<text x="{cxs}" y="{y1}" text-anchor="middle" font-size="13" fill="#111" font-weight="700">{n} 条</text><text x="{cxs}" y="{y2}" text-anchor="middle" font-size="10" fill="#64748b">业务线</text>"##,
        cxs = pyf(cx),
        y1 = pyf(cx - 4.0),
        y2 = pyf(cx + 12.0),
        n = segments.len()
    );

    format!(
        r##"<svg width="{size}" height="{size}" viewBox="0 0 {size} {size}">{paths}{center_text}{labels}</svg>"##,
        paths = paths.concat(),
        labels = labels.concat()
    )
}

/// Line chart: historical total revenue + 3-scenario 3-year projection.
pub fn svg_segment_projection(segments: &[Value], rev_hist: &[Value], width: i64, height: i64) -> String {
    if segments.is_empty() || rev_hist.is_empty() {
        return format!(r##"<svg width="{width}" height="{height}"></svg>"##);
    }
    let latest_rev = num(&rev_hist[rev_hist.len() - 1]);
    let mut bull_sum = 0.0_f64;
    let mut base_sum = 0.0_f64;
    let mut bear_sum = 0.0_f64;
    for s in segments {
        let share = num(s.get("latest_share_pct").unwrap_or(&Value::Number(0.into()))) / 100.0;
        let bull_cagr = num(s.get("bull_growth_3y_cagr").unwrap_or(&Value::Number(0.into()))) / 100.0;
        let base_cagr = num(s.get("base_growth_3y_cagr").unwrap_or(&Value::Number(0.into()))) / 100.0;
        let bear_cagr = num(s.get("bear_growth_3y_cagr").unwrap_or(&Value::Number(0.into()))) / 100.0;
        bull_sum += share * (1.0 + bull_cagr).powi(3);
        base_sum += share * (1.0 + base_cagr).powi(3);
        bear_sum += share * (1.0 + bear_cagr).powi(3);
    }

    let n_hist = rev_hist.len();
    let hist_y: Vec<f64> = rev_hist.iter().map(num).collect();
    let project = |total: f64| -> Vec<f64> {
        let yr_growth = if total >= 0.0 { total.powf(1.0 / 3.0) } else { 0.0 };
        (1..=3).map(|i| latest_rev * yr_growth.powi(i)).collect()
    };
    let bull_y = project(bull_sum);
    let base_y = project(base_sum);
    let bear_y = project(bear_sum);

    let mut all_y: Vec<f64> = hist_y.clone();
    all_y.push(bull_y[2].max(base_y[2]).max(bear_y[2]));
    let mut min_pool: Vec<f64> = all_y.clone();
    min_pool.extend(hist_y.iter());
    min_pool.extend(bear_y.iter());
    let mut max_pool: Vec<f64> = all_y.clone();
    max_pool.extend(hist_y.iter());
    max_pool.extend(bull_y.iter());
    let ymin = min_pool.iter().cloned().fold(f64::INFINITY, f64::min) * 0.9;
    let ymax = max_pool.iter().cloned().fold(f64::NEG_INFINITY, f64::max) * 1.05;
    let span = (ymax - ymin).max(1e-6);

    let pad = 40.0_f64;
    let chart_w = width as f64 - pad - 20.0;
    let chart_h = height as f64 - pad - 20.0;
    let total_pts = (n_hist + 3) as f64 - 1.0;
    let sx = |i: usize| pad + i as f64 / total_pts * chart_w;
    let sy = |v: f64| pad + (1.0 - (v - ymin) / span) * chart_h;

    let hist_path = format!(
        "M {}",
        hist_y
            .iter()
            .enumerate()
            .map(|(i, y)| format!("{:.1},{:.1}", sx(i), sy(*y)))
            .collect::<Vec<_>>()
            .join(" L ")
    );
    let future_path = |y_list: &[f64]| -> String {
        let start_idx = n_hist - 1;
        let mut pts = vec![format!("{:.1},{:.1}", sx(start_idx), sy(latest_rev))];
        for (j, y) in y_list.iter().enumerate() {
            pts.push(format!("{:.1},{:.1}", sx(start_idx + j + 1), sy(*y)));
        }
        format!("M {}", pts.join(" L "))
    };
    let bull_path = future_path(&bull_y);
    let base_path = future_path(&base_y);
    let bear_path = future_path(&bear_y);

    let mut grid = String::new();
    for frac in [0.25_f64, 0.5, 0.75, 1.0] {
        let y = pad + frac * chart_h;
        let v = ymax - frac * span;
        grid.push_str(&format!(
            r##"<line x1="{pad}" y1="{y:.1}" x2="{x2}" y2="{y:.1}" stroke="#e7ecf2" stroke-width="1" stroke-dasharray="2,2"/><text x="{lx}" y="{y:.1}" text-anchor="end" font-size="9" fill="#94a3b8" dy="3">{v:.0}</text>"##,
            x2 = width as f64 - 20.0,
            lx = pad - 6.0
        ));
    }

    let mut x_labels = String::new();
    for i in 0..(n_hist + 3) {
        let lbl = if i < n_hist {
            if i < n_hist - 1 {
                format!("T-{}", n_hist - 1 - i)
            } else {
                "T".to_string()
            }
        } else {
            format!("T+{}", i - n_hist + 1)
        };
        x_labels.push_str(&format!(
            r##"<text x="{:.1}" y="{}" text-anchor="middle" font-size="9" fill="#64748b">{lbl}</text>"##,
            sx(i),
            height as f64 - 10.0
        ));
    }

    let legend = format!(
        r##"<g transform="translate({tx}, {ty})"><rect x="-4" y="-12" width="120" height="16" fill="#fff" opacity="0.8" rx="3"/><line x1="0" y1="0" x2="12" y2="0" stroke="#059669" stroke-width="2"/><text x="16" y="3" font-size="10" fill="#059669">Bull</text><line x1="40" y1="0" x2="52" y2="0" stroke="#d97706" stroke-width="2"/><text x="56" y="3" font-size="10" fill="#d97706">Base</text><line x1="80" y1="0" x2="92" y2="0" stroke="#dc2626" stroke-width="2"/><text x="96" y="3" font-size="10" fill="#dc2626">Bear</text></g>"##,
        tx = width as f64 - 120.0,
        ty = pad - 22.0
    );

    let end_labels = format!(
        r##"<text x="{x:.1}" y="{y1:.1}" font-size="10" fill="#059669" font-weight="700" dy="3">{l1:.0}</text><text x="{x:.1}" y="{y2:.1}" font-size="10" fill="#d97706" font-weight="700" dy="3">{l2:.0}</text><text x="{x:.1}" y="{y3:.1}" font-size="10" fill="#dc2626" font-weight="700" dy="3">{l3:.0}</text>"##,
        x = sx(n_hist + 2) + 4.0,
        y1 = sy(bull_y[2]),
        y2 = sy(base_y[2]),
        y3 = sy(bear_y[2]),
        l1 = bull_y[2],
        l2 = base_y[2],
        l3 = bear_y[2]
    );

    let hist_dots: String = hist_y
        .iter()
        .enumerate()
        .map(|(i, y)| {
            format!(
                r##"<circle cx="{:.1}" cy="{:.1}" r="3" fill="#64748b"/>"##,
                sx(i),
                sy(*y)
            )
        })
        .collect();

    format!(
        r##"<svg width="{width}" height="{height}" viewBox="0 0 {width} {height}">
  {grid}
  <path d="{hist_path}" stroke="#64748b" stroke-width="2" fill="none"/>
  <path d="{bull_path}" stroke="#059669" stroke-width="2.5" fill="none" stroke-dasharray="5,3"/>
  <path d="{base_path}" stroke="#d97706" stroke-width="2.5" fill="none" stroke-dasharray="5,3"/>
  <path d="{bear_path}" stroke="#dc2626" stroke-width="2.5" fill="none" stroke-dasharray="5,3"/>
  {hist_dots}
  {x_labels}
  {legend}
  {end_labels}
</svg>"##
    )
}

