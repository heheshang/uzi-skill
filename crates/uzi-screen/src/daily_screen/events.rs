//! Port of `lib/daily_screen/events.py` — best-effort event and historical LHB
//! enrichment for preselected stocks.

use chrono::{DateTime, FixedOffset, NaiveDate, NaiveDateTime, TimeZone};
use serde_json::{Map, Value};

use super::models::StockSnapshot;

/// Asia/Shanghai is a fixed +08:00 zone; `evidence_time` attaches the same
/// offset `ZoneInfo("Asia/Shanghai")` does.
pub(crate) fn shanghai_offset() -> FixedOffset {
    FixedOffset::east_opt(8 * 3600).expect("+08:00 is a valid offset")
}

pub(crate) fn naive_to_shanghai(naive: NaiveDateTime) -> DateTime<FixedOffset> {
    shanghai_offset()
        .from_local_datetime(&naive)
        .single()
        .unwrap_or_else(|| shanghai_offset().from_utc_datetime(&naive))
}

/// Python `isoformat(timespec="seconds")`.
pub(crate) fn iso_seconds(dt: &DateTime<FixedOffset>) -> String {
    dt.format("%Y-%m-%dT%H:%M:%S%:z").to_string()
}

/// Python `isoformat()` — microseconds only when non-zero.
pub(crate) fn iso_auto(dt: &DateTime<FixedOffset>) -> String {
    if dt.timestamp_subsec_micros() == 0 {
        iso_seconds(dt)
    } else {
        dt.format("%Y-%m-%dT%H:%M:%S%.6f%:z").to_string()
    }
}

/// `evidence_time(value)` — `datetime.fromisoformat(str(value).replace("Z",
/// "+00:00"))`, a bare date meaning end-of-day, naive results interpreted in
/// Asia/Shanghai.
pub(crate) fn evidence_time(value: &Value) -> Option<DateTime<FixedOffset>> {
    let text = uzi_core::py::py_str(value).trim().replace('Z', "+00:00");
    if text.chars().count() == 10 {
        if let Ok(date) = NaiveDate::parse_from_str(&text, "%Y-%m-%d") {
            let naive = date.and_hms_micro_opt(23, 59, 59, 999_999)?;
            return Some(naive_to_shanghai(naive));
        }
        return None;
    }
    if let Ok(dt) = DateTime::parse_from_rfc3339(&text) {
        return Some(dt);
    }
    for fmt in [
        "%Y-%m-%dT%H:%M:%S%.f%:z",
        "%Y-%m-%d %H:%M:%S%.f%:z",
        "%Y-%m-%dT%H:%M%:z",
        "%Y-%m-%d %H:%M%:z",
    ] {
        if let Ok(dt) = DateTime::parse_from_str(&text, fmt) {
            return Some(dt);
        }
    }
    for fmt in [
        "%Y-%m-%dT%H:%M:%S%.f",
        "%Y-%m-%d %H:%M:%S%.f",
        "%Y-%m-%dT%H:%M",
        "%Y-%m-%d %H:%M",
    ] {
        if let Ok(naive) = NaiveDateTime::parse_from_str(&text, fmt) {
            return Some(naive_to_shanghai(naive));
        }
    }
    None
}

/// `filter_evidence(evidence, cutoff)`.
pub(crate) fn filter_evidence(
    evidence: &[Value],
    cutoff: &str,
) -> (Vec<Value>, Vec<String>) {
    let boundary = evidence_time(&Value::String(cutoff.to_string()));
    let mut kept = Vec::new();
    let mut gaps: Vec<String> = Vec::new();
    for item in evidence {
        let published = evidence_time(uzi_core::py::get(item, "published_at"));
        let (Some(boundary_dt), Some(published_dt)) = (&boundary, &published) else {
            gaps.push("evidence_time_unverified".to_string());
            continue;
        };
        if published_dt > boundary_dt {
            gaps.push("evidence_time_unverified".to_string());
            continue;
        }
        if uzi_core::py::get(item, "kind").as_str() == Some("lhb")
            && published_dt.date_naive() >= boundary_dt.date_naive()
        {
            continue;
        }
        kept.push(item.clone());
    }
    gaps.sort();
    gaps.dedup();
    (kept, gaps)
}

/// `is_business_evidence(item)` — an A/B event whose business fact is verified
/// and whose source URL is a real `http(s)` link.
pub(crate) fn is_business_evidence(item: &Value) -> bool {
    let kind_ok = uzi_core::py::get(item, "kind").as_str() == Some("event");
    let grade = uzi_core::py::get(item, "grade");
    let grade_ok = matches!(grade.as_str(), Some("A") | Some("B"));
    let verified_ok = matches!(uzi_core::py::get(item, "business_fact_verified"), Value::Bool(true));
    let fact = uzi_core::py::get(item, "business_fact");
    let fact_ok = matches!(
        fact.as_str(),
        Some("order") | Some("certification") | Some("production") | Some("earnings_contribution")
    );
    if !(kind_ok && grade_ok && verified_ok && fact_ok)
        || !uzi_core::py::truthy(uzi_core::py::get(item, "source"))
        || !uzi_core::py::truthy(uzi_core::py::get(item, "title"))
    {
        return false;
    }
    let raw = uzi_core::py::get(item, "url");
    let raw = if uzi_core::py::truthy(raw) {
        uzi_core::py::py_str(raw)
    } else {
        String::new()
    };
    // `safe_url` refuses anything whose scheme is not http/https or has no
    // netloc, which is exactly upstream's `urlparse` gate.
    uzi_report::security::safe_url_default(&Value::String(raw)) != "#"
}

/// `_wrapped_data(payload)`.
fn wrapped_data(payload: &Value) -> Value {
    if let Some(data) = payload.get("data") {
        if data.is_object() {
            return data.clone();
        }
    }
    if payload.is_object() {
        payload.clone()
    } else {
        Value::Object(Map::new())
    }
}

/// Python `a or b or c` over JSON values.
fn first_truthy(values: &[&Value]) -> Value {
    for v in values {
        if uzi_core::py::truthy(v) {
            return (*v).clone();
        }
    }
    Value::Null
}

/// Human-readable kind for a uzi-data fetch error, used in the `gaps` strings
/// upstream builds from `type(exc).__name__`.
fn error_name(err: &str) -> String {
    let head = err.split(':').next().unwrap_or("").trim();
    if !head.is_empty() && head.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        head.to_string()
    } else {
        "Exception".to_string()
    }
}

/// `enrich_stock(stock)` — best-effort; failures become `gaps`.
pub fn enrich_stock(stock: &StockSnapshot) -> (Vec<Value>, Vec<String>) {
    let mut evidence: Vec<Value> = Vec::new();
    let mut gaps: Vec<String> = Vec::new();

    match uzi_data::fetch::events::main(&stock.code) {
        Err(err) => gaps.push(format!("events:{}", error_name(&err))),
        Ok(payload) => {
            let data = wrapped_data(&payload);
            let source = payload
                .get("source")
                .and_then(|v| v.as_str())
                .map(str::to_string)
                .unwrap_or_else(|| "fetch_events".to_string());
            let mut events: Vec<Value> = Vec::new();
            for key in ["recent_news", "recent_notices"] {
                if let Some(arr) = uzi_core::py::get(&data, key).as_array() {
                    events.extend(arr.iter().cloned());
                }
            }
            if events.is_empty() {
                if let Some(arr) = uzi_core::py::get(&data, "event_timeline").as_array() {
                    events.extend(arr.iter().cloned());
                }
            }
            let mut seen: Vec<(String, String)> = Vec::new();
            for item in events.iter().take(40) {
                let (title, published, item_source, url, relevant) = match item.as_object() {
                    Some(_obj) => {
                        let title = first_truthy(&[
                            uzi_core::py::get(item, "event"),
                            uzi_core::py::get(item, "title"),
                            uzi_core::py::get(item, "headline"),
                            uzi_core::py::get(item, "summary"),
                        ]);
                        let published = first_truthy(&[
                            uzi_core::py::get(item, "date"),
                            uzi_core::py::get(item, "published_at"),
                            uzi_core::py::get(item, "time"),
                        ]);
                        let item_source = first_truthy(&[
                            uzi_core::py::get(item, "source"),
                            uzi_core::py::get(item, "type"),
                            &Value::String(source.clone()),
                        ]);
                        let source_is_url = item_source
                            .as_str()
                            .map(|s| s.starts_with("https://"))
                            .unwrap_or(false);
                        let empty_url = Value::String(String::new());
                        let url_fallback = if source_is_url { &item_source } else { &empty_url };
                        let url = first_truthy(&[uzi_core::py::get(item, "url"), url_fallback]);
                        let type_text = uzi_core::py::py_str(uzi_core::py::get(item, "type"));
                        let item_source_text = uzi_core::py::py_str(&item_source);
                        let title_text = uzi_core::py::py_str(&title);
                        let code_raw = stock.code.split('.').next().unwrap_or("");
                        let notice = type_text.contains("cninfo") || item_source_text == "hkexnews";
                        let relevant = notice
                            || title_text.contains(code_raw)
                            || (!stock.name.is_empty() && title_text.contains(&stock.name));
                        (title, published, item_source, url, relevant)
                    }
                    None => (
                        Value::String(uzi_core::py::py_str(item)),
                        Value::Null,
                        Value::String(source.clone()),
                        Value::String(String::new()),
                        false,
                    ),
                };
                let title_text = uzi_core::py::py_str(&title);
                let key = (
                    title_text.clone(),
                    uzi_core::py::py_str(&published),
                );
                if !(uzi_core::py::truthy(&title)) || seen.contains(&key) {
                    continue;
                }
                seen.push(key);
                let mut entry = Map::new();
                entry.insert("kind".into(), Value::String("event".into()));
                entry.insert("grade".into(), Value::String("C".into()));
                entry.insert("business_fact_verified".into(), Value::Bool(false));
                entry.insert("title".into(), Value::String(title_text));
                entry.insert("published_at".into(), published);
                entry.insert(
                    "observed_at".into(),
                    Value::String(stock.observed_at.clone()),
                );
                entry.insert("source".into(), item_source);
                entry.insert("url".into(), url);
                entry.insert("company_specific".into(), Value::Bool(relevant));
                evidence.push(Value::Object(entry));
            }
        }
    }

    if stock.market == "A" {
        match uzi_data::fetch::lhb::main(&stock.code) {
            Err(err) => gaps.push(format!("lhb:{}", error_name(&err))),
            Ok(payload) => {
                let data = wrapped_data(&payload);
                let records = uzi_core::py::get(&data, "lhb_records");
                if let Some(arr) = records.as_array() {
                    for record in arr.iter().take(10) {
                        if !record.is_object() {
                            continue;
                        }
                        let raw_date = first_truthy(&[
                            uzi_core::py::get(record, "date"),
                            uzi_core::py::get(record, "上榜日期"),
                            &Value::String(String::new()),
                        ]);
                        let record_date: String =
                            uzi_core::py::py_str(&raw_date).chars().take(10).collect();
                        let payload_source = payload
                            .get("source")
                            .and_then(|v| v.as_str())
                            .map(str::to_string)
                            .unwrap_or_else(|| "fetch_lhb".to_string());
                        let mut entry = Map::new();
                        entry.insert("kind".into(), Value::String("lhb".into()));
                        entry.insert("grade".into(), Value::String("B".into()));
                        entry.insert(
                            "title".into(),
                            Value::String(format!(
                                "历史龙虎榜 {}",
                                if record_date.is_empty() {
                                    "日期缺失"
                                } else {
                                    &record_date
                                }
                            )),
                        );
                        entry.insert(
                            "published_at".into(),
                            if record_date.is_empty() {
                                Value::Null
                            } else {
                                Value::String(record_date)
                            },
                        );
                        entry.insert(
                            "observed_at".into(),
                            Value::String(stock.observed_at.clone()),
                        );
                        entry.insert("source".into(), Value::String(payload_source));
                        entry.insert(
                            "attribution_confidence".into(),
                            Value::String("style_only".into()),
                        );
                        evidence.push(Value::Object(entry));
                    }
                }
            }
        }
    }

    let (evidence, time_gaps) = filter_evidence(&evidence, &stock.observed_at);
    let mut all_gaps = gaps;
    all_gaps.extend(time_gaps);
    all_gaps.sort();
    all_gaps.dedup();
    (evidence, all_gaps)
}
