//! Port of `lib/pipeline/schema.py` — the `DimResult` container every fetcher,
//! scorer and renderer shares.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Quality {
    Full,
    Partial,
    Missing,
    Error,
}

impl Quality {
    pub fn as_str(&self) -> &'static str {
        match self {
            Quality::Full => "full",
            Quality::Partial => "partial",
            Quality::Missing => "missing",
            Quality::Error => "error",
        }
    }

    pub fn parse(s: &str) -> Quality {
        match s {
            "full" => Quality::Full,
            "partial" => Quality::Partial,
            "error" => Quality::Error,
            _ => Quality::Missing,
        }
    }
}

impl Default for Quality {
    fn default() -> Self {
        Quality::Missing
    }
}

/// Single-dimension fetch result. All fetchers return this.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DimResult {
    #[serde(default)]
    pub dim_key: String,
    #[serde(default)]
    pub data: Value,
    #[serde(default = "default_source")]
    pub source: String,
    #[serde(default)]
    pub quality: Quality,
    #[serde(default)]
    pub error: Option<String>,
    #[serde(default)]
    pub data_gaps: Vec<String>,
    #[serde(default)]
    pub cached: bool,
    #[serde(default)]
    pub latency_ms: Option<u64>,
    #[serde(default)]
    pub fetched_at: Option<f64>,
    #[serde(default)]
    pub top_level_fields: Map<String, Value>,
}

fn default_source() -> String {
    "unknown".to_string()
}

impl Default for DimResult {
    fn default() -> Self {
        DimResult {
            dim_key: String::new(),
            data: Value::Object(Map::new()),
            source: default_source(),
            quality: Quality::Missing,
            error: None,
            data_gaps: Vec::new(),
            cached: false,
            latency_ms: None,
            fetched_at: None,
            top_level_fields: Map::new(),
        }
    }
}

impl DimResult {
    pub fn new(dim_key: impl Into<String>, source: impl Into<String>) -> Self {
        DimResult {
            dim_key: dim_key.into(),
            source: source.into(),
            ..Default::default()
        }
    }

    pub fn empty(dim_key: impl Into<String>, source: impl Into<String>) -> Self {
        DimResult::new(dim_key, source)
    }

    pub fn error_result(
        dim_key: impl Into<String>,
        error: impl AsRef<str>,
        source: impl Into<String>,
    ) -> Self {
        let error: String = error.as_ref().chars().take(200).collect();
        DimResult {
            dim_key: dim_key.into(),
            source: source.into(),
            quality: Quality::Error,
            error: Some(error),
            ..Default::default()
        }
    }

    /// Convert to the legacy-compatible `raw_data.json` shape.
    ///
    /// Legacy-compatible top-level keys (`ticker` is added by the collector),
    /// plus the `_pipeline` namespace for pipeline-only metadata.
    pub fn to_dict(&self) -> Value {
        let q = self.quality.as_str();
        serde_json::json!({
            "data": self.data,
            "source": self.source,
            "fallback": q == "error" || q == "missing",
            "_pipeline": {
                "dim_key": self.dim_key,
                "quality": q,
                "data_gaps": self.data_gaps,
                "latency_ms": self.latency_ms,
                "fetched_at": self.fetched_at,
                "cached": self.cached,
                "top_level_fields": Value::Object(self.top_level_fields.clone()),
                "error": self.error,
            },
        })
    }

    /// Restore from either the legacy shape or the pipeline shape.
    pub fn from_dict(d: &Value) -> DimResult {
        let pp = d.get("_pipeline");
        let quality_raw = pp
            .and_then(|p| p.get("quality"))
            .and_then(|q| q.as_str())
            .or_else(|| d.get("quality").and_then(|q| q.as_str()));
        let mut quality = quality_raw.map(Quality::parse).unwrap_or(Quality::Missing);
        if pp.is_none() && d.get("fallback").and_then(|f| f.as_bool()) == Some(true) {
            quality = Quality::Error;
        }
        let pick = |key: &str| -> Option<Value> {
            pp.and_then(|p| p.get(key))
                .filter(|v| !v.is_null())
                .cloned()
                .or_else(|| d.get(key).filter(|v| !v.is_null()).cloned())
        };
        DimResult {
            dim_key: pick("dim_key")
                .and_then(|v| v.as_str().map(str::to_string))
                .unwrap_or_default(),
            data: d
                .get("data")
                .cloned()
                .unwrap_or_else(|| Value::Object(Map::new())),
            source: d
                .get("source")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown")
                .to_string(),
            quality,
            error: pick("error").and_then(|v| v.as_str().map(str::to_string)),
            data_gaps: pick("data_gaps")
                .and_then(|v| v.as_array().cloned())
                .map(|a| {
                    a.iter()
                        .filter_map(|s| s.as_str().map(str::to_string))
                        .collect()
                })
                .unwrap_or_default(),
            cached: pick("cached").and_then(|v| v.as_bool()).unwrap_or(false),
            latency_ms: pick("latency_ms").and_then(|v| v.as_u64()),
            fetched_at: pick("fetched_at").and_then(|v| v.as_f64()),
            top_level_fields: pick("top_level_fields")
                .and_then(|v| v.as_object().cloned())
                .unwrap_or_default(),
        }
    }
}

/// Fetcher metadata declaration — drives the validator and collector.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FetcherSpec {
    pub dim_key: String,
    #[serde(default)]
    pub required_fields: Vec<String>,
    #[serde(default)]
    pub optional_fields: Vec<String>,
    #[serde(default)]
    pub top_level_fields: Vec<String>,
    #[serde(default)]
    pub sources: Vec<String>,
    #[serde(default = "default_markets")]
    pub markets: Vec<String>,
    #[serde(default = "default_ttl")]
    pub cache_ttl_sec: u64,
    #[serde(default)]
    pub depends_on: Vec<String>,
}

fn default_markets() -> Vec<String> {
    vec!["A".into(), "H".into(), "U".into()]
}

fn default_ttl() -> u64 {
    3600
}

impl FetcherSpec {
    pub fn new(dim_key: impl Into<String>) -> Self {
        let dim_key = dim_key.into();
        FetcherSpec {
            dim_key,
            required_fields: Vec::new(),
            optional_fields: Vec::new(),
            top_level_fields: Vec::new(),
            sources: Vec::new(),
            markets: default_markets(),
            cache_ttl_sec: default_ttl(),
            depends_on: Vec::new(),
        }
    }

    pub fn required(mut self, fields: &[&str]) -> Self {
        self.required_fields = fields.iter().map(|s| s.to_string()).collect();
        self
    }

    pub fn optional(mut self, fields: &[&str]) -> Self {
        self.optional_fields = fields.iter().map(|s| s.to_string()).collect();
        self
    }

    pub fn ttl(mut self, ttl: u64) -> Self {
        self.cache_ttl_sec = ttl;
        self
    }

    pub fn sources(mut self, sources: &[&str]) -> Self {
        self.sources = sources.iter().map(|s| s.to_string()).collect();
        self
    }

    pub fn depends_on(mut self, dims: &[&str]) -> Self {
        self.depends_on = dims.iter().map(|s| s.to_string()).collect();
        self
    }

    pub fn top_level(mut self, fields: &[&str]) -> Self {
        self.top_level_fields = fields.iter().map(|s| s.to_string()).collect();
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn to_dict_is_legacy_compatible() {
        let mut r = DimResult::new("0_basic", "akshare:stock_individual_info_em");
        r.data = json!({"code": "600519"});
        r.quality = Quality::Full;
        let d = r.to_dict();
        assert_eq!(d["data"]["code"], "600519");
        assert_eq!(d["source"], "akshare:stock_individual_info_em");
        assert_eq!(d["fallback"], json!(false));
        assert_eq!(d["_pipeline"]["quality"], "full");
        assert_eq!(d["_pipeline"]["dim_key"], "0_basic");
    }

    #[test]
    fn missing_and_error_map_to_fallback_true() {
        assert_eq!(DimResult::new("x", "s").to_dict()["fallback"], json!(true));
        assert_eq!(
            DimResult::error_result("x", "boom", "s").to_dict()["fallback"],
            json!(true)
        );
    }

    #[test]
    fn round_trips_both_schemas() {
        let mut r = DimResult::new("6_research", "src");
        r.data = json!({"reports": 3});
        r.quality = Quality::Partial;
        r.data_gaps = vec!["target_price".into()];
        r.fetched_at = Some(1234.5);
        let restored = DimResult::from_dict(&r.to_dict());
        assert_eq!(restored.quality, Quality::Partial);
        assert_eq!(restored.data_gaps, vec!["target_price".to_string()]);
        assert_eq!(restored.fetched_at, Some(1234.5));

        // legacy shape (no _pipeline) with fallback=true infers ERROR
        let legacy = json!({"data": {}, "source": "old", "fallback": true});
        let restored = DimResult::from_dict(&legacy);
        assert_eq!(restored.quality, Quality::Error);
        assert_eq!(restored.source, "old");
    }
}
