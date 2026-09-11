//! Port of `lib/report/svg_primitives.py` — SVG primitives + brand colors.
//!
//! Every function mirrors the upstream f-string byte for byte: attribute order,
//! indentation, and `:.Nf` formatting are preserved.

use crate::pyfmt::{disp, num, pyf};
use serde_json::Value;

pub const COLOR_BULL: &str = "#059669";
pub const COLOR_BEAR: &str = "#dc2626";
pub const COLOR_GOLD: &str = "#d97706";
pub const COLOR_CYAN: &str = "#0891b2";
pub const COLOR_BLUE: &str = "#2563eb";
pub const COLOR_PINK: &str = "#db2777";
pub const COLOR_INDIGO: &str = "#4f46e5";
pub const COLOR_MUTED: &str = "#94a3b8";
pub const COLOR_GRID: &str = "#e2e8f0";

fn nf(v: &Value) -> f64 {
    num(v)
}

/// Tiny line chart. Values normalized to fit.
pub fn svg_sparkline(
    values: &[Value],
    width: i64,
    height: i64,
    color: &str,
    fill: bool,
) -> String {
    if values.len() < 2 {
        return format!(
            r##"<svg viewBox="0 0 {width} {height}" style="display:block;width:100%;height:{height}px"></svg>"##
        );
    }
    let nums: Vec<f64> = values.iter().map(nf).collect();
    let vmin = nums.iter().cloned().fold(f64::INFINITY, f64::min);
    let vmax = nums.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    let span = (vmax - vmin).max(1e-9);
    let last = nums.len() - 1;
    let mut pts = Vec::with_capacity(nums.len());
    for (i, v) in nums.iter().enumerate() {
        let x = i as f64 / last as f64 * (width as f64 - 4.0) + 2.0;
        let y = height as f64 - 4.0 - (v - vmin) / span * (height as f64 - 8.0);
        pts.push(format!("{:.1},{:.1}", x, y));
    }
    let path = format!("M {}", pts.join(" L "));
    let fill_path = if fill {
        format!(
            r##"<path d="{path} L {},{height_m2} L 2,{height_m2} Z" fill="{color}" fill-opacity="0.12"/>"##,
            width - 2,
            height_m2 = height - 2
        )
    } else {
        String::new()
    };
    let last_pt = pts.last().unwrap();
    let (cx, cy) = last_pt.split_once(',').unwrap();
    format!(
        r##"<svg viewBox="0 0 {width} {height}" preserveAspectRatio="none" style="display:block;width:100%;height:{height}px">
  {fill_path}
  <path d="{path}" fill="none" stroke="{color}" stroke-width="2" stroke-linejoin="round" stroke-linecap="round" vector-effect="non-scaling-stroke"/>
  <circle cx="{cx}" cy="{cy}" r="3" fill="{color}"/>
</svg>"##
    )
}

/// Horizontal back-to-back bar comparing two values.
pub fn svg_h_bar_compare(
    label_a: &str,
    val_a: f64,
    label_b: &str,
    val_b: f64,
    unit: &str,
    width: i64,
) -> String {
    let _ = width;
    let max_v = val_a.abs().max(val_b.abs()).max(1.0);
    let pct_a = val_a.abs() / max_v * 100.0;
    let pct_b = val_b.abs() / max_v * 100.0;
    let color_a = if val_a >= val_b { COLOR_BULL } else { COLOR_MUTED };
    let color_b = if val_b > val_a { COLOR_BULL } else { COLOR_MUTED };
    format!(
        r##"<div style="font-family: Fira Code, monospace; font-size: 11px;">
  <div style="display:flex; justify-content:space-between; margin-bottom:4px; color:#475569;">
    <span>{label_a}</span><strong style="color:#0f172a">{va}{unit}</strong>
  </div>
  <div style="height:8px; background:#f1f5f9; border-radius:4px; overflow:hidden; margin-bottom:8px;">
    <div style="width:{pa}%; height:100%; background:{color_a}; border-radius:4px;"></div>
  </div>
  <div style="display:flex; justify-content:space-between; margin-bottom:4px; color:#475569;">
    <span>{label_b}</span><strong style="color:#0f172a">{vb}{unit}</strong>
  </div>
  <div style="height:8px; background:#f1f5f9; border-radius:4px; overflow:hidden;">
    <div style="width:{pb}%; height:100%; background:{color_b}; border-radius:4px;"></div>
  </div>
</div>"##,
        pa = pyf(pct_a),
        pb = pyf(pct_b),
        va = pyf(val_a),
        vb = pyf(val_b),
    )
}

/// Donut chart. segments = [(label, value, color), ...]
pub fn svg_donut(segments: &[(String, Value, String)], total: Option<f64>, label: &str, size: i64) -> String {
    if segments.is_empty() {
        return String::new();
    }
    let total = total.unwrap_or_else(|| segments.iter().map(|s| nf(&s.1)).sum());
    if total <= 0.0 {
        return String::new();
    }
    let cx = size as f64 / 2.0;
    let cy = cx;
    let r = size as f64 / 2.0 - 8.0;
    let inner_r = r * 0.6;
    let mut paths = Vec::new();
    let mut cur_angle = -90.0_f64;
    for (_, val, color) in segments {
        let sweep = nf(val) / total * 360.0;
        if sweep <= 0.0 {
            continue;
        }
        let end_angle = cur_angle + sweep;
        let large = if sweep > 180.0 { 1 } else { 0 };
        let x1 = cx + r * cur_angle.to_radians().cos();
        let y1 = cy + r * cur_angle.to_radians().sin();
        let x2 = cx + r * end_angle.to_radians().cos();
        let y2 = cy + r * end_angle.to_radians().sin();
        let x3 = cx + inner_r * end_angle.to_radians().cos();
        let y3 = cy + inner_r * end_angle.to_radians().sin();
        let x4 = cx + inner_r * cur_angle.to_radians().cos();
        let y4 = cy + inner_r * cur_angle.to_radians().sin();
        let d = format!(
            "M {},{} A {},{r} 0 {large} 1 {},{} L {},{} A {},{inner_r} 0 {large} 0 {},{} Z",
            pyf(x1),
            pyf(y1),
            pyf(r),
            pyf(x2),
            pyf(y2),
            pyf(x3),
            pyf(y3),
            pyf(inner_r),
            pyf(x4),
            pyf(y4)
        );
        paths.push(format!(r##"<path d="{d}" fill="{color}"/>"##));
        cur_angle = end_angle;
    }
    let legend: String = segments
        .iter()
        .map(|(l, v, c)| {
            format!(
                r##"<div style="display:flex; align-items:center; gap:6px; font-size:10px; margin-bottom:2px;"><span style="width:8px; height:8px; background:{c}; border-radius:2px"></span><span style="color:#475569">{l}</span><strong style="margin-left:auto; color:#0f172a">{v}</strong></div>"##,
                v = disp(v)
            )
        })
        .collect();
    let label_text = if !label.is_empty() {
        format!(
            r##"<text x="{cx}" y="{cy5}" text-anchor="middle" font-family="Fira Sans" font-weight="700" font-size="14" fill="#0f172a">{label}</text>"##,
            cx = pyf(cx),
            cy5 = pyf(cy + 5.0)
        )
    } else {
        String::new()
    };
    format!(
        r##"<div style="display:flex; align-items:center; gap:14px;">
  <svg width="{size}" height="{size}" viewBox="0 0 {size} {size}" style="flex-shrink:0">
    {paths}
    {label_text}
  </svg>
  <div style="flex:1; min-width:0">{legend}</div>
</div>"##,
        paths = paths.concat()
    )
}

/// Semi-circle gauge — larger, bolder.
pub fn svg_gauge(
    value: f64,
    max_val: f64,
    label: &str,
    size: f64,
    color: &str,
    unit: &str,
) -> String {
    let pct = (value / max_val).clamp(0.0, 1.0);
    let cx = size / 2.0;
    let cy = size * 0.65;
    let r = size * 0.40;
    let val_a = 180.0 - pct * 180.0;
    let (cx_s, cy_s, r_s) = (pyf(cx), pyf(cy), pyf(r));
    let bg = format!(
        r##"<path d="M {a},{cy_s} A {r_s},{r_s} 0 0 1 {b},{cy_s}" fill="none" stroke="#e2e8f0" stroke-width="14" stroke-linecap="round"/>"##,
        a = pyf(cx - r),
        b = pyf(cx + r)
    );
    let x2 = cx + r * val_a.to_radians().cos();
    let y2 = cy + r * val_a.to_radians().sin();
    let large = if pct > 0.5 { 1 } else { 0 };
    let val_arc = format!(
        r##"<path d="M {a},{cy_s} A {r_s},{r_s} 0 {large} 1 {x2},{y2}" fill="none" stroke="{color}" stroke-width="14" stroke-linecap="round"/>"##,
        a = pyf(cx - r),
        x2 = pyf(x2),
        y2 = pyf(y2)
    );
    let h = pyf(size * 0.78);
    format!(
        r##"<svg width="{size}" height="{h}" viewBox="0 0 {size} {h}">
  {bg}
  {val_arc}
  <text x="{cx_s}" y="{ty}" text-anchor="middle" font-family="Fira Sans" font-weight="900" font-size="52" fill="#0f172a" letter-spacing="-2">{val:.0}<tspan font-size="20" fill="#64748b" dx="2">{unit}</tspan></text>
  <text x="{cx_s}" y="{ly}" text-anchor="middle" font-family="Fira Sans" font-size="12" font-weight="600" fill="#475569">{label}</text>
</svg>"##,
        ty = pyf(cy - 4.0),
        ly = pyf(cy + 22.0),
        val = value
    )
}

/// 5-axis radar chart.
pub fn svg_radar(labels: &[String], values: &[f64], max_val: f64, size: f64) -> String {
    let n_axis = labels.len() as f64;
    let cx = size / 2.0;
    let cy = cx;
    let r = size * 0.38;
    let mut axes: Vec<String> = Vec::new();
    let (cx_s, cy_s) = (pyf(cx), pyf(cy));
    for (i, lbl) in labels.iter().enumerate() {
        let a = -std::f64::consts::PI / 2.0 + i as f64 * 2.0 * std::f64::consts::PI / n_axis;
        let x = cx + r * a.cos();
        let y = cy + r * a.sin();
        axes.push(format!(
            r##"<line x1="{cx_s}" y1="{cy_s}" x2="{}" y2="{}" stroke="#e2e8f0" stroke-width="1"/>"##,
            pyf(x),
            pyf(y)
        ));
        let lx = cx + (r + 12.0) * a.cos();
        let ly = cy + (r + 14.0) * a.sin();
        axes.push(format!(
            r##"<text x="{}" y="{}" text-anchor="middle" font-family="Fira Code" font-size="9" fill="#64748b">{lbl}</text>"##,
            pyf(lx),
            pyf(ly)
        ));
    }
    for ring in [0.33_f64, 0.66, 1.0] {
        let ring_r = r * ring;
        axes.push(format!(
            r##"<circle cx="{cx_s}" cy="{cy_s}" r="{}" fill="none" stroke="#f1f5f9"/>"##,
            pyf(ring_r)
        ));
    }
    let mut pts = Vec::new();
    for (i, v) in values.iter().enumerate() {
        let a = -std::f64::consts::PI / 2.0 + i as f64 * 2.0 * std::f64::consts::PI / n_axis;
        let rv = r * (v / max_val);
        let x = cx + rv * a.cos();
        let y = cy + rv * a.sin();
        pts.push(format!("{:.1},{:.1}", x, y));
    }
    let poly = format!(
        r##"<polygon points="{}" fill="{COLOR_CYAN}" fill-opacity="0.25" stroke="{COLOR_CYAN}" stroke-width="2"/>"##,
        pts.join(" ")
    );
    format!(
        r##"<svg width="{size}" height="{size}" viewBox="0 0 {size} {size}">{}{poly}</svg>"##,
        axes.concat()
    )
}

/// N LED dots, hit ones red, ok ones green.
pub fn svg_signal_lights(hit: usize, total: usize) -> String {
    let mut cells = Vec::new();
    for i in 0..total {
        let on = i < hit;
        let color = if on { COLOR_BEAR } else { COLOR_BULL };
        let opacity = if on { 1.0 } else { 0.35 };
        cells.push(format!(
            r##"<div style="width:24px;height:24px;border-radius:50%;background:{color};opacity:{opacity};box-shadow:0 0 8px {color}40;display:flex;align-items:center;justify-content:center;color:#fff;font-family:Fira Code;font-size:10px;font-weight:700">{i1}</div>"##,
            i1 = i + 1
        ));
    }
    let label = if hit > 0 { "🔴 命中信号" } else { "🟢 全部通过" };
    format!(
        r##"<div>
  <div style="display:flex;gap:6px;flex-wrap:wrap;margin-bottom:8px">{cells}</div>
  <div style="font-family:Fira Code;font-size:10px;color:#475569">{label} · {hit}/{total}</div>
</div>"##,
        cells = cells.concat()
    )
}

/// Visual upstream → company → downstream flow.
pub fn svg_supply_flow(upstream: &str, company: &str, downstream: &str) -> String {
    fn trunc(s: &str, max_len: usize) -> String {
        let s = s.trim();
        let chars: Vec<char> = s.chars().collect();
        if chars.len() > max_len {
            format!("{}…", chars[..max_len].iter().collect::<String>())
        } else {
            s.to_string()
        }
    }
    let upstream = trunc(upstream, 50);
    let company = trunc(company, 30);
    let downstream = trunc(downstream, 50);
    format!(
        r##"<div style="display:grid;grid-template-columns:1fr auto 1fr auto 1fr;gap:8px;align-items:center;font-family:Fira Sans;overflow:hidden">
  <div style="padding:10px 12px;background:#cffafe;border:1px solid #0891b2;border-radius:8px;text-align:center;overflow:hidden">
    <div style="font-size:9px;color:#0891b2;letter-spacing:.1em;margin-bottom:4px">UPSTREAM</div>
    <div style="font-size:11px;font-weight:600;color:#0f172a;line-height:1.4;word-break:break-all;overflow-wrap:break-word">{upstream}</div>
  </div>
  <div style="font-size:18px;color:#0891b2;flex-shrink:0">→</div>
  <div style="padding:10px 12px;background:#fef3c7;border:2px solid #d97706;border-radius:8px;text-align:center;overflow:hidden">
    <div style="font-size:9px;color:#d97706;letter-spacing:.1em;margin-bottom:4px">COMPANY</div>
    <div style="font-size:11px;font-weight:700;color:#0f172a;line-height:1.4">{company}</div>
  </div>
  <div style="font-size:18px;color:#0891b2;flex-shrink:0">→</div>
  <div style="padding:10px 12px;background:#d1fae5;border:1px solid #059669;border-radius:8px;text-align:center;overflow:hidden">
    <div style="font-size:9px;color:#059669;letter-spacing:.1em;margin-bottom:4px">DOWNSTREAM</div>
    <div style="font-size:11px;font-weight:600;color:#0f172a;line-height:1.4;word-break:break-all;overflow-wrap:break-word">{downstream}</div>
  </div>
</div>"##
    )
}

/// Vertical timeline of events.
pub fn svg_timeline(events: &[String]) -> String {
    if events.is_empty() {
        return String::new();
    }
    let items: String = events
        .iter()
        .map(|ev| {
            format!(
                r##"<div style="display:flex;gap:10px;padding:8px 0"><div style="width:10px;height:10px;border-radius:50%;background:{COLOR_GOLD};margin-top:4px;flex-shrink:0;box-shadow:0 0 0 3px #fef3c7"></div><div style="font-size:11px;color:#1e293b;line-height:1.5">{ev}</div></div>"##
            )
        })
        .collect();
    format!(
        r##"<div style="border-left:2px solid #e2e8f0;padding-left:12px;margin-left:5px">{items}</div>"##
    )
}

/// Vertical bar chart with optional overlay line.
#[allow(clippy::too_many_arguments)]
pub fn svg_bars(
    values: &[Value],
    labels: Option<&[String]>,
    width: i64,
    height: i64,
    color: &str,
    show_values: bool,
    overlay_line: Option<&[Value]>,
    line_color: &str,
) -> String {
    if values.is_empty() {
        return String::new();
    }
    let n = values.len();
    let (pad_l, pad_r, pad_t, pad_b) = (30.0_f64, 10.0, 14.0, 24.0);
    let chart_w = width as f64 - pad_l - pad_r;
    let chart_h = height as f64 - pad_t - pad_b;

    let mut all: Vec<f64> = values.iter().map(nf).collect();
    if let Some(ov) = overlay_line {
        all.extend(ov.iter().map(nf));
    }
    all.push(0.0);
    let max_v = all.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    let min_v = all.iter().cloned().fold(f64::INFINITY, f64::min);
    let span = (max_v - min_v).max(1e-9);
    let bar_w = chart_w / n as f64 * 0.7;
    let gap = chart_w / n as f64 * 0.3;

    let mut bars = Vec::new();
    let mut vals_txt = Vec::new();
    let mut labels_txt = Vec::new();
    for (i, v) in values.iter().enumerate() {
        let vf = nf(v);
        let x = pad_l + i as f64 * (chart_w / n as f64) + gap / 2.0;
        let bar_h = (vf - min_v) / span * chart_h;
        let y = pad_t + chart_h - bar_h;
        bars.push(format!(
            r##"<rect x="{:.1}" y="{:.1}" width="{:.1}" height="{:.1}" fill="{color}" rx="2"/>"##,
            x, y, bar_w, bar_h
        ));
        if show_values {
            vals_txt.push(format!(
                r##"<text x="{:.1}" y="{:.1}" text-anchor="middle" font-family="Fira Code" font-size="9" fill="#0f172a" font-weight="700">{}</text>"##,
                x + bar_w / 2.0,
                y - 4.0,
                disp(v)
            ));
        }
        if let Some(lbls) = labels {
            let l = lbls.get(i).cloned().unwrap_or_default();
            labels_txt.push(format!(
                r##"<text x="{:.1}" y="{}" text-anchor="middle" font-family="Fira Code" font-size="9" fill="#64748b">{l}</text>"##,
                x + bar_w / 2.0,
                pad_t + chart_h + 14.0
            ));
        }
    }

    let y_zero = pad_t + chart_h - (0.0 - min_v) / span * chart_h;
    let axis = format!(
        r##"<line x1="{pad_l}" y1="{y_zero:.1}" x2="{}" y2="{y_zero:.1}" stroke="#cbd5e1" stroke-width="1"/>"##,
        pad_l + chart_w
    );

    let mut line_path = String::new();
    let mut line_dots = String::new();
    if let Some(ov) = overlay_line {
        if ov.len() == n {
            let mut pts = Vec::new();
            for (i, v) in ov.iter().enumerate() {
                let x = pad_l + i as f64 * (chart_w / n as f64) + chart_w / n as f64 / 2.0;
                let y = pad_t + chart_h - (nf(v) - min_v) / span * chart_h;
                pts.push((x, y));
            }
            let path_str = format!(
                "M {}",
                pts.iter()
                    .map(|(x, y)| format!("{:.1},{:.1}", x, y))
                    .collect::<Vec<_>>()
                    .join(" L ")
            );
            line_path = format!(
                r##"<path d="{path_str}" fill="none" stroke="{line_color}" stroke-width="2.5"/>"##
            );
            line_dots = pts
                .iter()
                .map(|(x, y)| {
                    format!(
                        r##"<circle cx="{:.1}" cy="{:.1}" r="3" fill="{line_color}"/>"##,
                        x, y
                    )
                })
                .collect();
        }
    }

    format!(
        r##"<svg width="{width}" height="{height}" viewBox="0 0 {width} {height}">
  {axis}
  {bars}
  {line_path}
  {line_dots}
  {vals}
  {lbls}
</svg>"##,
        bars = bars.concat(),
        vals = vals_txt.concat(),
        lbls = labels_txt.concat()
    )
}

/// Hand-rolled SVG candlestick. candles = [{open, close, high, low, date}, ...]
pub fn svg_candlestick(
    candles: &[Value],
    width: i64,
    height: i64,
    ma_20: Option<&[Value]>,
    ma_60: Option<&[Value]>,
) -> String {
    if candles.is_empty() {
        return String::new();
    }
    let n = candles.len();
    let (pad_l, pad_r, pad_t, pad_b) = (40.0_f64, 10.0, 10.0, 24.0);
    let chart_w = width as f64 - pad_l - pad_r;
    let chart_h = height as f64 - pad_t - pad_b;
    let mut all_highs: Vec<f64> = candles.iter().map(|c| nf(&c["high"])).collect();
    let mut all_lows: Vec<f64> = candles.iter().map(|c| nf(&c["low"])).collect();
    if let Some(ma) = ma_20 {
        for v in ma {
            if !v.is_null() {
                all_highs.push(nf(v));
                all_lows.push(nf(v));
            }
        }
    }
    if let Some(ma) = ma_60 {
        for v in ma {
            if !v.is_null() {
                all_highs.push(nf(v));
                all_lows.push(nf(v));
            }
        }
    }
    let y_max = all_highs.iter().cloned().fold(f64::NEG_INFINITY, f64::max) * 1.02;
    let y_min = all_lows.iter().cloned().fold(f64::INFINITY, f64::min) * 0.98;
    let span = (y_max - y_min).max(1e-9);

    let y_of = |v: f64| pad_t + chart_h - (v - y_min) / span * chart_h;

    let cw = chart_w / n as f64 * 0.7;
    let gap = chart_w / n as f64 * 0.3;

    let mut elems: Vec<String> = Vec::new();
    for ring in [0.25_f64, 0.5, 0.75] {
        let yg = pad_t + chart_h * ring;
        elems.push(format!(
            r##"<line x1="{pad_l}" y1="{yg:.1}" x2="{}" y2="{yg:.1}" stroke="#f1f5f9" stroke-width="1"/>"##,
            pad_l + chart_w
        ));
    }
    for (frac, v) in [(0.0_f64, y_max), (0.5, (y_max + y_min) / 2.0), (1.0, y_min)] {
        let yt = pad_t + chart_h * frac;
        elems.push(format!(
            r##"<text x="{}" y="{:.1}" text-anchor="end" font-family="Fira Code" font-size="9" fill="#64748b">{v:.1}</text>"##,
            pad_l - 5.0,
            yt + 3.0
        ));
    }
    for (i, c) in candles.iter().enumerate() {
        let x = pad_l + i as f64 * (chart_w / n as f64) + gap / 2.0;
        let cx = x + cw / 2.0;
        let op = nf(&c["open"]);
        let cl = nf(&c["close"]);
        let hi = nf(&c["high"]);
        let lo = nf(&c["low"]);
        let is_up = cl >= op;
        let color = if is_up { COLOR_BEAR } else { COLOR_BULL };
        elems.push(format!(
            r##"<line x1="{cx:.1}" y1="{:.1}" x2="{cx:.1}" y2="{:.1}" stroke="{color}" stroke-width="1"/>"##,
            y_of(hi),
            y_of(lo)
        ));
        let top = y_of(op.max(cl));
        let bh = (y_of(cl) - y_of(op)).abs().max(1.0);
        elems.push(format!(
            r##"<rect x="{x:.1}" y="{top:.1}" width="{cw:.1}" height="{bh:.1}" fill="{color}" stroke="{color}" stroke-width="1"/>"##
        ));
    }

    fn ma_path(vals: Option<&[Value]>, color: &str, pad_l: f64, chart_w: f64, n: usize, cw: f64, gap: f64, y_of: &dyn Fn(f64) -> f64) -> String {
        let vals = match vals {
            Some(v) => v,
            None => return String::new(),
        };
        if vals.is_empty() {
            return String::new();
        }
        let mut pts = Vec::new();
        for (i, v) in vals.iter().enumerate() {
            if v.is_null() {
                continue;
            }
            let x = pad_l + i as f64 * (chart_w / n as f64) + cw / 2.0 + gap / 2.0;
            let y = y_of(crate::pyfmt::num(v));
            pts.push(format!("{:.1},{:.1}", x, y));
        }
        if pts.is_empty() {
            return String::new();
        }
        format!(
            r##"<polyline points="{}" fill="none" stroke="{color}" stroke-width="1.5" stroke-linejoin="round"/>"##,
            pts.join(" ")
        )
    }

    elems.push(ma_path(ma_20, COLOR_GOLD, pad_l, chart_w, n, cw, gap, &y_of));
    elems.push(ma_path(ma_60, COLOR_INDIGO, pad_l, chart_w, n, cw, gap, &y_of));

    if !candles[0].get("date").is_none() {
        for i in [0usize, n / 2, n - 1] {
            let x = pad_l + i as f64 * (chart_w / n as f64) + cw / 2.0;
            let date = candles[i].get("date").map(disp).unwrap_or_default();
            let d: String = date.chars().rev().take(5).collect::<Vec<_>>().into_iter().rev().collect();
            elems.push(format!(
                r##"<text x="{x:.1}" y="{}" text-anchor="middle" font-family="Fira Code" font-size="8" fill="#64748b">{d}</text>"##,
                pad_t + chart_h + 14.0
            ));
        }
    }

    format!(
        r##"<svg width="{width}" height="{height}" viewBox="0 0 {width} {height}" style="width:100%">
  {}
</svg>
<div style="display:flex;gap:14px;margin-top:6px;font-family:Fira Code;font-size:9px">
  <span><span style="display:inline-block;width:12px;height:2px;background:{COLOR_GOLD};vertical-align:middle"></span> MA20</span>
  <span><span style="display:inline-block;width:12px;height:2px;background:{COLOR_INDIGO};vertical-align:middle"></span> MA60</span>
</div>"##,
        elems.concat()
    )
}

/// PE historical line with percentile bands.
pub fn svg_pe_band(pe_history: &[Value], width: i64, height: i64) -> String {
    if pe_history.len() < 2 {
        return String::new();
    }
    let nums: Vec<f64> = pe_history.iter().map(nf).collect();
    let n = nums.len();
    let (pad_l, pad_r, pad_t, pad_b) = (36.0_f64, 10.0, 10.0, 20.0);
    let w = width as f64 - pad_l - pad_r;
    let h = height as f64 - pad_t - pad_b;

    let mut sorted_pe = nums.clone();
    sorted_pe.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let p25 = sorted_pe[(n as f64 * 0.25) as usize];
    let p50 = sorted_pe[(n as f64 * 0.5) as usize];
    let p75 = sorted_pe[(n as f64 * 0.75) as usize];
    let y_max = nums.iter().cloned().fold(f64::NEG_INFINITY, f64::max) * 1.05;
    let y_min = nums.iter().cloned().fold(f64::INFINITY, f64::min) * 0.95;
    let span = (y_max - y_min).max(1e-9);
    let y_of = |v: f64| pad_t + h - (v - y_min) / span * h;

    let y25 = y_of(p25);
    let y50 = y_of(p50);
    let y75 = y_of(p75);
    let bands_svg = format!(
        r##"
  <rect x="{pad_l}" y="{pad_t}" width="{w}" height="{h1:.1}" fill="#fee2e2" opacity="0.5"/>
  <rect x="{pad_l}" y="{y75:.1}" width="{w}" height="{h2:.1}" fill="#fef3c7" opacity="0.5"/>
  <rect x="{pad_l}" y="{y25:.1}" width="{w}" height="{h3:.1}" fill="#d1fae5" opacity="0.5"/>
  <line x1="{pad_l}" y1="{y25:.1}" x2="{x2}" y2="{y25:.1}" stroke="#059669" stroke-width="1" stroke-dasharray="3,3"/>
  <line x1="{pad_l}" y1="{y50:.1}" x2="{x2}" y2="{y50:.1}" stroke="#64748b" stroke-width="1" stroke-dasharray="3,3"/>
  <line x1="{pad_l}" y1="{y75:.1}" x2="{x2}" y2="{y75:.1}" stroke="#dc2626" stroke-width="1" stroke-dasharray="3,3"/>
  <text x="{x3}" y="{y25_3:.1}" text-anchor="end" font-family="Fira Code" font-size="8" fill="#059669">25%</text>
  <text x="{x3}" y="{y50_3:.1}" text-anchor="end" font-family="Fira Code" font-size="8" fill="#64748b">50%</text>
  <text x="{x3}" y="{y75_3:.1}" text-anchor="end" font-family="Fira Code" font-size="8" fill="#dc2626">75%</text>
    "##,
        h1 = y75 - pad_t,
        h2 = y25 - y75,
        h3 = pad_t + h - y25,
        x2 = pad_l + w,
        x3 = pad_l - 3.0,
        y25_3 = y25 + 3.0,
        y50_3 = y50 + 3.0,
        y75_3 = y75 + 3.0,
    );

    let mut pts = Vec::new();
    for (i, v) in nums.iter().enumerate() {
        let x = pad_l + i as f64 / (n as f64 - 1.0) * w;
        let y = y_of(*v);
        pts.push(format!("{:.1},{:.1}", x, y));
    }
    let line = format!(
        r##"<polyline points="{}" fill="none" stroke="{COLOR_BLUE}" stroke-width="2"/>"##,
        pts.join(" ")
    );
    let last_x = pad_l + w;
    let last_y = y_of(nums[n - 1]);
    let current = format!(
        r##"<circle cx="{last_x:.1}" cy="{last_y:.1}" r="5" fill="{COLOR_BLUE}" stroke="#fff" stroke-width="2"/>"##
    );
    let cur_label = format!(
        r##"<text x="{last_x:.1}" y="{:.1}" text-anchor="end" font-family="Fira Code" font-size="10" font-weight="700" fill="{COLOR_BLUE}">{:.1}</text>"##,
        last_y - 10.0,
        nums[n - 1]
    );

    format!(
        r##"<svg width="{width}" height="{height}" viewBox="0 0 {width} {height}" style="width:100%">
  {bands_svg}
  {line}
  {current}
  {cur_label}
</svg>"##
    )
}

/// Inline labeled progress bar.
pub fn svg_progress_row(label: &str, pct: f64, color: &str, suffix: &str) -> String {
    let pct_clamped = pct.clamp(0.0, 100.0);
    format!(
        r##"<div style="display:flex;align-items:center;gap:10px;margin:6px 0">
  <div style="width:70px;font-family:Fira Code;font-size:10px;color:#64748b">{label}</div>
  <div style="flex:1;height:8px;background:#f1f5f9;border-radius:4px;overflow:hidden">
    <div style="width:{pct_clamped}%;height:100%;background:{color};border-radius:4px"></div>
  </div>
  <div style="min-width:50px;text-align:right;font-family:Fira Code;font-size:11px;color:#0f172a;font-weight:700">{pct:.1}{suffix}</div>
</div>"##
    )
}

/// HTML comparison table.
pub fn svg_peer_table(rows: &[Value]) -> String {
    if rows.is_empty() {
        return String::new();
    }
    let head = r##"<tr style="background:#f8fafc">
  <th style="text-align:left;padding:8px 10px;font-family:Fira Code;font-size:9px;color:#64748b;font-weight:700;border-bottom:2px solid #e2e8f0">公司</th>
  <th style="text-align:right;padding:8px 10px;font-family:Fira Code;font-size:9px;color:#64748b;font-weight:700;border-bottom:2px solid #e2e8f0">PE</th>
  <th style="text-align:right;padding:8px 10px;font-family:Fira Code;font-size:9px;color:#64748b;font-weight:700;border-bottom:2px solid #e2e8f0">PB</th>
  <th style="text-align:right;padding:8px 10px;font-family:Fira Code;font-size:9px;color:#64748b;font-weight:700;border-bottom:2px solid #e2e8f0">ROE</th>
  <th style="text-align:right;padding:8px 10px;font-family:Fira Code;font-size:9px;color:#64748b;font-weight:700;border-bottom:2px solid #e2e8f0">营收增速</th>
</tr>"##;
    let mut body = String::new();
    for r in rows {
        let is_self = uzi_core::py::truthy(r.get("is_self").unwrap_or(&Value::Bool(false)));
        let row_style = if is_self {
            "background:#fef3c7;font-weight:700"
        } else {
            "background:#ffffff"
        };
        let star = if is_self { "⭐ " } else { "" };
        let getv = |k: &str| -> String {
            r.get(k)
                .filter(|v| !v.is_null())
                .map(disp)
                .unwrap_or_else(|| "—".to_string())
        };
        body.push_str(&format!(
            r##"<tr style="{row_style}">
  <td style="padding:8px 10px;font-family:Fira Sans;font-size:12px;color:#0f172a;border-bottom:1px solid #f1f5f9">{star}{name}</td>
  <td style="text-align:right;padding:8px 10px;font-family:Fira Code;font-size:11px;color:#0f172a;border-bottom:1px solid #f1f5f9">{pe}</td>
  <td style="text-align:right;padding:8px 10px;font-family:Fira Code;font-size:11px;color:#0f172a;border-bottom:1px solid #f1f5f9">{pb}</td>
  <td style="text-align:right;padding:8px 10px;font-family:Fira Code;font-size:11px;color:#0f172a;border-bottom:1px solid #f1f5f9">{roe}</td>
  <td style="text-align:right;padding:8px 10px;font-family:Fira Code;font-size:11px;color:#0f172a;border-bottom:1px solid #f1f5f9">{rg}</td>
</tr>"##,
            name = getv("name"),
            pe = getv("pe"),
            pb = getv("pb"),
            roe = getv("roe"),
            rg = getv("revenue_growth"),
        ));
    }
    format!(
        r##"<table style="width:100%;border-collapse:collapse;font-family:Fira Sans">{head}{body}</table>"##
    )
}

/// Future unlock timeline: list of {date, amount}.
pub fn svg_unlock_timeline(unlocks: &[Value], width: i64, height: i64) -> String {
    if unlocks.is_empty() {
        return r##"<div style="text-align:center;color:#94a3b8;font-size:11px;padding:10px">未来 12 个月无解禁</div>"##.to_string();
    }
    let n = unlocks.len();
    let (pad_l, pad_r, pad_t, pad_b) = (20.0_f64, 10.0, 16.0, 24.0);
    let w = width as f64 - pad_l - pad_r;
    let h = height as f64 - pad_t - pad_b;
    let max_a = unlocks
        .iter()
        .map(|u| nf(u.get("amount").unwrap_or(&Value::Number(0.into()))))
        .fold(f64::NEG_INFINITY, f64::max);
    let max_a = if max_a == 0.0 { 1.0 } else { max_a };
    let bar_w = w / n as f64 * 0.6;
    let gap = w / n as f64 * 0.4;
    let mut bars = Vec::new();
    for (i, u) in unlocks.iter().enumerate() {
        let amt_v = u.get("amount").unwrap_or(&Value::Number(0.into())).clone();
        let amt = nf(&amt_v);
        let date = u.get("date").map(disp).unwrap_or_default();
        let x = pad_l + i as f64 * (w / n as f64) + gap / 2.0;
        let bar_h = amt / max_a * h;
        let y = pad_t + h - bar_h;
        let color = if amt > max_a * 0.5 { COLOR_BEAR } else { COLOR_GOLD };
        bars.push(format!(
            r##"<rect x="{x:.1}" y="{y:.1}" width="{bar_w:.1}" height="{bar_h:.1}" fill="{color}" rx="2"/>"##
        ));
        bars.push(format!(
            r##"<text x="{:.1}" y="{:.1}" text-anchor="middle" font-family="Fira Code" font-size="9" fill="#0f172a" font-weight="700">{}</text>"##,
            x + bar_w / 2.0,
            y - 3.0,
            disp(&amt_v)
        ));
        bars.push(format!(
            r##"<text x="{:.1}" y="{}" text-anchor="middle" font-family="Fira Code" font-size="8" fill="#64748b">{date}</text>"##,
            x + bar_w / 2.0,
            pad_t + h + 14.0
        ));
    }
    let axis = format!(
        r##"<line x1="{pad_l}" y1="{}" x2="{}" y2="{}" stroke="#cbd5e1"/>"##,
        pad_t + h,
        pad_l + w,
        pad_t + h
    );
    format!(
        r##"<svg width="{width}" height="{height}" viewBox="0 0 {width} {height}" style="width:100%">{axis}{}</svg>"##,
        bars.concat()
    )
}

/// Dividend history: bars for amount + line for yield.
pub fn svg_dividend_combo(
    years: &[Value],
    amounts: &[Value],
    yields: &[Value],
    width: i64,
    height: i64,
) -> String {
    if years.is_empty() || amounts.is_empty() {
        return String::new();
    }
    let n = years.len();
    let (pad_l, pad_r, pad_t, pad_b) = (36.0_f64, 40.0, 14.0, 24.0);
    let w = width as f64 - pad_l - pad_r;
    let h = height as f64 - pad_t - pad_b;
    let max_a = amounts.iter().map(nf).fold(f64::NEG_INFINITY, f64::max);
    let max_a = if max_a == 0.0 { 1.0 } else { max_a };
    let max_y = if yields.is_empty() {
        5.0
    } else {
        yields.iter().map(nf).fold(f64::NEG_INFINITY, f64::max)
    };
    let bar_w = w / n as f64 * 0.55;
    let gap = w / n as f64 * 0.45;

    let mut bars = Vec::new();
    for (i, a) in amounts.iter().enumerate() {
        let af = nf(a);
        let x = pad_l + i as f64 * (w / n as f64) + gap / 2.0;
        let bar_h = af / max_a * h;
        let y = pad_t + h - bar_h;
        bars.push(format!(
            r##"<rect x="{x:.1}" y="{y:.1}" width="{bar_w:.1}" height="{bar_h:.1}" fill="{COLOR_CYAN}" rx="2"/>"##
        ));
        bars.push(format!(
            r##"<text x="{:.1}" y="{:.1}" text-anchor="middle" font-family="Fira Code" font-size="9" fill="#0f172a" font-weight="700">{}</text>"##,
            x + bar_w / 2.0,
            y - 3.0,
            disp(a)
        ));
        let year = years.get(i).map(disp).unwrap_or_default();
        bars.push(format!(
            r##"<text x="{:.1}" y="{}" text-anchor="middle" font-family="Fira Code" font-size="9" fill="#64748b">{year}</text>"##,
            x + bar_w / 2.0,
            pad_t + h + 14.0
        ));
    }

    if !yields.is_empty() {
        let mut pts = Vec::new();
        for (i, y) in yields.iter().enumerate() {
            let x = pad_l + i as f64 * (w / n as f64) + w / n as f64 / 2.0;
            let yy = pad_t + h - nf(y) / max_y * h;
            pts.push((x, yy));
        }
        let line = format!(
            r##"<polyline points="{}" fill="none" stroke="{COLOR_GOLD}" stroke-width="2.5"/>"##,
            pts.iter()
                .map(|(x, y)| format!("{:.1},{:.1}", x, y))
                .collect::<Vec<_>>()
                .join(" ")
        );
        let dots: String = pts
            .iter()
            .map(|(x, y)| {
                format!(
                    r##"<circle cx="{:.1}" cy="{:.1}" r="3" fill="{COLOR_GOLD}"/>"##,
                    x, y
                )
            })
            .collect();
        bars.push(line);
        bars.push(dots);
        bars.push(format!(
            r##"<text x="{}" y="{}" font-family="Fira Code" font-size="9" fill="{COLOR_GOLD}">{max_y:.1}%</text>"##,
            pad_l + w + 4.0,
            pad_t + 10.0
        ));
        bars.push(format!(
            r##"<text x="{}" y="{}" font-family="Fira Code" font-size="9" fill="{COLOR_GOLD}">0%</text>"##,
            pad_l + w + 4.0,
            pad_t + h
        ));
    }

    format!(
        r##"<svg width="{width}" height="{height}" viewBox="0 0 {width} {height}" style="width:100%">{}</svg>"##,
        bars.concat()
    )
}

/// Stacked/grouped bar of institutional holdings over quarters.
pub fn svg_institutional_quarters(data: &Value, width: i64, height: i64) -> String {
    let quarters = data.get("quarters").and_then(|v| v.as_array()).cloned().unwrap_or_default();
    if quarters.is_empty() {
        return String::new();
    }
    let series: Vec<(&str, Vec<Value>, &str)> = vec![
        ("公募", data.get("fund").and_then(|v| v.as_array()).cloned().unwrap_or_default(), COLOR_CYAN),
        ("QFII", data.get("qfii").and_then(|v| v.as_array()).cloned().unwrap_or_default(), COLOR_BLUE),
        ("社保", data.get("shehui").and_then(|v| v.as_array()).cloned().unwrap_or_default(), COLOR_GOLD),
    ];
    let n = quarters.len();
    let (pad_l, pad_r, pad_t, pad_b) = (10.0_f64, 10.0, 16.0, 22.0);
    let w = width as f64 - pad_l - pad_r;
    let h = height as f64 - pad_t - pad_b;
    let mut all_vals: Vec<f64> = series
        .iter()
        .flat_map(|(_, vals, _)| vals.iter().filter(|v| !v.is_null()).map(nf))
        .collect();
    all_vals.push(0.0);
    let max_v = all_vals.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    let max_v = if max_v == 0.0 { 1.0 } else { max_v };
    let bar_w = w / n as f64 * 0.28;
    let group_gap = w / n as f64 * 0.16;

    let mut elems = Vec::new();
    for i in 0..n {
        let bx = pad_l + i as f64 * (w / n as f64) + group_gap / 2.0;
        for (si, (_, vals, col)) in series.iter().enumerate() {
            if i >= vals.len() {
                continue;
            }
            let v = nf(&vals[i]);
            let bar_h = v / max_v * h;
            let x = bx + si as f64 * bar_w;
            let y = pad_t + h - bar_h;
            elems.push(format!(
                r##"<rect x="{x:.1}" y="{y:.1}" width="{:.1}" height="{bar_h:.1}" fill="{col}" rx="1"/>"##,
                bar_w - 0.5
            ));
        }
        let q = disp(&quarters[i]);
        elems.push(format!(
            r##"<text x="{:.1}" y="{}" text-anchor="middle" font-family="Fira Code" font-size="9" fill="#64748b">{q}</text>"##,
            bx + 1.5 * bar_w,
            pad_t + h + 14.0
        ));
    }
    let legend = format!(
        r##"<div style="display:flex;gap:10px;margin-top:4px;font-family:Fira Code;font-size:9px">
  <span style="color:{COLOR_CYAN}">■ 公募</span>
  <span style="color:{COLOR_BLUE}">■ QFII</span>
  <span style="color:{COLOR_GOLD}">■ 社保</span>
</div>"##
    );
    format!(
        r##"<svg width="{width}" height="{height}" viewBox="0 0 {width} {height}" style="width:100%">{}</svg>{legend}"##,
        elems.concat()
    )
}

/// Heat thermometer (vertical).
pub fn svg_thermometer(value: i64, max_val: i64, label: &str) -> String {
    let pct = (value as f64 / max_val as f64 * 100.0).clamp(0.0, 100.0);
    let color = if value > 80 {
        COLOR_BEAR
    } else if value > 50 {
        COLOR_GOLD
    } else {
        COLOR_BULL
    };
    format!(
        r##"<div style="display:flex;align-items:center;gap:14px">
  <div style="width:24px;height:120px;background:#f1f5f9;border:1px solid #cbd5e1;border-radius:12px;position:relative;overflow:hidden">
    <div style="position:absolute;bottom:0;left:0;right:0;height:{pct_s}%;background:linear-gradient(0deg,{color},{color}cc);border-radius:0 0 12px 12px;transition:height 1s"></div>
  </div>
  <div>
    <div style="font-family:Fira Sans;font-weight:900;font-size:32px;color:{color};line-height:1">{value}</div>
    <div style="font-family:Fira Code;font-size:9px;color:#64748b;letter-spacing:.1em">{label}</div>
  </div>
</div>"##,
        pct_s = pyf(pct)
    )
}
