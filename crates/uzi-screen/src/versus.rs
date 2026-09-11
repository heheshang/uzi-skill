//! Port of `lib/versus_runner.py` — 2-4 ticker head-to-head comparison.
//!
//! `versus_template.txt` is the upstream `_render_html` f-string with the
//! interpolations replaced by `@TOKEN@` placeholders (and f-string `{{`/`}}`
//! un-doubled); `tests/golden_runners.rs` compares the assembled document
//! byte-for-byte against the Python output.

use anyhow::{bail, Result};
use serde_json::{Map, Value};

use crate::paths::reports_root;
use crate::providers::run_pipeline;

const VERSUS_TEMPLATE: &str = include_str!("versus_template.txt");

/// The `<style>` block shared with the main report template.
fn main_style() -> &'static str {
    crate::daily_screen::renderer::main_style()
}

/// `_safe(v, default="—")` → `str`.
pub(crate) fn safe_text(value: &Value, default: &str) -> String {
    if value.is_null() || value.as_str() == Some("") || value.as_str() == Some("?") {
        default.to_string()
    } else {
        uzi_core::py::py_display(value)
    }
}

/// `_esc(v, default)` — `html.escape(str(_safe(v, default)), quote=True)`.
pub(crate) fn esc(value: &Value, default: &str) -> String {
    uzi_report::security::html_escape(&safe_text(value, default))
}

/// `_num(v, decimals)`.
pub(crate) fn num_str(value: &Value, decimals: usize) -> String {
    match py_float(value) {
        Some(x) => format!("{:.*}", decimals, x),
        None => "—".to_string(),
    }
}

/// Python `float(x) if x is not None else None`.
pub(crate) fn py_float(value: &Value) -> Option<f64> {
    match value {
        Value::Null => None,
        Value::Number(n) => n.as_f64(),
        Value::Bool(b) => Some(if *b { 1.0 } else { 0.0 }),
        Value::String(s) => crate::daily_screen::universe::parse_python_float(s.trim()),
        _ => None,
    }
}

/// `_winner(values, higher_is_better=True)` — index of the winner, `-1` if all
/// missing/zero. Ties keep the first occurrence (Python `max`/`min`).
fn winner(values: &[Option<f64>], higher_is_better: bool) -> i64 {
    let valid: Vec<(usize, f64)> = values
        .iter()
        .enumerate()
        .filter_map(|(i, v)| match v {
            Some(x) if *x != 0.0 => Some((i, *x)),
            _ => None,
        })
        .collect();
    if valid.is_empty() {
        return -1;
    }
    let mut best = 0;
    for i in 1..valid.len() {
        let better = if higher_is_better {
            valid[i].1 > valid[best].1
        } else {
            valid[i].1 < valid[best].1
        };
        if better {
            best = i;
        }
    }
    valid[best].0 as i64
}

/// `_load_cache(ticker)` — `{syn, raw, panel, ticker}`, or `None` (with the
/// upstream warning) when synthesis/raw_data are missing.
pub fn load_cache(ticker: &str) -> Option<Value> {
    let syn = uzi_core::cache::read_task_output(ticker, "synthesis");
    let raw = uzi_core::cache::read_task_output(ticker, "raw_data");
    let (Some(syn), Some(raw)) = (syn, raw) else {
        println!("   ⚠️  {ticker} cache 读取失败: FileNotFoundError");
        return None;
    };
    let panel = uzi_core::cache::read_task_output(ticker, "panel")
        .unwrap_or_else(|| Value::Object(Map::new()));
    let mut out = Map::new();
    out.insert("syn".into(), syn);
    out.insert("raw".into(), raw);
    out.insert("panel".into(), panel);
    out.insert("ticker".into(), Value::String(ticker.to_string()));
    Some(Value::Object(out))
}

fn dim_data<'a>(dims: &'a Value, key: &str) -> Value {
    match dims.get(key).and_then(|v| v.get("data")) {
        Some(v) if v.is_object() => v.clone(),
        _ => Value::Object(Map::new()),
    }
}

/// `_extract_metrics(bundle)`.
pub fn extract_metrics(bundle: &Value) -> Value {
    let syn = uzi_core::py::get(bundle, "syn");
    let raw = uzi_core::py::get(bundle, "raw");
    let panel = uzi_core::py::get(bundle, "panel");
    let dims = match raw.get("dimensions") {
        Some(v) if v.is_object() => v.clone(),
        _ => Value::Object(Map::new()),
    };
    let basic = dim_data(&dims, "0_basic");
    let fin = dim_data(&dims, "1_financials");
    let val = dim_data(&dims, "10_valuation");
    let sig = match panel.get("signal_distribution") {
        Some(v) if v.is_object() => v.clone(),
        _ => Value::Object(Map::new()),
    };
    let ticker = uzi_core::py::get(bundle, "ticker");
    let ticker_text = safe_text(ticker, "");
    let name = {
        let primary = uzi_core::py::get(&syn, "name");
        let fallback = uzi_core::py::get(&basic, "name");
        if uzi_core::py::truthy(primary) {
            primary.clone()
        } else if uzi_core::py::truthy(fallback) {
            fallback.clone()
        } else {
            ticker.clone()
        }
    };
    let or_value = |primary: &Value, fallback: &Value| -> Value {
        if uzi_core::py::truthy(primary) {
            primary.clone()
        } else {
            fallback.clone()
        }
    };
    let industry = or_value(
        uzi_core::py::get(&basic, "industry"),
        &Value::String("—".to_string()),
    );
    let verdict = or_value(
        uzi_core::py::get(&syn, "verdict_label"),
        &Value::String("—".to_string()),
    );
    let verdict_detail = or_value(
        uzi_core::py::get(&syn, "verdict_detail"),
        &Value::String(String::new()),
    );
    let punchline = match uzi_core::py::get(&syn, "debate").get("punchline") {
        Some(v) => v.clone(),
        None => Value::String(String::new()),
    };
    let trap_level = match dims
        .get("18_trap")
        .and_then(|v| v.get("data"))
        .and_then(|v| v.get("trap_level"))
    {
        Some(v) => v.clone(),
        None => Value::String("🟢 安全".to_string()),
    };

    let mut out = Map::new();
    out.insert("ticker".into(), Value::String(ticker_text));
    out.insert("name".into(), name);
    out.insert("industry".into(), industry);
    out.insert(
        "price".into(),
        opt_float(py_float(uzi_core::py::get(&basic, "price"))),
    );
    out.insert(
        "market_cap_yi".into(),
        opt_float(py_float(&or_value(
            uzi_core::py::get(&basic, "market_cap_yi"),
            uzi_core::py::get(&basic, "market_cap"),
        ))),
    );
    out.insert(
        "pe_ttm".into(),
        opt_float(py_float(&or_value(
            uzi_core::py::get(&basic, "pe_ttm"),
            uzi_core::py::get(&val, "pe_ttm"),
        ))),
    );
    out.insert(
        "pb".into(),
        opt_float(py_float(&or_value(
            uzi_core::py::get(&basic, "pb"),
            uzi_core::py::get(&val, "pb"),
        ))),
    );
    out.insert(
        "roe".into(),
        opt_float(py_float(&or_value(
            uzi_core::py::get(&fin, "roe"),
            uzi_core::py::get(&fin, "roe_ttm"),
        ))),
    );
    out.insert(
        "net_margin".into(),
        opt_float(py_float(uzi_core::py::get(&fin, "net_margin"))),
    );
    out.insert(
        "gross_margin".into(),
        opt_float(py_float(uzi_core::py::get(&fin, "gross_margin"))),
    );
    out.insert(
        "rev_growth_3y".into(),
        opt_float(py_float(&or_value(
            uzi_core::py::get(&fin, "rev_growth_3y"),
            uzi_core::py::get(&fin, "revenue_growth_3y"),
        ))),
    );
    out.insert(
        "overall_score".into(),
        opt_float(py_float(uzi_core::py::get(&syn, "overall_score"))),
    );
    out.insert(
        "fund_score".into(),
        opt_float(py_float(uzi_core::py::get(&syn, "fundamental_score"))),
    );
    out.insert(
        "consensus".into(),
        opt_float(py_float(&or_value(
            uzi_core::py::get(&syn, "panel_consensus"),
            uzi_core::py::get(&panel, "panel_consensus"),
        ))),
    );
    out.insert("verdict".into(), verdict);
    out.insert("verdict_detail".into(), verdict_detail);
    out.insert("bull_count".into(), count_or_zero(&sig, "bullish"));
    out.insert("bear_count".into(), count_or_zero(&sig, "bearish"));
    out.insert("neutral_count".into(), count_or_zero(&sig, "neutral"));
    out.insert("skip_count".into(), count_or_zero(&sig, "skip"));
    out.insert("punchline".into(), punchline);
    out.insert("trap_level".into(), trap_level);
    out.insert(
        "school_lock".into(),
        uzi_core::py::get(&syn, "school_lock").clone(),
    );
    out.insert("report_path".into(), Value::Null);
    Value::Object(out)
}

fn opt_float(value: Option<f64>) -> Value {
    match value {
        Some(x) => Value::from(x),
        None => Value::Null,
    }
}

fn count_or_zero(sig: &Value, key: &str) -> Value {
    match sig.get(key) {
        Some(v) => v.clone(),
        None => Value::from(0),
    }
}

/// Trailing slice `s[:n]` for Python strings.
pub(crate) fn take_chars(value: &Value, n: usize) -> String {
    uzi_core::py::py_display(value).chars().take(n).collect()
}

/// `ROWS` — `(label, key, decimals, higher_is_better, group)`.
const ROWS: &[(&str, &str, Option<usize>, Option<bool>)] = &[
    ("价格", "price", Some(2), None),
    ("市值（亿）", "market_cap_yi", Some(0), None),
    ("行业", "industry", None, None),
    ("PE TTM", "pe_ttm", Some(1), Some(false)),
    ("PB", "pb", Some(2), Some(false)),
    ("ROE %", "roe", Some(1), Some(true)),
    ("净利率 %", "net_margin", Some(1), Some(true)),
    ("毛利率 %", "gross_margin", Some(1), Some(true)),
    ("3y 营收增速 %", "rev_growth_3y", Some(1), Some(true)),
    ("总评 /100", "overall_score", Some(1), Some(true)),
    ("基本面分 /100", "fund_score", Some(1), Some(true)),
    ("评委共识 %", "consensus", Some(1), Some(true)),
];

/// `_render_comparison_grid(stocks)`.
pub fn render_comparison_grid(stocks: &[Value]) -> String {
    let n = stocks.len();
    let col_pct = if n == 0 {
        format!("{:.2}%", 100.0)
    } else {
        format!("{:.2}%", 100.0 / (n as f64 + 1.0))
    };
    let mut header_cells = vec![format!(
        "<th style=\"width:{col_pct};text-align:left;padding:12px 16px\">指标</th>"
    )];
    for s in stocks {
        header_cells.push(format!(
            "<th style=\"width:{col_pct};padding:12px 16px;text-align:center\"><div style=\"font-size:16px;font-weight:700;color:var(--text-bright)\">{}</div><div style=\"font-family:Fira Code,monospace;font-size:11px;color:var(--text-dim);margin-top:2px\">{}</div></th>",
            esc(uzi_core::py::get(s, "name"), "—"),
            esc(uzi_core::py::get(s, "ticker"), "—"),
        ));
    }
    let header_row = format!("<tr>{}</tr>", header_cells.join(""));

    let mut rows_html: Vec<String> = Vec::new();
    for (label, key, dec, higher) in ROWS {
        let vals: Vec<Option<f64>> = stocks
            .iter()
            .map(|s| s.get(*key).and_then(py_float))
            .collect();
        let all_missing = stocks.iter().all(|s| match s.get(*key) {
            None | Some(Value::Null) => true,
            Some(Value::String(text)) => text == "—",
            _ => false,
        });
        if all_missing {
            continue;
        }
        let real_idx = match higher {
            Some(higher_is_better) => {
                let numeric: Vec<Option<f64>> = vals.clone();
                let idx = winner(&numeric, *higher_is_better);
                if idx >= 0 {
                    numeric
                        .iter()
                        .enumerate()
                        .filter(|(_, v)| v.is_some())
                        .nth(idx as usize)
                        .map(|(i, _)| i as i64)
                        .unwrap_or(-1)
                } else {
                    -1
                }
            }
            None => -1,
        };
        let mut cells = vec![format!(
            "<td style=\"padding:10px 16px;color:var(--text-mid);font-size:12px;letter-spacing:.06em\">{label}</td>"
        )];
        for (i, s) in stocks.iter().enumerate() {
            let raw = s.get(*key).cloned().unwrap_or(Value::Null);
            let display = if raw.is_null() {
                "—".to_string()
            } else if dec.is_none() {
                uzi_core::py::py_display(&raw)
            } else {
                num_str(&raw, dec.unwrap_or(0))
            };
            let is_winner = i as i64 == real_idx;
            let color = if is_winner {
                "var(--bull-green)"
            } else {
                "var(--text-main)"
            };
            let weight = if is_winner { "700" } else { "500" };
            let badge = if is_winner {
                " <span style=\"font-size:9px;color:var(--bull-green);letter-spacing:.1em\">★ WIN</span>"
            } else {
                ""
            };
            cells.push(format!(
                "<td style=\"padding:10px 16px;text-align:center;color:{color};font-weight:{weight};font-variant-numeric:tabular-nums\">{display}{badge}</td>"
            ));
        }
        rows_html.push(format!("<tr>{}</tr>", cells.join("")));
    }

    format!(
        "<table style=\"width:100%;border-collapse:separate;border-spacing:0;background:var(--bg-card);border:1px solid var(--border);border-radius:12px;overflow:hidden\"><thead style=\"background:var(--bg-tinted);border-bottom:2px solid var(--border)\">{header_row}</thead><tbody>{}</tbody></table>",
        rows_html.join("")
    )
}

/// `_render_verdict_cards(stocks)`.
pub fn render_verdict_cards(stocks: &[Value]) -> String {
    let mut cards: Vec<String> = Vec::new();
    for s in stocks {
        let sc = py_float(uzi_core::py::get(s, "overall_score")).filter(|x| *x != 0.0);
        let sc = sc.unwrap_or(0.0);
        let sc_color = if sc >= 65.0 {
            "var(--bull-green)"
        } else if sc >= 50.0 {
            "var(--neon-gold)"
        } else {
            "var(--bear-red)"
        };
        let count = |key: &str| -> Value {
            match s.get(key) {
                Some(v) => v.clone(),
                None => Value::from(0),
            }
        };
        cards.push(format!(
            "<div style=\"flex:1;min-width:240px;background:var(--bg-card);border:1px solid var(--border);border-radius:12px;padding:20px;box-shadow:var(--shadow-sm)\">  <div style=\"font-size:11px;color:var(--text-dim);letter-spacing:.14em;margin-bottom:4px\">{}</div>  <div style=\"font-size:18px;font-weight:700;color:var(--text-bright)\">{}</div>  <div style=\"font-size:11px;color:var(--text-dim);margin-top:2px\">{}</div>  <div style=\"margin:14px 0;display:flex;align-items:baseline;gap:8px\">    <span style=\"font-size:36px;font-weight:900;color:{sc_color};font-variant-numeric:tabular-nums\" class=\"count-up\">{}</span>    <span style=\"font-size:14px;color:var(--text-dim)\">/ 100</span>  </div>  <div style=\"font-size:13px;font-weight:600;color:{sc_color}\">{}</div>  <div style=\"font-size:11px;color:var(--text-dim);margin-top:4px\">{}</div>  <div style=\"margin-top:14px;padding-top:14px;border-top:1px dashed var(--border);display:flex;gap:10px;font-size:11px\">    <span style=\"color:var(--bull-green)\">📈 {}</span>    <span style=\"color:var(--text-dim)\">⚖️ {}</span>    <span style=\"color:var(--bear-red)\">📉 {}</span>  </div>  <div style=\"margin-top:10px;font-size:11px;color:var(--text-mid);font-style:italic;line-height:1.4\">    {}  </div></div>",
            esc(uzi_core::py::get(s, "ticker"), "—"),
            esc(uzi_core::py::get(s, "name"), "—"),
            esc(uzi_core::py::get(s, "industry"), "—"),
            num_str(&Value::from(sc), 1),
            esc(uzi_core::py::get(s, "verdict"), "—"),
            esc(uzi_core::py::get(s, "verdict_detail"), ""),
            uzi_core::py::py_display(&count("bull_count")),
            uzi_core::py::py_display(&count("neutral_count")),
            uzi_core::py::py_display(&count("bear_count")),
            esc(
                &Value::String(take_chars(uzi_core::py::get(s, "punchline"), 120)),
                ""
            ),
        ));
    }
    format!(
        "<div style=\"display:flex;gap:16px;flex-wrap:wrap;margin:24px 0\">{}</div>",
        cards.join("")
    )
}

/// `_render_html(stocks, depth)`.
pub fn render_html(stocks: &[Value], depth: &str) -> String {
    let titles = stocks
        .iter()
        .map(|s| esc(uzi_core::py::get(s, "name"), "—"))
        .collect::<Vec<_>>()
        .join(" VS ");
    let tickers_csv = stocks
        .iter()
        .map(|s| esc(uzi_core::py::get(s, "ticker"), "—"))
        .collect::<Vec<_>>()
        .join(" · ");
    let now = chrono::Utc::now()
        .with_timezone(&crate::daily_screen::events::shanghai_offset())
        .format("%Y-%m-%d %H:%M")
        .to_string();

    let verdict_cards = render_verdict_cards(stocks);
    let grid = render_comparison_grid(stocks);

    let mut punch_block = String::new();
    if stocks
        .iter()
        .all(|s| uzi_core::py::truthy(uzi_core::py::get(s, "punchline")))
    {
        let mut cells: Vec<String> = Vec::new();
        for s in stocks {
            cells.push(format!(
                "<div style=\"flex:1;padding:16px 20px;background:var(--bg-tinted);border-left:3px solid var(--neon-cyan);border-radius:8px\"><div style=\"font-size:10px;letter-spacing:.16em;color:var(--neon-cyan);margin-bottom:6px\">PUNCHLINE · {}</div><div style=\"font-size:13px;color:var(--text-main);line-height:1.5;font-style:italic\">{}</div></div>",
                esc(uzi_core::py::get(s, "name"), "—"),
                esc(
                    &Value::String(take_chars(uzi_core::py::get(s, "punchline"), 200)),
                    ""
                ),
            ));
        }
        punch_block = format!(
            "<div style=\"display:flex;gap:14px;margin:24px 0;flex-wrap:wrap\">{}</div>",
            cells.join("")
        );
    }

    let first_ticker = stocks
        .first()
        .map(|s| uzi_core::py::py_display(uzi_core::py::get(s, "ticker")))
        .unwrap_or_default();
    VERSUS_TEMPLATE
        .replace("@TITLES@", &titles)
        .replace("@MAIN_STYLE@", main_style())
        .replace("@DEPTH@", depth)
        .replace("@NOW@", &now)
        .replace("@FIRST_TICKER@", &first_ticker)
        .replace("@TICKERS@", &tickers_csv)
        .replace("@CARDS@", &verdict_cards)
        .replace("@GRID@", &grid)
        .replace("@PUNCH@", &punch_block)
}

/// `run_versus(tickers, depth="lite", auto_open=True)` → report path.
///
/// `auto_open` is not applicable to a library entry point; the caller decides
/// whether to open the file.
pub fn run_versus(tickers: &[String]) -> Result<String> {
    if !(2..=4).contains(&tickers.len()) {
        bail!("--versus 接受 2-4 只 · 实际 {}", tickers.len());
    }
    let depth = "lite";
    println!();
    println!("{}", "━".repeat(60));
    println!("⚔️  横向对比模式 · {} · depth={depth}", tickers.join(" VS "));
    println!("{}", "━".repeat(60));
    if std::env::var("UZI_DEPTH").is_err() {
        std::env::set_var("UZI_DEPTH", depth);
    }

    let mut bundles: Vec<Value> = Vec::new();
    let started = std::time::Instant::now();
    for (i, ticker) in tickers.iter().enumerate() {
        println!("\n━━━ [{}/{}] {ticker} ━━━", i + 1, tickers.len());
        match run_pipeline(ticker) {
            Some(Ok(_)) => {}
            Some(Err(err)) => {
                println!(
                    "   ⚠️  {ticker} pipeline 异常: {err} · 继续读 cache"
                );
            }
            None => {
                bail!("pipeline_unavailable: run_pipeline is not registered");
            }
        }
        let info = uzi_core::parse_ticker(ticker);
        let bundle = load_cache(&info.full).or_else(|| load_cache(ticker));
        match bundle {
            Some(bundle) => bundles.push(bundle),
            None => println!("   ⚠️  {ticker} cache 缺失 · 跳过对比"),
        }
    }
    if bundles.len() < 2 {
        bail!("至少需要 2 只票成功读到 cache · 才能生成对比报告");
    }
    let metrics: Vec<Value> = bundles.iter().map(extract_metrics).collect();
    let safe_keys = metrics
        .iter()
        .map(|m| safe_text(uzi_core::py::get(m, "ticker"), "").replace('.', "_"))
        .collect::<Vec<_>>()
        .join("_vs_");
    let date = chrono::Utc::now()
        .with_timezone(&crate::daily_screen::events::shanghai_offset())
        .format("%Y%m%d")
        .to_string();
    let out_dir = reports_root().join(format!("versus_{safe_keys}_{date}"));
    std::fs::create_dir_all(&out_dir)?;
    let out_file = out_dir.join("index.html");
    std::fs::write(&out_file, render_html(&metrics, depth))?;
    let dt = started.elapsed().as_secs();
    println!("\n━━━ 横向对比完成 · {dt}s ━━━");
    println!("📄 报告: {}", out_file.display());
    Ok(out_file.to_string_lossy().to_string())
}
