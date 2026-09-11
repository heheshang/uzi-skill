//! Port of `lib/daily_screen/models.py` — typed contracts for immutable
//! daily-screen snapshots.
//!
//! Upstream uses `dataclasses.asdict`, so the `to_dict` key order below is the
//! dataclass field order and MUST stay that way (`serde_json` keeps insertion
//! order).

use serde_json::{Map, Number, Value};

/// Build a JSON number from an `f64` (finite screening values only).
pub(crate) fn num(x: f64) -> Value {
    Value::Number(Number::from_f64(x).unwrap_or_else(|| Number::from(0)))
}

/// Optional float → JSON number or `null`.
pub(crate) fn opt_num(x: Option<f64>) -> Value {
    match x {
        Some(v) => num(v),
        None => Value::Null,
    }
}

/// `StockSnapshot` dataclass.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct StockSnapshot {
    pub code: String,
    pub name: String,
    pub market: String,
    pub price: f64,
    pub change_pct: f64,
    pub amount: f64,
    pub industry: String,
    pub open_price: Option<f64>,
    pub prev_close: Option<f64>,
    pub high: Option<f64>,
    pub low: Option<f64>,
    pub turnover_rate: Option<f64>,
    pub volume_ratio: Option<f64>,
    pub market_cap: Option<f64>,
    pub observed_at: String,
    pub source: String,
    pub extra: Map<String, Value>,
}

impl StockSnapshot {
    pub fn new(code: impl Into<String>, name: impl Into<String>, market: impl Into<String>) -> Self {
        StockSnapshot {
            code: code.into(),
            name: name.into(),
            market: market.into(),
            industry: "未分类".to_string(),
            extra: Map::new(),
            ..Default::default()
        }
    }

    /// `StockSnapshot.to_dict()` — field order preserved.
    pub fn to_dict(&self) -> Value {
        let mut out = Map::new();
        out.insert("code".into(), Value::String(self.code.clone()));
        out.insert("name".into(), Value::String(self.name.clone()));
        out.insert("market".into(), Value::String(self.market.clone()));
        out.insert("price".into(), num(self.price));
        out.insert("change_pct".into(), num(self.change_pct));
        out.insert("amount".into(), num(self.amount));
        out.insert("industry".into(), Value::String(self.industry.clone()));
        out.insert("open_price".into(), opt_num(self.open_price));
        out.insert("prev_close".into(), opt_num(self.prev_close));
        out.insert("high".into(), opt_num(self.high));
        out.insert("low".into(), opt_num(self.low));
        out.insert("turnover_rate".into(), opt_num(self.turnover_rate));
        out.insert("volume_ratio".into(), opt_num(self.volume_ratio));
        out.insert("market_cap".into(), opt_num(self.market_cap));
        out.insert("observed_at".into(), Value::String(self.observed_at.clone()));
        out.insert("source".into(), Value::String(self.source.clone()));
        out.insert("extra".into(), Value::Object(self.extra.clone()));
        Value::Object(out)
    }

    /// Inverse of [`StockSnapshot::to_dict`] — the frozen-snapshot input path.
    pub fn from_value(value: &Value) -> Option<StockSnapshot> {
        let obj = value.as_object()?;
        let s = |key: &str| obj.get(key).and_then(|v| v.as_str()).map(str::to_string);
        let n = |key: &str| obj.get(key).and_then(|v| v.as_f64());
        Some(StockSnapshot {
            code: s("code")?,
            name: s("name").unwrap_or_default(),
            market: s("market").unwrap_or_default(),
            price: n("price").unwrap_or(0.0),
            change_pct: n("change_pct").unwrap_or(0.0),
            amount: n("amount").unwrap_or(0.0),
            industry: s("industry").unwrap_or_else(|| "未分类".to_string()),
            open_price: n("open_price"),
            prev_close: n("prev_close"),
            high: n("high"),
            low: n("low"),
            turnover_rate: n("turnover_rate"),
            volume_ratio: n("volume_ratio"),
            market_cap: n("market_cap"),
            observed_at: s("observed_at").unwrap_or_default(),
            source: s("source").unwrap_or_default(),
            extra: obj
                .get("extra")
                .and_then(|v| v.as_object())
                .cloned()
                .unwrap_or_default(),
        })
    }

    /// `stock.extra.get(key)` with the upstream default `0`.
    pub(crate) fn extra_num(&self, key: &str, default: f64) -> f64 {
        match self.extra.get(key) {
            Some(v) => uzi_core::py::f0(v),
            None => default,
        }
    }
}

/// `PersonaVerdict` dataclass.
#[derive(Debug, Clone, PartialEq)]
pub struct PersonaVerdict {
    pub investor_id: String,
    pub name: String,
    pub style: String,
    pub eligible: bool,
    pub signal: String,
    pub confidence: i64,
    pub matched_patterns: Vec<String>,
    pub vetoes: Vec<String>,
    pub reasoning_summary: String,
    pub entry_condition: String,
    pub invalidation: String,
    pub horizon: String,
}

impl PersonaVerdict {
    pub fn to_dict(&self) -> Value {
        let mut out = Map::new();
        out.insert("investor_id".into(), Value::String(self.investor_id.clone()));
        out.insert("name".into(), Value::String(self.name.clone()));
        out.insert("style".into(), Value::String(self.style.clone()));
        out.insert("eligible".into(), Value::Bool(self.eligible));
        out.insert("signal".into(), Value::String(self.signal.clone()));
        out.insert("confidence".into(), Value::from(self.confidence));
        out.insert(
            "matched_patterns".into(),
            Value::Array(
                self.matched_patterns
                    .iter()
                    .map(|s| Value::String(s.clone()))
                    .collect(),
            ),
        );
        out.insert(
            "vetoes".into(),
            Value::Array(self.vetoes.iter().map(|s| Value::String(s.clone())).collect()),
        );
        out.insert(
            "reasoning_summary".into(),
            Value::String(self.reasoning_summary.clone()),
        );
        out.insert(
            "entry_condition".into(),
            Value::String(self.entry_condition.clone()),
        );
        out.insert("invalidation".into(), Value::String(self.invalidation.clone()));
        out.insert("horizon".into(), Value::String(self.horizon.clone()));
        Value::Object(out)
    }
}

/// `ScreenCandidate` dataclass.
#[derive(Debug, Clone)]
pub struct ScreenCandidate {
    pub snapshot: StockSnapshot,
    pub research_confidence: f64,
    pub action: String,
    pub why_now: String,
    pub entry_condition: String,
    pub invalidation: String,
    pub theme_rank: Option<i64>,
    pub leader_rank: Option<i64>,
    pub theme_breadth_pct: Option<f64>,
    pub persona_verdicts: Vec<PersonaVerdict>,
    pub serenity: Option<PersonaVerdict>,
    pub evidence: Vec<Value>,
    pub data_gaps: Vec<String>,
    pub risk_flags: Vec<String>,
}

impl ScreenCandidate {
    pub fn to_dict(&self) -> Value {
        let mut out = Map::new();
        out.insert("snapshot".into(), self.snapshot.to_dict());
        out.insert("research_confidence".into(), num(self.research_confidence));
        out.insert("action".into(), Value::String(self.action.clone()));
        out.insert("why_now".into(), Value::String(self.why_now.clone()));
        out.insert(
            "entry_condition".into(),
            Value::String(self.entry_condition.clone()),
        );
        out.insert("invalidation".into(), Value::String(self.invalidation.clone()));
        out.insert(
            "theme_rank".into(),
            match self.theme_rank {
                Some(v) => Value::from(v),
                None => Value::Null,
            },
        );
        out.insert(
            "leader_rank".into(),
            match self.leader_rank {
                Some(v) => Value::from(v),
                None => Value::Null,
            },
        );
        out.insert("theme_breadth_pct".into(), opt_num(self.theme_breadth_pct));
        out.insert(
            "persona_verdicts".into(),
            Value::Array(self.persona_verdicts.iter().map(|v| v.to_dict()).collect()),
        );
        out.insert(
            "serenity".into(),
            match &self.serenity {
                Some(v) => v.to_dict(),
                None => Value::Null,
            },
        );
        out.insert("evidence".into(), Value::Array(self.evidence.clone()));
        out.insert(
            "data_gaps".into(),
            Value::Array(
                self.data_gaps
                    .iter()
                    .map(|s| Value::String(s.clone()))
                    .collect(),
            ),
        );
        out.insert(
            "risk_flags".into(),
            Value::Array(
                self.risk_flags
                    .iter()
                    .map(|s| Value::String(s.clone()))
                    .collect(),
            ),
        );
        Value::Object(out)
    }
}
