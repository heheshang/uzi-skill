//! Port of `lib/report/dim_viz.py` — per-dimension specialized visualizations
//! (`_viz_xxx`) plus the `DIM_VIZ_RENDERERS` dispatch table.

use crate::global_peers::render_global_peer_comparison;
use crate::pyfmt::{disp, num, pyf};
use crate::svg::*;
use serde_json::{Map, Value};

/// Local `_safe` helper (no `nan` placeholder here).
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

/// Score → CSS class.
pub fn score_class(score: &Value) -> &'static str {
    if score.is_null() {
        return "na";
    }
    let s = num(score);
    if s >= 7.0 {
        "high"
    } else if s >= 4.0 {
        "mid"
    } else {
        "low"
    }
}

fn alist(v: &Value, k: &str) -> Vec<Value> {
    match v.get(k) {
        Some(Value::Array(a)) => a.clone(),
        _ => Vec::new(),
    }
}

fn first_int(s: &str) -> Option<i64> {
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() && !bytes[i].is_ascii_digit() {
        i += 1;
    }
    if i >= bytes.len() {
        return None;
    }
    let start = i;
    while i < bytes.len() && bytes[i].is_ascii_digit() {
        i += 1;
    }
    s[start..i].parse::<i64>().ok()
}

fn first_num(s: &str) -> f64 {
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len()
        && !(bytes[i].is_ascii_digit() || bytes[i] == b'+' || bytes[i] == b'-')
    {
        i += 1;
    }
    if i >= bytes.len() {
        return 0.0;
    }
    let start = i;
    if bytes[i] == b'+' || bytes[i] == b'-' {
        i += 1;
    }
    while i < bytes.len() && bytes[i].is_ascii_digit() {
        i += 1;
    }
    if i < bytes.len() && bytes[i] == b'.' {
        i += 1;
        while i < bytes.len() && bytes[i].is_ascii_digit() {
            i += 1;
        }
    }
    s[start..i].parse::<f64>().unwrap_or(0.0)
}

fn is_num(v: &Value) -> bool {
    matches!(v, Value::Number(_))
}

// ─── 维度专属可视化 dispatch ───

pub fn viz_chain(raw: &Value) -> String {
    let upstream = disp(raw.get("upstream").unwrap_or(&Value::String("—".into())));
    let downstream = disp(raw.get("downstream").unwrap_or(&Value::String("—".into())));
    let client_conc = disp(raw.get("client_concentration").unwrap_or(&Value::String("".into())));
    let supplier_conc = disp(raw.get("supplier_concentration").unwrap_or(&Value::String("".into())));
    let flow = svg_supply_flow(&upstream, "本公司", &downstream);

    let mut extras = String::new();
    if (raw.get("client_concentration").map(uzi_core::py::truthy).unwrap_or(false))
        || (raw.get("supplier_concentration").map(uzi_core::py::truthy).unwrap_or(false))
    {
        extras = format!(
            r##"<div style="display:flex;justify-content:space-around;margin-top:10px;padding:10px;background:#ffffff;border:1px solid #e7ecf2;border-radius:6px;font-family:Fira Code;font-size:11px;color:#475569">
  <span>🔧 供应商 <strong style="color:#0f172a">{supplier_conc}</strong></span>
  <span>🎯 大客户 <strong style="color:#0f172a">{client_conc}</strong></span>
</div>"##
        );
    }

    let main_biz = alist(raw, "main_business_breakdown");
    let mut pie = String::new();
    if !main_biz.is_empty() {
        let colors = [COLOR_CYAN, COLOR_BLUE, COLOR_GOLD, COLOR_BULL, COLOR_INDIGO, COLOR_PINK];
        let segments: Vec<(String, Value, String)> = main_biz
            .iter()
            .take(6)
            .enumerate()
            .filter_map(|(i, item)| {
                if !item.is_object() {
                    return None;
                }
                let name = disp(item.get("name").unwrap_or(&Value::String(String::new())));
                let value = item.get("value").cloned().unwrap_or(Value::Number(0.into()));
                Some((name, value, colors[i % colors.len()].to_string()))
            })
            .collect();
        if !segments.is_empty() {
            pie.push_str(r##"<div style="margin-top:12px;padding-top:10px;border-top:1px solid #e7ecf2">"##);
            pie.push_str(r##"<div style="font-family:Fira Code;font-size:10px;color:#64748b;margin-bottom:8px">🥧 主营业务构成</div>"##);
            pie.push_str(&svg_donut(&segments, None, "主营", 120));
            pie.push_str("</div>");
        }
    }

    format!("{flow}{extras}{pie}")
}

pub fn viz_trap(raw: &Value) -> String {
    let hit_str = disp(raw.get("signals_hit").unwrap_or(&Value::String("0/8".into())));
    let hit = first_int(&hit_str).unwrap_or(0);
    let level = disp(raw.get("trap_level").unwrap_or(&Value::String("🟢 安全".into())));
    let lights = svg_signal_lights(hit as usize, 8);
    format!(
        r##"{lights}<div style="margin-top:10px;font-family:Fira Sans;font-size:14px;font-weight:700;color:#0f172a">{level}</div>"##
    )
}

pub fn viz_valuation(raw: &Value) -> String {
    let q_str = disp(raw.get("pe_quantile").unwrap_or(&Value::String(String::new())));
    let val = first_int(&q_str).unwrap_or(50) as f64;
    let color = if val < 30.0 {
        COLOR_BULL
    } else if val < 70.0 {
        COLOR_GOLD
    } else {
        COLOR_BEAR
    };
    let pe = disp(raw.get("pe").unwrap_or(&Value::String("—".into())));
    let industry_pe = disp(raw.get("industry_pe").unwrap_or(&Value::String("—".into())));
    let dcf = disp(raw.get("dcf").unwrap_or(&Value::String("—".into())));

    let mut viz = format!(
        r##"<div style="text-align:center">{}</div>"##,
        svg_gauge(val, 100.0, "PE 5 年分位数", 220.0, color, "%")
    );

    let pe_hist = alist(raw, "pe_history");
    if !pe_hist.is_empty() {
        viz.push_str(r##"<div style="margin-top:12px">"##);
        viz.push_str(r##"<div style="font-family:Fira Code;font-size:10px;color:#64748b;margin-bottom:4px">📉 PE 历史 Band · 红区=偏贵 / 黄区=合理 / 绿区=便宜</div>"##);
        viz.push_str(&svg_pe_band(&pe_hist, 320, 160));
        viz.push_str("</div>");
    }

    viz.push_str(&format!(
        r##"<div style="display:grid;grid-template-columns:repeat(3,1fr);gap:6px;margin-top:12px;padding-top:10px;border-top:1px solid #e7ecf2;text-align:center">
  <div style="padding:8px;background:#ffffff;border:1px solid #e7ecf2;border-radius:6px">
    <div style="font-family:Fira Code;font-size:9px;color:#64748b">当前 PE</div>
    <div style="font-family:Fira Sans;font-size:16px;color:#0f172a;font-weight:700">{pe}</div>
  </div>
  <div style="padding:8px;background:#ffffff;border:1px solid #e7ecf2;border-radius:6px">
    <div style="font-family:Fira Code;font-size:9px;color:#64748b">行业均值</div>
    <div style="font-family:Fira Sans;font-size:16px;color:#0f172a;font-weight:700">{industry_pe}</div>
  </div>
  <div style="padding:8px;background:#ffffff;border:1px solid #e7ecf2;border-radius:6px">
    <div style="font-family:Fira Code;font-size:9px;color:#64748b">DCF 内在</div>
    <div style="font-family:Fira Sans;font-size:16px;color:#0f172a;font-weight:700">{dcf}</div>
  </div>
</div>"##
    ));

    let dcf_matrix = raw.get("dcf_sensitivity").cloned().unwrap_or(Value::Null);
    let waccs = alist(&dcf_matrix, "waccs");
    let growths = alist(&dcf_matrix, "growths");
    let values_matrix = alist(&dcf_matrix, "values");
    if !waccs.is_empty() && !growths.is_empty() && !values_matrix.is_empty() {
        let current_price = num(dcf_matrix.get("current_price").unwrap_or(&Value::Number(0.into())));
        viz.push_str(r##"<div style="margin-top:12px;padding-top:10px;border-top:1px solid #e7ecf2">"##);
        viz.push_str(r##"<div style="font-family:Fira Code;font-size:10px;color:#64748b;margin-bottom:6px">🧮 DCF 敏感度矩阵 (行=WACC, 列=增长率)</div>"##);
        viz.push_str(r##"<table style="width:100%;border-collapse:collapse;font-family:Fira Code;font-size:10px">"##);
        let mut header = String::from("<tr><td></td>");
        for g in &growths {
            header.push_str(&format!(
                r##"<td style="padding:4px;text-align:center;color:#64748b">{}%</td>"##,
                disp(g)
            ));
        }
        header.push_str("</tr>");
        viz.push_str(&header);
        for (ri, w) in waccs.iter().enumerate() {
            let row = values_matrix.get(ri).and_then(|v| v.as_array());
            viz.push_str(&format!(
                r##"<tr><td style="padding:4px;color:#64748b">{}%</td>"##,
                disp(w)
            ));
            for (ci, _g) in growths.iter().enumerate() {
                let v = row
                    .and_then(|r| r.get(ci))
                    .map(num)
                    .unwrap_or(0.0);
                let rel = if current_price != 0.0 {
                    (v - current_price) / current_price
                } else {
                    0.0
                };
                let bg = if rel > 0.1 {
                    COLOR_BULL
                } else if rel > -0.1 {
                    COLOR_GOLD
                } else {
                    COLOR_BEAR
                };
                viz.push_str(&format!(
                    r##"<td style="padding:4px;text-align:center;background:{bg};color:#fff;font-weight:700">{v:.1}</td>"##
                ));
            }
            viz.push_str("</tr>");
        }
        viz.push_str("</table>");
        viz.push_str("</div>");
    }

    viz
}

pub fn viz_financials(raw: &Value) -> String {
    let rev_hist = alist(raw, "revenue_history");
    let roe_hist = alist(raw, "roe_history");
    let np_hist = alist(raw, "net_profit_history");
    let years: Vec<String> = match raw.get("financial_years") {
        Some(v) if uzi_core::py::truthy(v) => v
            .as_array()
            .map(|a| a.iter().map(disp).collect())
            .unwrap_or_default(),
        _ => (1..=rev_hist.len()).map(|i| format!("{i}Y")).collect(),
    };

    let mut viz = String::new();
    if !rev_hist.is_empty() {
        let mut growth: Vec<Value> = Vec::with_capacity(rev_hist.len());
        for i in 0..rev_hist.len() {
            if i == 0 {
                growth.push(Value::Number(0.into()));
            } else {
                let prev = num(&rev_hist[i - 1]);
                let g = if prev != 0.0 {
                    uzi_core::py::round((num(&rev_hist[i]) - prev) / prev * 100.0, 1)
                } else {
                    0.0
                };
                growth.push(Value::from(g));
            }
        }
        viz.push_str(r##"<div style="font-family:Fira Code;font-size:10px;color:#64748b;margin-bottom:4px">📊 营收（亿）· 金线=同比增速 %</div>"##);
        viz.push_str(&svg_bars(
            &rev_hist,
            Some(&years),
            320,
            130,
            COLOR_CYAN,
            true,
            Some(&growth),
            COLOR_GOLD,
        ));
    }

    fn spark_row(label: &str, values: &[Value], unit: &str, color: &str) -> String {
        if values.len() < 2 {
            return String::new();
        }
        let last = &values[values.len() - 1];
        let first = &values[0];
        let last_f = num(last);
        let delta = last_f - num(first);
        let arrow = if delta > 0.0 {
            "↑"
        } else if delta < 0.0 {
            "↓"
        } else {
            "→"
        };
        let dcolor = if delta > 0.0 {
            COLOR_BULL
        } else if delta < 0.0 {
            COLOR_BEAR
        } else {
            COLOR_MUTED
        };
        let spark = svg_sparkline(values, 150, 30, color, true);
        format!(
            r##"<div style="display:flex;align-items:center;gap:10px;padding:6px 0;border-top:1px solid #f4f7fa">
  <div style="width:52px;font-family:Fira Code;font-size:10px;color:#64748b">{label}</div>
  <div style="flex:1">{spark}</div>
  <div style="font-family:Fira Code;font-size:11px;text-align:right;min-width:72px">
    <div style="color:#0f172a;font-weight:700">{last}{unit}</div>
    <div style="color:{dcolor};font-size:9px">{arrow} {abs:.1}</div>
  </div>
</div>"##,
            last = disp(last),
            abs = delta.abs()
        )
    }

    viz.push_str(r##"<div style="margin-top:10px">"##);
    viz.push_str(&spark_row("ROE", &roe_hist, "%", COLOR_BULL));
    viz.push_str(&spark_row("净利", &np_hist, "亿", COLOR_GOLD));
    viz.push_str("</div>");

    let div_years = alist(raw, "dividend_years");
    let div_amounts = alist(raw, "dividend_amounts");
    let div_yields = alist(raw, "dividend_yields");
    if !div_years.is_empty() && !div_amounts.is_empty() {
        viz.push_str(r##"<div style="margin-top:12px;padding-top:10px;border-top:1px solid #e7ecf2">"##);
        viz.push_str(r##"<div style="font-family:Fira Code;font-size:10px;color:#64748b;margin-bottom:4px">💰 分红（元/10股）· 金线=股息率 %</div>"##);
        viz.push_str(&svg_dividend_combo(&div_years, &div_amounts, &div_yields, 320, 130));
        viz.push_str("</div>");
    }

    let health = raw.get("financial_health").cloned().unwrap_or(Value::Null);
    if uzi_core::py::truthy(&health) {
        viz.push_str(r##"<div style="margin-top:12px;padding-top:10px;border-top:1px solid #e7ecf2">"##);
        viz.push_str(r##"<div style="font-family:Fira Code;font-size:10px;color:#64748b;margin-bottom:6px">💪 财务健康度</div>"##);
        for (k, label, max_v, good_high) in [
            ("current_ratio", "流动比率", 3.0_f64, true),
            ("debt_ratio", "资产负债率 %", 100.0, false),
            ("fcf_margin", "现金流/净利 %", 150.0, true),
            ("roic", "ROIC %", 30.0, true),
        ] {
            let v = health.get(k).cloned().unwrap_or(Value::Null);
            if v.is_null() {
                continue;
            }
            let pct = (num(&v) / max_v * 100.0).min(100.0);
            let pct = if !good_high { 100.0 - pct } else { pct };
            let color = if pct > 66.0 {
                COLOR_BULL
            } else if pct > 33.0 {
                COLOR_GOLD
            } else {
                COLOR_BEAR
            };
            viz.push_str(&svg_progress_row(label, num(&v), color, ""));
        }
        viz.push_str("</div>");
    }

    viz
}

pub fn viz_kline(raw: &Value) -> String {
    let candles = alist(raw, "candles_60d");
    let ma20 = alist(raw, "ma20_60d");
    let ma60 = alist(raw, "ma60_60d");
    let closes = alist(raw, "close_60d");

    let stage = disp(raw.get("stage").unwrap_or(&Value::String("—".into())));
    let ma_align = disp(raw.get("ma_align").unwrap_or(&Value::String("—".into())));
    let macd = disp(raw.get("macd").unwrap_or(&Value::String("—".into())));
    let rsi = disp(raw.get("rsi").unwrap_or(&Value::String("—".into())));

    let mut viz = String::new();
    if candles.len() >= 10 {
        viz.push_str(&svg_candlestick(
            &candles,
            340,
            200,
            Some(&ma20),
            Some(&ma60),
        ));
    } else if !closes.is_empty() {
        let color = if num(&closes[closes.len() - 1]) > num(&closes[0]) {
            COLOR_BULL
        } else {
            COLOR_BEAR
        };
        viz.push_str(&svg_sparkline(&closes, 320, 80, color, true));
    }

    let mut badges = format!(
        r##"<div style="display:flex;flex-wrap:wrap;gap:6px;margin-top:10px">
  <span style="padding:4px 10px;background:#fffaeb;color:#d97706;border-radius:4px;font-family:Fira Code;font-size:11px;font-weight:600">{stage}</span>
  <span style="padding:4px 10px;background:#eef4ff;color:#2563eb;border-radius:4px;font-family:Fira Code;font-size:11px;font-weight:600">MA {ma_align}</span>
  <span style="padding:4px 10px;background:#ecfdf5;color:#059669;border-radius:4px;font-family:Fira Code;font-size:11px;font-weight:600">MACD {macd}</span>
  <span style="padding:4px 10px;background:#eef4ff;color:#4f46e5;border-radius:4px;font-family:Fira Code;font-size:11px;font-weight:600">RSI {rsi}</span>"##
    );
    let ind = raw.get("indicators").cloned().unwrap_or(Value::Null);
    if let Some(kj) = ind.get("kdj_j").filter(|v| !v.is_null()) {
        let kjf = num(kj);
        let kc = if kjf > 100.0 {
            "#dc2626"
        } else if kjf < 0.0 {
            "#059669"
        } else {
            "#7c3aed"
        };
        badges.push_str(&format!(
            r##"<span style="padding:4px 10px;background:#f3e8ff;color:{kc};border-radius:4px;font-family:Fira Code;font-size:11px;font-weight:600">KDJ-J {kjf:.0}</span>"##
        ));
    }
    if let Some(wr) = ind.get("williams_r").filter(|v| !v.is_null()) {
        let wrf = num(wr);
        let wc = if wrf > -20.0 {
            "#dc2626"
        } else if wrf < -80.0 {
            "#059669"
        } else {
            "#64748b"
        };
        badges.push_str(&format!(
            r##"<span style="padding:4px 10px;background:#f4f7fa;color:{wc};border-radius:4px;font-family:Fira Code;font-size:11px;font-weight:600">W%R {wrf:.0}</span>"##
        ));
    }
    if let Some(obv) = ind.get("obv_trend_up").filter(|v| !v.is_null()) {
        let up = uzi_core::py::truthy(obv);
        let ot = if up { "OBV↑" } else { "OBV↓" };
        let oc = if up { "#059669" } else { "#dc2626" };
        badges.push_str(&format!(
            r##"<span style="padding:4px 10px;background:#ecfdf5;color:{oc};border-radius:4px;font-family:Fira Code;font-size:11px;font-weight:600">{ot}</span>"##
        ));
    }
    badges.push_str("</div>");

    let stats = raw.get("kline_stats").cloned().unwrap_or(Value::Null);
    if uzi_core::py::truthy(&stats) {
        let mut stat_items = Vec::new();
        for (k, lbl) in [
            ("beta", "Beta"),
            ("volatility", "年化波动"),
            ("max_drawdown", "最大回撤"),
            ("ytd_return", "年初至今"),
        ] {
            if let Some(v) = stats.get(k).filter(|v| !v.is_null()) {
                stat_items.push(format!(
                    r##"<div><div style="font-family:Fira Code;font-size:9px;color:#64748b">{lbl}</div><div style="font-family:Fira Code;font-size:12px;color:#0f172a;font-weight:700">{}</div></div>"##,
                    disp(v)
                ));
            }
        }
        if !stat_items.is_empty() {
            badges.push_str(&format!(
                r##"<div style="display:grid;grid-template-columns:repeat(4,1fr);gap:8px;margin-top:10px;padding-top:10px;border-top:1px solid #e7ecf2">{}</div>"##,
                stat_items.concat()
            ));
        }
    }

    format!("{viz}{badges}")
}

pub fn viz_macro(raw: &Value) -> String {
    let get = |k: &str| disp(raw.get(k).unwrap_or(&Value::String("—".to_string())));
    let items = [
        ("利率", get("rate_cycle"), "📉"),
        ("汇率", get("fx_trend"), "💱"),
        ("地缘", get("geo_risk"), "🌐"),
        ("大宗", get("commodity"), "📦"),
    ];
    let cells: String = items
        .iter()
        .map(|(l, v, ic)| {
            format!(
                r##"<div style="padding:10px;background:#ffffff;border:1px solid #e7ecf2;border-radius:8px;text-align:center"><div style="font-size:18px;margin-bottom:4px">{ic}</div><div style="font-family:Fira Code;font-size:9px;color:#64748b;letter-spacing:.1em">{l}</div><div style="font-family:Fira Sans;font-size:11px;color:#0f172a;font-weight:600;margin-top:2px">{v}</div></div>"##
            )
        })
        .collect();
    format!(
        r##"<div style="display:grid;grid-template-columns:repeat(2,1fr);gap:6px">{cells}</div>"##
    )
}

pub fn viz_peers(raw: &Value) -> String {
    let peer_table = alist(raw, "peer_table");
    let mut viz = String::new();
    if !peer_table.is_empty() {
        viz.push_str(r##"<div style="font-family:Fira Code;font-size:10px;color:#64748b;margin-bottom:6px">🏆 同业估值对比</div>"##);
        viz.push_str(&svg_peer_table(&peer_table));
    }

    let metrics = alist(raw, "peer_comparison");
    if !metrics.is_empty() {
        viz.push_str(r##"<div style="margin-top:12px;padding-top:10px;border-top:1px solid #e7ecf2">"##);
        viz.push_str(r##"<div style="font-family:Fira Code;font-size:10px;color:#64748b;margin-bottom:6px">📊 关键指标 vs 行业均值</div>"##);
        for m in metrics.iter().take(4) {
            let name = disp(m.get("name").unwrap_or(&Value::String(String::new())));
            let self_v = m.get("self").cloned().unwrap_or(Value::Null);
            let peer_v = m.get("peer").cloned().unwrap_or(Value::Null);
            let missing = |v: &Value| {
                v.is_null() || matches!(v, Value::String(s) if s.is_empty() || s == "—")
            };
            if missing(&self_v) || missing(&peer_v) {
                continue;
            }
            let self_f = match self_v {
                Value::Number(_) => num(&self_v),
                Value::String(s) => match s.trim().parse::<f64>() {
                    Ok(x) => x,
                    Err(_) => continue,
                },
                _ => continue,
            };
            let peer_f = match peer_v {
                Value::Number(_) => num(&peer_v),
                Value::String(s) => match s.trim().parse::<f64>() {
                    Ok(x) => x,
                    Err(_) => continue,
                },
                _ => continue,
            };
            let max_v = self_f.abs().max(peer_f.abs()).max(1.0);
            let self_pct = self_f.abs() / max_v * 100.0;
            let peer_pct = peer_f.abs() / max_v * 100.0;
            let self_color = if self_f >= peer_f { COLOR_BULL } else { COLOR_BEAR };
            viz.push_str(&format!(
                r##"<div style="margin-bottom:10px">
  <div style="display:flex;justify-content:space-between;font-size:11px;color:#64748b;margin-bottom:4px">
    <span>{name}</span>
    <span><strong style="color:#0f172a">自己 {sv}</strong> vs 行业 {pv}</span>
  </div>
  <div style="position:relative;height:10px;background:#f4f7fa;border-radius:5px">
    <div style="position:absolute;height:100%;width:{pp}%;background:{COLOR_MUTED};border-radius:5px;opacity:.6"></div>
    <div style="position:absolute;height:100%;width:{sp}%;background:{self_color};border-radius:5px"></div>
  </div>
</div>"##,
                sv = pyf(self_f),
                pv = pyf(peer_f),
                pp = pyf(peer_pct),
                sp = pyf(self_pct),
            ));
        }
        viz.push_str("</div>");
    }
    viz.push_str(&render_global_peer_comparison(
        raw.get("global_peer_comparison").unwrap_or(&Value::Null),
    ));
    if viz.is_empty() {
        return r##"<div style="color:#94a3b8;font-size:11px">未获取同行数据</div>"##.to_string();
    }
    viz
}

pub fn viz_research(raw: &Value) -> String {
    let rating = disp(raw.get("rating").unwrap_or(&Value::String(String::new())));
    let buy_n = first_int(
        &rating
            .split("买入")
            .nth(1)
            .map(|s| s.to_string())
            .unwrap_or_default(),
    )
    .unwrap_or(0);
    let overwt_n = first_int(
        &rating
            .split("增持")
            .nth(1)
            .map(|s| s.to_string())
            .unwrap_or_default(),
    )
    .unwrap_or(0);
    let neu_n = first_int(
        &rating
            .split("中性")
            .nth(1)
            .map(|s| s.to_string())
            .unwrap_or_default(),
    )
    .unwrap_or(0);
    let total = buy_n + overwt_n + neu_n;
    if total == 0 {
        return format!(
            r##"<div style="font-family:Fira Code;font-size:11px">{rating}</div>"##
        );
    }
    let donut = svg_donut(
        &[
            ("买入".to_string(), Value::from(buy_n), COLOR_BULL.to_string()),
            ("增持".to_string(), Value::from(overwt_n), COLOR_CYAN.to_string()),
            ("中性".to_string(), Value::from(neu_n), COLOR_MUTED.to_string()),
        ],
        None,
        &format!("{total}家"),
        120,
    );
    let target_avg = raw
        .get("target_avg")
        .cloned()
        .unwrap_or(Value::String("—".into()));
    let mut upside = raw.get("upside").cloned().unwrap_or(Value::Null);
    let upside_is_missing = upside.is_null()
        || matches!(&upside, Value::String(s) if s.is_empty() || s == "None");
    if upside_is_missing {
        let px = num(raw.get("price").unwrap_or(&Value::Number(0.into())));
        let ta_str = disp(&target_avg);
        let ta = if ta_str.replace('.', "").chars().all(|c| c.is_ascii_digit())
            && !ta_str.replace('.', "").is_empty()
        {
            ta_str.parse::<f64>().unwrap_or(0.0)
        } else {
            0.0
        };
        upside = if px != 0.0 && ta != 0.0 {
            Value::String(format!("{:.1}%", (ta - px) / px * 100.0))
        } else {
            Value::String("—".to_string())
        };
    }
    let tail = format!(
        r##"<div style="display:flex;justify-content:space-between;margin-top:10px;padding:8px;background:#fffaeb;border-radius:6px">
  <span style="font-family:Fira Code;font-size:10px;color:#64748b">一致目标价</span>
  <span style="font-family:Fira Code;font-size:12px;color:#d97706;font-weight:700">{ta} ({up})</span>
</div>"##,
        ta = disp(&target_avg),
        up = disp(&upside)
    );
    format!("{donut}{tail}")
}

pub fn viz_industry(raw: &Value) -> String {
    let growth = disp(raw.get("growth").unwrap_or(&Value::String("—".into())));
    let tam = disp(raw.get("tam").unwrap_or(&Value::String("—".into())));
    let penetration = disp(raw.get("penetration").unwrap_or(&Value::String("—".into())));
    let lifecycle = disp(raw.get("lifecycle").unwrap_or(&Value::String("—".into())));
    let growth_val = first_int(&growth).unwrap_or(0);
    let gauge = svg_gauge(
        (growth_val.min(100)) as f64,
        100.0,
        "行业增速 %",
        220.0,
        if growth_val > 15 { COLOR_BULL } else { COLOR_GOLD },
        "",
    );
    let tail = format!(
        r##"<div style="display:grid;grid-template-columns:repeat(3,1fr);gap:6px;margin-top:8px;text-align:center">
  <div style="padding:6px;background:#ffffff;border:1px solid #e7ecf2;border-radius:6px">
    <div style="font-family:Fira Code;font-size:9px;color:#64748b">TAM</div>
    <div style="font-family:Fira Sans;font-size:13px;font-weight:700;color:#0f172a">{tam}</div>
  </div>
  <div style="padding:6px;background:#ffffff;border:1px solid #e7ecf2;border-radius:6px">
    <div style="font-family:Fira Code;font-size:9px;color:#64748b">渗透率</div>
    <div style="font-family:Fira Sans;font-size:13px;font-weight:700;color:#0f172a">{penetration}</div>
  </div>
  <div style="padding:6px;background:#ffffff;border:1px solid #e7ecf2;border-radius:6px">
    <div style="font-family:Fira Code;font-size:9px;color:#64748b">周期</div>
    <div style="font-family:Fira Sans;font-size:11px;font-weight:700;color:#0f172a">{lifecycle}</div>
  </div>
</div>"##
    );
    format!(r##"<div style="text-align:center">{gauge}</div>{tail}"##)
}

pub fn viz_materials(raw: &Value) -> String {
    let core = disp(raw.get("core_material").unwrap_or(&Value::String("—".into())));
    let trend_str = disp(raw.get("price_trend").unwrap_or(&Value::String("—".into())));
    let cost_share = disp(raw.get("cost_share").unwrap_or(&Value::String("—".into())));
    let import_dep = disp(raw.get("import_dep").unwrap_or(&Value::String("—".into())));
    let trend_vals = alist(raw, "price_history_12m");
    let mut spark_html = String::new();
    if !trend_vals.is_empty() {
        let color = if num(&trend_vals[trend_vals.len() - 1]) < num(&trend_vals[0]) {
            COLOR_BULL
        } else {
            COLOR_BEAR
        };
        spark_html = svg_sparkline(&trend_vals, 260, 48, color, true);
    }
    format!(
        r##"{spark_html}
<div style="margin-top:8px;font-family:Fira Code;font-size:11px;line-height:1.9;color:#475569">
  <div>🔩 核心: <strong style="color:#0f172a">{core}</strong></div>
  <div>📉 12M: <strong style="color:#0f172a">{trend_str}</strong></div>
  <div>💰 成本占比: <strong style="color:#0f172a">{cost_share}</strong> · 🌍 进口依赖: <strong style="color:#0f172a">{import_dep}</strong></div>
</div>"##
    )
}

pub fn viz_futures(raw: &Value) -> String {
    let linked = disp(raw.get("linked_contract").unwrap_or(&Value::String("—".into())));
    let trend = disp(raw.get("contract_trend").unwrap_or(&Value::String("—".into())));
    format!(
        r##"<div style="padding:16px;text-align:center;background:#ffffff;border:1px dashed #d7dfe9;border-radius:8px">
  <div style="font-family:Fira Code;font-size:9px;color:#64748b;letter-spacing:.15em">LINKED CONTRACT</div>
  <div style="font-family:Fira Sans;font-size:16px;color:#0f172a;font-weight:700;margin-top:4px">{linked}</div>
  <div style="font-size:11px;color:#475569;margin-top:4px">{trend}</div>
</div>"##
    )
}

pub fn viz_governance(raw: &Value) -> String {
    let pledge_raw = raw
        .get("pledge")
        .cloned()
        .unwrap_or(Value::String("—".to_string()));
    let pledge = match &pledge_raw {
        Value::Array(a) if !a.is_empty() => {
            let first = if a[0].is_object() {
                a[0].clone()
            } else {
                Value::Object(Map::new())
            };
            let ratio = first.get("质押比例").cloned().unwrap_or(Value::Number(0.into()));
            if uzi_core::py::truthy(&ratio) {
                format!("质押比例 {}%", disp(&ratio))
            } else {
                format!("有 {} 条质押记录", a.len())
            }
        }
        Value::String(s) => s.clone(),
        _ => "—".to_string(),
    };

    let insider_src = match raw.get("insider_trades_1y") {
        Some(v) if uzi_core::py::truthy(v) => v.clone(),
        _ => raw
            .get("insider")
            .cloned()
            .unwrap_or(Value::String("—".to_string())),
    };
    let insider = match &insider_src {
        Value::Array(a) if !a.is_empty() => format!("近 1 年 {} 笔交易", a.len()),
        Value::String(s) if !s.is_empty() => s.clone(),
        _ => "暂无近期增减持".to_string(),
    };

    let hits = raw.get("violation_hits").cloned().unwrap_or(Value::Null);
    let violations = match &hits {
        Value::Array(a) if !a.is_empty() => format!("{} 条待核", a.len()),
        Value::Array(_) => "未发现".to_string(),
        Value::Number(n) if n.as_f64() == Some(0.0) => "未发现".to_string(),
        _ => "暂无公开违规记录".to_string(),
    };
    let no_violations = violations == "未发现";

    let low_pledge = matches!(&pledge_raw, Value::Array(a) if !a.is_empty())
        && pledge_raw
            .as_array()
            .and_then(|a| a.first())
            .and_then(|f| f.get("质押比例"))
            .map(num)
            .unwrap_or(100.0)
            < 20.0;
    let insider_positive = insider.contains("增持") || insider.contains("买入");

    fn badge(label: &str, val: &str, positive: Option<bool>) -> String {
        let color = if positive == Some(true) {
            COLOR_BULL
        } else if positive == Some(false) {
            COLOR_BEAR
        } else {
            COLOR_GOLD
        };
        let bg = if positive == Some(true) {
            "#ecfdf5"
        } else if positive == Some(false) {
            "#fef3f2"
        } else {
            "#fffaeb"
        };
        format!(
            r##"<div style="padding:10px 12px;background:{bg};border-left:3px solid {color};border-radius:0 8px 8px 0">
  <div style="font-family:Fira Code;font-size:9px;color:#64748b;letter-spacing:.1em">{label}</div>
  <div style="font-family:Fira Sans;font-size:13px;color:#0f172a;font-weight:700;margin-top:2px">{val}</div>
</div>"##
        )
    }
    let rows = badge("实控人质押", &pledge, Some(low_pledge))
        + &badge("近12月增减持", &insider, Some(insider_positive))
        + &badge("关联交易/违规", &violations, Some(no_violations));
    format!(
        r##"<div style="display:flex;flex-direction:column;gap:6px">{rows}</div>"##
    )
}

pub fn viz_capital_flow(raw: &Value) -> String {
    fn mini(label: &str, values: &[f64], summary: &str, color: &str) -> String {
        let summary = safe(&Value::String(summary.to_string()), "数据暂缺");
        if values.len() < 2 {
            return format!(
                r##"<div style="padding:10px;background:#ffffff;border:1px solid #e7ecf2;border-radius:8px">
  <div style="font-family:Fira Code;font-size:9px;color:#64748b">{label}</div>
  <div style="font-family:Fira Code;font-size:12px;font-weight:700;color:#64748b;margin-top:2px">{summary}</div>
</div>"##
            );
        }
        let vals: Vec<Value> = values.iter().map(|v| Value::from(*v)).collect();
        let spark = svg_sparkline(&vals, 120, 34, color, true);
        format!(
            r##"<div style="padding:10px;background:#ffffff;border:1px solid #e7ecf2;border-radius:8px">
  <div style="display:flex;justify-content:space-between;align-items:center;margin-bottom:4px">
    <span style="font-family:Fira Code;font-size:9px;color:#64748b">{label}</span>
    <strong style="font-family:Fira Code;font-size:10px;color:#0f172a">{summary}</strong>
  </div>
  {spark}
</div>"##
        )
    }

    let main_flow = alist(raw, "main_fund_flow_20d");
    let main_values: Vec<f64> = main_flow
        .iter()
        .take(20)
        .filter(|r| r.is_object())
        .map(|r| num(r.get("主力净流入-净额").unwrap_or(&Value::Number(0.into()))).abs())
        .collect();
    let mut main_5d_summary = raw
        .get("main_5d")
        .cloned()
        .unwrap_or(Value::String("—".to_string()));
    if matches!(&main_5d_summary, Value::String(s) if s == "—") && !main_flow.is_empty() {
        let recent = &main_flow[..main_flow.len().min(5)];
        let net: f64 = recent
            .iter()
            .filter(|r| r.is_object())
            .map(|r| num(r.get("主力净流入-净额").unwrap_or(&Value::Number(0.into()))))
            .sum();
        main_5d_summary = if net.abs() > 0.0 {
            Value::String(format!(
                "{} {:.1}亿",
                if net > 0.0 { "净流入" } else { "净流出" },
                net.abs() / 1e8
            ))
        } else {
            Value::String("—".to_string())
        };
    }

    let block = alist(raw, "block_trades_recent");
    let block_summary = if !block.is_empty() {
        format!("近期 {} 笔", block.len())
    } else {
        "无近期大宗".to_string()
    };

    let holders_hist = alist(raw, "holder_count_history");
    let holders_vals: Vec<f64> = holders_hist
        .iter()
        .take(10)
        .filter(|r| r.is_object())
        .map(|r| num(r.get("股东户数-本次").unwrap_or(&Value::Number(0.into()))))
        .collect();

    let north = mini("主力资金 20日", &main_values, &disp(&main_5d_summary), COLOR_CYAN);
    let margin = mini("大宗交易", &[], &block_summary, COLOR_BLUE);
    let holders = mini(
        "股东户数",
        &holders_vals,
        &disp(raw.get("holders_trend").unwrap_or(&Value::String("—".to_string()))),
        COLOR_GOLD,
    );
    let margin_trend = disp(raw.get("margin_trend").unwrap_or(&Value::String("—".to_string())));
    let main_summary = if margin_trend != "—" {
        margin_trend
    } else {
        "数据暂缺".to_string()
    };
    let main = mini("融资余额", &[], &main_summary, COLOR_MUTED);

    let mut viz = format!(
        r##"<div style="display:grid;grid-template-columns:1fr 1fr;gap:6px">{north}{margin}{holders}{main}</div>"##
    );

    let inst = raw.get("institutional_history").cloned().unwrap_or(Value::Null);
    if uzi_core::py::truthy(inst.get("quarters").unwrap_or(&Value::Null)) {
        viz.push_str(r##"<div style="margin-top:12px;padding-top:10px;border-top:1px solid #e7ecf2">"##);
        viz.push_str(r##"<div style="font-family:Fira Code;font-size:10px;color:#64748b;margin-bottom:4px">🏛 机构持仓变化（近 8 季）</div>"##);
        viz.push_str(&svg_institutional_quarters(&inst, 320, 120));
        viz.push_str("</div>");
    }

    let unlocks = alist(raw, "unlock_schedule");
    if !unlocks.is_empty() {
        viz.push_str(r##"<div style="margin-top:12px;padding-top:10px;border-top:1px solid #e7ecf2">"##);
        viz.push_str(r##"<div style="font-family:Fira Code;font-size:10px;color:#64748b;margin-bottom:4px">🔓 未来 12 月解禁时间表（亿元）</div>"##);
        viz.push_str(&svg_unlock_timeline(&unlocks, 320, 110));
        viz.push_str("</div>");
    }

    viz
}

pub fn viz_policy(raw: &Value) -> String {
    let items: [(&str, String, Option<bool>); 4] = [
        ("方向", safe(raw.get("policy_dir").unwrap_or(&Value::Null), "—"), Some(true)),
        ("补贴", safe(raw.get("subsidy").unwrap_or(&Value::Null), "—"), Some(true)),
        ("监管", safe(raw.get("monitoring").unwrap_or(&Value::Null), "—"), None),
        ("反垄断", safe(raw.get("anti_trust").unwrap_or(&Value::Null), "—"), None),
    ];
    let mut cells = String::new();
    for (label, val, positive) in items {
        if val == "—" || val == "不适用" || val == "无" || val == "数据暂缺" {
            cells.push_str(&format!(
                r##"<div style="padding:10px;background:#f7f9fc;border:1px solid #e7ecf2;border-radius:8px"><div style="font-family:Fira Code;font-size:9px;color:#94a3b8">{label}</div><div style="font-size:11px;color:#94a3b8;margin-top:2px">{val}</div></div>"##
            ));
        } else {
            let color = if positive == Some(true) { COLOR_BULL } else { COLOR_GOLD };
            let bg = if positive == Some(true) { "#ecfdf5" } else { "#fffaeb" };
            cells.push_str(&format!(
                r##"<div style="padding:10px;background:{bg};border:1px solid {color};border-radius:8px"><div style="font-family:Fira Code;font-size:9px;color:#64748b">{label}</div><div style="font-family:Fira Sans;font-size:11px;color:#0f172a;font-weight:600;margin-top:2px">{val}</div></div>"##
            ));
        }
    }
    format!(
        r##"<div style="display:grid;grid-template-columns:1fr 1fr;gap:6px">{cells}</div>"##
    )
}

pub fn viz_moat(raw: &Value) -> String {
    let cats: [(&str, &str); 5] = [
        ("intangible", "无形"),
        ("switching", "转换"),
        ("network", "网络"),
        ("scale", "规模"),
        ("efficient_scale", "有效规模"),
    ];
    let mut values: Vec<f64> = Vec::new();
    let mut labels: Vec<String> = Vec::new();
    for (k, lbl) in cats {
        let raw_v = raw.get(k).cloned().unwrap_or(Value::String(String::new()));
        let score = if is_num(&raw_v) {
            num(&raw_v)
        } else {
            let s = disp(&raw_v);
            if s.contains("强") || s.contains("高") || s.contains("最") {
                8.0
            } else if s.contains("弱") || s.contains("低") {
                3.0
            } else if !s.is_empty() && s != "—" {
                6.0
            } else {
                2.0
            }
        };
        values.push(score);
        labels.push(lbl.to_string());
    }
    while values.len() < 5 {
        values.push(0.0);
        labels.push("—".to_string());
    }
    let radar = svg_radar(&labels[..5], &values[..5], 10.0, 180.0);
    let tail: String = ["intangible", "switching", "network", "scale"]
        .iter()
        .filter(|k| {
            raw.get(**k)
                .map(|v| uzi_core::py::truthy(v) && v != &Value::String("—".to_string()))
                .unwrap_or(false)
        })
        .map(|k| {
            format!(
                r##"<div style="font-size:10px;color:#475569;padding:3px 0">• {k}: <strong style="color:#0f172a">{}</strong></div>"##,
                disp(raw.get(*k).unwrap_or(&Value::Null))
            )
        })
        .collect();
    format!(r##"<div style="text-align:center">{radar}</div><div style="margin-top:6px">{tail}</div>"##)
}

pub fn viz_events(raw: &Value) -> String {
    let mut events: Vec<String> = alist(raw, "event_timeline").iter().map(disp).collect();
    if events.is_empty() {
        for key in ["recent_news", "catalyst", "earnings_preview"] {
            if let Some(v) = raw.get(key) {
                if uzi_core::py::truthy(v) && v != &Value::String("—".to_string()) {
                    events.push(disp(v));
                }
            }
        }
    }
    if events.is_empty() {
        return r##"<div style="color:#94a3b8;font-size:11px">暂无事件</div>"##.to_string();
    }
    svg_timeline(&events)
}

pub fn viz_lhb(raw: &Value) -> String {
    let matched = raw.get("youzi_matched").cloned().unwrap_or(Value::String(String::new()));
    let matched_list: Vec<String> = match &matched {
        Value::String(s) => s
            .split('/')
            .map(|m| m.trim().to_string())
            .filter(|m| !m.is_empty())
            .collect(),
        Value::Array(a) => a.iter().map(disp).collect(),
        _ => Vec::new(),
    };
    let nick_to_id: &[(&str, &str)] = &[
        ("章盟主", "zhang_mz"),
        ("孙哥", "sun_ge"),
        ("赵老哥", "zhao_lg"),
        ("佛山无影脚", "fs_wyj"),
        ("炒股养家", "yangjia"),
        ("陈小群", "chen_xq"),
        ("呼家楼", "hu_jl"),
        ("方新侠", "fang_xx"),
        ("作手新一", "zuoshou"),
        ("小鳄鱼", "xiao_ey"),
        ("交易猿", "jiao_yy"),
        ("毛老板", "mao_lb"),
        ("消闲派", "xiao_xian"),
        ("拉萨天团", "lasa"),
        ("成都帮", "chengdu"),
        ("苏南帮", "sunan"),
        ("宁波桑田路", "ningbo_st"),
        ("六一中路", "liuyi_zl"),
        ("流沙河", "liu_sh"),
        ("古北路", "gu_bl"),
        ("北京炒家", "bj_cj"),
        ("瑞鹤仙", "wang_zr"),
        ("鑫多多", "xin_dd"),
    ];
    let mut avatars_row = String::new();
    if !matched_list.is_empty() {
        let mut cells = String::new();
        for nick in matched_list.iter().take(6) {
            let inv_id = nick_to_id
                .iter()
                .find(|(n, _)| n == nick)
                .map(|(_, id)| (*id).to_string())
                .unwrap_or_else(|| nick.clone());
            cells.push_str(&format!(
                r##"<div style="display:flex;flex-direction:column;align-items:center;gap:3px">
  <img src="avatars/{inv_id}.svg" style="width:36px;height:36px;image-rendering:pixelated;border:2px solid #d97706;border-radius:6px;background:#fff">
  <span style="font-family:Fira Code;font-size:9px;color:#0f172a;font-weight:600">{nick}</span>
</div>"##
            ));
        }
        avatars_row = format!(
            r##"<div style="display:flex;gap:8px;flex-wrap:wrap;padding:10px;background:#fffaeb;border-radius:8px;margin-bottom:10px">{cells}</div>"##
        );
    }
    let inst_vs = raw.get("inst_vs_youzi").cloned().unwrap_or(Value::Null);
    let inst_net_v = if inst_vs.is_object() {
        inst_vs.get("institutional_net").cloned().unwrap_or(Value::Number(0.into()))
    } else {
        Value::String("—".to_string())
    };
    let youzi_net_v = if inst_vs.is_object() {
        inst_vs.get("youzi_net").cloned().unwrap_or(Value::Number(0.into()))
    } else {
        Value::String("—".to_string())
    };
    let fmt_net = |v: &Value| -> String {
        if is_num(v) && num(v) != 0.0 {
            format!("{:+.1}亿", num(v) / 1e8)
        } else {
            "—".to_string()
        }
    };
    let inst_net = fmt_net(&inst_net_v);
    let youzi_net = fmt_net(&youzi_net_v);
    let lhb_30d = match raw.get("lhb_count_30d") {
        Some(v) if uzi_core::py::truthy(v) => disp(v),
        _ => "—".to_string(),
    };
    let i = first_num(&inst_net);
    let y = first_num(&youzi_net);
    let total = if i.abs() + y.abs() == 0.0 {
        1.0
    } else {
        i.abs() + y.abs()
    };
    let i_pct = i.abs() / total * 100.0;
    let y_pct = y.abs() / total * 100.0;
    let balance = format!(
        r##"<div>
  <div style="display:flex;justify-content:space-between;font-size:10px;margin-bottom:4px">
    <span style="color:#2563eb;font-weight:700">🏛 机构 {inst_net}</span>
    <span style="color:#d97706;font-weight:700">🐉 游资 {youzi_net}</span>
  </div>
  <div style="display:flex;height:10px;border-radius:5px;overflow:hidden;border:1px solid #e7ecf2">
    <div style="width:{ip}%;background:#2563eb"></div>
    <div style="width:{yp}%;background:#d97706"></div>
  </div>
  <div style="text-align:center;font-family:Fira Code;font-size:10px;color:#64748b;margin-top:6px">近 30 天上榜 <strong style="color:#0f172a">{lhb_30d}</strong></div>
</div>"##,
        ip = pyf(i_pct),
        yp = pyf(y_pct)
    );
    let sector_lhb = alist(raw, "sector_lhb_top50");
    let mut sector_html = String::new();
    if matched_list.is_empty() && !sector_lhb.is_empty() {
        let mut rows = String::new();
        for r in sector_lhb.iter().take(5) {
            if r.is_object() {
                let name = disp(r.get("名称").unwrap_or(&Value::String("—".to_string())));
                let date: String = disp(r.get("最近上榜日").unwrap_or(&Value::String(String::new())))
                    .chars()
                    .take(10)
                    .collect();
                let reason = if r.get("上榜原因").is_some() {
                    disp(r.get("上榜原因").unwrap_or(&Value::Null))
                } else {
                    String::new()
                };
                rows.push_str(&format!(
                    r##"<tr><td style="padding:4px 8px;font-size:12px;font-weight:600">{name}</td><td style="padding:4px 8px;font-size:11px;color:#64748b">{date}</td><td style="padding:4px 8px;font-size:11px;color:#64748b">{reason}</td></tr>"##
                ));
            }
        }
        if !rows.is_empty() {
            sector_html = format!(
                r##"
            <div style="margin-top:10px;padding-top:8px;border-top:1px dashed #e7ecf2">
              <div style="font-size:10px;color:#94a3b8;margin-bottom:6px">📋 本股近期无龙虎榜 · 同板块龙虎榜 TOP 5:</div>
              <table style="width:100%;border-collapse:collapse;font-size:12px"><tbody>{rows}</tbody></table>
            </div>"##
            );
        }
    }
    format!("{avatars_row}{balance}{sector_html}")
}

pub fn viz_sentiment(raw: &Value) -> String {
    let heat_str = disp(raw.get("xueqiu_heat").unwrap_or(&Value::String("50".to_string())));
    let heat_val = first_int(&heat_str).unwrap_or(50);
    let thermo = svg_thermometer(heat_val, 100, "雪球热度");
    let big_v = disp(raw.get("big_v_mentions").unwrap_or(&Value::String("—".to_string())));
    let positive = disp(raw.get("positive_pct").unwrap_or(&Value::String("—".to_string())));
    let guba = disp(raw.get("guba_volume").unwrap_or(&Value::String("—".to_string())));
    let tail = format!(
        r##"<div style="flex:1;font-family:Fira Code;font-size:11px;line-height:1.8;color:#475569">
  <div>📣 <strong style="color:#0f172a">{big_v}</strong></div>
  <div>💬 股吧 <strong style="color:#0f172a">{guba}</strong></div>
  <div>😊 正面 <strong style="color:#059669">{positive}</strong></div>
</div>"##
    );
    format!(r##"<div style="display:flex;align-items:center;gap:14px">{thermo}{tail}</div>"##)
}

pub fn viz_contests(raw: &Value) -> String {
    let xq_cubes_list = alist(raw, "xq_cubes_list");
    let tgb_list = alist(raw, "tgb_list");
    let ths_list = alist(raw, "ths_list");
    let xq_summary = disp(raw.get("xq_cubes").unwrap_or(&Value::String("—".to_string())));
    let high_return = disp(raw.get("high_return_cubes").unwrap_or(&Value::String("—".to_string())));

    let mut html = format!(
        r##"<div style="padding:10px;background:#fffaeb;border:1px solid #d97706;border-radius:8px;margin-bottom:12px;display:flex;justify-content:space-around;text-align:center">
  <div><div style="font-family:Fira Sans;font-size:22px;font-weight:900;color:#d97706;line-height:1">{xq_summary}</div><div style="font-family:Fira Code;font-size:9px;color:#64748b;margin-top:2px">XUEQIU 组合</div></div>
  <div><div style="font-family:Fira Sans;font-size:22px;font-weight:900;color:#059669;line-height:1">{high_return}</div><div style="font-family:Fira Code;font-size:9px;color:#64748b;margin-top:2px">高收益 &gt;50%</div></div>
</div>"##
    );

    if !xq_cubes_list.is_empty() {
        let mut cube_rows = String::new();
        for c in xq_cubes_list.iter().take(30) {
            let name = disp(c.get("name").unwrap_or(&Value::String(String::new())));
            let owner = disp(c.get("owner").unwrap_or(&Value::String(String::new())));
            let gain_v = c.get("total_gain").cloned().unwrap_or(Value::String(String::new()));
            let gain = disp(&gain_v);
            let url = disp(c.get("url").unwrap_or(&Value::String(String::new())));
            let gain_color = if gain.contains('+') || (is_num(&gain_v) && num(&gain_v) > 0.0) {
                COLOR_BULL
            } else {
                COLOR_BEAR
            };
            cube_rows.push_str(&format!(
                r##"<a href="{url}" target="_blank" rel="noopener" style="display:flex;justify-content:space-between;align-items:center;padding:8px 10px;background:#ffffff;border:1px solid #e7ecf2;border-radius:6px;text-decoration:none;margin-bottom:4px;transition:all .15s">
  <div style="min-width:0;flex:1">
    <div style="font-family:Fira Sans;font-size:12px;color:#0f172a;font-weight:600;white-space:nowrap;overflow:hidden;text-overflow:ellipsis">{name}</div>
    <div style="font-family:Fira Code;font-size:9px;color:#64748b">@{owner}</div>
  </div>
  <div style="font-family:Fira Code;font-size:13px;font-weight:700;color:{gain_color};margin-left:10px">{gain}</div>
</a>"##
            ));
        }
        html.push_str(&format!(
            r##"<details open style="margin-bottom:10px">
  <summary style="cursor:pointer;font-family:Fira Code;font-size:10px;color:#2563eb;padding:4px 0;letter-spacing:.1em">▼ 雪球组合持仓 ({n} 个)</summary>
  <div style="max-height:280px;overflow-y:auto;padding-right:4px">{cube_rows}</div>
</details>"##,
            n = xq_cubes_list.len()
        ));
    }

    if !tgb_list.is_empty() {
        let mut tgb_rows = String::new();
        for t in tgb_list.iter().take(20) {
            let title = disp(t.get("title").unwrap_or(&Value::String(String::new())));
            let url = disp(t.get("url").unwrap_or(&Value::String(String::new())));
            tgb_rows.push_str(&format!(
                r##"<a href="{url}" target="_blank" rel="noopener" style="display:block;padding:6px 10px;background:#ffffff;border:1px solid #e7ecf2;border-radius:6px;text-decoration:none;margin-bottom:4px;font-size:11px;color:#1e293b">• {title}</a>"##
            ));
        }
        html.push_str(&format!(
            r##"<details style="margin-bottom:10px">
  <summary style="cursor:pointer;font-family:Fira Code;font-size:10px;color:#2563eb;padding:4px 0;letter-spacing:.1em">▼ 淘股吧讨论 ({n} 条)</summary>
  <div style="max-height:220px;overflow-y:auto;padding-right:4px">{tgb_rows}</div>
</details>"##,
            n = tgb_list.len()
        ));
    }

    if !ths_list.is_empty() {
        let mut ths_rows = String::new();
        for p in ths_list.iter().take(20) {
            let nickname = disp(p.get("nickname").unwrap_or(&Value::String(String::new())));
            let ret = disp(p.get("return_pct").unwrap_or(&Value::String(String::new())));
            ths_rows.push_str(&format!(
                r##"<div style="display:flex;justify-content:space-between;padding:6px 10px;background:#ffffff;border:1px solid #e7ecf2;border-radius:6px;margin-bottom:4px"><span style="font-size:11px;color:#1e293b">{nickname}</span><strong style="font-family:Fira Code;font-size:11px;color:#059669">+{ret}%</strong></div>"##
            ));
        }
        html.push_str(&format!(
            r##"<details>
  <summary style="cursor:pointer;font-family:Fira Code;font-size:10px;color:#2563eb;padding:4px 0;letter-spacing:.1em">▼ 同花顺模拟 ({n} 位)</summary>
  <div style="max-height:220px;overflow-y:auto;padding-right:4px">{ths_rows}</div>
</details>"##,
            n = ths_list.len()
        ));
    }

    html
}

/// `DIM_VIZ_RENDERERS` — dim_key → specialized viz function.
pub fn viz_for(dim_key: &str) -> Option<fn(&Value) -> String> {
    Some(match dim_key {
        "1_financials" => viz_financials,
        "2_kline" => viz_kline,
        "3_macro" => viz_macro,
        "4_peers" => viz_peers,
        "5_chain" => viz_chain,
        "6_research" => viz_research,
        "7_industry" => viz_industry,
        "8_materials" => viz_materials,
        "9_futures" => viz_futures,
        "10_valuation" => viz_valuation,
        "11_governance" => viz_governance,
        "12_capital_flow" => viz_capital_flow,
        "13_policy" => viz_policy,
        "14_moat" => viz_moat,
        "15_events" => viz_events,
        "16_lhb" => viz_lhb,
        "17_sentiment" => viz_sentiment,
        "18_trap" => viz_trap,
        "19_contests" => viz_contests,
        _ => return None,
    })
}

/// Keys of `DIM_VIZ_RENDERERS` in declaration order (for parity checks).
pub const DIM_VIZ_KEYS: &[&str] = &[
    "1_financials",
    "2_kline",
    "3_macro",
    "4_peers",
    "5_chain",
    "6_research",
    "7_industry",
    "8_materials",
    "9_futures",
    "10_valuation",
    "11_governance",
    "12_capital_flow",
    "13_policy",
    "14_moat",
    "15_events",
    "16_lhb",
    "17_sentiment",
    "18_trap",
    "19_contests",
];

