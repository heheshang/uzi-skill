//! Port of `lib/pipeline/base_fetcher.py` — the `BaseFetcher` contract every
//! adapter follows: `fetch()` always returns a `DimResult`, never a bare dict,
//! and always swallows exceptions into `DimResult::error_result`.
//!
//! Rust has no subclassing; a fetcher is a value implementing [`Fetcher`]. The
//! template method [`Fetcher::fetch`] performs exactly the upstream sequence:
//! call `_fetch_raw`, unwrap the legacy return shape, normalise, extract
//! top-level fields, then validate.

use serde_json::{json, Map, Value};

use uzi_core::dim::{DimResult, FetcherSpec, Quality};
use uzi_core::validators::{normalize_data, validate_result};

/// What `_fetch_raw` produces upstream: the payload plus the captured
/// `_actual_source` / `_legacy_error` side-channel.
#[derive(Debug, Clone)]
pub struct RawOutcome {
    pub data: Value,
    /// `self._actual_source` — set when the legacy module returns a `source`.
    pub actual_source: Option<String>,
    /// `self._legacy_error` — a legacy `error` key means "failed" when empty.
    pub legacy_error: Option<String>,
}

impl RawOutcome {
    pub fn bare(data: Value) -> Self {
        RawOutcome {
            data,
            actual_source: None,
            legacy_error: None,
        }
    }
}

/// `_first_source(spec)` — first declared source, else "unknown".
pub fn first_source(spec: &FetcherSpec) -> String {
    spec.sources
        .first()
        .cloned()
        .unwrap_or_else(|| "unknown".to_string())
}

/// The upstream `BaseFetcher` surface.
pub trait Fetcher {
    /// `spec` — required for validation and quality inference.
    fn spec(&self) -> &FetcherSpec;

    /// `_legacy_module` identity, used by `collect` for the mini-racer group.
    fn legacy_module(&self) -> &str {
        ""
    }

    /// `_fetch_raw(ticker, raw_context)` — returns the raw payload.
    fn fetch_raw(&self, ticker: &Value, raw: &Value) -> Result<RawOutcome, String>;

    /// `keep_zero_fields` — fields where `0` is a semantically valid value.
    fn keep_zero_fields(&self) -> &'static [&'static str] {
        &[]
    }

    /// `extract_top_level(data)` — overflow fields written to the raw top level.
    fn extract_top_level(&self, data: &Value) -> Map<String, Value> {
        let mut out = Map::new();
        if let Some(obj) = data.as_object() {
            for key in &self.spec().top_level_fields {
                if let Some(v) = obj.get(key) {
                    out.insert(key.clone(), v.clone());
                }
            }
        }
        out
    }

    /// `fetch(ticker)` — the template method; never panics.
    fn fetch(&self, ticker: &Value, raw: &Value) -> DimResult {
        let spec = self.spec();
        let start = std::time::Instant::now();
        let outcome = match self.fetch_raw(ticker, raw) {
            Ok(v) => v,
            Err(e) => {
                let msg: String = e.chars().take(100).collect();
                return DimResult::error_result(
                    &spec.dim_key,
                    format!("Error: {msg}"),
                    first_source(spec),
                );
            }
        };
        if !outcome.data.is_object() {
            return DimResult::error_result(
                &spec.dim_key,
                "fetch returned non-dict, expected dict",
                first_source(spec),
            );
        }

        let actual_source = outcome
            .actual_source
            .clone()
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| first_source(spec));

        let mut normalized = outcome.data;
        let empty = normalized
            .as_object()
            .map(|o| o.is_empty())
            .unwrap_or(true);
        if let Some(err) = outcome.legacy_error.filter(|_| empty) {
            return DimResult::error_result(&spec.dim_key, err, actual_source);
        }

        let keep: Vec<&str> = self.keep_zero_fields().to_vec();
        normalize_data(&mut normalized, &keep);

        let top_level = self.extract_top_level(&normalized);
        let mut data = normalized.as_object().cloned().unwrap_or_default();
        for key in top_level.keys() {
            data.remove(key);
        }

        let mut result = DimResult::new(&spec.dim_key, actual_source);
        result.data = Value::Object(data);
        result.top_level_fields = top_level;
        result.latency_ms = Some(start.elapsed().as_millis() as u64);
        result.fetched_at = Some(now_secs());
        result.quality = Quality::Missing;
        validate_result(result, spec)
    }
}

/// `now_secs()`.
pub fn now_secs() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

/// Adapt a plain function into a [`Fetcher`] (the `_make_adapter` factory).
///
/// `raw_fn` returns the legacy module's dict verbatim; the upstream unwrapping
/// rules are applied here, exactly as `_make_adapter._fetch_raw` does.
pub struct FnFetcher {
    pub spec: FetcherSpec,
    pub legacy_module: String,
    pub keep_zero: &'static [&'static str],
    pub raw_fn: fn(&Value, &Value) -> Result<Value, String>,
}

impl Fetcher for FnFetcher {
    fn spec(&self) -> &FetcherSpec {
        &self.spec
    }
    fn legacy_module(&self) -> &str {
        &self.legacy_module
    }

    fn fetch_raw(&self, ticker: &Value, raw: &Value) -> Result<RawOutcome, String> {
        let result = (self.raw_fn)(ticker, raw)?;
        let Some(obj) = result.as_object() else {
            return Ok(RawOutcome::bare(json!({})));
        };
        // Style 1: {"ticker": …, "data": {...}, "source": …, "fallback": bool}
        if let Some(data) = obj.get("data").filter(|d| d.is_object()) {
            return Ok(RawOutcome {
                data: data.clone(),
                actual_source: obj
                    .get("source")
                    .and_then(|s| s.as_str())
                    .map(|s| s.to_string()),
                legacy_error: obj
                    .get("error")
                    .and_then(|e| e.as_str())
                    .map(|e| e.to_string()),
            });
        }
        // Style 2: bare dict (fetch_macro / fetch_policy / fetch_industry)
        Ok(RawOutcome::bare(result))
    }

    fn keep_zero_fields(&self) -> &'static [&'static str] {
        self.keep_zero
    }
}

/// Convenience: low-level access to the raw data payload of `fn main`.
pub fn data_of(value: &Value) -> Value {
    value
        .get("data")
        .filter(|d| d.is_object())
        .cloned()
        .unwrap_or_else(|| value.clone())
}

/// `DimResult.empty(dim_key)` as a JSON dict.
pub fn empty_dict(dim_key: &str) -> Value {
    DimResult::empty(dim_key, "unknown").to_dict()
}

/// `DimResult.error_result(...)` as a JSON dict.
pub fn error_dict(dim_key: &str, error: &str, source: &str) -> Value {
    DimResult::error_result(dim_key, error, source).to_dict()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    struct Ok1;

    impl Fetcher for Ok1 {
        fn spec(&self) -> &FetcherSpec {
            static SPEC: std::sync::LazyLock<FetcherSpec> = std::sync::LazyLock::new(|| {
                FetcherSpec::new("x").required(&["a"]).sources(&["legacy:x"])
            });
            &SPEC
        }
        fn fetch_raw(&self, _t: &Value, _r: &Value) -> Result<RawOutcome, String> {
            Ok(RawOutcome::bare(json!({"a": 1, "b": "—", "c": "-"})))
        }
    }

    struct Boom;

    impl Fetcher for Boom {
        fn spec(&self) -> &FetcherSpec {
            static SPEC: std::sync::LazyLock<FetcherSpec> =
                std::sync::LazyLock::new(|| FetcherSpec::new("x"));
            &SPEC
        }
        fn fetch_raw(&self, _t: &Value, _r: &Value) -> Result<RawOutcome, String> {
            Err("boom".into())
        }
    }

    struct LegacyErr;

    impl Fetcher for LegacyErr {
        fn spec(&self) -> &FetcherSpec {
            static SPEC: std::sync::LazyLock<FetcherSpec> =
                std::sync::LazyLock::new(|| FetcherSpec::new("x"));
            &SPEC
        }
        fn fetch_raw(&self, _t: &Value, _r: &Value) -> Result<RawOutcome, String> {
            Ok(RawOutcome {
                data: json!({}),
                actual_source: Some("legacy:y".into()),
                legacy_error: Some("network down".into()),
            })
        }
    }

    #[test]
    fn fetch_normalizes_empty_sentinels_to_null() {
        let r = Ok1.fetch(&json!("t"), &json!({}));
        assert_eq!(r.data["b"], Value::Null);
        assert_eq!(r.data["c"], Value::Null);
        // only "a" is required and present → FULL
        assert_eq!(r.quality, Quality::Full);
        assert_eq!(r.source, "legacy:x");
    }

    #[test]
    fn fetch_swallows_errors() {
        let r = Boom.fetch(&json!("t"), &json!({}));
        assert_eq!(r.quality, Quality::Error);
        assert!(r.error.unwrap().contains("boom"));
    }

    #[test]
    fn legacy_error_with_empty_data_is_error_result() {
        let r = LegacyErr.fetch(&json!("t"), &json!({}));
        assert_eq!(r.quality, Quality::Error);
        assert_eq!(r.error.as_deref(), Some("network down"));
        assert_eq!(r.source, "legacy:y");
    }

    #[test]
    fn raw_outcome_unwraps_legacy_shapes() {
        let f = FnFetcher {
            spec: FetcherSpec::new("x"),
            legacy_module: "fetch_basic".into(),
            keep_zero: &[],
            raw_fn: |_t, _r| {
                Ok(json!({"ticker": "600519.SH", "data": {"name": "贵州茅台"},
                          "source": "akshare:A", "fallback": false}))
            },
        };
        let out = f.fetch_raw(&json!("600519.SH"), &json!({})).unwrap();
        assert_eq!(out.data["name"], json!("贵州茅台"));
        assert_eq!(out.actual_source.as_deref(), Some("akshare:A"));
        assert!(out.legacy_error.is_none());

        // bare dict shape passes through untouched
        let f2 = FnFetcher {
            spec: FetcherSpec::new("x"),
            legacy_module: "fetch_macro".into(),
            keep_zero: &[],
            raw_fn: |_t, _r| Ok(json!({"rate_cycle": "中性"})),
        };
        let out = f2.fetch_raw(&json!("x"), &json!({})).unwrap();
        assert_eq!(out.data["rate_cycle"], json!("中性"));
        assert!(out.actual_source.is_none());
    }
}
