//! Port of `lib/report/global_peers.py` — self-contained global peer performance
//! visualization (scatter + comparison table + percentile summary).

use crate::pyfmt::{disp, group_f};
use crate::security::html_escape;
use serde_json::{Map, Value};
use std::collections::BTreeSet;

static NULL: Value = Value::Null;

/// Python `.get(k) or next` chain over the given keys.
fn pick<'a>(v: &'a Value, keys: &[&str]) -> &'a Value {
    for k in keys {
        let x = v.get(*k).unwrap_or(&NULL);
        if uzi_core::py::truthy(x) {
            return x;
        }
    }
    &NULL
}

fn as_f64(value: &Value) -> Option<f64> {
    match value {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.trim().parse::<f64>().ok(),
        Value::Bool(b) => Some(if *b { 1.0 } else { 0.0 }),
        _ => None,
    }
}

/// `_num`: finite float or None.
fn num(value: &Value) -> Option<f64> {
    as_f64(value).filter(|n| n.is_finite())
}

/// `_esc`: escape a plain value (no unescape pass).
fn esc(value: &Value) -> String {
    if value.is_null() || matches!(value, Value::String(s) if s.is_empty()) {
        return "—".to_string();
    }
    html_escape(&disp(value))
}

fn fmt(value: &Value, suffix: &str) -> String {
    match num(value) {
        None => "—".to_string(),
        Some(n) => format!("{}{}", group_f(n, 1), suffix),
    }
}

fn fmt_amount(value: &Value) -> String {
    let n = match num(value) {
        None => return "—".to_string(),
        Some(n) => n,
    };
    let absolute = n.abs();
    for (divisor, suffix) in [(1e12_f64, "T"), (1e9, "B"), (1e6, "M"), (1e3, "K")] {
        if absolute >= divisor {
            return format!("{}{}", group_f(n / divisor, 1), suffix);
        }
    }
    group_f(n, 1)
}

/// `_latest(financials)` → (period, facts).
fn latest(financials: &Value) -> (String, Value) {
    let periods = match financials.get("periods").and_then(|v| v.as_object()) {
        Some(p) if !p.is_empty() => p,
        _ => return ("—".to_string(), Value::Object(Map::new())),
    };
    let period = periods.keys().max().cloned().unwrap_or_default();
    let facts = periods.get(&period).cloned().unwrap_or(Value::Null);
    let facts = if uzi_core::py::truthy(&facts) {
        facts
    } else {
        Value::Object(Map::new())
    };
    (period, facts)
}

fn scatter(target: &Value, peers: &[Value], base_currency: &str) -> String {
    struct Point {
        entity: Value,
        is_target: bool,
        period: String,
        revenue: f64,
        margin: f64,
    }
    let mut points: Vec<Point> = Vec::new();
    let consider = |entity: &Value, is_target: bool, points: &mut Vec<Point>| {
        let fin = entity.get("financials").cloned().unwrap_or(Value::Null);
        let (period, facts) = latest(&fin);
        let revenue = facts.get("revenue_base").and_then(num);
        let margin = facts.get("gross_margin").and_then(num);
        if let (Some(r), Some(m)) = (revenue, margin) {
            if r > 0.0 {
                points.push(Point {
                    entity: entity.clone(),
                    is_target,
                    period,
                    revenue: r,
                    margin: m,
                });
            }
        }
    };
    consider(target, true, &mut points);
    for peer in peers {
        consider(peer, false, &mut points);
    }
    if points.len() < 2 {
        return r##"<div style="color:#94a3b8;font-size:11px">规模/盈利散点数据不足</div>"##.to_string();
    }

    let width = 760.0_f64;
    let height = 310.0_f64;
    let (left, right, top, bottom) = (62.0_f64, 22.0, 22.0, 48.0);
    let plot_w = width - left - right;
    let plot_h = height - top - bottom;
    let xs: Vec<f64> = points.iter().map(|p| p.revenue.log10()).collect();
    let ys: Vec<f64> = points.iter().map(|p| p.margin).collect();
    let mut x_min = xs.iter().cloned().fold(f64::INFINITY, f64::min);
    let mut x_max = xs.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    let mut y_min = ys.iter().cloned().fold(f64::INFINITY, f64::min);
    let mut y_max = ys.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    if x_min == x_max {
        x_min -= 0.5;
        x_max += 0.5;
    }
    if y_min == y_max {
        y_min -= 5.0;
        y_max += 5.0;
    }
    let y_pad = ((y_max - y_min) * 0.12).max(2.0);
    y_min -= y_pad;
    y_max += y_pad;

    let px = |value: f64| left + (value.log10() - x_min) / (x_max - x_min) * plot_w;
    let py = |value: f64| top + (y_max - value) / (y_max - y_min) * plot_h;

    let mut grid = Vec::new();
    for idx in 0..5 {
        let y = top + idx as f64 * plot_h / 4.0;
        let value = y_max - idx as f64 * (y_max - y_min) / 4.0;
        grid.push(format!(
            r##"<line x1="{left}" y1="{y:.1}" x2="{x2}" y2="{y:.1}" stroke="#e2e8f0"/><text x="{lx}" y="{ly:.1}" text-anchor="end" font-size="10" fill="#64748b">{value:.1}%</text>"##,
            x2 = width - right,
            lx = left - 8.0,
            ly = y + 4.0
        ));
    }

    let mut dots = Vec::new();
    let mut placed_labels: Vec<(f64, f64)> = Vec::new();
    for point in &points {
        let x = px(point.revenue);
        let y = py(point.margin);
        let (color, radius) = if point.is_target {
            ("#f59e0b", 7)
        } else {
            ("#6478d3", 5)
        };
        let name = disp(pick(&point.entity, &["name", "symbol"]));
        let label = esc(&Value::String(name.chars().take(18).collect()));
        let title = esc(&Value::String(format!(
            "{} | {} | {} {} | 毛利率 {:.1}%",
            name,
            point.period,
            base_currency,
            group_f(point.revenue, 1),
            point.margin
        )));
        let near_right = x > width - right - 145.0;
        let label_x = if near_right { x - 8.0 } else { x + 8.0 };
        let anchor = if near_right { "end" } else { "start" };
        let mut label_y = y - 7.0;
        while placed_labels
            .iter()
            .any(|(ox, oy)| (label_x - ox).abs() < 165.0 && (label_y - oy).abs() < 14.0)
        {
            label_y += 14.0;
        }
        label_y = label_y.max(top + 10.0).min(height - bottom - 5.0);
        placed_labels.push((label_x, label_y));
        dots.push(format!(
            r##"<g><title>{title}</title><circle cx="{x:.1}" cy="{y:.1}" r="{radius}" fill="{color}" opacity=".9"/><text x="{lx:.1}" y="{ly:.1}" text-anchor="{anchor}" font-size="10" fill="#334155">{label}</text></g>"##,
            lx = label_x,
            ly = label_y
        ));
    }

    format!(
        r##"<div style="margin-top:12px">
  <div style="font-size:11px;color:#64748b;margin-bottom:6px">规模与盈利质量 · 横轴为 {base_currency} 营收（对数）</div>
  <svg viewBox="0 0 {w} {h}" role="img" aria-label="全球同行营收与毛利率散点图" style="width:100%;height:auto;display:block">
    {grid}
    <line x1="{left}" y1="{yb}" x2="{x2}" y2="{yb}" stroke="#94a3b8"/>
    <line x1="{left}" y1="{top}" x2="{left}" y2="{yb}" stroke="#94a3b8"/>
    {dots}
    <text x="{cx:.1}" y="{by}" text-anchor="middle" font-size="10" fill="#64748b">营收规模（对数轴）</text>
  </svg>
</div>"##,
        w = width,
        h = height,
        grid = grid.concat(),
        dots = dots.concat(),
        x2 = width - right,
        yb = height - bottom,
        cx = width / 2.0,
        by = height - 10.0,
    )
}

/// Render the global peer comparison block (empty when unavailable/disabled).
pub fn render_global_peer_comparison(comparison: &Value) -> String {
    let obj = match comparison.as_object() {
        Some(o) if !o.is_empty() => o,
        _ => return String::new(),
    };
    let status = obj
        .get("conclusion_status")
        .and_then(|v| v.as_str())
        .unwrap_or("unavailable");
    if status == "disabled" || status == "insufficient_target_profile" {
        return String::new();
    }
    if status == "unavailable" {
        return r##"<div style="margin-top:12px;color:#94a3b8;font-size:11px">全球同行数据暂不可用</div>"##.to_string();
    }

    let target = obj.get("target").cloned().unwrap_or(Value::Null);
    let peers: Vec<Value> = obj
        .get("peers")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let bc_pick = pick(comparison, &["base_currency"]);
    let base_currency_raw = if bc_pick.is_null() {
        Value::String("USD".to_string())
    } else {
        bc_pick.clone()
    };
    let base_currency = esc(&base_currency_raw);
    let scatter_currency = disp(&base_currency_raw).to_string();

    let mut markets: BTreeSet<String> = BTreeSet::new();
    for peer in &peers {
        let m = pick(peer, &["market", "country"]);
        if !m.is_null() {
            markets.insert(disp(m));
        }
    }
    let market_count = markets.len();

    let percentile = obj.get("target_percentile").cloned().unwrap_or(Value::Null);
    let gross_pct = percentile.get("gross_margin").cloned().unwrap_or(Value::Null);
    let latest_benchmark = obj
        .get("benchmarks")
        .and_then(|v| v.get("gross_margin"))
        .cloned()
        .unwrap_or(Value::Null);
    let benchmark_year = latest_benchmark
        .as_object()
        .and_then(|o| o.keys().max().cloned());
    let benchmark = match benchmark_year {
        Some(y) => latest_benchmark.get(&y).cloned().unwrap_or(Value::Null),
        None => Value::Null,
    };

    let mut rows = Vec::new();
    let mut entities: Vec<(&Value, bool)> = vec![(&target, true)];
    for peer in &peers {
        entities.push((peer, false));
    }
    for (entity, is_target) in entities {
        let fin = entity.get("financials").cloned().unwrap_or(Value::Null);
        let (period, facts) = latest(&fin);
        let style = if is_target {
            r##" style="background:#fffbeb;font-weight:700""##
        } else {
            ""
        };
        rows.push(format!(
            r##"<tr{style}>
  <td>{name}</td>
  <td>{symbol}</td>
  <td>{market}</td>
  <td>{period}</td>
  <td>{revenue}</td>
  <td>{gm}</td>
  <td>{nm}</td>
  <td>{roe}</td>
</tr>"##,
            name = esc(pick(entity, &["name", "symbol"])),
            symbol = esc(pick(entity, &["symbol", "uzi_symbol"])),
            market = esc(pick(entity, &["market", "country"])),
            period = esc(&Value::String(period)),
            revenue = fmt_amount(facts.get("revenue_base").unwrap_or(&NULL)),
            gm = fmt(facts.get("gross_margin").unwrap_or(&NULL), "%"),
            nm = fmt(facts.get("net_margin").unwrap_or(&NULL), "%"),
            roe = fmt(facts.get("roe").unwrap_or(&NULL), "%"),
        ));
    }

    let target_name = esc(pick(&target, &["name", "symbol"]));
    format!(
        r##"<style>
  .global-peer-comparison{{box-sizing:border-box;max-width:100%;min-width:0;overflow:hidden}}
  .global-peer-head>div{{min-width:0}}
  .global-peer-meta{{overflow-wrap:anywhere}}
  .global-peer-table{{max-width:100%;overflow-x:auto}}
  @media(max-width:640px){{
    .global-peer-head{{align-items:flex-start!important}}
    .global-peer-meta{{width:100%}}
    .global-peer-summary{{grid-template-columns:1fr!important}}
  }}
</style>
<div class="global-peer-comparison" style="margin-top:16px;padding-top:14px;border-top:1px solid #e2e8f0">
  <div class="global-peer-head" style="display:flex;justify-content:space-between;gap:12px;align-items:flex-end;flex-wrap:wrap">
    <div><div style="font-size:13px;font-weight:700;color:#0f172a">全球同行业绩对比</div>
    <div style="font-size:10px;color:#64748b;margin-top:2px">跨市场、跨币种标准化 · 原币数据保留</div></div>
    <div class="global-peer-meta" style="font-size:10px;color:#475569">有效同行 <strong>{npeers}</strong> · 市场 <strong>{market_count}</strong> · 基准币 {base_currency}</div>
  </div>
  <div class="global-peer-summary" style="display:grid;grid-template-columns:repeat(3,minmax(0,1fr));gap:8px;margin-top:10px">
    <div style="padding:8px;background:#f8fafc;border:1px solid #e2e8f0"><div style="font-size:9px;color:#64748b">目标公司</div><div style="font-size:12px;font-weight:700">{target_name}</div></div>
    <div style="padding:8px;background:#f8fafc;border:1px solid #e2e8f0"><div style="font-size:9px;color:#64748b">同行中位数 · 毛利率</div><div style="font-size:12px;font-weight:700">{bench_median}</div></div>
    <div style="padding:8px;background:#f8fafc;border:1px solid #e2e8f0"><div style="font-size:9px;color:#64748b">目标毛利率分位</div><div style="font-size:12px;font-weight:700">{gross_pct}</div></div>
  </div>
  {scatter}
  <div class="global-peer-table" style="overflow-x:auto;margin-top:12px"><table style="width:100%;border-collapse:collapse;font-size:10px;white-space:nowrap">
    <thead><tr><th>公司</th><th>代码</th><th>市场</th><th>报告期</th><th>营收({base_currency})</th><th>毛利率</th><th>净利率</th><th>ROE</th></tr></thead>
    <tbody>{rows}</tbody>
  </table></div>
</div>"##,
        npeers = peers.len(),
        bench_median = fmt(benchmark.get("median").unwrap_or(&NULL), "%"),
        gross_pct = fmt(&gross_pct, "%"),
        scatter = scatter(&target, &peers, &scatter_currency),
        rows = rows.concat(),
    )
}
