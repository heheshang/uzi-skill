//! Port of `lib/segmental_model.py` and `compute_segmental.py`.
//!
//! `discover_segments` / `render_skeleton_markdown` / `validate_model` are the
//! pure functions; `cmd_discover` / `cmd_validate` are the CLI entry points that
//! read and write `.cache/<ticker>/*.json` through `uzi_core::cache`.

use serde_json::{Map, Value};
use std::collections::BTreeSet;
use uzi_core::cache::{read_task_output, write_task_output};
use uzi_core::py::{f_fin, round, truthy};

/// `compute_segmental` default `min_share_pct`.
pub const MIN_SHARE_PCT: f64 = 3.0;
/// `compute_segmental` default `max_segments`.
pub const MAX_SEGMENTS: usize = 6;

const INFLECTION_KEYWORDS: &[&str] = &[
    "收购", "分拆", "剥离", "重组", "合并", "新品", "上市", "产能投产", "海外", "出海", "转型",
    "高端化", "客户突破", "订单", "中标", "ODM", "OEM", "自研", "专利", "集采", "降价", "提价",
    "扩产", "投产",
];

// ───────────────────── Python value rendering ─────────────────────

fn num_repr(n: &serde_json::Number) -> String {
    match n.as_f64() {
        Some(x) if n.is_f64() => {
            if x.fract() == 0.0 && x.abs() < 1e16 {
                format!("{:.1}", x)
            } else {
                format!("{}", x)
            }
        }
        _ => n.to_string(),
    }
}

/// Python `str(v)` for a JSON value (f-string interpolation).
fn py_display(v: &Value) -> String {
    match v {
        Value::Null => "None".to_string(),
        Value::Bool(b) => if *b { "True" } else { "False" }.to_string(),
        Value::Number(n) => num_repr(n),
        Value::String(s) => s.clone(),
        Value::Array(_) | Value::Object(_) => py_repr(v),
    }
}

/// Python `repr(v)` for a JSON value.
fn py_repr(v: &Value) -> String {
    match v {
        Value::Null => "None".to_string(),
        Value::Bool(b) => if *b { "True" } else { "False" }.to_string(),
        Value::Number(n) => num_repr(n),
        Value::String(s) => format!("'{}'", s.replace('\\', "\\\\").replace('\'', "\\'")),
        Value::Array(a) => format!(
            "[{}]",
            a.iter().map(py_repr).collect::<Vec<_>>().join(", ")
        ),
        Value::Object(o) => format!(
            "{{{}}}",
            o.iter()
                .map(|(k, v)| format!("{}: {}", py_repr(&Value::from(k.as_str())), py_repr(v)))
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}

// ───────────────────────── helpers ─────────────────────────

fn arr<'a>(v: &'a Value, key: &str) -> &'a [Value] {
    match v.get(key) {
        Some(Value::Array(a)) => a,
        _ => &[],
    }
}

/// `(dims[key] or {}).get("data") or {}`.
fn dd<'a>(raw: &'a Value, key: &str) -> &'a Value {
    static NULL: Value = Value::Null;
    raw.get("dimensions")
        .and_then(|d| d.get(key))
        .and_then(|e| e.get("data"))
        .unwrap_or(&NULL)
}

/// Python `float(v)` — plain parse, **not** `_f` (upstream `discover_segments`
/// calls bare `float()`, so `"12.5亿"` is a failure, not 12.5).
fn parse_num(v: &Value) -> Option<f64> {
    match v {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.trim().parse::<f64>().ok(),
        _ => None,
    }
}

/// `str(v)` — Python `str`, used for period/classification comparisons.
fn s_of(v: &Value) -> String {
    uzi_core::py::py_str(v)
}

fn empty_segment(name: &str, latest_revenue_yi: Value, latest_share_pct: Value) -> Value {
    let mut m = Map::new();
    m.insert("name".into(), Value::from(name));
    m.insert("latest_revenue_yi".into(), latest_revenue_yi);
    m.insert("latest_share_pct".into(), latest_share_pct);
    m.insert("yoy_growth_pct".into(), Value::Null);
    m.insert("gross_margin_pct".into(), Value::Null);
    m.insert("profit_share_pct".into(), Value::Null);
    m.insert("revenue_history_yi".into(), Value::Array(Vec::new()));
    m.insert("history_periods".into(), Value::Array(Vec::new()));
    m.insert("drivers".into(), Value::Array(Vec::new()));
    m.insert("thesis_tag".into(), Value::from(""));
    m.insert("bull_growth_3y_cagr".into(), Value::Null);
    m.insert("base_growth_3y_cagr".into(), Value::Null);
    m.insert("bear_growth_3y_cagr".into(), Value::Null);
    m.insert("agent_note".into(), Value::from(""));
    Value::Object(m)
}

// ───────────────────────── discover_segments ─────────────────────────

/// Port of `segmental_model.discover_segments(raw, min_share_pct=3.0, max_segments=6)`.
///
/// Returns the `SegmentalSkeleton.to_dict()` JSON object.
pub fn discover_segments(raw: &Value) -> Value {
    discover_segments_with(raw, MIN_SHARE_PCT, MAX_SEGMENTS)
}

/// [`discover_segments`] with explicit thresholds.
pub fn discover_segments_with(raw: &Value, min_share_pct: f64, max_segments: usize) -> Value {
    let basic = dd(raw, "0_basic");
    let chain = dd(raw, "5_chain");
    let fin = dd(raw, "1_financials");
    let events = dd(raw, "15_events");

    let name = match basic.get("name") {
        Some(v) if truthy(v) => v.clone(),
        _ => raw.get("ticker").cloned().unwrap_or(Value::from("")),
    };
    let currency = match fin.get("currency") {
        Some(v) if truthy(v) => s_of(v),
        _ => match basic.get("market").and_then(|m| m.as_str()) {
            Some("H") => "HKD".to_string(),
            Some("U") => "USD".to_string(),
            _ => "CNY".to_string(),
        },
    };

    let rev_hist = arr(fin, "revenue_history");
    let latest_rev = rev_hist.last().cloned().unwrap_or(Value::from(0.0));
    let latest_rev_f = f_fin(&latest_rev, 0.0);

    let mut segments: Vec<Value> = Vec::new();
    let mut source_notes: Vec<Value> = Vec::new();

    // ═══ Tier 1 · main_business_raw ═══
    let mb_raw = arr(chain, "main_business_raw");
    if !mb_raw.is_empty() {
        let mut periods: BTreeSet<String> = BTreeSet::new();
        for r in mb_raw {
            let d = r.get("报告日期").cloned().unwrap_or(Value::Null);
            if truthy(&d) {
                periods.insert(s_of(&d));
            }
        }
        let latest_period = periods.iter().next_back().cloned();

        if let Some(latest_period) = latest_period {
            let same_period: Vec<&Value> = mb_raw
                .iter()
                .filter(|r| s_of(r.get("报告日期").unwrap_or(&Value::Null)) == latest_period)
                .collect();
            let has_product = |r: &&Value| {
                s_of(r.get("分类类型").unwrap_or(&Value::Null)).contains("产品")
            };
            let has_industry = |r: &&Value| {
                s_of(r.get("分类类型").unwrap_or(&Value::Null)).contains("行业")
            };
            let by_product: Vec<&Value> = same_period.iter().copied().filter(has_product).collect();
            let by_industry: Vec<&Value> =
                same_period.iter().copied().filter(has_industry).collect();
            let (chosen, classification) = if !by_product.is_empty() {
                (by_product, "按产品分类")
            } else if !by_industry.is_empty() {
                (by_industry, "按行业分类")
            } else {
                (same_period, "mixed")
            };

            let mut other_share = 0.0f64;
            for r in &chosen {
                let nm = s_of(r.get("主营构成").unwrap_or(&Value::Null))
                    .trim()
                    .to_string();
                if nm.is_empty() || matches!(nm.as_str(), "合计" | "总计" | "其他(补充)") {
                    continue;
                }
                let rev_yuan = match r.get("主营收入") {
                    Some(v) if truthy(v) => parse_num(v),
                    _ => Some(0.0),
                };
                let share_dec = match r.get("收入比例") {
                    Some(v) if truthy(v) => parse_num(v),
                    _ => Some(0.0),
                };
                let (Some(rev_yuan), Some(share_dec)) = (rev_yuan, share_dec) else {
                    continue;
                };
                if rev_yuan <= 0.0 {
                    continue;
                }
                let share_pct = round(share_dec * 100.0, 2);
                if share_pct < min_share_pct {
                    other_share += share_pct;
                    continue;
                }

                let gross_margin_pct = match r.get("毛利率") {
                    None | Some(Value::Null) => None,
                    Some(v) => match v {
                        Value::Number(n) if n.as_f64().map(|x| x.is_nan()).unwrap_or(false) => None,
                        _ => parse_num(v).map(|x| round(x * 100.0, 1)),
                    },
                };
                let profit_share_pct = match r.get("利润比例") {
                    None | Some(Value::Null) => None,
                    Some(v) => parse_num(v).map(|x| round(x * 100.0, 2)),
                };

                // 该 segment 的历史营收（同分类同 segment 多期）
                let mut same_seg: Vec<&Value> = mb_raw
                    .iter()
                    .filter(|rr| {
                        s_of(rr.get("主营构成").unwrap_or(&Value::Null)).trim() == nm
                            && s_of(rr.get("分类类型").unwrap_or(&Value::Null)) == classification
                    })
                    .collect();
                same_seg.sort_by_key(|x| s_of(x.get("报告日期").unwrap_or(&Value::Null)));
                let mut hist_rev: Vec<Value> = Vec::new();
                let mut hist_periods: Vec<Value> = Vec::new();
                for rr in same_seg {
                    let yi = match rr.get("主营收入") {
                        Some(v) => match parse_num(v) {
                            Some(x) => x / 1e8,
                            None => continue,
                        },
                        _ => continue,
                    };
                    if yi > 0.0 {
                        hist_rev.push(Value::from(round(yi, 2)));
                        hist_periods.push(Value::from(
                            s_of(rr.get("报告日期").unwrap_or(&Value::Null))
                                .chars()
                                .take(10)
                                .collect::<String>(),
                        ));
                    }
                }

                let mut seg = empty_segment(
                    &nm.chars().take(20).collect::<String>(),
                    Value::from(round(rev_yuan / 1e8, 2)),
                    Value::from(share_pct),
                );
                let m = seg.as_object_mut().unwrap();
                m.insert("gross_margin_pct".into(), opt_num(gross_margin_pct));
                m.insert("profit_share_pct".into(), opt_num(profit_share_pct));
                m.insert("revenue_history_yi".into(), Value::Array(hist_rev));
                m.insert("history_periods".into(), Value::Array(hist_periods));
                segments.push(seg);
                if segments.len() >= max_segments {
                    break;
                }
            }

            if other_share >= min_share_pct {
                segments.push(empty_segment(
                    "其他合计",
                    Value::from(round(latest_rev_f * other_share / 100.0, 2)),
                    Value::from(round(other_share, 2)),
                ));
            }
            source_notes.push(Value::from(format!(
                "分段来源: 5_chain.main_business_raw · {} · 报告期 {} · {} 项",
                classification,
                latest_period,
                segments.len()
            )));
        }
    }

    // ═══ Tier 2 · main_business_breakdown ═══
    if segments.is_empty() {
        let mb_bd = arr(chain, "main_business_breakdown");
        if !mb_bd.is_empty() {
            for item in mb_bd.iter().take(max_segments) {
                let nm = s_of(item.get("name").unwrap_or(&Value::Null))
                    .trim()
                    .to_string();
                if nm.is_empty() || nm.contains("补充") {
                    continue;
                }
                let v = match item.get("value") {
                    Some(v) if truthy(v) => parse_num(v),
                    _ => Some(0.0),
                };
                let Some(v) = v else { continue };
                let share_pct = if v <= 1.0 {
                    round(v * 100.0, 2)
                } else {
                    round(v, 2)
                };
                if share_pct < min_share_pct {
                    continue;
                }
                let seg_rev = if latest_rev_f != 0.0 {
                    round(latest_rev_f * share_pct / 100.0, 2)
                } else {
                    0.0
                };
                segments.push(empty_segment(
                    &nm.chars().take(20).collect::<String>(),
                    Value::from(seg_rev),
                    Value::from(share_pct),
                ));
            }
            source_notes.push(Value::from(format!(
                "分段来源: 5_chain.main_business_breakdown ({} 项)",
                segments.len()
            )));
        }
    }

    // ═══ Tier 3 · breakdown_top ═══
    if segments.is_empty() {
        let breakdown_top = arr(chain, "breakdown_top");
        if !breakdown_top.is_empty() {
            for item in breakdown_top.iter().take(max_segments) {
                let nm: String = s_of(item.get("name").unwrap_or(&Value::Null))
                    .chars()
                    .take(20)
                    .collect();
                let share = match item.get("value") {
                    Some(v) if truthy(v) => parse_num(v).unwrap_or(0.0),
                    _ => 0.0,
                };
                if share < min_share_pct {
                    continue;
                }
                let seg_rev = if latest_rev_f != 0.0 {
                    round(latest_rev_f * share / 100.0, 2)
                } else {
                    0.0
                };
                segments.push(empty_segment(
                    &nm,
                    Value::from(seg_rev),
                    Value::from(share),
                ));
            }
            source_notes.push(Value::from(format!(
                "分段来源: 5_chain.breakdown_top ({} 项)",
                segments.len()
            )));
        }
    }

    if segments.is_empty() {
        source_notes.push(Value::from(
            "⚠️ 5_chain 无可用分段数据 — agent 需从 6_research 或 0_basic.main_business 文字描述手动归纳",
        ));
    }

    // 拐点候选
    let mut inflection_candidates: Vec<Value> = Vec::new();
    let evs: Vec<Value> = match events.get("events") {
        Some(Value::Array(a)) if !a.is_empty() => a.clone(),
        _ => match events.get("recent_events") {
            Some(Value::Array(a)) if !a.is_empty() => a.clone(),
            _ => Vec::new(),
        },
    };
    for ev in evs.iter().take(30) {
        let title = if let Value::Object(o) = ev {
            let t = o.get("title").cloned().unwrap_or(Value::Null);
            if truthy(&t) {
                s_of(&t)
            } else {
                s_of(o.get("name").unwrap_or(&Value::Null))
            }
        } else {
            s_of(ev)
        };
        for kw in INFLECTION_KEYWORDS {
            if title.contains(kw) {
                inflection_candidates
                    .push(Value::from(title.chars().take(80).collect::<String>()));
                break;
            }
        }
        if inflection_candidates.len() >= 8 {
            break;
        }
    }
    if inflection_candidates.is_empty() {
        source_notes.push(Value::from(
            "⚠️ 15_events 里未抽到拐点关键词 — agent 需从 6_research 或 14_moat 补",
        ));
    }

    let mut out = Map::new();
    out.insert(
        "ticker".into(),
        raw.get("ticker").cloned().unwrap_or(Value::from("")),
    );
    out.insert("name".into(), name);
    out.insert("currency".into(), Value::from(currency));
    out.insert(
        "total_revenue_latest_yi".into(),
        Value::from(round(latest_rev_f, 2)),
    );
    out.insert(
        "total_revenue_history_yi".into(),
        Value::Array(
            rev_hist
                .iter()
                .map(|x| Value::from(round(f_fin(x, 0.0), 2)))
                .collect(),
        ),
    );
    out.insert("segments".into(), Value::Array(segments));
    out.insert(
        "inflection_candidates".into(),
        Value::Array(inflection_candidates),
    );
    out.insert("source_notes".into(), Value::Array(source_notes));
    Value::Object(out)
}

fn opt_num(x: Option<f64>) -> Value {
    match x {
        Some(v) => Value::from(v),
        None => Value::Null,
    }
}

// ───────────────────────── validate_model ─────────────────────────

/// Port of `segmental_model.validate_model(filled, raw)`.
pub fn validate_model(filled: &Value, raw: &Value) -> Value {
    let mut errors: Vec<Value> = Vec::new();
    let mut warnings: Vec<Value> = Vec::new();
    let mut summary: Map<String, Value> = Map::new();

    let fin = dd(raw, "1_financials");
    let rev_hist = arr(fin, "revenue_history");
    let total_rev = if !rev_hist.is_empty() {
        rev_hist.last().cloned().unwrap_or(Value::Null)
    } else {
        Value::from(0)
    };
    let total_rev_f = f_fin(&total_rev, 0.0);

    let segments = arr(filled, "segments");
    if segments.is_empty() {
        errors.push(Value::from("segments 为空"));
        let mut out = Map::new();
        out.insert("passed".into(), Value::Bool(false));
        out.insert("errors".into(), Value::Array(errors));
        out.insert("warnings".into(), Value::Array(warnings));
        out.insert("summary".into(), Value::Object(summary));
        return Value::Object(out);
    }

    let sum_rev: f64 = segments
        .iter()
        .map(|s| f_fin(s.get("latest_revenue_yi").unwrap_or(&Value::Null), 0.0))
        .sum();
    let reconciliation_pct = if total_rev_f != 0.0 {
        (sum_rev - total_rev_f).abs() / total_rev_f.max(1.0) * 100.0
    } else {
        0.0
    };
    summary.insert("total_actual".into(), total_rev.clone());
    summary.insert("sum_segments".into(), Value::from(round(sum_rev, 2)));
    summary.insert(
        "reconciliation_gap_pct".into(),
        Value::from(round(reconciliation_pct, 2)),
    );

    // 规则 1: 对账
    if total_rev_f > 0.0 && reconciliation_pct > 10.0 {
        errors.push(Value::from(format!(
            "segments 总和 {:.1} 亿 vs 实际 revenue {:.1} 亿 差 {:.0}%（阈值 10%）",
            sum_rev, total_rev_f, reconciliation_pct
        )));
    }

    // 规则 2: CAGR 单调性
    for s in segments {
        let bull = s.get("bull_growth_3y_cagr").cloned().unwrap_or(Value::Null);
        let base = s.get("base_growth_3y_cagr").cloned().unwrap_or(Value::Null);
        let bear = s.get("bear_growth_3y_cagr").cloned().unwrap_or(Value::Null);
        if bull.is_null() || base.is_null() || bear.is_null() {
            warnings.push(Value::from(format!(
                "segment {} 缺 3 情景 CAGR，agent 未填完",
                py_repr(s.get("name").unwrap_or(&Value::Null))
            )));
            continue;
        }
        let (b, ba, be) = (
            f_fin(&bull, 0.0),
            f_fin(&base, 0.0),
            f_fin(&bear, 0.0),
        );
        if !(b >= ba && ba >= be) {
            errors.push(Value::from(format!(
                "segment {} CAGR 不单调: bull={} base={} bear={}",
                py_repr(s.get("name").unwrap_or(&Value::Null)),
                py_display(&bull),
                py_display(&base),
                py_display(&bear)
            )));
        }
    }

    // 规则 3: Base 情景总增速
    let mut base_3y_total_growth = 0.0f64;
    for s in segments {
        let base = s.get("base_growth_3y_cagr").cloned().unwrap_or(Value::Null);
        let base = if truthy(&base) { f_fin(&base, 0.0) } else { 0.0 };
        let share = f_fin(s.get("latest_share_pct").unwrap_or(&Value::Null), 0.0) / 100.0;
        base_3y_total_growth += share * ((1.0 + base / 100.0).powf(3.0) - 1.0) * 100.0;
    }
    summary.insert(
        "base_3y_total_growth_pct".into(),
        Value::from(round(base_3y_total_growth, 1)),
    );
    if base_3y_total_growth > 100.0 {
        warnings.push(Value::from(format!(
            "Base 情景 3 年总营收增速 {:.0}%（>100%）—— 需要明确收购/新业务 note 支撑",
            base_3y_total_growth
        )));
    }

    // 规则 4: drivers + thesis_tag
    for s in segments {
        if !truthy(s.get("drivers").unwrap_or(&Value::Null)) {
            warnings.push(Value::from(format!(
                "segment {} 未填 drivers（价/量/市占/渗透）",
                py_repr(s.get("name").unwrap_or(&Value::Null))
            )));
        }
        if !truthy(s.get("thesis_tag").unwrap_or(&Value::Null)) {
            warnings.push(Value::from(format!(
                "segment {} 未填 thesis_tag",
                py_repr(s.get("name").unwrap_or(&Value::Null))
            )));
        }
    }

    let mut out = Map::new();
    out.insert("passed".into(), Value::Bool(errors.is_empty()));
    out.insert("errors".into(), Value::Array(errors));
    out.insert("warnings".into(), Value::Array(warnings));
    out.insert("summary".into(), Value::Object(summary));
    Value::Object(out)
}

// ───────────────────────── render_skeleton_markdown ─────────────────────────

/// Port of `segmental_model.render_skeleton_markdown(skel)`.
pub fn render_skeleton_markdown(skel: &Value) -> String {
    let segments = arr(skel, "segments");
    let name = py_display(skel.get("name").unwrap_or(&Value::Null));
    let ticker = py_display(skel.get("ticker").unwrap_or(&Value::Null));

    let mut lines: Vec<String> = vec![
        format!("# Segmental Build-Up · {} ({})", name, ticker),
        String::new(),
        format!(
            "**总营收最新**: {} 亿 {}",
            py_display(skel.get("total_revenue_latest_yi").unwrap_or(&Value::Null)),
            py_display(skel.get("currency").unwrap_or(&Value::Null))
        ),
        format!(
            "**历史营收（近 6 年）**: {}",
            py_display(skel.get("total_revenue_history_yi").unwrap_or(&Value::Null))
        ),
        String::new(),
        format!("## 业务分段（{} 条）", segments.len()),
    ];

    for (i, s) in segments.iter().enumerate() {
        lines.push(format!(
            "\n### {}. {}",
            i + 1,
            py_display(s.get("name").unwrap_or(&Value::Null))
        ));
        lines.push(format!(
            "  - 最新营收: {} 亿（占比 {}%）",
            py_display(s.get("latest_revenue_yi").unwrap_or(&Value::Null)),
            py_display(s.get("latest_share_pct").unwrap_or(&Value::Null))
        ));
        let yoy = match s.get("yoy_growth_pct") {
            None | Some(Value::Null) => "—".to_string(),
            Some(v) => py_display(v),
        };
        lines.push(format!("  - 同比: {}", yoy));
        lines.push("  - [agent 待填] drivers: ".to_string());
        lines.push("  - [agent 待填] thesis_tag: ".to_string());
        lines.push("  - [agent 待填] Bull 3Y CAGR: __% · Base __% · Bear __%".to_string());
    }

    let inflection = arr(skel, "inflection_candidates");
    if !inflection.is_empty() {
        lines.push("\n## 潜在拐点候选（来自 15_events）".to_string());
        for c in inflection.iter().take(6) {
            lines.push(format!("  - {}", py_display(c)));
        }
    }
    let notes = arr(skel, "source_notes");
    if !notes.is_empty() {
        lines.push("\n## 数据溯源注记".to_string());
        for n in notes {
            lines.push(format!("  - {}", py_display(n)));
        }
    }
    lines.join("\n")
}

// ───────────────────────── compute_segmental CLI glue ─────────────────────────

/// `compute_segmental.cmd_discover` — writes the skeleton cache and prints the markdown.
pub fn cmd_discover(ticker: &str) -> i32 {
    let raw = read_task_output(ticker, "raw_data").unwrap_or(Value::Null);
    if !truthy(&raw) {
        eprintln!("❌ {} raw_data.json 不存在 — 请先跑 stage1", ticker);
        return 1;
    }

    let skel = discover_segments(&raw);
    let _ = write_task_output(ticker, "segmental_skeleton", &skel);

    let md = render_skeleton_markdown(&skel);
    println!("{}", md);
    println!("\n→ 骨架 JSON 已写入: .cache/{}/segmental_skeleton.json", ticker);
    println!("→ Agent 下一步: 读此 JSON，填 segments[*].drivers / thesis_tag / bull_base_bear_growth_3y_cagr");
    println!("→ 填完写回 .cache/{}/segmental_model.json", ticker);
    println!("→ 然后跑: uzi {} --segmental validate", ticker);
    0
}

/// `compute_segmental.cmd_validate` — validates the agent-filled model, writes the report.
pub fn cmd_validate(ticker: &str) -> i32 {
    let raw = read_task_output(ticker, "raw_data").unwrap_or(Value::Null);
    if !truthy(&raw) {
        eprintln!("❌ {} raw_data.json 不存在", ticker);
        return 1;
    }
    let filled = read_task_output(ticker, "segmental_model").unwrap_or(Value::Null);
    if !truthy(&filled) {
        eprintln!("❌ {} segmental_model.json 不存在 — agent 尚未填入", ticker);
        return 1;
    }

    let report = validate_model(&filled, &raw);
    let _ = write_task_output(ticker, "segmental_validation", &report);

    println!(
        "{} Segmental Model Validation · {}",
        if truthy(report.get("passed").unwrap_or(&Value::Null)) {
            "✓"
        } else {
            "✗"
        },
        ticker
    );
    let summary = report.get("summary").cloned().unwrap_or(Value::Null);
    if truthy(&summary) {
        let sq = |k: &str| py_display(summary.get(k).unwrap_or(&Value::Null));
        println!(
            "  对账: sum={} vs actual={} · gap={}%",
            sq("sum_segments"),
            sq("total_actual"),
            sq("reconciliation_gap_pct")
        );
        if summary.get("base_3y_total_growth_pct").is_some() {
            println!(
                "  Base 情景 3 年总增速: {}%",
                sq("base_3y_total_growth_pct")
            );
        }
    }
    if let Value::Array(errs) = report.get("errors").unwrap_or(&Value::Null) {
        if !errs.is_empty() {
            println!("\n🔴 ERRORS:");
            for e in errs {
                println!("  - {}", py_display(e));
            }
        }
    }
    if let Value::Array(ws) = report.get("warnings").unwrap_or(&Value::Null) {
        if !ws.is_empty() {
            println!("\n🟡 WARNINGS:");
            for w in ws {
                println!("  - {}", py_display(w));
            }
        }
    }
    let passed = truthy(report.get("passed").unwrap_or(&Value::Null));
    let no_warnings = report
        .get("warnings")
        .and_then(|w| w.as_array())
        .map(|a| a.is_empty())
        .unwrap_or(true);
    if passed && no_warnings {
        println!("  全部通过 · 可进 synthesis/HTML");
    }

    if passed {
        0
    } else {
        1
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn discovery_without_segment_data_warns_and_stays_empty() {
        let raw = json!({"ticker": "002273.SZ", "dimensions": {
            "0_basic": {"data": {"name": "水晶光电"}},
            "1_financials": {"data": {"revenue_history": [32.1, 52.3]}},
            "5_chain": {"data": {"main_business_breakdown": [{"name": "光学", "revenue_pct": 62.5}]}}
        }});
        let skel = discover_segments(&raw);
        assert_eq!(skel["segments"], json!([]));
        assert_eq!(skel["currency"], json!("CNY"));
        assert_eq!(skel["total_revenue_latest_yi"], json!(52.3));
        assert_eq!(skel["total_revenue_history_yi"], json!([32.1, 52.3]));
        let notes = skel["source_notes"].as_array().unwrap();
        assert!(notes.iter().any(|n| n.as_str().unwrap().contains("无可用分段数据")));
    }

    #[test]
    fn discovery_reads_richest_main_business_raw_tier() {
        let raw = json!({"ticker": "600519.SH", "dimensions": {
            "0_basic": {"data": {"name": "贵州茅台"}},
            "1_financials": {"data": {"revenue_history": [100.0]}},
            "5_chain": {"data": {"main_business_raw": [
                {"报告日期": "2025-12-31", "分类类型": "按产品分类", "主营构成": "茅台酒",
                 "主营收入": 8000000000.0, "收入比例": 0.8, "毛利率": 0.92, "利润比例": 0.9},
                {"报告日期": "2025-12-31", "分类类型": "按产品分类", "主营构成": "系列酒",
                 "主营收入": 1500000000.0, "收入比例": 0.15, "毛利率": 0.6, "利润比例": 0.08},
                {"报告日期": "2025-12-31", "分类类型": "按产品分类", "主营构成": "其他",
                 "主营收入": 500000000.0, "收入比例": 0.05}
            ]}}
        }});
        let skel = discover_segments(&raw);
        let segs = skel["segments"].as_array().unwrap();
        assert_eq!(segs.len(), 3);
        assert_eq!(segs[0]["name"], json!("茅台酒"));
        assert_eq!(segs[0]["latest_revenue_yi"], json!(80.0));
        assert_eq!(segs[0]["latest_share_pct"], json!(80.0));
        assert_eq!(segs[0]["gross_margin_pct"], json!(92.0));
        assert_eq!(segs[0]["revenue_history_yi"], json!([80.0]));
        // 5% is above the 3% floor so it stays a named segment, not 其他合计
        assert_eq!(segs[2]["name"], json!("其他"));
    }

    #[test]
    fn validation_fails_when_segments_empty() {
        let out = validate_model(&json!({"segments": []}), &json!({}));
        assert_eq!(out["passed"], json!(false));
        assert_eq!(out["errors"], json!(["segments 为空"]));
    }

    #[test]
    fn validation_flags_reconciliation_gap_and_monotonicity() {
        let raw = json!({"dimensions": {"1_financials": {"data": {"revenue_history": [100.0]}}}});
        let filled = json!({"segments": [
            {"name": "A", "latest_revenue_yi": 50.0, "latest_share_pct": 50.0,
             "bull_growth_3y_cagr": 10.0, "base_growth_3y_cagr": 20.0, "bear_growth_3y_cagr": 5.0}
        ]});
        let out = validate_model(&filled, &raw);
        assert_eq!(out["passed"], json!(false));
        let errs = out["errors"].as_array().unwrap();
        assert!(errs.iter().any(|e| e.as_str().unwrap().contains("不单调")));
        assert!(errs.iter().any(|e| e.as_str().unwrap().contains("阈值 10%")));
        assert_eq!(out["summary"]["reconciliation_gap_pct"], json!(50.0));
    }

    #[test]
    fn markdown_renders_segments_and_notes() {
        let skel = json!({
            "ticker": "600519.SH", "name": "贵州茅台", "currency": "CNY",
            "total_revenue_latest_yi": 100.0, "total_revenue_history_yi": [80.0, 100.0],
            "segments": [{"name": "茅台酒", "latest_revenue_yi": 80.0, "latest_share_pct": 80.0,
                          "yoy_growth_pct": null}],
            "inflection_candidates": ["提价"], "source_notes": ["note"]
        });
        let md = render_skeleton_markdown(&skel);
        assert!(md.starts_with("# Segmental Build-Up · 贵州茅台 (600519.SH)"));
        assert!(md.contains("**历史营收（近 6 年）**: [80.0, 100.0]"));
        assert!(md.contains("## 业务分段（1 条）"));
        assert!(md.contains("\n### 1. 茅台酒"));
        assert!(md.contains("  - 同比: —"));
        assert!(md.contains("  - 提价"));
        assert!(md.contains("  - note"));
    }
}
