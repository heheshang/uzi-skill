//! Port of `fetch_kline.py`.
//!
//! Dimension 2 · K线 (OHLCV + 均线 + MACD + RSI + 筹码分布 + 简单形态).
//!
//! Raw rows come from [`crate::sources::fetch_kline`] (upstream's 7-layer chain,
//! including the `[{"_kline_fetch_error": ...}]` failure row). Every indicator is
//! computed locally from those rows; the only AkShare-only branch is the
//! `stock_cyq_em` chip distribution, which degrades to upstream's exception
//! payload.

use serde_json::{Map, Value};

use uzi_core::py::{round, truthy};
use uzi_core::ticker::parse_ticker;

// ─────────────────────────────────────────────────────────────
// Indicators
// ─────────────────────────────────────────────────────────────

fn as_f64(v: &Value) -> Option<f64> {
    match v {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.trim().parse::<f64>().ok(),
        _ => None,
    }
}

/// `float(r.get(a) or r.get(b) or 0)` — first truthy key wins.
fn num_or_keys(row: &Value, keys: &[&str]) -> f64 {
    for k in keys {
        if let Some(v) = row.get(*k) {
            if truthy(v) {
                if let Some(x) = as_f64(v) {
                    return x;
                }
            }
        }
    }
    0.0
}

/// `_ma(closes, n)` — running mean with a partial window at the head.
fn ma(closes: &[f64], n: usize) -> Vec<f64> {
    (0..closes.len())
        .map(|i| {
            let lo = i.saturating_sub(n - 1);
            let sum: f64 = closes[lo..=i].iter().sum();
            sum / (i + 1).min(n) as f64
        })
        .collect()
}

/// `_ema(values, n)`.
fn ema(values: &[f64], n: usize) -> Vec<f64> {
    let k = 2.0 / (n as f64 + 1.0);
    let mut out = Vec::with_capacity(values.len());
    let mut prev: Option<f64> = None;
    for &v in values {
        prev = Some(match prev {
            None => v,
            Some(p) => v * k + p * (1.0 - k),
        });
        out.push(prev.unwrap_or(v));
    }
    out
}

/// `_rsi(closes, n=14)`.
fn rsi(closes: &[f64], n: usize) -> Option<f64> {
    if closes.len() < n + 1 {
        return None;
    }
    let mut gains = Vec::with_capacity(closes.len() - 1);
    let mut losses = Vec::with_capacity(closes.len() - 1);
    for i in 1..closes.len() {
        let diff = closes[i] - closes[i - 1];
        gains.push(diff.max(0.0));
        losses.push((-diff).max(0.0));
    }
    let mean = |xs: &[f64]| xs.iter().sum::<f64>() / xs.len() as f64;
    let avg_gain = mean(&gains[gains.len() - n..]);
    let avg_loss = {
        let m = mean(&losses[losses.len() - n..]);
        if m == 0.0 {
            1e-9
        } else {
            m
        }
    };
    let rs = avg_gain / avg_loss;
    Some(100.0 - 100.0 / (1.0 + rs))
}

/// `_kdj(closes, highs, lows, n=9)` — the last K/D/J.
fn kdj(closes: &[f64], highs: &[f64], lows: &[f64], n: usize) -> (Option<f64>, Option<f64>, Option<f64>) {
    if closes.len() < n {
        return (None, None, None);
    }
    let (mut k, mut d) = (50.0_f64, 50.0_f64);
    for i in (n - 1)..closes.len() {
        let lo = i + 1 - n;
        let hh = highs[lo..=i].iter().cloned().fold(f64::NEG_INFINITY, f64::max);
        let ll = lows[lo..=i].iter().cloned().fold(f64::INFINITY, f64::min);
        let rsv = if hh > ll {
            (closes[i] - ll) / (hh - ll) * 100.0
        } else {
            50.0
        };
        k = 2.0 / 3.0 * k + 1.0 / 3.0 * rsv;
        d = 2.0 / 3.0 * d + 1.0 / 3.0 * k;
    }
    let j = 3.0 * k - 2.0 * d;
    (Some(round(k, 1)), Some(round(d, 1)), Some(round(j, 1)))
}

/// `_obv(closes, vols)` — `(obv_last, obv_trend_up)`.
fn obv(closes: &[f64], vols: &[f64]) -> (Option<f64>, Option<bool>) {
    if closes.len() < 2 {
        return (None, None);
    }
    let mut out = vec![0.0_f64];
    for i in 1..closes.len() {
        let last = out[i - 1];
        let next = if closes[i] > closes[i - 1] {
            last + vols[i]
        } else if closes[i] < closes[i - 1] {
            last - vols[i]
        } else {
            last
        };
        out.push(next);
    }
    let trend_up = if out.len() >= 20 {
        Some(out[out.len() - 1] > out[out.len() - 20])
    } else {
        None
    };
    (Some(round(out[out.len() - 1], 0)), trend_up)
}

/// `_williams_r(closes, highs, lows, n=14)`.
fn williams_r(closes: &[f64], highs: &[f64], lows: &[f64], n: usize) -> Option<f64> {
    if closes.len() < n {
        return None;
    }
    let hh = highs[highs.len() - n..].iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    let ll = lows[lows.len() - n..].iter().cloned().fold(f64::INFINITY, f64::min);
    if hh <= ll {
        return Some(-50.0);
    }
    Some(round((hh - closes[closes.len() - 1]) / (hh - ll) * -100.0, 1))
}

/// `_stage(closes, ma200)` — Weinstein stage 1/2/3/4 (0 = undecided).
fn stage(closes: &[f64], ma200: &[f64]) -> i64 {
    if closes.len() < 60 || ma200.is_empty() {
        return 0;
    }
    let last = closes[closes.len() - 1];
    let ma200_now = ma200[ma200.len() - 1];
    let ma200_60ago = if ma200.len() >= 60 {
        ma200[ma200.len() - 60]
    } else {
        ma200[0]
    };
    let above = last > ma200_now;
    let rising = ma200_now > ma200_60ago;
    match (above, rising) {
        (true, true) => 2,
        (false, true) => 1,
        (true, false) => 3,
        (false, false) => 4,
    }
}

/// Python negative-slice window `arr[-end:-start]` (or `arr[-end:]`).
fn py_slice(arr: &[f64], start: usize, end: usize) -> &[f64] {
    let len = arr.len();
    let end = end.min(len);
    if start > 0 {
        let lo = len - end;
        let hi = (len as isize - start as isize).max(0) as usize;
        if lo < hi {
            &arr[lo..hi]
        } else {
            &[]
        }
    } else {
        &arr[len - end..]
    }
}

/// `_vcp_score(highs, lows)` — range contraction over 30/60/90-day windows.
/// Returns the Python int `0` when no contraction is measurable.
fn vcp_score(highs: &[f64], lows: &[f64]) -> Value {
    let rng = |start: usize, end: usize| -> f64 {
        let h = py_slice(highs, start, end);
        if h.is_empty() {
            return 0.0;
        }
        let l = py_slice(lows, start, end);
        let hmax = h.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
        let lmin = l.iter().cloned().fold(f64::INFINITY, f64::min);
        (hmax - lmin) / lmin.max(1e-9)
    };
    let (r30, r60, r90) = (rng(0, 30), rng(30, 60), rng(60, 90));
    if r60 > 0.0 && r90 > 0.0 {
        json_f64((1.0 - r30 / r60).max(0.0) + (1.0 - r60 / r90).max(0.0))
    } else {
        Value::from(0)
    }
}

/// `compute_indicators(klines)`.
pub fn compute_indicators(klines: &[Value]) -> Value {
    if klines.is_empty() {
        return Value::Object(Map::new());
    }
    let closes: Vec<f64> = klines.iter().map(|r| num_or_keys(r, &["收盘", "Close"])).collect();
    let highs: Vec<f64> = klines.iter().map(|r| num_or_keys(r, &["最高", "High"])).collect();
    let lows: Vec<f64> = klines.iter().map(|r| num_or_keys(r, &["最低", "Low"])).collect();
    let vols: Vec<f64> = klines.iter().map(|r| num_or_keys(r, &["成交量", "Volume"])).collect();
    if closes.is_empty() || closes.iter().all(|c| *c == 0.0) {
        return Value::Object(Map::new());
    }

    let ma5 = ma(&closes, 5);
    let ma10 = ma(&closes, 10);
    let ma20 = ma(&closes, 20);
    let ma60 = ma(&closes, 60);
    let ma120 = ma(&closes, 120);
    let ma200 = ma(&closes, 200);
    let ema12 = ema(&closes, 12);
    let ema26 = ema(&closes, 26);
    let dif: Vec<f64> = ema12.iter().zip(ema26.iter()).map(|(a, b)| a - b).collect();
    let dea = ema(&dif, 9);
    let macd_hist: Vec<f64> = dif.iter().zip(dea.iter()).map(|(d, e)| (d - e) * 2.0).collect();

    let last = closes[closes.len() - 1];
    let avg_vol_5 = if vols.len() >= 5 {
        vols[vols.len() - 5..].iter().sum::<f64>() / 5.0
    } else {
        0.0
    };
    let avg_vol_20 = if vols.len() >= 20 {
        vols[vols.len() - 20..].iter().sum::<f64>() / 20.0
    } else {
        0.0
    };

    let ma200_last = ma200.last().copied();
    let year_high = if closes.len() >= 250 {
        closes[closes.len() - 250..].iter().cloned().fold(f64::NEG_INFINITY, f64::max)
    } else {
        closes.iter().cloned().fold(f64::NEG_INFINITY, f64::max)
    };
    let year_low = if closes.len() >= 250 {
        closes[closes.len() - 250..].iter().cloned().fold(f64::INFINITY, f64::min)
    } else {
        closes.iter().cloned().fold(f64::INFINITY, f64::min)
    };
    let pct_from_year_high = if closes.len() >= 250 {
        json_f64((last - year_high) / year_high * 100.0)
    } else {
        Value::from(0)
    };
    let (kdj_k, kdj_d, kdj_j) = kdj(&closes, &highs, &lows, 9);
    let (obv_last, obv_trend_up) = obv(&closes, &vols);
    let golden_cross = if dif.len() > 1 {
        dif[dif.len() - 1] > dea[dea.len() - 1] && dif[dif.len() - 2] <= dea[dea.len() - 2]
    } else {
        false
    };

    let mut out = Map::new();
    out.insert("last_close".into(), json_f64(last));
    out.insert("ma5".into(), json_f64(ma5[ma5.len() - 1]));
    out.insert("ma10".into(), json_f64(ma10[ma10.len() - 1]));
    out.insert("ma20".into(), json_f64(ma20[ma20.len() - 1]));
    out.insert("ma60".into(), json_f64(ma60[ma60.len() - 1]));
    out.insert("ma120".into(), json_f64(ma120[ma120.len() - 1]));
    out.insert("ma200".into(), ma200_last.map(json_f64).unwrap_or(Value::Null));
    out.insert("above_ma20".into(), Value::Bool(last > ma20[ma20.len() - 1]));
    out.insert(
        "above_ma200".into(),
        ma200_last.map(|m| Value::Bool(last > m)).unwrap_or(Value::Null),
    );
    out.insert(
        "ma_bull_alignment".into(),
        Value::Bool(
            ma5[ma5.len() - 1] > ma10[ma10.len() - 1]
                && ma10[ma10.len() - 1] > ma20[ma20.len() - 1]
                && ma20[ma20.len() - 1] > ma60[ma60.len() - 1]
                && ma60[ma60.len() - 1] > ma120[ma120.len() - 1],
        ),
    );
    out.insert("macd_dif".into(), json_f64(dif[dif.len() - 1]));
    out.insert("macd_dea".into(), json_f64(dea[dea.len() - 1]));
    out.insert("macd_hist".into(), json_f64(macd_hist[macd_hist.len() - 1]));
    out.insert("macd_golden_cross".into(), Value::Bool(golden_cross));
    out.insert("rsi_14".into(), rsi(&closes, 14).map(json_f64).unwrap_or(Value::Null));
    out.insert("kdj_k".into(), kdj_k.map(json_f64).unwrap_or(Value::Null));
    out.insert("kdj_d".into(), kdj_d.map(json_f64).unwrap_or(Value::Null));
    out.insert("kdj_j".into(), kdj_j.map(json_f64).unwrap_or(Value::Null));
    out.insert("obv".into(), obv_last.map(json_f64).unwrap_or(Value::Null));
    out.insert("obv_trend_up".into(), obv_trend_up.map(Value::Bool).unwrap_or(Value::Null));
    out.insert(
        "williams_r".into(),
        williams_r(&closes, &highs, &lows, 14).map(json_f64).unwrap_or(Value::Null),
    );
    out.insert("year_high".into(), json_f64(year_high));
    out.insert("year_low".into(), json_f64(year_low));
    out.insert("pct_from_year_high".into(), pct_from_year_high);
    out.insert("stage".into(), Value::from(stage(&closes, &ma200)));
    out.insert(
        "vol_5_vs_20".into(),
        if avg_vol_20 != 0.0 {
            json_f64(avg_vol_5 / avg_vol_20)
        } else {
            Value::Null
        },
    );
    out.insert("vcp_score".into(), vcp_score(&highs, &lows));
    Value::Object(out)
}

fn json_f64(x: f64) -> Value {
    serde_json::Number::from_f64(x).map(Value::Number).unwrap_or(Value::Null)
}

// ─────────────────────────────────────────────────────────────
// Chip distribution
// ─────────────────────────────────────────────────────────────

/// `fetch_chip_distribution(ti)` — `ak.stock_cyq_em` is AkShare-only; upstream
/// returns `{"error": str(e)}` when the call raises.
pub fn fetch_chip_distribution(ti: &uzi_core::ticker::TickerInfo) -> Value {
    if ti.market != "A" {
        return Value::Object(Map::new());
    }
    serde_json::json!({"error": "ImportError: akshare not installed"})
}

// ─────────────────────────────────────────────────────────────
// Viz shape
// ─────────────────────────────────────────────────────────────

/// `_extract_for_viz(klines)` — `_v(r, *keys, default=0)` semantics (first
/// present non-null key, even when zero).
fn viz_num(row: &Value, keys: &[&str]) -> f64 {
    for k in keys {
        if let Some(v) = row.get(*k) {
            if !v.is_null() {
                if let Some(x) = as_f64(v) {
                    return x;
                }
            }
        }
    }
    0.0
}

/// `statistics.stdev` (sample) — `None` when fewer than two points.
fn sample_stdev(xs: &[f64]) -> Option<f64> {
    let n = xs.len();
    if n < 2 {
        return None;
    }
    let mean = xs.iter().sum::<f64>() / n as f64;
    let var = xs.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / (n - 1) as f64;
    Some(var.sqrt())
}

/// `_extract_for_viz(klines)`.
pub fn extract_for_viz(klines: &[Value]) -> Value {
    if klines.is_empty() {
        return Value::Object(Map::new());
    }
    let closes: Vec<f64> = klines.iter().map(|r| viz_num(r, &["收盘", "Close"])).collect();
    let opens: Vec<f64> = klines.iter().map(|r| viz_num(r, &["开盘", "Open"])).collect();
    let highs: Vec<f64> = klines.iter().map(|r| viz_num(r, &["最高", "High"])).collect();
    let lows: Vec<f64> = klines.iter().map(|r| viz_num(r, &["最低", "Low"])).collect();

    let dates: Vec<String> = klines
        .iter()
        .map(|r| {
            let d = r
                .get("日期")
                .filter(|v| truthy(v))
                .or_else(|| r.get("Date").filter(|v| truthy(v)));
            let s = d.map(uzi_core::py::py_str).unwrap_or_default();
            s.chars().take(10).collect()
        })
        .collect();

    let last_n = klines.len().min(60);
    let start_i = klines.len() - last_n;
    let candles: Vec<Value> = (start_i..klines.len())
        .map(|i| {
            serde_json::json!({
                "date": dates[i],
                "open": round(opens[i], 2),
                "close": round(closes[i], 2),
                "high": round(highs[i], 2),
                "low": round(lows[i], 2),
            })
        })
        .collect();

    let ma20_full = ma(&closes, 20);
    let ma60_full = ma(&closes, 60);
    let ma20_60d: Vec<Value> = ma20_full
        .iter()
        .enumerate()
        .map(|(i, v)| if i >= 19 { json_f64(round(*v, 2)) } else { Value::Null })
        .collect::<Vec<_>>()[start_i..]
        .to_vec();
    let ma60_60d: Vec<Value> = ma60_full
        .iter()
        .enumerate()
        .map(|(i, v)| if i >= 59 { json_f64(round(*v, 2)) } else { Value::Null })
        .collect::<Vec<_>>()[start_i..]
        .to_vec();

    let mut stats = Map::new();
    if closes.len() >= 252 {
        let ytd_idx = closes.len() - 252;
        let ytd_return = (closes[closes.len() - 1] - closes[ytd_idx]) / closes[ytd_idx] * 100.0;
        stats.insert("ytd_return".into(), Value::String(format!("{ytd_return:+.1}%")));
    }
    if closes.len() >= 20 {
        let rets: Vec<f64> = (1..closes.len()).map(|i| closes[i] / closes[i - 1] - 1.0).collect();
        if !rets.is_empty() {
            let window = if rets.len() >= 252 { &rets[rets.len() - 252..] } else { &rets[..] };
            if let Some(sd) = sample_stdev(window) {
                let vol = sd * 252f64.sqrt() * 100.0;
                stats.insert("volatility".into(), Value::String(format!("{vol:.1}%")));
            }
        }
        let window: &[f64] = if closes.len() >= 252 {
            &closes[closes.len() - 252..]
        } else {
            &closes[..]
        };
        let mut peak = window[0];
        let mut max_dd = 0.0_f64;
        for &c in window {
            if c > peak {
                peak = c;
            }
            let dd = (c - peak) / peak;
            if dd < max_dd {
                max_dd = dd;
            }
        }
        stats.insert("max_drawdown".into(), Value::String(format!("{:.1}%", max_dd * 100.0)));
    }

    let close_60d: Vec<Value> = closes[start_i..].iter().map(|c| json_f64(round(*c, 2))).collect();

    let mut out = Map::new();
    out.insert("candles_60d".into(), Value::Array(candles));
    out.insert("ma20_60d".into(), Value::Array(ma20_60d));
    out.insert("ma60_60d".into(), Value::Array(ma60_60d));
    out.insert("close_60d".into(), Value::Array(close_60d));
    out.insert("kline_stats".into(), Value::Object(stats));
    Value::Object(out)
}

// ─────────────────────────────────────────────────────────────
// main
// ─────────────────────────────────────────────────────────────

const STAGE_LABEL: [&str; 5] = ["—", "Stage 1 底部", "Stage 2 上升", "Stage 3 顶部", "Stage 4 下跌"];

/// `main(ticker)`.
pub fn main(ticker: &str) -> Result<Value, String> {
    let ti = parse_ticker(ticker);
    let klines: Vec<Value> = crate::sources::fetch_kline(&ti, "daily", "20240101", "qfq")
        .as_array()
        .cloned()
        .unwrap_or_default();
    let chips = fetch_chip_distribution(&ti);
    Ok(assemble_dim(
        &ti.full,
        &klines,
        chips,
        "akshare:stock_zh_a_hist + stock_cyq_em (+ 6 path fallback chain)",
    ))
}

/// Build the `2_kline` legacy payload from raw OHLCV rows.
///
/// Shared by the equity path ([`main`]) and the crypto path
/// (`uzi-data::crypto`), so both produce the identical indicator/viz shape the
/// scorers and renderers consume. Raw rows use the A-share key names
/// (`日期`/`开盘`/`收盘`/`最高`/`最低`/`成交量`).
pub fn assemble_dim(ticker: &str, klines: &[Value], chips: Value, source: &str) -> Value {
    let indicators = compute_indicators(klines);
    let viz_shape = extract_for_viz(klines);

    let stage = indicators.get("stage").and_then(|v| v.as_i64()).unwrap_or(0);
    let stage_label = STAGE_LABEL.get(stage as usize).copied().unwrap_or("—");
    let ma_align = if truthy(indicators.get("ma_bull_alignment").unwrap_or(&Value::Null)) {
        "多头排列"
    } else {
        "非多头"
    };
    let macd_dif = indicators.get("macd_dif").and_then(as_f64).unwrap_or(0.0);
    let macd_hist = indicators.get("macd_hist").and_then(as_f64).unwrap_or(0.0);
    let golden_cross = truthy(indicators.get("macd_golden_cross").unwrap_or(&Value::Null));
    let macd_label = if golden_cross && macd_dif > 0.0 {
        "金叉水上"
    } else if macd_dif > 0.0 && macd_hist < 0.0 {
        "死叉水上"
    } else if macd_dif < 0.0 {
        "水下"
    } else {
        "中性"
    };
    let rsi_label = match indicators.get("rsi_14") {
        Some(v) if !v.is_null() => format!("{:.0}", as_f64(v).unwrap_or(0.0)),
        _ => "—".to_string(),
    };

    let mut data = Map::new();
    data.insert("kline_count".into(), Value::from(klines.len() as u64));
    data.insert("indicators".into(), indicators);
    data.insert("stage".into(), Value::String(stage_label.to_string()));
    data.insert("ma_align".into(), Value::String(ma_align.to_string()));
    data.insert("macd".into(), Value::String(macd_label.to_string()));
    data.insert("rsi".into(), Value::String(rsi_label));
    data.insert("chip_distribution".into(), chips);
    if let Value::Object(viz) = viz_shape {
        for (k, v) in viz {
            data.insert(k, v);
        }
    }

    serde_json::json!({
        "ticker": ticker,
        "data": Value::Object(data),
        "source": source,
        "fallback": false,
    })
}
