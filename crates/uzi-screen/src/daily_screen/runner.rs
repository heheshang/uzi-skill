//! Port of `lib/daily_screen/runner.py` — end-to-end orchestration for the A/H
//! daily screen, plus the `screen.py` CLI surface.

use anyhow::{bail, Result};
use chrono::{DateTime, FixedOffset, Timelike};
use serde_json::{Map, Value};
use std::path::{Path, PathBuf};

use super::events::{evidence_time, iso_seconds};
use super::models::StockSnapshot;
use super::ranker::{build_candidate, preselect, rank_candidates};
use super::renderer::render_report;
use super::sources::{enrich_intraday, fetch_industries};
use super::themes::build_theme_context;
use super::tracker::append_signals;
use super::universe::{apply_hard_filters, fetch_market_universe};
use super::events::enrich_stock;
use crate::paths::{assets_dir, daily_screen_ledger, reports_root};

/// `_enrich_candidate(stock)`.
fn enrich_candidate(stock: &mut StockSnapshot) -> (Vec<Value>, Vec<String>) {
    let (evidence, gaps) = enrich_stock(stock);
    enrich_intraday(stock);
    (evidence, gaps)
}

/// `_atomic_json(path, payload)`.
fn atomic_json(path: &Path, payload: &Value) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = PathBuf::from(format!("{}.tmp", path.display()));
    std::fs::write(&tmp, uzi_core::json::to_pretty(payload))?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

/// `_cutoff(now, market, mode)`.
fn cutoff(now: &DateTime<FixedOffset>, market: &str, mode: &str) -> String {
    let (hour, minute) = if mode == "noon" {
        if market == "A" {
            (11, 30)
        } else {
            (12, 0)
        }
    } else if market == "A" {
        (15, 0)
    } else {
        (16, 0)
    };
    let mut boundary = now
        .with_hour(hour)
        .and_then(|d| d.with_minute(minute))
        .and_then(|d| d.with_second(0))
        .and_then(|d| d.with_nanosecond(0))
        .unwrap_or(*now);
    if *now < boundary {
        boundary = *now;
    }
    iso_seconds(&boundary)
}

/// Report id fragment: `f"{dt:%Y%m%d}"` / `f"{dt:%H%M%S%f}"`.
fn report_id_of(now: &DateTime<FixedOffset>, mode: &str) -> String {
    format!(
        "{}-{}-ah-{}{:06}",
        now.format("%Y%m%d"),
        mode,
        now.format("%H%M%S"),
        now.timestamp_subsec_micros()
    )
}

/// Counter over `picks` actions, preserving first-seen order.
fn action_summary(picks: &[super::models::ScreenCandidate]) -> Map<String, Value> {
    let mut out = Map::new();
    for pick in picks {
        let entry = out
            .entry(pick.action.clone())
            .or_insert_with(|| Value::from(0));
        *entry = Value::from(entry.as_i64().unwrap_or(0) + 1);
    }
    out
}

/// Full upstream signature (`run_daily_screen`); `now` / `frozen_stocks` feed
/// the historical, deterministic test path.
#[allow(clippy::too_many_arguments)]
pub fn run_daily_screen_full(
    mode: &str,
    markets: &[String],
    top_n: usize,
    min_turnover_local: f64,
    enrich: bool,
    track: bool,
    output_root: Option<&Path>,
    now: Option<DateTime<FixedOffset>>,
    frozen_stocks: Option<&[StockSnapshot]>,
    // Overrides the report's `generated_at` (upstream reads the wall clock
    // here); the differential tests pin it so the report is reproducible.
    evaluated_at: Option<DateTime<FixedOffset>>,
) -> Result<Value> {
    if mode != "noon" && mode != "close" {
        bail!("mode must be noon or close");
    }
    if !(1..=10).contains(&top_n) {
        bail!("top_n must be between 1 and 10");
    }
    if !min_turnover_local.is_finite() || min_turnover_local < 2e8 {
        bail!("minimum turnover must be finite and at least 200 million");
    }
    if now.is_some() && frozen_stocks.is_none() {
        bail!("historical/as-of runs require frozen_stocks; live quotes cannot be backdated");
    }
    if frozen_stocks.is_some() && enrich {
        bail!("frozen snapshots must not be enriched with live news; use enrich=false");
    }
    let now = match now {
        Some(dt) => dt,
        None => chrono::Utc::now().with_timezone(&super::events::shanghai_offset()),
    };

    let mut markets_seen: Vec<String> = Vec::new();
    for market in markets {
        let item = market.trim().to_uppercase();
        if item.is_empty() || markets_seen.contains(&item) {
            continue;
        }
        markets_seen.push(item);
    }
    if markets_seen.is_empty() || markets_seen.iter().any(|m| m != "A" && m != "H") {
        bail!("markets only supports A,H");
    }
    let markets = markets_seen;

    let mut theme_universe: Vec<StockSnapshot> = Vec::new();
    let mut universe_stats = Map::new();
    let mut market_errors = Map::new();
    let mut as_of_by_market = Map::new();
    for market in &markets {
        let fetched: Vec<StockSnapshot> = match frozen_stocks {
            Some(frozen) => {
                let boundary = evidence_time(&Value::String(cutoff(&now, market, mode)));
                frozen
                    .iter()
                    .filter(|stock| {
                        stock.market == *market
                            && evidence_time(&Value::String(stock.observed_at.clone()))
                                .map(|stamp| Some(stamp) <= boundary)
                                .unwrap_or(false)
                    })
                    .cloned()
                    .collect()
            }
            None => match fetch_market_universe(market) {
                Ok(fetched) => fetched,
                Err(err) => {
                    let message: String = err.to_string().chars().take(160).collect();
                    let msg = format!("ValueError: {message}");
                    universe_stats.insert(
                        market.clone(),
                        serde_json::json!({ "input": 0, "liquid": 0, "error": msg }),
                    );
                    market_errors.insert(market.clone(), Value::String(msg));
                    continue;
                }
            },
        };
        let timestamps: Vec<Option<DateTime<FixedOffset>>> = fetched
            .iter()
            .map(|s| evidence_time(&Value::String(s.observed_at.clone())))
            .collect();
        if fetched.is_empty() || timestamps.iter().any(Option::is_none) {
            let msg = "ValueError: market snapshot is empty or lacks observation timestamps";
            market_errors.insert(market.clone(), Value::String(msg.to_string()));
            universe_stats.insert(
                market.clone(),
                serde_json::json!({ "input": 0, "liquid": 0, "error": msg }),
            );
            continue;
        }
        as_of_by_market.insert(
            market.clone(),
            Value::String(iso_seconds(
                &timestamps.iter().flatten().max().copied().unwrap(),
            )),
        );
        let stats = apply_hard_filters(&fetched, min_turnover_local)?.1;
        theme_universe.extend(fetched);
        universe_stats.insert(market.clone(), stats);
    }

    let industry_health = if enrich && frozen_stocks.is_none() {
        fetch_industries(&mut theme_universe)?
    } else {
        Value::Object(Map::new())
    };

    let all_stocks: Vec<StockSnapshot> = theme_universe
        .iter()
        .filter(|stock| stock.amount >= min_turnover_local)
        .cloned()
        .collect();
    let mut shortlisted = preselect(&all_stocks, 40);
    let mut enrichments: Vec<(String, (Vec<Value>, Vec<String>))> = shortlisted
        .iter()
        .map(|stock| (stock.code.clone(), (Vec::new(), vec!["snapshot_only".to_string()])))
        .collect();
    if enrich && !shortlisted.is_empty() {
        shortlisted = enrich_parallel(&mut shortlisted, &mut enrichments);
    }

    // Evaluate freshness once, after all source work.
    let generated_at = evaluated_at.unwrap_or_else(|| {
        chrono::Utc::now().with_timezone(&super::events::shanghai_offset())
    });
    let refreshed_filter_stats = apply_hard_filters(&shortlisted, min_turnover_local)?.1;
    let themes = build_theme_context(&theme_universe);
    let mut refreshed_as_of = Map::new();
    for (market, _) in as_of_by_market.iter() {
        let stamps: Vec<DateTime<FixedOffset>> = theme_universe
            .iter()
            .filter(|s| s.market == *market)
            .filter_map(|s| evidence_time(&Value::String(s.observed_at.clone())))
            .collect();
        refreshed_as_of.insert(
            market.clone(),
            Value::String(iso_seconds(&stamps.into_iter().max().unwrap())),
        );
    }
    let as_of_by_market = refreshed_as_of;

    let mut candidates = Vec::new();
    for stock in &shortlisted {
        let (evidence, gaps) = enrichments
            .iter()
            .find(|(code, _)| *code == stock.code)
            .map(|(_, payload)| payload.clone())
            .unwrap_or_default();
        let theme = themes
            .get(&stock.code)
            .cloned()
            .unwrap_or_else(|| Value::Object(Map::new()));
        candidates.push(build_candidate(
            stock,
            &theme,
            &evidence,
            &gaps,
            &generated_at,
        ));
    }
    let (picks, rejected) = rank_candidates(&candidates, top_n, 70.0);
    let summary = action_summary(&picks);

    let report_id = report_id_of(&generated_at, mode);
    let output_root = match output_root {
        Some(root) => root.to_path_buf(),
        None => reports_root()
            .join("screens")
            .join(generated_at.format("%Y-%m-%d").to_string())
            .join(&report_id),
    };

    let mut filters = Map::new();
    filters.insert("exclude_st".into(), Value::Bool(true));
    filters.insert("exclude_suspended".into(), Value::Bool(true));
    filters.insert("min_turnover_local".into(), Value::from(min_turnover_local));
    filters.insert("top_n".into(), Value::from(top_n as i64));
    filters.insert("min_research_confidence".into(), Value::from(70));

    let mut data_quality = Map::new();
    data_quality.insert(
        "markets_requested".into(),
        Value::Array(markets.iter().map(|m| Value::String(m.clone())).collect()),
    );
    data_quality.insert(
        "markets_available".into(),
        Value::Array(
            markets
                .iter()
                .filter(|m| !market_errors.contains_key(*m))
                .map(|m| Value::String(m.clone()))
                .collect(),
        ),
    );
    data_quality.insert("enrichment_enabled".into(), Value::Bool(enrich));
    data_quality.insert("shortlisted".into(), Value::from(shortlisted.len()));
    data_quality.insert(
        "removed_after_quote_refresh".into(),
        refreshed_filter_stats
            .get("removed_low_turnover")
            .cloned()
            .unwrap_or(Value::from(0)),
    );
    data_quality.insert("industry_sources".into(), industry_health);
    data_quality.insert(
        "intraday_available".into(),
        Value::from(
            shortlisted
                .iter()
                .filter(|s| {
                    s.extra
                        .get("intraday")
                        .and_then(|v| v.get("bars"))
                        .and_then(|v| v.as_array())
                        .map(|a| !a.is_empty())
                        .unwrap_or(false)
                })
                .count(),
        ),
    );

    let mut performance_contract = Map::new();
    performance_contract.insert(
        "entry".into(),
        Value::String("first_executable_price_after_publish".into()),
    );
    performance_contract.insert(
        "horizons".into(),
        Value::Array(
            ["close", "next_open", "next_close", "3d"]
                .iter()
                .map(|h| Value::String((*h).to_string()))
                .collect(),
        ),
    );
    performance_contract.insert("status".into(), Value::String("paper_trading".into()));

    let mut report = Map::new();
    report.insert("report_id".into(), Value::String(report_id.clone()));
    report.insert("mode".into(), Value::String(mode.to_string()));
    report.insert(
        "generated_at".into(),
        Value::String(iso_seconds(&generated_at)),
    );
    report.insert("as_of_by_market".into(), Value::Object(as_of_by_market));
    report.insert(
        "snapshot_kind".into(),
        Value::String(if frozen_stocks.is_some() {
            "frozen".into()
        } else {
            "live".into()
        }),
    );
    report.insert("analysis_basis".into(), Value::String("rule_only".into()));
    report.insert("filters".into(), Value::Object(filters));
    report.insert("universe_stats".into(), Value::Object(universe_stats));
    report.insert("market_errors".into(), Value::Object(market_errors));
    report.insert("action_summary".into(), Value::Object(summary));
    report.insert(
        "picks".into(),
        Value::Array(picks.iter().map(|p| p.to_dict()).collect()),
    );
    report.insert(
        "rejected".into(),
        Value::Array(rejected.iter().take(20).map(|p| p.to_dict()).collect()),
    );
    report.insert("data_quality".into(), Value::Object(data_quality));
    report.insert(
        "performance_contract".into(),
        Value::Object(performance_contract),
    );
    let report = Value::Object(report);

    atomic_json(&output_root.join("picks.json"), &report)?;
    atomic_json(
        &output_root.join("report.meta.json"),
        &serde_json::json!({
            "report_id": report_id,
            "generated_at": report.get("generated_at").cloned().unwrap_or(Value::Null),
            "picks_count": picks.len(),
            "markets": markets,
            "html": "index.html",
        }),
    )?;
    let avatars = assets_dir().join("avatars");
    let html_path = render_report(&report, &output_root.join("index.html"), &avatars)?;
    if track {
        let _ = append_signals(&daily_screen_ledger(), &report);
    }
    let mut report = report;
    if let Some(map) = report.as_object_mut() {
        map.insert(
            "report_path".into(),
            Value::String(html_path.to_string_lossy().to_string()),
        );
    }
    Ok(report)
}

/// `_enrich_candidate` over a 6-worker pool (`ThreadPoolExecutor(max_workers=6)`).
fn enrich_parallel(
    shortlisted: &mut [StockSnapshot],
    enrichments: &mut [(String, (Vec<Value>, Vec<String>))],
) -> Vec<StockSnapshot> {
    use rayon::prelude::*;
    let mut run = || {
        shortlisted
            .par_iter_mut()
            .map(|stock| {
                let (evidence, gaps) = enrich_candidate(stock);
                (stock.code.clone(), (evidence, gaps))
            })
            .collect::<Vec<_>>()
    };
    let results = match rayon::ThreadPoolBuilder::new().num_threads(6).build() {
        Ok(pool) => pool.install(run),
        Err(_) => run(),
    };
    for (code, payload) in results {
        if let Some(slot) = enrichments.iter_mut().find(|(c, _)| *c == code) {
            slot.1 = payload;
        }
    }
    shortlisted.to_vec()
}

/// `screen.py main()` — `run_daily_screen(mode, markets, schools, top,
/// min_turnover, snapshot_only)`.
///
/// `schools` is fixed to `F,I` upstream (`--schools` is validated there); the
/// parameter is kept so the CLI wiring stays identical.
pub fn run_daily_screen(
    mode: &str,
    markets: &str,
    schools: &str,
    top: usize,
    min_turnover: f64,
    snapshot_only: bool,
) -> Result<Value> {
    let mut parsed: Vec<String> = schools
        .split(',')
        .map(|item| item.trim().to_uppercase())
        .filter(|item| !item.is_empty())
        .collect();
    // Upstream compares sets: `{item.strip().upper() for item in schools.split(",") if item.strip()} != {"F","I"}`.
    parsed.sort();
    parsed.dedup();
    if parsed != ["F".to_string(), "I".to_string()] {
        bail!("daily screen 当前要求 --schools F,I");
    }
    let markets: Vec<String> = markets
        .split(',')
        .map(str::to_string)
        .collect();
    run_daily_screen_full(
        mode,
        &markets,
        top,
        min_turnover,
        !snapshot_only,
        true,
        None,
        None,
        None,
        None,
    )
}
