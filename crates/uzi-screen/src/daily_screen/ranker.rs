//! Port of `lib/daily_screen/ranker.py` — hard gates, evidence confidence and
//! actionability decisions.

use chrono::{DateTime, FixedOffset};
use serde_json::Value;

use super::events::{evidence_time, filter_evidence};
use super::execution::execution_gaps;
use super::models::{ScreenCandidate, StockSnapshot};
use super::personas::{evaluate_f_personas, evaluate_serenity};

/// `preselect(stocks, limit=40)` — stable descending sort by the liquidity /
/// strength / activity proxy.
pub fn preselect(stocks: &[StockSnapshot], limit: usize) -> Vec<StockSnapshot> {
    let score = |stock: &StockSnapshot| -> f64 {
        let liquidity = (stock.amount / 10e8).min(4.0);
        let strength = stock.change_pct.min(10.0).max(-3.0);
        let activity = (stock.turnover_rate.unwrap_or(0.0) / 3.0).min(3.0);
        strength * 2.0 + liquidity + activity
    };
    let mut out: Vec<StockSnapshot> = stocks.to_vec();
    out.sort_by(|a, b| {
        score(b)
            .partial_cmp(&score(a))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    out.truncate(limit);
    out
}

fn theme_number(theme: &Value, key: &str, default: f64) -> f64 {
    match theme.get(key) {
        Some(v) if uzi_core::py::truthy(v) => uzi_core::py::f0(v),
        _ => default,
    }
}

fn theme_opt_f64(theme: &Value, key: &str) -> Option<f64> {
    theme.get(key).and_then(|v| v.as_f64())
}

/// `build_candidate(stock, theme, evidence, gaps, evaluated_at=…)`.
pub fn build_candidate(
    stock: &StockSnapshot,
    theme: &Value,
    evidence: &[Value],
    gaps: &[String],
    evaluated_at: &DateTime<FixedOffset>,
) -> ScreenCandidate {
    let (evidence, time_gaps) = filter_evidence(evidence, &stock.observed_at);
    let mut gaps: Vec<String> = gaps.to_vec();
    gaps.extend(time_gaps);

    let verdicts = evaluate_f_personas(stock, theme, Some(&evidence));
    let serenity = evaluate_serenity(stock, theme, Some(&evidence));
    let active_f: Vec<&super::models::PersonaVerdict> =
        verdicts.iter().filter(|item| item.eligible).collect();
    let bullish: Vec<&super::models::PersonaVerdict> = active_f
        .iter()
        .copied()
        .filter(|item| item.signal == "bullish")
        .collect();
    let bearish: Vec<&super::models::PersonaVerdict> = active_f
        .iter()
        .copied()
        .filter(|item| item.signal == "bearish")
        .collect();

    // `sum(value not in (None, "") for value in [price, amount, change_pct,
    // observed_at, source]) / 5 * 25`.
    let quality_fields: [Value; 5] = [
        Value::from(stock.price),
        Value::from(stock.amount),
        Value::from(stock.change_pct),
        Value::String(stock.observed_at.clone()),
        Value::String(stock.source.clone()),
    ];
    let quality_present = quality_fields
        .iter()
        .filter(|v| !v.is_null() && v.as_str() != Some(""))
        .count();
    let data_quality = quality_present as f64 / 5.0 * 25.0;

    let mut theme_score = 0.0_f64;
    if uzi_core::py::truthy(uzi_core::py::get(theme, "theme_rank")) {
        let rank = uzi_core::py::f0(uzi_core::py::get(theme, "theme_rank"));
        theme_score += (15.0 - (rank - 1.0) * 2.0).max(0.0);
    }
    theme_score += (theme_number(theme, "breadth_pct", 0.0) / 10.0).min(10.0);

    let event_score: f64 = evidence
        .iter()
        .filter(|item| uzi_core::py::get(item, "kind").as_str() == Some("event"))
        .map(|item| match uzi_core::py::get(item, "grade").as_str() {
            Some("A") => 8.0,
            Some("B") => 5.0,
            _ => 2.0,
        })
        .sum::<f64>()
        .min(20.0);

    let mut tape_score =
        ((stock.change_pct + 5.0).max(0.0) + (stock.amount / 2e8).min(8.0)).min(20.0);
    if matches!(stock.high, Some(high) if high != 0.0 && stock.price >= high * 0.985) {
        tape_score = (tape_score + 3.0).min(20.0);
    }
    let serenity_bonus = if serenity.signal == "bullish" {
        4.0
    } else if serenity.signal == "neutral" {
        1.0
    } else {
        0.0
    };
    let role_score = (bullish.len() as f64 * 1.2 + serenity_bonus).min(10.0);
    let confidence = uzi_core::py::round(
        (data_quality + theme_score + event_score + tape_score + role_score).min(100.0),
        1,
    );

    let mut risk_flags: Vec<String> = Vec::new();
    let mut blocking = execution_gaps(stock, evaluated_at);
    if !uzi_core::py::truthy(theme) {
        blocking.push("industry_context_missing".to_string());
    }
    let industry_source_ok = stock
        .extra
        .get("industry_source")
        .and_then(|v| v.get("source"))
        .map(uzi_core::py::truthy)
        .unwrap_or(false);
    if stock.extra_num("industry_coverage", 0.0) < 0.95 || !industry_source_ok {
        blocking.push("industry_coverage_unverified".to_string());
    }
    let theme_time = evidence_time(uzi_core::py::get(theme, "observed_at"));
    match theme_time {
        Some(theme_dt) => {
            let delta = evaluated_at
                .signed_duration_since(theme_dt)
                .num_milliseconds();
            if !(0..=300_000).contains(&delta) {
                blocking.push("industry_snapshot_stale".to_string());
            }
        }
        None => blocking.push("industry_snapshot_stale".to_string()),
    }
    if stock.change_pct >= 8.0 {
        risk_flags.push("短时涨幅偏大，追价风险高".to_string());
    }
    if !bearish.is_empty() && bearish.len() > bullish.len() {
        risk_flags.push("F 组反对人数高于看多人数".to_string());
    }
    let mut eligible_news: Vec<&Value> = Vec::new();
    for item in &evidence {
        let published = evidence_time(uzi_core::py::get(item, "published_at"));
        let raw_url = uzi_core::py::get(item, "url");
        let raw_url = if uzi_core::py::truthy(raw_url) {
            uzi_core::py::py_str(raw_url)
        } else {
            String::new()
        };
        let url_ok = uzi_report::security::safe_url_default(&Value::String(raw_url)) != "#";
        let source_ok = uzi_core::py::truthy(uzi_core::py::get(item, "source"));
        let company_specific = matches!(
            uzi_core::py::get(item, "company_specific"),
            Value::Bool(true)
        );
        let fresh = match published {
            Some(published_dt) => {
                let delta = evaluated_at
                    .signed_duration_since(published_dt)
                    .num_milliseconds();
                (0..=72 * 3600 * 1000).contains(&delta)
            }
            None => false,
        };
        if uzi_core::py::get(item, "kind").as_str() == Some("event")
            && company_specific
            && source_ok
            && url_ok
            && fresh
        {
            eligible_news.push(item);
        }
    }
    if eligible_news.is_empty() {
        blocking.push("event_evidence_missing".to_string());
    }
    if gaps.iter().any(|g| g == "snapshot_only") {
        blocking.push("snapshot_only".to_string());
    }

    let leader = theme_number(theme, "leader_rank", 999.0) <= 2.0;
    let action = if !blocking.is_empty() || confidence < 70.0 {
        "watch_only"
    } else if stock.change_pct >= 7.0 {
        "wait_pullback"
    } else if leader
        && stock.change_pct >= 3.0
        && (!bullish.is_empty() || serenity.signal == "bullish")
    {
        "buyable"
    } else if leader {
        "wait_reseal"
    } else {
        "watch_only"
    };
    gaps.extend(blocking);

    let mut supporters: Vec<String> = bullish
        .iter()
        .take(4)
        .map(|item| item.name.clone())
        .collect();
    if serenity.signal == "bullish" {
        supporters.push("Serenity".to_string());
    }
    let support_text = if supporters.is_empty() {
        "暂无强看多角色".to_string()
    } else {
        supporters.join("、")
    };
    let theme_rank_display = match theme.get("theme_rank") {
        Some(v) => uzi_core::py::py_display(v),
        None => "—".to_string(),
    };
    let leader_rank_display = match theme.get("leader_rank") {
        Some(v) => uzi_core::py::py_display(v),
        None => "—".to_string(),
    };
    let why_now = format!(
        "行业横截面第 {}，个股板块内第 {}；涨幅 {:+.2}%，成交额 {:.1} 亿；{}。",
        theme_rank_display,
        leader_rank_display,
        stock.change_pct,
        stock.amount / 1e8,
        support_text
    );
    let entry_condition = match bullish.first() {
        Some(item) => item.entry_condition.clone(),
        None => serenity.entry_condition.clone(),
    };
    let invalidation = match bearish.first() {
        Some(item) => item.invalidation.clone(),
        None => match bullish.first() {
            Some(item) => item.invalidation.clone(),
            None => "跌破上午承接位且板块宽度继续下降".to_string(),
        },
    };

    gaps.sort();
    gaps.dedup();
    ScreenCandidate {
        snapshot: stock.clone(),
        research_confidence: confidence,
        action: action.to_string(),
        why_now,
        entry_condition,
        invalidation,
        theme_rank: theme_opt_f64(theme, "theme_rank").map(|v| v as i64),
        leader_rank: theme_opt_f64(theme, "leader_rank").map(|v| v as i64),
        theme_breadth_pct: theme_opt_f64(theme, "breadth_pct"),
        persona_verdicts: verdicts,
        serenity: Some(serenity),
        evidence,
        data_gaps: gaps,
        risk_flags,
    }
}

/// `rank_candidates(candidates, top_n=10, min_confidence=70)`.
pub fn rank_candidates(
    candidates: &[ScreenCandidate],
    top_n: usize,
    min_confidence: f64,
) -> (Vec<ScreenCandidate>, Vec<ScreenCandidate>) {
    let mut ordered: Vec<ScreenCandidate> = candidates.to_vec();
    ordered.sort_by(|a, b| {
        (b.research_confidence, b.snapshot.amount)
            .partial_cmp(&(a.research_confidence, a.snapshot.amount))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let picks: Vec<ScreenCandidate> = ordered
        .iter()
        .filter(|item| item.research_confidence >= min_confidence && item.action != "avoid")
        .take(top_n)
        .cloned()
        .collect();
    let rejected: Vec<ScreenCandidate> = ordered
        .iter()
        .filter(|item| !picks.iter().any(|p| p.snapshot.code == item.snapshot.code))
        .cloned()
        .collect();
    (picks, rejected)
}
