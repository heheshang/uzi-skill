//! Port of `lib/cache.py` — tiered JSON cache, task outputs, market clock.

use chrono::{DateTime, Datelike, NaiveTime, Timelike, Utc};
use chrono_tz::Tz;
use serde_json::Value;
use std::path::{Path, PathBuf};

pub const TTL_REALTIME: u64 = 60;
pub const TTL_INTRADAY: u64 = 5 * 60;
pub const TTL_HOURLY: u64 = 60 * 60;
pub const TTL_DAILY: u64 = 2 * 60 * 60;
pub const TTL_QUARTERLY: u64 = 24 * 60 * 60;
pub const TTL_STATIC: u64 = 7 * 24 * 60 * 60;
pub const CACHE_TTL_SECONDS: u64 = TTL_INTRADAY;

/// Root of the on-disk cache. `UZI_CACHE_ROOT` overrides `.cache` so tests and
/// sandboxes stay isolated.
pub fn cache_root() -> PathBuf {
    match std::env::var("UZI_CACHE_ROOT") {
        Ok(p) if !p.is_empty() => PathBuf::from(p),
        _ => PathBuf::from(".cache"),
    }
}

pub fn no_cache() -> bool {
    std::env::var("STOCK_NO_CACHE").map(|v| v == "1").unwrap_or(false)
}

fn md5_hex12(input: &str) -> String {
    use md5::{Digest, Md5};
    let mut hasher = Md5::new();
    hasher.update(input.as_bytes());
    let digest = hasher.finalize();
    let hex: String = digest.iter().map(|b| format!("{:02x}", b)).collect();
    hex[..12].to_string()
}

fn sanitize_key(key: &str) -> String {
    key.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-') {
                c
            } else {
                '_'
            }
        })
        .take(60)
        .collect()
}

/// `.cache/{ticker}/api_cache/{safe_key}__{md5[:12]}.json`
pub fn cache_path(ticker: &str, key: &str) -> PathBuf {
    cache_root()
        .join(ticker)
        .join("api_cache")
        .join(format!("{}__{}.json", sanitize_key(key), md5_hex12(key)))
}

/// Return the cached value if fresh, else compute `fetch_fn`, store and return.
pub fn cached<F, E>(ticker: &str, key: &str, ttl: u64, fetch_fn: F) -> Result<Value, E>
where
    F: FnOnce() -> Result<Value, E>,
    E: From<std::io::Error> + From<serde_json::Error>,
{
    let path = cache_path(ticker, key);
    let now = Utc::now().timestamp() as f64;

    if !no_cache() && path.exists() {
        if let Ok(text) = std::fs::read_to_string(&path) {
            if let Ok(payload) = serde_json::from_str::<Value>(&text) {
                let cached_at = payload.get("_cached_at").and_then(|v| v.as_f64());
                if let Some(cached_at) = cached_at {
                    if now - cached_at < ttl as f64 {
                        if let Some(data) = payload.get("data") {
                            return Ok(data.clone());
                        }
                    }
                }
            }
        }
    }

    let data = fetch_fn()?;
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let payload = serde_json::json!({
        "_cached_at": now,
        "data": data,
        "_ttl": ttl,
    });
    let _ = std::fs::write(&path, serde_json::to_string(&payload).unwrap_or_default());
    Ok(data)
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct MarketStatus {
    pub is_open: bool,
    pub label: String,
    pub now: String,
    pub market: String,
    pub timezone: String,
    pub calendar_verified: bool,
}

const SESSIONS_HK: &[(NaiveTime, NaiveTime)] = &[
    (
        NaiveTime::from_hms_opt(9, 30, 0).unwrap(),
        NaiveTime::from_hms_opt(12, 0, 0).unwrap(),
    ),
    (
        NaiveTime::from_hms_opt(13, 0, 0).unwrap(),
        NaiveTime::from_hms_opt(16, 0, 0).unwrap(),
    ),
];
const SESSIONS_US: &[(NaiveTime, NaiveTime)] = &[(
    NaiveTime::from_hms_opt(9, 30, 0).unwrap(),
    NaiveTime::from_hms_opt(16, 0, 0).unwrap(),
)];
const SESSIONS_A: &[(NaiveTime, NaiveTime)] = &[
    (
        NaiveTime::from_hms_opt(9, 30, 0).unwrap(),
        NaiveTime::from_hms_opt(11, 30, 0).unwrap(),
    ),
    (
        NaiveTime::from_hms_opt(13, 0, 0).unwrap(),
        NaiveTime::from_hms_opt(15, 0, 0).unwrap(),
    ),
];

struct Clock {
    tz: Tz,
    sessions: &'static [(NaiveTime, NaiveTime)],
}

fn clock_for(market: &str) -> Clock {
    match market {
        "H" => Clock {
            tz: chrono_tz::Asia::Hong_Kong,
            sessions: SESSIONS_HK,
        },
        "U" => Clock {
            tz: chrono_tz::America::New_York,
            sessions: SESSIONS_US,
        },
        _ => Clock {
            tz: chrono_tz::Asia::Shanghai,
            sessions: SESSIONS_A,
        },
    }
}

/// Exchange-aware market status for A/H/US equities.
///
/// Upstream additionally consults `exchange_calendars`; the Rust port keeps the
/// weekday/session logic and reports `calendar_verified: false`.
pub fn market_status(market: &str, now: Option<DateTime<Utc>>) -> MarketStatus {
    let mut market = market.to_uppercase();
    if market == "US" || market == "USA" {
        market = "U".to_string();
    }
    // Crypto trades continuously — no sessions, no weekend close.
    if market == "C" {
        let local_now = now.unwrap_or_else(Utc::now);
        return MarketStatus {
            is_open: true,
            label: "24/7 交易中".to_string(),
            now: local_now.format("%Y-%m-%dT%H:%M:%S").to_string(),
            market,
            timezone: "UTC".to_string(),
            calendar_verified: true,
        };
    }
    if !matches!(market.as_str(), "A" | "H" | "U") {
        market = "A".to_string();
    }
    let clock = clock_for(&market);
    let local_now = now.unwrap_or_else(Utc::now).with_timezone(&clock.tz);
    let weekday = local_now.weekday().num_days_from_monday();
    let t = local_now.time();
    let t = NaiveTime::from_hms_opt(t.hour(), t.minute(), t.second()).unwrap();

    let (label, is_open) = if weekday >= 5 {
        ("已收盘 (周末)", false)
    } else if clock.sessions.iter().any(|(s, e)| t >= *s && t < *e) {
        ("交易中", true)
    } else if clock.sessions.len() > 1
        && clock.sessions[0].1 <= t
        && t < clock.sessions[1].0
    {
        ("午间休市", false)
    } else if t < clock.sessions[0].0 {
        ("未开盘", false)
    } else {
        ("已收盘", false)
    };

    MarketStatus {
        is_open,
        label: label.to_string(),
        now: local_now.format("%Y-%m-%dT%H:%M:%S").to_string(),
        market,
        timezone: clock.tz.name().to_string(),
        calendar_verified: false,
    }
}

pub fn task_output_path(ticker: &str, task_name: &str) -> PathBuf {
    cache_root().join(ticker).join(format!("{}.json", task_name))
}

/// Write `{task_name}.json` · upstream uses `indent=2, ensure_ascii=False`.
pub fn write_task_output(
    ticker: &str,
    task_name: &str,
    data: &Value,
) -> std::io::Result<PathBuf> {
    let path = task_output_path(ticker, task_name);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let text = serde_json::to_string_pretty(data).unwrap_or_else(|_| "null".into());
    std::fs::write(&path, text)?;
    Ok(path)
}

pub fn read_task_output(ticker: &str, task_name: &str) -> Option<Value> {
    let path = task_output_path(ticker, task_name);
    let text = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&text).ok()
}

pub fn require_task_output(ticker: &str, task_name: &str) -> anyhow::Result<Value> {
    read_task_output(ticker, task_name).ok_or_else(|| {
        anyhow::anyhow!(
            "Gate failed: {}.json missing for {}. Run the previous task first.",
            task_name,
            ticker
        )
    })
}

/// `Path` helper used across fetchers.
pub fn read_json(path: &Path) -> Option<Value> {
    let text = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&text).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn md5_matches_python_hashlib() {
        // hashlib.md5(b"akshare:stock_individual_info_em").hexdigest()[:12]
        assert_eq!(md5_hex12("akshare:stock_individual_info_em"), "a827847a7ffc");
        // non-ASCII keys are hashed as UTF-8, same as Python
        assert_eq!(md5_hex12("雪球热度"), "25cfc7436685");
    }

    #[test]
    fn sanitizes_key_like_python() {
        assert_eq!(sanitize_key("a/b:c d"), "a_b_c_d");
        assert_eq!(sanitize_key("k.x-y_z"), "k.x-y_z");
    }

    #[test]
    fn a_share_market_status_states() {
        // 2026-09-11 is a Friday; 10:00 Shanghai is mid-morning session
        let ts = Utc.with_ymd_and_hms(2026, 9, 11, 2, 0, 0).unwrap();
        let s = market_status("A", Some(ts));
        assert!(s.is_open);
        assert_eq!(s.label, "交易中");
        assert_eq!(s.market, "A");

        // 12:00 Shanghai (UTC 04:00) → lunch break
        let ts = Utc.with_ymd_and_hms(2026, 9, 11, 4, 0, 0).unwrap();
        assert_eq!(market_status("A", Some(ts)).label, "午间休市");

        // Saturday
        let ts = Utc.with_ymd_and_hms(2026, 9, 12, 4, 0, 0).unwrap();
        assert_eq!(market_status("A", Some(ts)).label, "已收盘 (周末)");

        // 08:00 Shanghai → 未开盘
        let ts = Utc.with_ymd_and_hms(2026, 9, 11, 0, 0, 0).unwrap();
        assert_eq!(market_status("A", Some(ts)).label, "未开盘");
    }

    #[test]
    fn us_alias_resolves_to_u() {
        let ts = Utc.with_ymd_and_hms(2026, 9, 11, 14, 0, 0).unwrap();
        let s = market_status("US", Some(ts));
        assert_eq!(s.market, "U");
        assert_eq!(s.timezone, "America/New_York");
        assert!(s.is_open);
    }
}
