//! Port of `lib/fund_holdings_runner.py` — ETF/LOF/mutual-fund holdings loop.
//!
//! Upstream reaches this from `run.py` once stage-1 classifies the ticker as a
//! fund; [`run_fund_holdings`] performs that classification and the holdings
//! fetch here.
//!
//! `fund_template.txt` is the upstream `_generate_summary_html` f-string with
//! `@TOKEN@` placeholders.

use anyhow::{bail, Result};
use serde_json::{Map, Value};
use std::path::{Component, Path, PathBuf};

use crate::paths::reports_root;
use crate::providers::report_path_of;

const FUND_TEMPLATE: &str = include_str!("fund_template.txt");

/// `PER_STOCK_TIME_BY_DEPTH` (seconds).
pub const PER_STOCK_TIME_BY_DEPTH: &[(&str, i64)] = &[("lite", 60), ("medium", 240), ("deep", 900)];

/// `_estimate_runtime(n_stocks, depth)`.
pub fn estimate_runtime(n_stocks: i64, depth: &str) -> String {
    let per_stock = PER_STOCK_TIME_BY_DEPTH
        .iter()
        .find(|(key, _)| *key == depth)
        .map(|(_, secs)| *secs)
        .unwrap_or(240);
    let sec = n_stocks * per_stock;
    if sec < 120 {
        return format!("约 {sec} 秒");
    }
    let minutes = sec as f64 / 60.0;
    if minutes < 60.0 {
        return format!("约 {minutes:.0} 分钟");
    }
    format!("约 {:.1} 小时", minutes / 60.0)
}

/// `confirm_and_run_holdings(...)`.
///
/// `interactive` uses `std::io::IsTerminal` where upstream checks
/// `sys.stdin.isatty()`.
pub fn confirm_and_run_holdings(
    fund_ticker: &str,
    fund_label: &str,
    top_holdings: &[Value],
    depth: &str,
    auto_yes: bool,
    interactive: Option<bool>,
) -> Result<Value> {
    if top_holdings.is_empty() {
        return Ok(serde_json::json!({
            "status": "no_holdings",
            "fund_ticker": fund_ticker,
            "message": format!("{fund_label} {fund_ticker} 拉不到持仓清单 · 跳过批量分析"),
        }));
    }
    let interactive = interactive.unwrap_or_else(|| {
        use std::io::IsTerminal;
        std::io::stdin().is_terminal() && !auto_yes
    });
    let n_total = top_holdings.len() as i64;
    println!();
    println!("{}", "━".repeat(60));
    println!("📊 {fund_label} {fund_ticker} · 持仓批量分析");
    println!("{}", "━".repeat(60));
    println!("\n该基金前 {n_total} 大持仓：");
    for h in top_holdings {
        let pct = match h.get("weight_pct").and_then(|v| v.as_f64()) {
            Some(_) => format!(
                "{:.2}%",
                h.get("weight_pct").and_then(|v| v.as_f64()).unwrap_or(0.0)
            ),
            None => "—".to_string(),
        };
        println!(
            "  {:>2}. {:<14} ({:<10})  占比 {}",
            h.get("rank").and_then(|v| v.as_i64()).unwrap_or(0),
            crate::versus::safe_text(h.get("name").unwrap_or(&Value::Null), ""),
            crate::versus::safe_text(h.get("code").unwrap_or(&Value::Null), ""),
            pct,
        );
    }
    let est = estimate_runtime(n_total, depth);
    println!("\n⚠️  循环分析 {n_total} 只成分股 · 预计 {est}（按 depth={depth})");
    println!("    每只股票会跑完整 22 维 + 51 评委 + 17 机构方法 → 生成单独 HTML 报告");
    println!("    所有报告都是缓存的（resume=True）· 重跑只会增量");

    let n_to_run: i64;
    if auto_yes {
        n_to_run = n_total;
        println!("\n   → auto_yes=True · 跑全部 {n_total} 只");
    } else if interactive {
        use std::io::Write;
        print!(
            "\n继续？\n  y    = 跑全部 {n_total} 只\n  数字 = 只跑前 K 只（如输入 5 跑前 5）\n  N    = 取消（默认）\n  请选择: "
        );
        let _ = std::io::stdout().flush();
        let mut line = String::new();
        if std::io::stdin().read_line(&mut line).is_err() {
            println!("\n   已取消。");
            return Ok(serde_json::json!({"status": "cancelled", "fund_ticker": fund_ticker}));
        }
        let choice = line.trim().to_lowercase();
        if choice == "y" || choice == "yes" {
            n_to_run = n_total;
        } else if !choice.is_empty() && choice.chars().all(|c| c.is_ascii_digit()) {
            let picked: i64 = choice.parse().unwrap_or(0);
            n_to_run = picked.clamp(1, n_total);
        } else {
            println!("   已取消。");
            return Ok(serde_json::json!({"status": "cancelled", "fund_ticker": fund_ticker}));
        }
    } else {
        println!("   ⚠️  非交互环境 · 默认取消（agent 应传 UZI_FUND_AUTO_YES=1 明确确认）");
        return Ok(serde_json::json!({"status": "cancelled", "fund_ticker": fund_ticker}));
    }
    let selected = &top_holdings[..n_to_run as usize];
    println!(
        "\n   ✓ 即将分析 {n_to_run} 只成分股 · 估算 {}",
        estimate_runtime(n_to_run, depth)
    );
    println!();

    let started = std::time::Instant::now();
    let mut analyzed: Vec<Value> = Vec::new();
    let mut report_paths: Vec<String> = Vec::new();
    let mut failed: Vec<Value> = Vec::new();
    for (i, h) in selected.iter().enumerate() {
        let code = crate::versus::safe_text(h.get("code").unwrap_or(&Value::Null), "");
        let name = match h.get("name") {
            Some(v) if uzi_core::py::truthy(v) => crate::versus::safe_text(v, ""),
            _ => code.clone(),
        };
        println!("\n━━━ [{}/{}] {name} ({code}) ━━━", i + 1, n_to_run);
        match crate::providers::run_pipeline(&code) {
            Some(Ok(report)) => {
                analyzed.push(Value::String(code.clone()));
                report_paths.push(report_path_of(&report).unwrap_or_else(|| report.to_string()));
            }
            Some(Err(err)) => {
                let message = err.to_string();
                println!(
                    "   ⚠️  {code} 分析失败: {}",
                    message.chars().take(80).collect::<String>()
                );
                failed.push(serde_json::json!({
                    "code": code,
                    "name": name,
                    "error": message.chars().take(200).collect::<String>(),
                }));
                continue;
            }
            None => bail!("pipeline_unavailable: run_pipeline is not registered"),
        }
    }
    let dt = started.elapsed().as_secs();
    println!(
        "\n━━━ 批量分析完成 · {dt}s · 成功 {}/{n_to_run} ━━━",
        analyzed.len()
    );

    let analyzed_codes: Vec<String> = analyzed
        .iter()
        .filter_map(|v| v.as_str().map(str::to_string))
        .collect();
    let summary_html = generate_summary_html(
        fund_ticker,
        fund_label,
        top_holdings,
        &analyzed_codes,
        &report_paths,
        &failed,
    )?;
    Ok(serde_json::json!({
        "status": "completed",
        "fund_ticker": fund_ticker,
        "fund_label": fund_label,
        "analyzed": analyzed,
        "failed": failed,
        "report_paths": report_paths,
        "summary_html": summary_html,
        "total_runtime_sec": dt as i64,
    }))
}

/// `_generate_summary_html(...)`.
pub fn generate_summary_html(
    fund_ticker: &str,
    fund_label: &str,
    all_holdings: &[Value],
    analyzed_codes: &[String],
    report_paths: &[String],
    failed: &[Value],
) -> Result<PathBuf> {
    let date = chrono::Utc::now()
        .with_timezone(&crate::daily_screen::events::shanghai_offset())
        .format("%Y%m%d")
        .to_string();
    let out_dir = reports_root().join(format!("{fund_ticker}_holdings_{date}"));
    std::fs::create_dir_all(&out_dir)?;
    let out_file = out_dir.join("fund-holdings-summary.html");

    let mut rows: Vec<String> = Vec::new();
    for h in all_holdings {
        let code = crate::versus::safe_text(h.get("code").unwrap_or(&Value::Null), "");
        let name = match h.get("name") {
            Some(v) if uzi_core::py::truthy(v) => crate::versus::safe_text(v, ""),
            _ => code.clone(),
        };
        let weight = h.get("weight_pct").and_then(|v| v.as_f64());
        let weight_str = match weight {
            Some(w) => format!("{w:.2}%"),
            None => "—".to_string(),
        };
        let status_html = if let Some(index) = analyzed_codes.iter().position(|c| *c == code) {
            match report_paths.get(index) {
                Some(path) => {
                    let rel = relpath(Path::new(path), &out_dir);
                    format!("<a href=\"{rel}\" target=\"_blank\" style=\"color:#2563eb\">查看报告 →</a>")
                }
                None => "<span style=\"color:#94a3b8\">报告路径异常</span>".to_string(),
            }
        } else if let Some(entry) = failed
            .iter()
            .find(|f| uzi_core::py::get(f, "code").as_str() == Some(code.as_str()))
        {
            let err = crate::versus::safe_text(uzi_core::py::get(entry, "error"), "");
            format!("<span style=\"color:#dc2626\" title=\"{err}\">❌ 失败</span>")
        } else {
            "<span style=\"color:#94a3b8\">未分析</span>".to_string()
        };
        rows.push(format!(
            "<tr><td>{}</td><td><strong>{name}</strong></td><td><code>{code}</code></td><td>{weight_str}</td><td>{status_html}</td></tr>",
            h.get("rank").and_then(|v| v.as_i64()).unwrap_or(0)
        ));
    }
    let gen_time = chrono::Utc::now()
        .with_timezone(&crate::daily_screen::events::shanghai_offset())
        .format("%Y-%m-%d %H:%M")
        .to_string();
    let html = FUND_TEMPLATE
        .replace("@FUND_LABEL@", fund_label)
        .replace("@FUND_TICKER@", fund_ticker)
        .replace("@GEN_TIME@", &gen_time)
        .replace("@ANALYZED@", &analyzed_codes.len().to_string())
        .replace("@FAILED@", &failed.len().to_string())
        .replace("@TOTAL@", &all_holdings.len().to_string())
        .replace("@ROWS@", &rows.join("\n"));
    std::fs::write(&out_file, html)?;
    println!("\n📄 持仓分析汇总报告: {}", out_file.display());
    Ok(out_file)
}

/// `os.path.relpath(target, base)`.
fn relpath(target: &Path, base: &Path) -> String {
    let target_parts: Vec<Component> = target.components().collect();
    let base_parts: Vec<Component> = base.components().collect();
    let mut common = 0;
    while common < target_parts.len()
        && common < base_parts.len()
        && target_parts[common] == base_parts[common]
    {
        common += 1;
    }
    if common == 0 && target.is_absolute() != base.is_absolute() {
        return target.to_string_lossy().to_string();
    }
    let mut parts: Vec<String> = Vec::new();
    for _ in common..base_parts.len() {
        parts.push("..".to_string());
    }
    for part in &target_parts[common..] {
        parts.push(part.as_os_str().to_string_lossy().to_string());
    }
    if parts.is_empty() {
        ".".to_string()
    } else {
        parts.join("/")
    }
}

/// `NON_STOCK_GUIDANCE` labels for the fund security types.
fn fund_label(sec_type: uzi_core::ticker::SecurityType) -> &'static str {
    use uzi_core::ticker::SecurityType;
    match sec_type {
        SecurityType::Etf => "ETF",
        SecurityType::Lof => "LOF 基金",
        SecurityType::MutualFund => "开放式基金",
        _ => "ETF/LOF",
    }
}

/// `run_fund_holdings(ticker)` — classify a fund ticker, fetch its top holdings
/// through uzi-data, then run [`confirm_and_run_holdings`].
pub fn run_fund_holdings(ticker: &str) -> Result<Value> {
    let fund = uzi_core::parse_ticker(ticker);
    let sec_type = uzi_core::ticker::classify_security_type(&fund.code);
    let label = fund_label(sec_type);
    let payload = uzi_data::sources::fetch_fund_portfolio_hold(&fund.code);
    let top_holdings = normalize_top_holdings(&payload);
    let auto_yes = std::env::var("UZI_FUND_AUTO_YES").map(|v| v == "1").unwrap_or(false);
    let depth = std::env::var("UZI_DEPTH").unwrap_or_else(|_| "medium".to_string());
    confirm_and_run_holdings(&fund.full, label, &top_holdings, &depth, auto_yes, None)
}

/// The `top_holdings` projection upstream builds in
/// `pipeline/preflight_helpers.py` from `ak.fund_portfolio_hold_em`.
fn normalize_top_holdings(payload: &Value) -> Vec<Value> {
    let mut out = Vec::new();
    let Some(rows) = payload.as_array() else {
        return out;
    };
    for (index, row) in rows.iter().take(10).enumerate() {
        let stock_code = {
            let primary = uzi_core::py::get(row, "股票代码");
            let fallback = uzi_core::py::get(row, "code");
            let value = if uzi_core::py::truthy(primary) {
                primary
            } else {
                fallback
            };
            crate::versus::safe_text(value, "").trim().to_string()
        };
        let stock_name = {
            let primary = uzi_core::py::get(row, "股票名称");
            let fallback = uzi_core::py::get(row, "name");
            let value = if uzi_core::py::truthy(primary) {
                primary
            } else {
                fallback
            };
            crate::versus::safe_text(value, "").trim().to_string()
        };
        let pct_raw = {
            let candidates = ["占净值比例", "比例", "weight"];
            candidates
                .iter()
                .find_map(|key| {
                    let value = uzi_core::py::get(row, key);
                    if uzi_core::py::truthy(value) {
                        Some(value.clone())
                    } else {
                        None
                    }
                })
                .unwrap_or(Value::String(String::new()))
        };
        let pct = if uzi_core::py::truthy(&pct_raw) {
            let text = uzi_core::py::py_str(&pct_raw).replace('%', "");
            text.trim().parse::<f64>().unwrap_or(0.0)
        } else {
            0.0
        };
        if !stock_code.is_empty() && !stock_name.is_empty() {
            let full_code = {
                let parsed = uzi_core::parse_ticker(&stock_code);
                if parsed.full.is_empty() {
                    stock_code.clone()
                } else {
                    parsed.full
                }
            };
            let mut entry = Map::new();
            entry.insert("rank".into(), Value::from(index as i64 + 1));
            entry.insert("code".into(), Value::String(full_code));
            entry.insert("name".into(), Value::String(stock_name));
            entry.insert(
                "weight_pct".into(),
                if pct != 0.0 {
                    Value::from(uzi_core::py::round(pct, 2))
                } else {
                    Value::Null
                },
            );
            out.push(Value::Object(entry));
        }
    }
    out
}
