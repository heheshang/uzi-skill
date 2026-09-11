//! Port of `lib/portfolio_runner.py` — CSV portfolio health analysis.
//!
//! `portfolio_template.txt` is the upstream `_render_html` f-string with
//! `@TOKEN@` placeholders (raw float interpolations go through
//! `uzi_core::py::float_str`, since an f-string renders `81.0`, not `81`).

use anyhow::{bail, Result};
use serde_json::{Map, Value};
use std::path::{Path, PathBuf};

use crate::csvlite::{parse_records, strip_bom};
use crate::paths::reports_root;
use crate::providers::run_pipeline;
use crate::versus::{esc, extract_metrics, load_cache, num_str, py_float, safe_text};

const PORTFOLIO_TEMPLATE: &str = include_str!("portfolio_template.txt");

/// `_parse_csv(path)` — tolerant holdings parser.
pub fn parse_csv(path: &Path) -> Result<Vec<Value>> {
    if !path.exists() {
        bail!("组合文件不存在: {}", path.display());
    }
    let text = std::fs::read_to_string(path)
        .map_err(|err| anyhow::anyhow!("组合文件读取失败: {err}"))?;
    let text = strip_bom(&text);
    let first_line = text.lines().next().unwrap_or("").trim().to_string();
    let has_header = ["ticker", "code", "symbol", "weight", "权重", "股票"]
        .iter()
        .any(|key| first_line.to_lowercase().contains(key));

    let mut rows: Vec<Value> = Vec::new();
    let records = parse_records(text);
    if has_header {
        let fieldnames: Vec<String> = records.first().cloned().unwrap_or_default();
        let ticker_keys = ["ticker", "code", "symbol", "股票", "代码"];
        let weight_keys = ["weight", "权重", "仓位", "pct", "比例"];
        let note_keys = ["note", "备注", "remark"];
        for record in records.iter().skip(1) {
            // csv.DictReader: short rows pad with None, extra fields land under
            // the `None` key (dropped here, like upstream's `if k`).
            let mut norm: Map<String, Value> = Map::new();
            for (index, name) in fieldnames.iter().enumerate() {
                if name.is_empty() {
                    continue;
                }
                let value = record
                    .get(index)
                    .cloned()
                    .map(Value::String)
                    .unwrap_or(Value::Null);
                norm.insert(name.to_lowercase().trim().to_string(), value);
            }
            let lookup = |keys: &[&str]| -> Option<Value> {
                for key in keys {
                    if let Some(value) = norm.get(*key) {
                        if uzi_core::py::truthy(value) {
                            return Some(value.clone());
                        }
                    }
                }
                None
            };
            let Some(ticker) = lookup(&ticker_keys) else {
                continue;
            };
            let weight_raw = lookup(&weight_keys);
            let note = lookup(&note_keys);
            let weight = match weight_raw.and_then(|v| py_float(&v)) {
                Some(mut w) => {
                    if w > 1.0 {
                        w /= 100.0;
                    }
                    Some(w)
                }
                None => None,
            };
            let mut row = Map::new();
            row.insert(
                "ticker".into(),
                Value::String(safe_text(&ticker, "").trim().to_string()),
            );
            row.insert("weight".into(), opt_float(weight));
            row.insert(
                "note".into(),
                Value::String(safe_text(&note.unwrap_or(Value::Null), "").trim().to_string()),
            );
            rows.push(Value::Object(row));
        }
    } else {
        for record in records {
            if let Some(first) = record.first() {
                if !first.trim().is_empty() {
                    rows.push(serde_json::json!({
                        "ticker": first.trim(),
                        "weight": Value::Null,
                        "note": "",
                    }));
                }
            }
        }
    }
    if rows.is_empty() {
        bail!("组合文件 {} 解析后为空", path.display());
    }
    Ok(rows)
}

fn opt_float(value: Option<f64>) -> Value {
    match value {
        Some(x) => Value::from(x),
        None => Value::Null,
    }
}

/// `_normalize_weights(holdings)` — mutates in place, returns the same list.
pub fn normalize_weights(holdings: &mut [Value]) -> Vec<Value> {
    let n = holdings.len();
    let mut weighted: Vec<usize> = Vec::new();
    let mut unweighted: Vec<usize> = Vec::new();
    for (i, h) in holdings.iter().enumerate() {
        match h.get("weight") {
            Some(Value::Null) | None => unweighted.push(i),
            _ => weighted.push(i),
        }
    }
    let set_weight = |h: &mut Value, value: f64| {
        if let Some(map) = h.as_object_mut() {
            map.insert("weight".into(), Value::from(value));
        }
    };
    if weighted.is_empty() {
        for index in 0..n {
            set_weight(&mut holdings[index], 1.0 / n as f64);
        }
        return holdings.to_vec();
    }
    let weight_of = |h: &Value| -> f64 { h.get("weight").and_then(|v| v.as_f64()).unwrap_or(0.0) };
    let mut total: f64 = weighted.iter().map(|i| weight_of(&holdings[*i])).sum();
    if !unweighted.is_empty() {
        let remain = (1.0 - total).max(0.0);
        let share = if remain > 0.0 {
            remain / unweighted.len() as f64
        } else {
            0.0
        };
        for index in unweighted {
            set_weight(&mut holdings[index], share);
        }
        total = holdings.iter().map(weight_of).sum();
    }
    if total > 0.0 {
        for holding in holdings.iter_mut() {
            let value = weight_of(holding) / total;
            set_weight(holding, value);
        }
    }
    holdings.to_vec()
}

/// `_load_metrics_for(ticker)`.
pub fn load_metrics_for(ticker: &str) -> Option<Value> {
    load_cache(ticker).map(|bundle| extract_metrics(&bundle))
}

/// `_portfolio_health(metrics)`.
pub fn portfolio_health(metrics: &[Value]) -> Value {
    let valid: Vec<&Value> = metrics
        .iter()
        .filter(|m| !uzi_core::py::get(m, "overall_score").is_null())
        .collect();
    if valid.is_empty() {
        return serde_json::json!({
            "weighted_score": 0,
            "max_weight": 0,
            "n_industries": 0,
            "verdict": "数据不足",
        });
    }
    let weighted_score: f64 = valid
        .iter()
        .map(|m| {
            uzi_core::py::f0(uzi_core::py::get(m, "overall_score"))
                * uzi_core::py::f0(uzi_core::py::get(m, "_weight"))
        })
        .sum();
    let max_weight = valid
        .iter()
        .map(|m| uzi_core::py::f0(uzi_core::py::get(m, "_weight")))
        .fold(f64::NEG_INFINITY, f64::max);
    let mut industries: Vec<String> = Vec::new();
    for m in &valid {
        let industry = m
            .get("industry")
            .map(uzi_core::py::py_display)
            .unwrap_or_else(|| "—".to_string());
        if industry != "—" && !industries.contains(&industry) {
            industries.push(industry);
        }
    }
    industries.sort();
    let verdict = if weighted_score >= 70.0 && max_weight < 0.40 && industries.len() >= 3 {
        "🟢 健康 · 加权分高 · 分散度好"
    } else if weighted_score >= 55.0 {
        "🟡 一般 · 有改善空间"
    } else {
        "🔴 风险 · 加权分偏低或过度集中"
    };
    serde_json::json!({
        "weighted_score": uzi_core::py::round(weighted_score, 1),
        "max_weight": uzi_core::py::round(max_weight, 3),
        "n_industries": industries.len(),
        "industries": industries,
        "verdict": verdict,
        "n_valid": valid.len(),
        "n_total": metrics.len(),
    })
}

/// `_render_html(portfolio_name, metrics, health, depth)`.
pub fn render_html(portfolio_name: &str, metrics: &[Value], health: &Value, depth: &str) -> String {
    let now = chrono::Utc::now()
        .with_timezone(&crate::daily_screen::events::shanghai_offset())
        .format("%Y-%m-%d %H:%M")
        .to_string();
    let portfolio_name = esc(&Value::String(portfolio_name.to_string()), "");

    let mut ranked: Vec<&Value> = metrics.iter().collect();
    ranked.sort_by(|a, b| {
        let sa = py_float(uzi_core::py::get(a, "overall_score")).unwrap_or(0.0);
        let sb = py_float(uzi_core::py::get(b, "overall_score")).unwrap_or(0.0);
        sb.partial_cmp(&sa).unwrap_or(std::cmp::Ordering::Equal)
    });

    let mut rows: Vec<String> = Vec::new();
    for (index, m) in ranked.iter().enumerate() {
        let i = index + 1;
        let sc = py_float(uzi_core::py::get(m, "overall_score")).unwrap_or(0.0);
        let sc_color = if sc >= 65.0 {
            "var(--bull-green)"
        } else if sc >= 50.0 {
            "var(--neon-gold)"
        } else {
            "var(--bear-red)"
        };
        let w = uzi_core::py::f0(uzi_core::py::get(m, "_weight")) * 100.0;
        rows.push(format!(
            "<tr style=\"border-bottom:1px solid var(--border)\">  <td style=\"padding:10px 16px;color:var(--text-dim);font-family:Fira Code,monospace;font-size:11px\">#{i}</td>  <td style=\"padding:10px 16px\"><div style=\"font-weight:700;color:var(--text-bright)\">{}</div>    <div style=\"font-size:11px;color:var(--text-dim);font-family:Fira Code,monospace\">{} · {}</div></td>  <td style=\"padding:10px 16px;text-align:right;font-variant-numeric:tabular-nums\">{w:.1}%</td>  <td style=\"padding:10px 16px;text-align:right;color:{sc_color};font-weight:700;font-variant-numeric:tabular-nums\">{}</td>  <td style=\"padding:10px 16px;font-size:12px;color:var(--text-main)\">{}</td>  <td style=\"padding:10px 16px;text-align:center;font-size:11px\">    <span style=\"color:var(--bull-green)\">📈{}</span> ·     <span style=\"color:var(--bear-red)\">📉{}</span>  </td></tr>",
            esc(uzi_core::py::get(m, "name"), "—"),
            esc(uzi_core::py::get(m, "ticker"), "—"),
            esc(uzi_core::py::get(m, "industry"), "—"),
            num_str(&Value::from(sc), 1),
            esc(uzi_core::py::get(m, "verdict"), "—"),
            uzi_core::py::py_display(uzi_core::py::get(m, "bull_count")),
            uzi_core::py::py_display(uzi_core::py::get(m, "bear_count")),
        ));
    }

    let verdict_text = match health.get("verdict").and_then(|v| v.as_str()) {
        Some(v) => v.to_string(),
        None => String::new(),
    };
    let health_color = if verdict_text.contains('🟢') {
        "var(--bull-green)"
    } else if verdict_text.contains('🟡') {
        "var(--neon-gold)"
    } else {
        "var(--bear-red)"
    };
    let n_valid = py_int(health, "n_valid");
    let n_total = py_int(health, "n_total");
    let n_industries = py_int(health, "n_industries");
    let max_weight = py_float(health.get("max_weight").unwrap_or(&Value::Null)).unwrap_or(0.0);
    let concentration = if max_weight > 0.4 {
        "⚠️ 集中度偏高"
    } else {
        "✓ 分散合理"
    };
    let industries: Vec<String> = health
        .get("industries")
        .and_then(|v| v.as_array())
        .map(|list| list.iter().map(|v| esc(v, "—")).collect())
        .unwrap_or_default();
    let industries_head = industries
        .iter()
        .take(3)
        .cloned()
        .collect::<Vec<_>>()
        .join(", ");
    let industries_tail = if industries.len() > 3 { "..." } else { "" };

    PORTFOLIO_TEMPLATE
        .replace("@NAME@", &portfolio_name)
        .replace("@MAIN_STYLE@", crate::daily_screen::renderer::main_style())
        .replace("@DEPTH@", depth)
        .replace("@NOW@", &now)
        .replace("@N_VALID@", &n_valid)
        .replace("@N_TOTAL@", &n_total)
        .replace("@N_IND@", &n_industries)
        .replace("@HEALTH_COLOR@", health_color)
        .replace(
            "@WEIGHTED@",
            // `{health["weighted_score"]}` in an f-string → `str(float)` (`81.0`).
            &weighted_display(health),
        )
        .replace("@MAXW@", &format!("{:.1}", max_weight * 100.0))
        .replace("@CONC@", concentration)
        .replace("@INDS@", &industries_head)
        .replace("@INDS_TAIL@", industries_tail)
        .replace("@VERDICT@", &verdict_text)
        .replace("@ROWS@", &rows.join(""))
}

/// Python `str(health["weighted_score"])`.
fn weighted_display(health: &Value) -> String {
    match py_float(health.get("weighted_score").unwrap_or(&Value::Null)) {
        Some(x) => uzi_core::py::float_str(x),
        None => "0".to_string(),
    }
}

fn py_int(value: &Value, key: &str) -> String {
    match value.get(key) {
        Some(v) => uzi_core::py::py_display(v),
        None => "0".to_string(),
    }
}

/// `~` expansion (Python `Path.expanduser()`).
fn expanduser(path: &str) -> PathBuf {
    if let Some(rest) = path.strip_prefix("~/") {
        if let Ok(home) = std::env::var("HOME") {
            return PathBuf::from(home).join(rest);
        }
    }
    PathBuf::from(path)
}

/// `run_portfolio(csv_path, depth="lite", auto_open=True)` → report path.
pub fn run_portfolio(csv_path: &str) -> Result<String> {
    let path = expanduser(csv_path);
    let path = std::fs::canonicalize(&path).unwrap_or(path);
    let mut rows = match parse_csv(&path) {
        Ok(rows) => rows,
        Err(err) => bail!("csv_error: {err}"),
    };
    let name = path
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();
    let depth = "lite";
    println!();
    println!("{}", "━".repeat(60));
    println!("📊 组合分析模式 · {name} · {} 只成分股 · depth={depth}", rows.len());
    println!("{}", "━".repeat(60));
    for row in &rows {
        let ticker = safe_text(uzi_core::py::get(row, "ticker"), "");
        let weight = uzi_core::py::get(row, "weight");
        let weight_text = match weight.as_f64() {
            Some(w) => format!("{:.1}%", w * 100.0),
            None => "—".to_string(),
        };
        println!(
            "  · {:<18}  仓位 {:<6}  {}",
            ticker,
            weight_text,
            safe_text(uzi_core::py::get(row, "note"), "")
        );
    }
    if std::env::var("UZI_DEPTH").is_err() {
        std::env::set_var("UZI_DEPTH", depth);
    }

    let started = std::time::Instant::now();
    rows = normalize_weights(&mut rows);
    let mut metrics_list: Vec<Value> = Vec::new();
    let mut failed: Vec<Value> = Vec::new();
    let total = rows.len();
    for (index, row) in rows.iter().enumerate() {
        let ticker = safe_text(uzi_core::py::get(row, "ticker"), "");
        println!("\n━━━ [{}/{}] {ticker} ━━━", index + 1, total);
        match run_pipeline(&ticker) {
            Some(Ok(_)) => {}
            Some(Err(err)) => {
                println!("   ⚠️  pipeline 异常: {err} · 继续读 cache");
            }
            None => bail!("pipeline_unavailable: run_pipeline is not registered"),
        }
        let info = uzi_core::parse_ticker(&ticker);
        let metric = load_metrics_for(&info.full).or_else(|| load_metrics_for(&ticker));
        match metric {
            Some(mut m) => {
                if let Some(map) = m.as_object_mut() {
                    map.insert(
                        "_weight".into(),
                        uzi_core::py::get(row, "weight").clone(),
                    );
                    map.insert(
                        "_note".into(),
                        uzi_core::py::get(row, "note").clone(),
                    );
                }
                metrics_list.push(m);
            }
            None => failed.push(serde_json::json!({
                "ticker": ticker,
                "weight": uzi_core::py::get(row, "weight").clone(),
            })),
        }
    }
    if metrics_list.is_empty() {
        bail!("无可用成分股 · 至少需 1 只成功才能出组合报告");
    }
    let health = portfolio_health(&metrics_list);

    let date = chrono::Utc::now()
        .with_timezone(&crate::daily_screen::events::shanghai_offset())
        .format("%Y%m%d")
        .to_string();
    let safe_name = name.replace(' ', "_").replace('/', "_");
    let out_dir = reports_root().join(format!("portfolio_{safe_name}_{date}"));
    std::fs::create_dir_all(&out_dir)?;
    let out_file = out_dir.join("index.html");
    std::fs::write(
        &out_file,
        render_html(&name, &metrics_list, &health, depth),
    )?;

    let holdings: Vec<Value> = metrics_list
        .iter()
        .map(|m| {
            serde_json::json!({
                "ticker": uzi_core::py::get(m, "ticker").clone(),
                "name": uzi_core::py::get(m, "name").clone(),
                "weight": uzi_core::py::get(m, "_weight").clone(),
                "overall_score": uzi_core::py::get(m, "overall_score").clone(),
                "verdict": uzi_core::py::get(m, "verdict").clone(),
                "note": uzi_core::py::get(m, "_note").clone(),
            })
        })
        .collect();
    let meta = serde_json::json!({
        "portfolio_name": name,
        "depth": depth,
        "generated_at": local_iso_auto(),
        "health": health,
        "holdings": holdings,
        "failed": failed,
    });
    std::fs::write(
        out_dir.join("metadata.json"),
        uzi_core::json::to_pretty(&meta),
    )?;

    let dt = started.elapsed().as_secs();
    println!(
        "\n━━━ 组合分析完成 · {dt}s · 成功 {}/{} ━━━",
        metrics_list.len(),
        total
    );
    println!("📄 报告: {}", out_file.display());
    println!(
        "📊 加权评分 {} · {}",
        uzi_core::py::py_display(uzi_core::py::get(&health, "weighted_score")),
        uzi_core::py::py_display(uzi_core::py::get(&health, "verdict"))
    );
    Ok(out_file.to_string_lossy().to_string())
}

/// `datetime.now().isoformat()` (naive local, microseconds only when non-zero).
fn local_iso_auto() -> String {
    let now = chrono::Local::now().naive_local();
    if now.and_utc().timestamp_subsec_micros() == 0 {
        now.format("%Y-%m-%dT%H:%M:%S").to_string()
    } else {
        now.format("%Y-%m-%dT%H:%M:%S%.6f").to_string()
    }
}
