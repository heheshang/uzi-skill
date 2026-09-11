//! Port of `lib/seat_db.py` — the 游资 seat table and the `is_in_range`
//! affordability/theme filter used by `investor_evaluator`.
//!
//! `src/data/seats.json` is `json.dumps(SEATS, ensure_ascii=False)` from the
//! upstream module.

use serde_json::{Map, Value};
use std::sync::LazyLock;

const SEATS_JSON: &str = include_str!("data/seats.json");

/// v2.13.3 · implicit upper bound (yuan) for 游资 without an explicit `max_mcap`.
pub const FALLBACK_YOUZI_MAX_MCAP_YUAN: f64 = 50_000_000_000.0;
/// 游资 allowed to trade mega caps (exempt from the fallback cap).
const MEGA_CAP_ALLOWLIST: &[&str] = &["章盟主"];

/// `seat_db.SEATS`, in source key order.
pub fn seats() -> &'static Map<String, Value> {
    static SEATS: LazyLock<Map<String, Value>> = LazyLock::new(|| {
        serde_json::from_str::<Value>(SEATS_JSON)
            .expect("embedded SEATS json")
            .as_object()
            .cloned()
            .expect("SEATS is a dict")
    });
    &SEATS
}

/// Python `a == b` for the scalar types that appear in `fit_rules`: `True == 1`,
/// `False == 0`, strings compare by value.
fn py_eq(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => x.as_f64() == y.as_f64(),
        (Value::Bool(x), Value::Bool(y)) => x == y,
        (Value::Bool(x), Value::Number(n)) | (Value::Number(n), Value::Bool(x)) => {
            n.as_f64() == Some(if *x { 1.0 } else { 0.0 })
        }
        (Value::String(x), Value::String(y)) => x == y,
        _ => false,
    }
}

fn as_f64(v: &Value) -> f64 {
    v.as_f64().unwrap_or(0.0)
}

/// `seat_db.is_in_range` — does this stock fit the 游资's range/theme?
///
/// `nickname not in SEATS` → `False` (upstream returns `False` for unknown names).
pub fn is_in_range(nickname: &str, ticker_features: &Value) -> bool {
    let Some(info) = seats().get(nickname) else {
        return false;
    };
    let rules = info.get("fit_rules").and_then(Value::as_object).cloned().unwrap_or_default();

    // `mc = ticker_features.get("market_cap", 0) or 0`
    let mc = match ticker_features.get("market_cap") {
        None | Some(Value::Null) => 0.0,
        Some(v) if uzi_core::py::truthy(v) => as_f64(v),
        _ => 0.0,
    };

    if let Some(min) = rules.get("min_mcap") {
        if mc < as_f64(min) {
            return false;
        }
    }
    if let Some(max) = rules.get("max_mcap") {
        if mc > as_f64(max) {
            return false;
        }
    }
    if !rules.contains_key("max_mcap") && !MEGA_CAP_ALLOWLIST.contains(&nickname) {
        if mc > FALLBACK_YOUZI_MAX_MCAP_YUAN {
            return false;
        }
    }

    for (k, v) in &rules {
        if k.starts_with("min_") || k.starts_with("max_") {
            continue;
        }
        if let Some(got) = ticker_features.get(k) {
            if !py_eq(got, v) {
                return false;
            }
        }
    }
    true
}

/// `seat_db.match_seats_in_lhb` — 游资 whose seat keyword appears in an LHB row.
///
/// Rows are flat scalar dicts in practice; values are joined with Python `str()`
/// semantics (`uzi_core::py::num_str`).
pub fn match_seats_in_lhb(lhb_records: &[Value]) -> Value {
    let mut matches = Map::new();
    for (nick, info) in seats() {
        let Some(keywords) = info.get("seats").and_then(Value::as_array) else {
            continue;
        };
        let keywords: Vec<&str> = keywords.iter().filter_map(Value::as_str).collect();
        let mut hits = Vec::new();
        for row in lhb_records {
            let text = match row.as_object() {
                Some(o) => o.values().map(uzi_core::py::num_str).collect::<Vec<_>>().join(" "),
                None => uzi_core::py::num_str(row),
            };
            if keywords.iter().any(|kw| text.contains(kw)) {
                hits.push(row.clone());
            }
        }
        if !hits.is_empty() {
            matches.insert(nick.clone(), Value::Array(hits));
        }
    }
    Value::Object(matches)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn seat_table_matches_upstream_shape() {
        assert_eq!(seats().len(), 23);
        assert!(seats().get("张盟主").is_none()); // the real key is 章盟主
        assert_eq!(seats()["章盟主"]["real_name"], "章建平");
        assert_eq!(seats()["章盟主"]["fit_rules"]["min_mcap"], json!(20_000_000_000i64));
        assert_eq!(seats()["炒股养家"]["premium"], "next_day_70");
        assert!(seats()["炒股养家"].get("real_name").is_none());
    }

    #[test]
    fn lhb_rows_match_seat_keywords_in_table_order() {
        let rows = vec![
            json!({"营业部名称": "国泰君安证券股份有限公司上海江苏路证券营业部", "买入": 1000}),
            json!({"营业部名称": "东方财富证券股份有限公司拉萨团结路第二证券营业部"}),
            json!({"营业部名称": "华鑫证券有限责任公司上海红宝石路证券营业部"}),
        ];
        let m = match_seats_in_lhb(&rows);
        let keys: Vec<&str> = m.as_object().unwrap().keys().map(String::as_str).collect();
        // SEATS order: 章盟主, 炒股养家, 拉萨天团, … 鑫多多 (its 华鑫证券 keyword)
        assert_eq!(keys, vec!["章盟主", "炒股养家", "拉萨天团", "鑫多多"]);
        assert_eq!(m["章盟主"].as_array().unwrap().len(), 1);
        assert_eq!(m["拉萨天团"].as_array().unwrap().len(), 1);
        assert!(m.get("孙哥").is_none()); // no hits → key omitted
    }

    #[test]
    fn range_respects_explicit_and_implicit_caps() {
        // 章盟主 explicitly demands ≥ 200 亿
        assert!(!is_in_range("章盟主", &json!({"market_cap": 19_999_999_999i64})));
        assert!(is_in_range("章盟主", &json!({"market_cap": 20_000_000_000i64})));
        // 章盟主 is allowlisted against the implicit 500 亿 ceiling
        assert!(is_in_range("章盟主", &json!({"market_cap": 9_000_000_000_000i64})));
        // 孙哥 has no max → implicit 500 亿 cap applies
        assert!(is_in_range("孙哥", &json!({"market_cap": 50_000_000_000i64})));
        assert!(!is_in_range("孙哥", &json!({"market_cap": 50_000_000_001i64})));
        // 佛山无影脚's explicit max is 80 亿 and wins over the implicit cap
        assert!(is_in_range("佛山无影脚", &json!({"market_cap": 8_000_000_000i64})));
        assert!(!is_in_range("佛山无影脚", &json!({"market_cap": 8_000_000_001i64})));
        // unknown nickname → False, missing market_cap → 0
        assert!(!is_in_range("不存在", &json!({})));
        assert!(is_in_range("拉萨天团", &json!({})));
    }

    #[test]
    fn non_range_rules_are_conjunctive() {
        // 赵老哥 requires is_first_or_second_board and is_sector_leader when present
        assert!(is_in_range("赵老哥", &json!({"market_cap": 30_000_000_000i64})));
        assert!(!is_in_range(
            "赵老哥",
            &json!({"market_cap": 30_000_000_000i64, "is_sector_leader": false})
        ));
        assert!(is_in_range(
            "赵老哥",
            &json!({"market_cap": 30_000_000_000i64, "is_sector_leader": true, "is_first_or_second_board": true})
        ));
        // absent keys never fail a rule
        assert!(is_in_range("陈小群", &json!({"market_cap": 3_000_000_000i64})));
        assert!(!is_in_range(
            "陈小群",
            &json!({"market_cap": 3_000_000_000i64, "is_hot_theme": false})
        ));
    }
}
