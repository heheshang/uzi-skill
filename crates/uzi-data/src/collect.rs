//! Port of `lib/pipeline/collect.py` — the wave-based data-collection
//! orchestrator.
//!
//! Wave structure mirrors upstream exactly:
//! * wave 1 · `0_basic` alone (later fetchers need its `industry`)
//! * wave 2 · every non-dependent dim concurrently (`max_workers`), with the
//!   mini-racer modules pinned to a serial group
//! * wave 3 · [`DEPENDENT_DIMS`], serial, with `0_basic` (+ `8_materials`,
//!   `1_financials`) handed to `args_fn`
//!
//! Resume reuses `raw_previous["dimensions"][dim]` when
//! [`is_resume_valid`] accepts it (quality not missing/error, `fallback` not
//! true, non-empty data, `fetched_at` within the dim's TTL).
//!
//! Degradation contract: `collect` never panics and always emits every dim as
//! `{data, source, fallback, _pipeline}` (plus the `fund_managers` /
//! `similar_stocks` top-level overflow keys).

use std::collections::BTreeSet;

use serde_json::{json, Map, Value};

use uzi_core::dim::{DimResult, Quality};
use uzi_core::validators::{normalize_data, validate_result};

use crate::base_fetcher::{now_secs, Fetcher};
use crate::fetch;
use crate::fetchers::{self, DEPENDENT_DIMS, MINI_RACER_LEGACY_MODULES};
use crate::process_runner::{run_process_jobs, ProcessJob, ProcessOutcome};

/// `_positive_env_float(name, default)`.
fn positive_env_float(name: &str, default: f64) -> f64 {
    std::env::var(name)
        .ok()
        .and_then(|v| v.parse::<f64>().ok())
        .filter(|v| *v > 0.0)
        .unwrap_or(default)
}

/// `_mini_racer_disabled()`.
///
/// mini_racer is a V8 binding that does not exist in the Rust port; the flag is
/// still honoured so users running with `UZI_DISABLE_MINI_RACER=1` (or a stale
/// crash sentinel) get upstream's degraded path for the mini-racer modules.
pub fn mini_racer_disabled() -> bool {
    if std::env::var("UZI_DISABLE_MINI_RACER").as_deref() == Ok("1") {
        return true;
    }
    if std::env::var("UZI_FORCE_MINI_RACER").as_deref() == Ok("1") {
        return false;
    }
    sentinel_path().exists()
}

fn sentinel_path() -> std::path::PathBuf {
    uzi_core::cache::cache_root().join("_minirackercrash.sentinel")
}

/// `_cache_ttl(fetcher)`.
fn cache_ttl(dim_key: &str) -> u64 {
    fetchers::spec_of(dim_key)
        .map(|s| s.cache_ttl_sec)
        .unwrap_or(3600)
}

/// `_is_resume_valid(dim_dict, ttl_sec, now)` — legacy + v3 schemas.
pub fn is_resume_valid(dim_dict: &Value, ttl_sec: Option<u64>, now: Option<f64>) -> bool {
    let Some(d) = dim_dict.as_object() else {
        return false;
    };
    if d.get("data")
        .map(|data| !uzi_core::py::truthy(data))
        .unwrap_or(true)
    {
        return false;
    }
    let pp = d.get("_pipeline").and_then(|p| p.as_object());
    let q = pp
        .and_then(|p| p.get("quality"))
        .and_then(|q| q.as_str())
        .or_else(|| d.get("quality").and_then(|q| q.as_str()))
        .unwrap_or("");
    if q == "missing" || q == "error" {
        return false;
    }
    if d.get("fallback").and_then(|f| f.as_bool()) == Some(true) {
        return false;
    }
    let Some(ttl) = ttl_sec else {
        return true;
    };
    let fetched_at = pp
        .and_then(|p| p.get("fetched_at"))
        .filter(|v| !v.is_null())
        .and_then(|v| v.as_f64())
        .or_else(|| d.get("fetched_at").and_then(|v| v.as_f64()));
    let Some(fetched_at) = fetched_at else {
        return false;
    };
    let current = now.unwrap_or_else(now_secs);
    let age = current - fetched_at;
    (0.0..=ttl as f64).contains(&age)
}

/// `_run_fetcher_job(dim_key, ticker, raw_context)`.
///
/// Returns `(dim_key, result_dict, top_level_fields)` — the two dicts are
/// merged by [`apply_outcome`].
pub fn run_fetcher_job(dim_key: &str, ticker: &str, raw_context: Option<&Value>) -> (String, Value, Value) {
    let key = dim_key.to_string();
    let Some(fetcher) = fetchers::get_fetcher(dim_key) else {
        return (
            key,
            DimResult::empty(dim_key, "unknown").to_dict(),
            json!({}),
        );
    };

    // ── Crypto venue · market "C" ──
    // Same dim keys, crypto-native sources and payloads. Unhandled dims fall
    // through to the generic path below.
    let ti = uzi_core::ticker::parse_ticker(ticker);
    if crate::crypto::is_crypto(&ti) {
        if let Some(cd) = crate::crypto::dim(dim_key, &ti) {
            let mut result = DimResult::new(dim_key, cd.source);
            result.data = cd.data;
            result.fetched_at = Some(now_secs());
            // The equity `FetcherSpec` (ROE / PE / 营收 …) does not apply to a
            // token: quality is "does this payload carry data at all", and the
            // per-venue field coverage is tracked centrally by
            // `uzi-review::data_integrity::CRYPTO_CHECKS`.
            normalize_data(&mut result.data, &[]);
            result.quality = if uzi_core::validators::has_meaningful_data(&result.data) {
                Quality::Full
            } else {
                Quality::Missing
            };
            return (key, result.to_dict(), json!({}));
        }
    }

    let legacy_mod = fetcher.legacy_module().to_string();
    let is_mini_racer = MINI_RACER_LEGACY_MODULES.contains(&legacy_mod.as_str());
    let ticker_value = json!(ticker);
    let empty_context = json!({});
    let raw = raw_context.unwrap_or(&empty_context);

    if is_mini_racer && mini_racer_disabled() {
        if legacy_mod == "fetch_valuation" {
            if let Ok(raw_result) = fetch::valuation::main_safe(ticker) {
                let mut data = raw_result.get("data").cloned().unwrap_or_else(|| json!({}));
                normalize_data(&mut data, &[]);
                let spec = fetcher.spec.clone();
                let mut result = DimResult::new(dim_key, "fetch_valuation (mini_racer-safe)");
                result.data = data;
                result.fetched_at = Some(now_secs());
                if let Some(src) = raw_result.get("source").and_then(|s| s.as_str()) {
                    if !src.is_empty() {
                        result.source = src.to_string();
                    }
                }
                let validated = validate_result(result, &spec);
                return (key, validated.to_dict(), json!({}));
            }
        }
        let result = DimResult::empty(dim_key, format!("{legacy_mod} (skipped)"));
        return (key, result.to_dict(), json!({}));
    }

    let result = fetcher.fetch(&ticker_value, raw);
    let top_level = Value::Object(result.top_level_fields.clone());
    (key, result.to_dict(), top_level)
}

/// `_apply_outcome(out, outcome)`.
pub fn apply_outcome(out: &mut Map<String, Value>, outcome: &ProcessOutcome) {
    if let Some(error) = &outcome.error {
        let source = if outcome.timed_out {
            "pipeline:timeout"
        } else {
            "pipeline:worker"
        };
        let result = DimResult::error_result(&outcome.key, error.clone(), source);
        out.insert(outcome.key.clone(), result.to_dict());
        return;
    }
    let Some(value) = &outcome.value else {
        return;
    };
    let Some(dim_key) = value.first().and_then(|v| v.as_str()) else {
        return;
    };
    let result_dict = value.get(1).cloned().unwrap_or_else(|| json!({}));
    if let Some(top_level) = value.get(2).and_then(|v| v.as_object()) {
        for (k, v) in top_level {
            out.insert(k.clone(), v.clone());
        }
    }
    out.insert(dim_key.to_string(), result_dict);
}

/// `collect(ticker, raw_previous, max_workers)` — the frozen crate entry point.
///
/// Returns the legacy-compatible raw dict: `ticker`, every dim key as
/// `DimResult::to_dict()`, and the wave-3 top-level overflow fields
/// (`fund_managers`, `similar_stocks`).
pub fn collect(
    ticker: &str,
    raw_previous: Option<&Value>,
    max_workers: usize,
    enabled: Option<&BTreeSet<String>>,
) -> Value {
    let mut out: Map<String, Value> = Map::new();
    out.insert("ticker".to_string(), json!(ticker));

    let previous_dim = |dim_key: &str| -> Option<Value> {
        raw_previous
            .and_then(|r| r.get("dimensions"))
            .and_then(|d| d.get(dim_key))
            .cloned()
    };

    let fetcher_timeout = positive_env_float("UZI_PIPELINE_FETCHER_TIMEOUT", 120.0);
    let wave_timeout = positive_env_float("UZI_PIPELINE_WAVE_TIMEOUT", 300.0);

    // `analysis_profile.fetchers_enabled` — `lite` fetches only its core dims.
    // Upstream filters wave 2 by the profile; `0_basic` is unconditional because
    // every later fetcher needs its `industry`. `None` means "no filtering"
    // (unknown depth), matching upstream's `except Exception` fallback.
    let wanted = |dim_key: &str| -> bool {
        match enabled {
            Some(set) => dim_key == "0_basic" || set.contains(dim_key),
            None => true,
        }
    };

    // ── Wave 1 · 0_basic ──
    let basic_cached = previous_dim("0_basic");
    match basic_cached {
        Some(cached) if is_resume_valid(&cached, Some(cache_ttl("0_basic")), None) => {
            out.insert("0_basic".to_string(), cached);
        }
        _ => {
            let t = ticker.to_string();
            let job = ProcessJob::new("0_basic", move || {
                let (k, r, tl) = run_fetcher_job("0_basic", &t, None);
                Ok(vec![json!(k), r, tl])
            })
            .timeout(fetcher_timeout);
            match run_process_jobs(vec![job], 1, fetcher_timeout) {
                Ok(outcomes) => {
                    for outcome in &outcomes {
                        apply_outcome(&mut out, outcome);
                    }
                }
                Err(e) => {
                    apply_outcome(
                        &mut out,
                        &ProcessOutcome {
                            key: "0_basic".into(),
                            value: None,
                            error: Some(e),
                            timed_out: false,
                        },
                    );
                }
            }
        }
    }

    // ── Wave 2 · non-dependent dims concurrently ──
    let mut jobs: Vec<ProcessJob> = Vec::new();
    for dim_key in fetchers::dim_keys() {
        if DEPENDENT_DIMS.contains(&dim_key) || dim_key == "0_basic" {
            continue;
        }
        if !wanted(dim_key) {
            continue;
        }
        if fetchers::get_fetcher(dim_key).is_none() {
            out.insert(dim_key.to_string(), DimResult::empty(dim_key, "unknown").to_dict());
            continue;
        }
        if let Some(cached) = previous_dim(dim_key) {
            if is_resume_valid(&cached, Some(cache_ttl(dim_key)), None) {
                out.insert(dim_key.to_string(), cached);
                continue;
            }
        }
        let t = ticker.to_string();
        let key = dim_key.to_string();
        let mut job = ProcessJob::new(dim_key, move || {
            let (k, r, tl) = run_fetcher_job(&key, &t, None);
            Ok(vec![json!(k), r, tl])
        })
        .timeout(fetcher_timeout);
        if fetchers::is_mini_racer(dim_key) {
            job = job.serial_group("mini_racer");
        }
        jobs.push(job);
    }
    match run_process_jobs(jobs, max_workers.max(1), wave_timeout) {
        Ok(outcomes) => {
            for outcome in &outcomes {
                apply_outcome(&mut out, outcome);
            }
        }
        Err(_) => {
            // A scheduling error must still leave every registry dim present.
            fill_missing_dims(&mut out, "pipeline:worker", "wave scheduling failed");
        }
    }

    // ── Wave 3 · dependent dims, serial, with shared context ──
    let mut raw_for_deps: Map<String, Value> = Map::new();
    if let Some(basic) = out.get("0_basic") {
        raw_for_deps.insert("0_basic".to_string(), basic.clone());
    }
    for extra in ["8_materials", "1_financials"] {
        if let Some(v) = out.get(extra) {
            raw_for_deps.insert(extra.to_string(), v.clone());
        }
    }
    let raw_for_deps = Value::Object(raw_for_deps);

    let mut dependent_jobs: Vec<ProcessJob> = Vec::new();
    let mut deps: Vec<&str> = DEPENDENT_DIMS.to_vec();
    deps.sort_unstable();
    for dim_key in deps {
        if fetchers::get_fetcher(dim_key).is_none() {
            continue;
        }
        if !wanted(dim_key) {
            continue;
        }
        if let Some(cached) = previous_dim(dim_key) {
            if is_resume_valid(&cached, Some(cache_ttl(dim_key)), None) {
                out.insert(dim_key.to_string(), cached);
                continue;
            }
        }
        let t = ticker.to_string();
        let key = dim_key.to_string();
        let ctx = raw_for_deps.clone();
        let mut job = ProcessJob::new(dim_key, move || {
            let (k, r, tl) = run_fetcher_job(&key, &t, Some(&ctx));
            Ok(vec![json!(k), r, tl])
        })
        .timeout(fetcher_timeout);
        if fetchers::is_mini_racer(dim_key) {
            job = job.serial_group("mini_racer");
        }
        dependent_jobs.push(job);
    }
    match run_process_jobs(dependent_jobs, 1, wave_timeout) {
        Ok(outcomes) => {
            for outcome in &outcomes {
                apply_outcome(&mut out, outcome);
            }
        }
        Err(_) => {
            fill_missing_dims(&mut out, "pipeline:worker", "wave scheduling failed");
        }
    }

    // Every registered dim must be present, even after scheduling failures.
    fill_missing_dims(&mut out, "pipeline:worker", "dim not produced");

    // Top-level overflow keys used by downstream stages. Upstream emits them
    // only when the wave-3 dim produced them; emitting them unconditionally
    // keeps `raw["fund_managers"]` / `raw["similar_stocks"]` readable.
    for key in ["fund_managers", "similar_stocks"] {
        if !out.contains_key(key) {
            out.insert(key.to_string(), json!([]));
        }
    }

    nest_dimensions(&mut out);
    Value::Object(out)
}

/// Move every dimension into `out["dimensions"]`, the shape the rest of the
/// pipeline consumes.
///
/// Upstream's `raw` nests them (`raw["dimensions"]["1_financials"]["data"]`), and
/// so do the checked-in fixtures. `score_dimensions`, `extract_features`,
/// `data_integrity`, `self_review`, and the report layer all read
/// `raw["dimensions"]`, so emitting them at the top level left every consumer
/// looking at an empty map — live runs scored defaults ("ROE 0.0%") while the
/// fixture-based parity tests passed, because those fixtures were already nested.
///
/// Non-dimension keys (`ticker`, `code`, `market`, `fund_managers`,
/// `similar_stocks`) stay at the top level, as upstream has them.
fn nest_dimensions(out: &mut Map<String, Value>) {
    let dim_keys: Vec<String> = out.keys().filter(|k| is_dim_key(k)).cloned().collect();
    let mut dims = Map::new();
    for key in dim_keys {
        if let Some(value) = out.remove(&key) {
            dims.insert(key, value);
        }
    }
    out.insert("dimensions".to_string(), Value::Object(dims));
}

/// Whether a key names a data dimension (`1_financials`, `20_valuation_models`).
///
/// A dimension key is `{index}_{name}`; everything else in the snapshot is a
/// metadata or overflow field and belongs at the top level.
fn is_dim_key(key: &str) -> bool {
    let Some((index, name)) = key.split_once('_') else {
        return false;
    };
    !index.is_empty()
        && index.bytes().all(|b| b.is_ascii_digit())
        && !name.is_empty()
        && name.bytes().all(|b| b.is_ascii_lowercase() || b == b'_')
}

fn fill_missing_dims(out: &mut Map<String, Value>, source: &str, reason: &str) {
    for dim_key in fetchers::dim_keys() {
        if !out.contains_key(dim_key) {
            let result = DimResult::error_result(dim_key, reason, source);
            out.insert(dim_key.to_string(), result.to_dict());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn resume_validity_matches_upstream_rules() {
        let now = 1_000_000.0;
        let good = json!({
            "data": {"price": 1.0},
            "_pipeline": {"quality": "full", "fetched_at": now - 10.0}
        });
        assert!(is_resume_valid(&good, Some(60), Some(now)));

        // quality missing / error invalidates
        for q in ["missing", "error"] {
            let v = json!({"data": {"x": 1}, "_pipeline": {"quality": q, "fetched_at": now}});
            assert!(!is_resume_valid(&v, Some(60), Some(now)), "{q}");
        }
        // legacy fallback=True invalidates
        let legacy = json!({"data": {"x": 1}, "fallback": true, "fetched_at": now});
        assert!(!is_resume_valid(&legacy, Some(60), Some(now)));
        // empty data invalidates
        let empty = json!({"data": {}, "_pipeline": {"quality": "full", "fetched_at": now}});
        assert!(!is_resume_valid(&empty, Some(60), Some(now)));
        // TTL window
        let stale = json!({"data": {"x": 1}, "_pipeline": {"quality": "full", "fetched_at": now - 61.0}});
        assert!(!is_resume_valid(&stale, Some(60), Some(now)));
        assert!(is_resume_valid(&stale, None, Some(now)));
        // non-dict
        assert!(!is_resume_valid(&json!([]), Some(60), Some(now)));
    }

    #[test]
    fn apply_outcome_error_uses_pipeline_sources() {
        let mut out = Map::new();
        apply_outcome(
            &mut out,
            &ProcessOutcome {
                key: "3_macro".into(),
                value: None,
                error: Some("boom".into()),
                timed_out: true,
            },
        );
        assert_eq!(out["3_macro"]["_pipeline"]["quality"], json!("error"));
        assert_eq!(out["3_macro"]["source"], json!("pipeline:timeout"));
        assert_eq!(out["3_macro"]["fallback"], json!(true));
    }

    #[test]
    fn apply_outcome_merges_top_level_fields() {
        let mut out = Map::new();
        let dict = DimResult::new("6_fund_holders", "src").to_dict();
        apply_outcome(
            &mut out,
            &ProcessOutcome {
                key: "6_fund_holders".into(),
                value: Some(vec![json!("6_fund_holders"), dict, json!({"fund_managers": [1, 2]})]),
                error: None,
                timed_out: false,
            },
        );
        assert!(out.contains_key("6_fund_holders"));
        assert_eq!(out["fund_managers"], json!([1, 2]));
    }
}
