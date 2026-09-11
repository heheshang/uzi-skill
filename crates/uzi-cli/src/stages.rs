//! Port of the stage orchestration in `run_real_test.py`:
//! `stage1` (collect → integrity → modeling → scoring) and `stage2`
//! (synthesis → report assembly).

use std::path::{Path, PathBuf};

use serde_json::{json, Value};
use uzi_core::cache::{cache_root, read_task_output, write_task_output};

use crate::agent_review::{load_fresh_agent_analysis, write_review_context};

fn cache_dir(ticker: &str) -> PathBuf {
    cache_root().join(ticker)
}

/// Task 1 — data collection, integrity report and recovery-task artifact.
///
/// Returns the raw snapshot plus the resolved ticker info.
pub fn collect_and_audit(ticker: &str, max_workers: usize) -> Value {
    // `lite` fetches only its core dims; an unreadable profile means no filtering.
    let enabled = crate::profile::get_profile(None).ok().map(|p| p.fetchers_enabled);
    let mut raw = uzi_data::collect(ticker, None, max_workers, enabled.as_ref());

    let full = raw
        .get("ticker")
        .and_then(|v| v.as_str())
        .unwrap_or(ticker)
        .to_string();

    // Data integrity check
    let integrity = uzi_review::data_integrity::validate(&raw);
    println!("\n{}", uzi_review::data_integrity::format_report(&integrity));
    raw["_integrity"] = integrity.clone();

    // v2.3 · agent-consumable recovery task list (never aborts; the agent takes over)
    let tasks = uzi_review::data_integrity::generate_recovery_tasks(&raw, &integrity);
    let task_list = tasks.as_array().cloned().unwrap_or_default();
    if !task_list.is_empty() {
        let gaps_path = cache_dir(&full).join("_data_gaps.json");
        if let Some(parent) = gaps_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let doc = json!({
            "ticker": full,
            "coverage_pct": integrity.get("coverage_pct").cloned().unwrap_or(json!(0)),
            "critical_missing": integrity.get("critical_missing").cloned().unwrap_or(json!(false)),
            "tasks": tasks,
        });
        let _ = std::fs::write(&gaps_path, uzi_core::json::to_pretty(&doc));

        let crit_n = task_list
            .iter()
            .filter(|t| t.get("severity").and_then(|s| s.as_str()) == Some("critical"))
            .count();
        println!("\n{}", "▓".repeat(50));
        println!("⚠️  检测到 {} 个数据缺口 ({} critical)", task_list.len(), crit_n);
        println!("   恢复任务清单: .cache/{}/_data_gaps.json", full);
        println!("   Agent 必须尝试用以下手段补齐（按优先级）:");
        println!("     1. Chrome/Playwright MCP 访问 xueqiu/eastmoney");
        println!("     2. MX API (若 MX_APIKEY 已设置)");
        println!("     3. WebSearch 精确到代码");
        println!("     4. 已有数据逻辑推导");
        println!("   仍拿不到的字段 → 在 agent_analysis.json 显式标 data_gap_acknowledged");
        println!("   HTML 报告会对这些字段显示 ⚠️ 橙色徽章而非假数据");
        println!("{}", "▓".repeat(50));
    }

    // v2.6.1 · fill empty qualitative fields without waiting for the agent
    println!("\n🤖 v2.6.1 · 自动兜底补齐定性维度空字段（MX → ddgs）...");
    uzi_data::autofill_qualitative_via_mx(&mut raw, &full);

    // v2.13.5 · browser fallback for dims still empty or low-quality. Upstream
    // runs this immediately after the MX autofill.
    browser_fallback(&mut raw, &full);

    let _ = write_task_output(&full, "raw_data", &raw);

    raw
}

/// Browser (CDP) fallback over the dims the primary chain left thin.
///
/// The depth profile gates whether a browser may start at all: `lite` never
/// does, `medium` needs `UZI_PLAYWRIGHT_ENABLE=1`, `deep` enables it by default.
/// `UZI_PLAYWRIGHT_FORCE=1` retries every whitelisted dim instead of only the
/// empty ones. This mirrors the upstream HARD-GATE that requires the browser to
/// be attempted before the run declares a dimension's data missing.
///
/// Degenerates to a no-op — with a printed reason — when the gate is closed or no
/// Chromium is installed, so `raw` is never left half-mutated.
fn browser_fallback(raw: &mut Value, full: &str) {
    // Crypto has no CDP fallback path — every crypto dim comes from JSON APIs.
    if uzi_core::ticker::parse_ticker(full).market == uzi_core::ticker::CRYPTO_MARKET {
        return;
    }

    let profile = match crate::profile::get_profile(None) {
        Ok(p) => p,
        Err(e) => {
            println!("   ℹ️  Playwright skip · profile 加载失败: {e}");
            return;
        }
    };

    // Reuse the profile the preflight already wrote. A probe is deliberately not
    // triggered here: upstream reads the cache file only, and probing would stall
    // every collection run. Absent profile → no network filtering, like upstream.
    let network = std::fs::read_to_string(
        uzi_core::cache::cache_root()
            .join("_global")
            .join("network_profile.json"),
    )
    .ok()
    .and_then(|text| serde_json::from_str::<Value>(&text).ok())
    .filter(|v| v.is_object());

    let enabled_env = std::env::var("UZI_PLAYWRIGHT_ENABLE")
        .map(|v| v == "1")
        .unwrap_or(false);
    let force = std::env::var("UZI_PLAYWRIGHT_FORCE")
        .map(|v| v == "1")
        .unwrap_or(false);

    uzi_data::browser::fallback::autofill_via_browser(
        raw,
        full,
        &profile.playwright_mode,
        &profile.depth,
        &profile.playwright_dims,
        network.as_ref(),
        enabled_env,
        force,
    );
}

/// Task 1.5–3 — institutional modeling, 22-dim scoring, investor panel.
pub fn modeling_and_scoring(ticker: &str, mut raw: Value) -> Value {
    let full = raw
        .get("ticker")
        .and_then(|v| v.as_str())
        .unwrap_or(ticker)
        .to_string();

    let dims_in = raw
        .get("dimensions")
        .cloned()
        .filter(|v| v.is_object())
        .unwrap_or(json!({}));
    if !raw.get("dimensions").map(|d| d.is_object()).unwrap_or(false) {
        raw["dimensions"] = json!({});
    }

    // Upstream passes the INNER data dicts to the chained models:
    //   d20 = raw["dimensions"]["20_valuation_models"]["data"]
    //   compute_dim_21(features, raw, d20)
    //   d21 = raw["dimensions"]["21_research_workflow"]["data"]
    //   compute_dim_22(features, raw, d20, d21)
    println!("\n🏛  Task 1.5 · 机构级财务建模 (Dims 20-22)");
    let is_crypto = uzi_core::ticker::parse_ticker(&full).market
        == uzi_core::ticker::CRYPTO_MARKET;
    let features = uzi_core::features::sanitize_features(&uzi_features::extract_features(
        &raw,
        raw.get("dimensions").unwrap_or(&dims_in),
    ));

    let d20 = uzi_models::compute_dim_20(&features, &raw);
    let d20_data = d20.get("data").cloned().unwrap_or(json!({}));
    raw["dimensions"]["20_valuation_models"] = d20.clone();

    let d21 = uzi_models::compute_dim_21(&features, &raw, &d20_data);
    let d21_data = d21.get("data").cloned().unwrap_or(json!({}));
    raw["dimensions"]["21_research_workflow"] = d21.clone();

    let d22 = uzi_models::compute_dim_22(&features, &raw, &d20_data, &d21_data);
    raw["dimensions"]["22_deep_methods"] = d22.clone();

    let gaps_path = cache_dir(&full).join("_data_gaps.json");
    uzi_review::data_integrity::refresh_recovery_artifact(&mut raw, &full, &gaps_path);
    let _ = write_task_output(&full, "raw_data", &raw);

    let s20 = d20_data.get("summary").cloned().unwrap_or(json!({}));
    let s21 = d21_data.get("summary").cloned().unwrap_or(json!({}));
    let s22 = d22
        .get("data")
        .and_then(|d| d.get("summary"))
        .cloned()
        .unwrap_or(json!({}));
    if is_crypto {
        let vm = d20_data.get("valuation_model").cloned().unwrap_or(json!({}));
        println!(
            "  NVT 估值: 公允价值 ${} · 安全边际 {}% · {}",
            disp(vm.get("fair_price")),
            disp(vm.get("safety_margin_pct")),
            disp(vm.get("verdict"))
        );
        println!(
            "  首次覆盖: {} · 目标价 ${} ({}%)",
            disp(s21.get("rec_rating")),
            disp(s21.get("target_price")),
            disp(s21.get("upside_pct"))
        );
        println!("  IC Memo: {}", disp(s22.get("ic_recommendation")));
        println!(
            "  赛道定位: {} · 行业吸引力 {}%",
            disp(s22.get("bcg_position")),
            disp(s22.get("industry_attractiveness"))
        );
    } else {
        println!(
            "  DCF: ¥{} · 安全边际 {}% · {}",
            disp(s20.get("dcf_intrinsic")),
            disp(s20.get("dcf_safety_margin_pct")),
            disp(s20.get("dcf_verdict"))
        );
        println!(
            "  LBO: IRR {}% · {}",
            disp(s20.get("lbo_irr_pct")),
            disp(s20.get("lbo_verdict"))
        );
        println!(
            "  首次覆盖: {} · TP ¥{} ({}%)",
            disp(s21.get("rec_rating")),
            disp(s21.get("target_price")),
            disp(s21.get("upside_pct"))
        );
        println!("  IC Memo: {}", disp(s22.get("ic_recommendation")));
        println!(
            "  BCG: {} · 行业吸引力 {}%",
            disp(s22.get("bcg_position")),
            disp(s22.get("industry_attractiveness"))
        );
    }

    println!("\n📏 Task 2 · 22 维打分");
    let dims = uzi_pipeline::score::score_dimensions(&raw);
    let _ = write_task_output(&full, "dimensions", &dims);
    println!(
        "  基本面得分: {}/100",
        disp(dims.get("fundamental_score"))
    );

    println!("\n🎭 Task 3 · 评委规则引擎（骨架分）");
    let panel = uzi_pipeline::panel::generate_panel(&dims, &raw);
    let _ = write_task_output(&full, "panel", &panel);
    let review_context = write_review_context(&cache_dir(&full), &raw).unwrap_or(json!({}));
    let distribution = panel.get("signal_distribution").cloned().unwrap_or(json!({}));
    let skip_n = distribution.get("skip").and_then(|v| v.as_i64()).unwrap_or(0);
    let active_n = panel.get("long_active").and_then(|v| v.as_i64()).unwrap_or_else(|| {
        ["bullish", "neutral", "bearish"]
            .iter()
            .map(|k| distribution.get(*k).and_then(|v| v.as_i64()).unwrap_or(0))
            .sum()
    });
    println!(
        "  参与 {} · 跳过 {} · 看多 {} · 中性 {} · 看空 {}",
        active_n,
        skip_n,
        disp(distribution.get("bullish")),
        disp(distribution.get("neutral")),
        disp(distribution.get("bearish"))
    );
    if panel.get("consensus_valid").and_then(|v| v.as_bool()) == Some(false) {
        println!("  ⚠️ {}", disp(panel.get("consensus_warning")));
        let hollow: Vec<String> = panel
            .get("hollow_ids")
            .and_then(|v| v.as_array())
            .map(|a| {
                a.iter()
                    .take(8)
                    .map(uzi_core::py::py_str)
                    .collect()
            })
            .unwrap_or_default();
        println!("     空判评委: {}", hollow.join(", "));
    }

    println!("\n{}", "━".repeat(50));
    println!("📋 Stage 1 建模与评分完成 · 骨架分已生成");
    println!("   数据: .cache/{}/raw_data.json", full);
    println!("   评分: .cache/{}/dimensions.json", full);
    println!("   评委: .cache/{}/panel.json", full);
    println!(
        "   输入指纹: {}",
        disp(review_context.get("analysis_input_hash"))
    );
    println!("{}", "━".repeat(50));

    json!({"ticker": full, "raw": raw, "dims": dims, "panel": panel, "features": features})
}

/// Stage 1 — collect, model and score, returning the payload for the agent.
pub fn stage1(ticker: &str, max_workers: usize) -> Value {
    let prepared = uzi_data::prepare_target(ticker, None);
    if prepared.get("ok").and_then(|v| v.as_bool()) != Some(true) {
        return prepared.get("payload").cloned().unwrap_or_else(|| json!({}));
    }
    let full = prepared
        .get("ticker_info")
        .and_then(|t| t.get("full"))
        .and_then(|v| v.as_str())
        .unwrap_or(ticker)
        .to_string();

    println!("📊 Task 1 · 数据采集");
    let raw = collect_and_audit(&full, max_workers);
    modeling_and_scoring(&full, raw)
}

fn validate_agent_analysis(agent_analysis: Option<&Value>, ticker: &str) -> (Option<Value>, Value) {
    let Some(aa) = agent_analysis else {
        return (None, json!([]));
    };
    let issues = uzi_review::validator::validate_agent_analysis(aa);
    if issues.as_array().map(|a| a.is_empty()).unwrap_or(true) {
        return (Some(aa.clone()), issues);
    }
    println!("\n{}", uzi_review::validator::format_issues(&issues));
    let errors: Vec<&Value> = issues
        .as_array()
        .map(|a| {
            a.iter()
                .filter(|i| i.get("severity").and_then(|s| s.as_str()) == Some("error"))
                .collect()
        })
        .unwrap_or_default();
    let err_path = cache_dir(ticker).join("_agent_analysis_errors.json");
    if let Some(parent) = err_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(&err_path, uzi_core::json::to_pretty(&issues));
    if errors.is_empty() {
        (Some(aa.clone()), issues)
    } else {
        println!("   → 详细 issue 写入 {}", err_path.display());
        println!(
            "   → {} 条结构性错误，已回退到脚本骨架；agent 应修正后重跑 stage2",
            errors.len()
        );
        (None, issues)
    }
}

/// Stage 2 — synthesis (merging agent overrides) and report assembly.
///
/// Returns the standalone HTML path.
pub fn stage2(ticker: &str) -> anyhow::Result<String> {
    let ti = resolve_cached_target(ticker, &["raw_data", "dimensions", "panel"])?;
    let full = ti.full.clone();

    let raw = read_task_output(&full, "raw_data")
        .ok_or_else(|| anyhow::anyhow!("Stage 2 缺少数据，请先跑 stage1('{}')", ticker))?;
    let dims = read_task_output(&full, "dimensions")
        .ok_or_else(|| anyhow::anyhow!("Stage 2 缺少数据，请先跑 stage1('{}')", ticker))?;
    let panel = read_task_output(&full, "panel")
        .ok_or_else(|| anyhow::anyhow!("Stage 2 缺少数据，请先跑 stage1('{}')", ticker))?;

    // Agent analysis must match the current raw snapshot; stale role-play is never reused.
    let (fresh, freshness_reason) = load_fresh_agent_analysis(&cache_dir(&full), &raw);
    let (agent_analysis, _issues) = validate_agent_analysis(fresh.as_ref(), &full);

    let agent_analysis = match agent_analysis {
        Some(aa) if aa.get("agent_reviewed").and_then(|v| v.as_bool()) == Some(true) => {
            println!("\n🧠 Agent 分析已加载 · agent_analysis.json");
            let ag_dc = aa.get("dim_commentary").cloned().unwrap_or(json!({}));
            let written = ag_dc
                .as_object()
                .map(|m| {
                    m.values()
                        .filter(|v| {
                            v.as_str()
                                .map(|s| !s.contains("[脚本占位]"))
                                .unwrap_or(false)
                        })
                        .count()
                })
                .unwrap_or(0);
            println!("   dim_commentary: {} 个维度有 agent 定性评语", written);
            println!(
                "   panel_insights: {}",
                mark(aa.get("panel_insights").map(truthy).unwrap_or(false))
            );
            println!(
                "   narrative_override: {}",
                mark(aa.get("narrative_override").map(truthy).unwrap_or(false))
            );
            println!(
                "   great_divide_override: {}",
                mark(aa.get("great_divide_override").map(truthy).unwrap_or(false))
            );
            Some(aa)
        }
        _ => {
            if crate::agent_review::requires_agent_review(&cache_dir(&full)) {
                anyhow::bail!(
                    "deep 档 HARD-GATE: 当前快照没有可用的 Agent role-play（{}）。请读取 .cache/{}/_agent_review_context.json，将 analysis_input_hash 写入 agent_analysis.json 后重跑 stage2。",
                    freshness_reason,
                    full
                );
            }
            println!("\n⚠️  未检测到当前快照可用的 agent_analysis.json · 将使用脚本骨架生成 synthesis");
            println!("   原因: {}", freshness_reason);
            None
        }
    };

    println!("\n⚖ Task 4 · 综合研判");
    let mut syn = uzi_pipeline::synthesis::generate_synthesis(
        &raw,
        &dims,
        &panel,
        agent_analysis.as_ref(),
    );

    // v2.3 · merge _data_gaps.json into synthesis for the orange gap badges
    let gaps_path = cache_dir(&full).join("_data_gaps.json");
    if let Some(doc) = uzi_core::cache::read_json(&gaps_path) {
        let mut tasks = doc.get("tasks").cloned().unwrap_or(json!([]));
        let acks = agent_analysis
            .as_ref()
            .and_then(|aa| aa.get("data_gap_acknowledged"))
            .cloned()
            .unwrap_or(json!({}));
        if let Some(list) = tasks.as_array_mut() {
            for t in list.iter_mut() {
                let dim = t.get("dim").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let field = t.get("field").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let key = format!("{}.{}", dim, field);
                let ack = acks
                    .get(&key)
                    .or_else(|| acks.get(&dim))
                    .filter(|v| truthy(v))
                    .cloned();
                if let Some(note) = ack {
                    t["status"] = json!("acknowledged");
                    t["agent_note"] = note;
                }
            }
        }
        let unresolved = tasks
            .as_array()
            .map(|a| {
                a.iter()
                    .filter(|t| t.get("status").and_then(|s| s.as_str()) == Some("pending"))
                    .count()
            })
            .unwrap_or(0);
        let total = tasks.as_array().map(|a| a.len()).unwrap_or(0);
        syn["data_gaps"] = json!({
            "coverage_pct": doc.get("coverage_pct").cloned().unwrap_or(json!(0)),
            "total_gaps": total,
            "unresolved": unresolved,
            "tasks": tasks,
        });
        println!("  data_gaps: {} 项 · 已 ack {}", total, total - unresolved);
    }

    write_task_output(&full, "synthesis", &syn)?;
    println!(
        "  综合评分: {}/100 · {}",
        disp(syn.get("overall_score")),
        disp(syn.get("verdict_label"))
    );
    println!(
        "  agent_reviewed: {}",
        syn.get("agent_reviewed").and_then(|v| v.as_bool()).unwrap_or(false)
    );

    println!("\n📄 Task 5 · 报告组装");
    let out = uzi_report::assemble(&full)?;
    println!("  → {}", out);

    Ok(out)
}

/// Full run: `stage1` then `stage2` with no agent intervention (quick path).
pub fn run(ticker: &str, max_workers: usize) -> anyhow::Result<Value> {
    let result = stage1(ticker, max_workers);
    continue_to_report(ticker, result)
}

/// Continue a finished Stage 1 into Stage 2 (synthesis + report assembly).
///
/// Shared by the quick path ([`run`]) and the `--from-modeling` resume path, so a
/// resume actually yields the report instead of stopping at the payload.
///
/// A Stage 1 payload carrying `status` is an early exit the agent must act on
/// (unresolved name, non-stock security); it is returned unchanged and no report
/// is written.
pub fn continue_to_report(ticker: &str, result: Value) -> anyhow::Result<Value> {
    if let Some(status) = result.get("status").and_then(|v| v.as_str()) {
        match status {
            "name_not_resolved" => {
                println!("\n⚠️  因股票名无法解析，跳过 stage2（不会生成空报告）");
                return Ok(result);
            }
            "non_stock_security" => {
                println!("\n⚠️  非个股标的，跳过 stage2（已输出成分股清单给 agent）");
                return Ok(result);
            }
            _ => {}
        }
    }
    let full = result
        .get("ticker")
        .and_then(|v| v.as_str())
        .unwrap_or(ticker)
        .to_string();
    let report = stage2(&full)?;
    println!("\n🎯 完整流程结束 · 报告: {}", report);
    Ok(json!(report))
}

/// Where Stage 1 left its artifacts, and what the agent must do next.
///
/// Printed by `--stage1`. Upstream's agent reads the same files out of
/// `.cache/{ticker}/`; this message exists so the handoff does not depend on the
/// agent inferring the paths.
pub fn stage1_handoff(result: &Value, ticker: &str) -> String {
    // An early exit is a payload to act on, not a completed stage.
    if let Some(status) = result.get("status").and_then(|v| v.as_str()) {
        return format!(
            "\n⚠️  Stage 1 未进入评分 · status={status}\n{}\n",
            uzi_core::json::to_pretty(result)
        );
    }

    let full = result
        .get("ticker")
        .and_then(|v| v.as_str())
        .unwrap_or(ticker);
    let dir = cache_dir(full);

    let mut out = format!("\n✅ Stage 1 完成 · {full}\n   产物目录: {}\n", dir.display());
    for (name, what) in [
        ("raw_data", "22 维原始数据"),
        ("dimensions", "22 维评分"),
        ("panel", "评委骨架分"),
    ] {
        let mark = if dir.join(format!("{name}.json")).exists() {
            "✓"
        } else {
            "✗"
        };
        out.push_str(&format!("   {mark} {name}.json · {what}\n"));
    }
    if dir.join("_data_gaps.json").exists() {
        out.push_str("   ⚠ _data_gaps.json · 采集缺口（agent 需接管）\n");
    }
    out.push_str(&format!(
        "\n🧠 下一步 · agent 介入:\n   1. 读 {0}/panel.json 与 dimensions.json\n   2. 写 {0}/agent_analysis.json（agent_reviewed=true）\n   3. uzi {full} --stage2\n",
        dir.display()
    ));
    out
}

/// `_resolve_cached_target` — resolve a code or Chinese name to a cache dir that
/// already holds the required outputs, rejecting unsafe path components.
pub fn resolve_cached_target(ticker: &str, required: &[&str]) -> anyhow::Result<uzi_core::TickerInfo> {
    let safe_key = |s: &str| -> bool {
        let mut chars = s.chars();
        match chars.next() {
            Some(c) if c.is_ascii_uppercase() || c.is_ascii_digit() => {}
            _ => return false,
        }
        s.len() <= 40
            && s.chars()
                .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '.' || c == '-')
    };
    let has_outputs = |key: &str| -> bool {
        if !safe_key(key) {
            return false;
        }
        let dir = cache_root().join(key);
        required
            .iter()
            .all(|name| dir.join(format!("{}.json", name)).is_file())
    };

    let parsed = uzi_core::parse_ticker(ticker);
    if has_outputs(&parsed.full) {
        return Ok(parsed);
    }

    if uzi_core::ticker::is_chinese_name(ticker) {
        if ticker.chars().count() > 80
            || ticker.contains('/')
            || ticker.contains('\\')
            || ticker.contains('\0')
        {
            anyhow::bail!("股票名称包含不安全路径字符");
        }
        let root = cache_root();
        if root.is_dir() {
            if let Ok(entries) = std::fs::read_dir(&root) {
                for entry in entries.flatten() {
                    let name = entry.file_name().to_string_lossy().to_string();
                    if !entry.path().is_dir() || !safe_key(&name) {
                        continue;
                    }
                    let Some(raw) = uzi_core::cache::read_json(&entry.path().join("raw_data.json"))
                    else {
                        continue;
                    };
                    let basic_name = raw
                        .get("dimensions")
                        .and_then(|d| d.get("0_basic"))
                        .and_then(|d| d.get("data"))
                        .and_then(|d| d.get("name"))
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .trim()
                        .to_string();
                    if basic_name == ticker.trim() && has_outputs(&name) {
                        return Ok(uzi_core::parse_ticker(&name));
                    }
                }
            }
        }
        let prepared = uzi_data::prepare_target(ticker, None);
        if prepared.get("ok").and_then(|v| v.as_bool()) == Some(true) {
            let info = prepared.get("ticker_info").cloned().unwrap_or(json!({}));
            let full = info.get("full").and_then(|v| v.as_str()).unwrap_or(ticker);
            return Ok(uzi_core::parse_ticker(full));
        }
        anyhow::bail!(
            "{}",
            prepared
                .get("payload")
                .and_then(|p| p.get("message"))
                .and_then(|v| v.as_str())
                .map(str::to_string)
                .unwrap_or_else(|| format!("无法解析股票名称: {}", ticker))
        );
    }

    if !safe_key(&parsed.full) {
        anyhow::bail!("无效股票代码: {:?}", ticker);
    }
    Ok(parsed)
}

/// Resume modeling+scoring from a cached `raw_data.json` (upstream `stage1_modeling`).
pub fn stage1_modeling(ticker: &str) -> anyhow::Result<Value> {
    let ti = resolve_cached_target(ticker, &["raw_data"])?;
    let raw = read_task_output(&ti.full, "raw_data")
        .ok_or_else(|| anyhow::anyhow!("建模恢复缺少 .cache/{}/raw_data.json", ti.full))?;
    println!("♻️  从 raw_data.json 恢复建模: {} → {}", ticker, ti.full);
    Ok(modeling_and_scoring(&ti.full, raw))
}

fn truthy(v: &Value) -> bool {
    uzi_core::py::truthy(v)
}

fn mark(b: bool) -> &'static str {
    if b {
        "✓"
    } else {
        "✗"
    }
}

/// Python-`repr`-style display; `None`/absent renders as `None`.
fn disp(v: Option<&Value>) -> String {
    match v {
        None | Some(Value::Null) => "None".to_string(),
        Some(v) => uzi_core::py::py_display(v),
    }
}

/// Directory holding a report and its sibling artifacts.
pub fn report_dir_of(standalone: &Path) -> PathBuf {
    standalone
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."))
}
