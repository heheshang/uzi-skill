//! Port of `lib/daily_screen/universe.py` — A/H market universe loading and
//! hard filters.

use anyhow::{bail, Result};
use serde_json::{Map, Value};

use super::models::StockSnapshot;

/// Column aliases, in the exact order upstream probes them.
const COLUMN_ALIASES: &[(&str, &[&str])] = &[
    ("code", &["代码", "symbol", "代码编号"]),
    ("name", &["名称", "name", "股票名称"]),
    ("price", &["最新价", "现价", "price", "最新"]),
    ("change_pct", &["涨跌幅", "涨幅", "change_pct"]),
    ("amount", &["成交额", "amount", "成交金额"]),
    ("industry", &["所属行业", "行业", "industry"]),
    ("open_price", &["今开", "开盘", "open"]),
    ("prev_close", &["昨收", "昨结", "prev_close"]),
    ("high", &["最高", "high"]),
    ("low", &["最低", "low"]),
    ("turnover_rate", &["换手率", "turnover_rate"]),
    ("volume_ratio", &["量比", "volume_ratio"]),
    ("market_cap", &["总市值", "market_cap"]),
];

/// Python `float(str(value).replace(",", "").replace("%", ""))` bounded by
/// `math.isfinite`. Note `str(None)` → `"None"` fails to parse, so — like
/// upstream — a `null` cell is *not* zero.
pub(crate) fn number(value: &Value) -> Option<f64> {
    if value.is_null() {
        return None;
    }
    let text = uzi_core::py::py_str(value);
    let cleaned: String = text.chars().filter(|c| *c != ',' && *c != '%').collect();
    let cleaned = cleaned.trim();
    if cleaned.is_empty() {
        return None;
    }
    let x = parse_python_float(cleaned)?;
    if x.is_finite() {
        Some(x)
    } else {
        None
    }
}

/// `float(s)` for the strings `str()` can produce (Python accepts `inf` /
/// `Infinity` / `nan`, and so does this).
pub(crate) fn parse_python_float(s: &str) -> Option<f64> {
    match s.to_ascii_lowercase().as_str() {
        "inf" | "+inf" | "infinity" | "+infinity" => return Some(f64::INFINITY),
        "-inf" | "-infinity" => return Some(f64::NEG_INFINITY),
        "nan" | "+nan" | "-nan" => return Some(f64::NAN),
        _ => {}
    }
    s.parse::<f64>().ok()
}

/// `_value(row, key, default)` — first present, non-null alias wins.
fn value<'a>(row: &'a Map<String, Value>, key: &str, default: &'a Value) -> &'a Value {
    let aliases = COLUMN_ALIASES
        .iter()
        .find(|(k, _)| *k == key)
        .map(|(_, a)| *a)
        .unwrap_or(&[]);
    for column in aliases {
        if let Some(item) = row.get(*column) {
            if !item.is_null() {
                return item;
            }
        }
    }
    default
}

/// Python `str(x or "")`.
fn str_or_empty(v: &Value) -> String {
    if uzi_core::py::truthy(v) {
        uzi_core::py::py_str(v)
    } else {
        String::new()
    }
}

/// `_full_code(code, market)` — normalize to a canonical A/H code.
pub(crate) fn full_code(code: &Value, market: &str) -> String {
    let raw = str_or_empty(code);
    let raw = raw.trim();
    let raw = raw.split('.').next().unwrap_or("");
    if raw.is_empty() || !raw.chars().all(|c| c.is_ascii_digit()) {
        return String::new();
    }
    if market == "H" {
        return format!("{raw:0>5}.HK");
    }
    if raw.chars().count() != 6 {
        return String::new();
    }
    let suffix = if raw.starts_with('4') || raw.starts_with('8') || raw.starts_with("92") {
        "BJ"
    } else if raw.starts_with('5') || raw.starts_with('6') || raw.starts_with('9') {
        "SH"
    } else {
        "SZ"
    };
    format!("{raw}.{suffix}")
}

/// `normalize_universe_frame(frame, market, observed_at, source)`.
///
/// Upstream receives a pandas frame; uzi-data hands us the same rows as a JSON
/// array of column-keyed objects.
pub fn normalize_universe_frame(
    frame: &Value,
    market: &str,
    observed_at: &str,
    source: &str,
) -> Vec<StockSnapshot> {
    let market = market.to_uppercase();
    let Some(rows) = frame.as_array() else {
        return Vec::new();
    };
    let empty = Value::String(String::new());
    let mut out = Vec::new();
    for row in rows {
        let row = match row.as_object() {
            Some(map) => map,
            None => continue,
        };
        let name = str_or_empty(value(row, "name", &empty)).trim().to_string();
        let upper_name = name.to_uppercase();
        let price = number(value(row, "price", &empty));
        let amount = number(value(row, "amount", &empty));
        let code = full_code(value(row, "code", &empty), &market);
        let change_pct = number(value(row, "change_pct", &empty));
        if code.is_empty()
            || name.is_empty()
            || price.is_none()
            || price.unwrap_or(0.0) <= 0.0
            || amount.is_none()
            || change_pct.is_none()
        {
            continue;
        }
        if market == "A"
            && (upper_name.starts_with("ST")
                || upper_name.starts_with("*ST")
                || upper_name.starts_with("SST")
                || name.contains('退'))
        {
            continue;
        }
        if market == "H" && (name.contains("退市") || name.contains("停牌")) {
            continue;
        }
        let default_industry = Value::String("未分类".to_string());
        let industry = value(row, "industry", &default_industry);
        let industry = if uzi_core::py::truthy(industry) {
            uzi_core::py::py_str(industry)
        } else {
            "未分类".to_string()
        };
        out.push(StockSnapshot {
            code,
            name,
            market: market.clone(),
            price: price.unwrap_or(0.0),
            change_pct: change_pct.unwrap_or(0.0),
            amount: amount.unwrap_or(0.0),
            industry,
            open_price: number(value(row, "open_price", &empty)),
            prev_close: number(value(row, "prev_close", &empty)),
            high: number(value(row, "high", &empty)),
            low: number(value(row, "low", &empty)),
            turnover_rate: number(value(row, "turnover_rate", &empty)),
            volume_ratio: number(value(row, "volume_ratio", &empty)),
            market_cap: number(value(row, "market_cap", &empty)),
            observed_at: observed_at.to_string(),
            source: source.to_string(),
            extra: Map::new(),
        });
    }
    out
}

/// `apply_hard_filters(stocks, min_turnover_local)`.
pub fn apply_hard_filters(
    stocks: &[StockSnapshot],
    min_turnover_local: f64,
) -> Result<(Vec<StockSnapshot>, Value)> {
    if !min_turnover_local.is_finite() || min_turnover_local < 2e8 {
        bail!("minimum turnover must be finite and at least 200 million");
    }
    let kept: Vec<StockSnapshot> = stocks
        .iter()
        .filter(|s| s.amount >= min_turnover_local)
        .cloned()
        .collect();
    let stats = serde_json::json!({
        "input": stocks.len(),
        "liquid": kept.len(),
        "removed_low_turnover": stocks.len() - kept.len(),
        "min_turnover_local": min_turnover_local,
    });
    Ok((kept, stats))
}

/// `fetch_market_universe(market)` — the A/H cross-section via uzi-data.
///
/// Upstream stamps the frame with the machine-local retrieval time
/// (`datetime.now().astimezone()`); uzi-data's `fetch_a_spot` / `fetch_hk_spot`
/// return the same rows (empty when the endpoint is unreachable), and the
/// observation stamp is applied here.
pub fn fetch_market_universe(market: &str) -> Result<Vec<StockSnapshot>> {
    let market = market.to_uppercase();
    let (frame, source) = match market.as_str() {
        "A" => (
            uzi_data::sources::fetch_a_spot(),
            "akshare:stock_zh_a_spot_em",
        ),
        "H" => (uzi_data::sources::fetch_hk_spot(), "akshare:stock_hk_spot_em"),
        other => bail!("daily screen only supports A/H markets, got {other:?}"),
    };
    let observed_at = chrono::Local::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, false);
    Ok(normalize_universe_frame(
        &frame,
        &market,
        &observed_at,
        source,
    ))
}
