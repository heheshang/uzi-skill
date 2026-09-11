//! Port of `lib/net_timeout_guard.py` plus the shared `requests` usage that
//! every upstream data-source module relies on.
//!
//! Upstream monkey-patches `requests.Session.request` to inject a default
//! timeout (`UZI_HTTP_TIMEOUT`, 20s). The Rust port applies the same default at
//! every call site through [`get`] / [`get_json`] and decodes the GBK responses
//! the Tencent / Sina quote endpoints emit (upstream does
//! `r.encoding = "gbk"`).

use serde_json::Value;
use std::time::Duration;

/// Upstream `_UA` constants are all Chrome-on-desktop strings; keep the family
/// name so servers that fingerprint by UA behave the same.
pub const UA: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/122.0 Safari/537.36";

/// `UZI_HTTP_TIMEOUT` default 20s (upstream `net_timeout_guard`).
pub fn timeout_default() -> u64 {
    std::env::var("UZI_HTTP_TIMEOUT")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .filter(|v| *v > 0)
        .unwrap_or(20)
}

/// A completed HTTP response, decoded lazily.
#[derive(Debug, Clone)]
pub struct Resp {
    pub status: u16,
    pub body: Vec<u8>,
}

impl Resp {
    pub fn is_ok(&self) -> bool {
        (200..300).contains(&self.status)
    }

    /// UTF-8 (lossy) text — what `requests` returns when no encoding is forced.
    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }

    /// GBK / GB18030 text — upstream `r.encoding = "gbk"` for qt.gtimg.cn and
    /// hq.sinajs.cn.
    pub fn gbk_text(&self) -> String {
        decode_gbk(&self.body)
    }

    pub fn json(&self) -> Option<Value> {
        serde_json::from_slice(&self.body).ok()
    }

    /// `r.json() or {}` idiom.
    pub fn json_obj(&self) -> Value {
        self.json().unwrap_or_else(|| Value::Object(Default::default()))
    }
}

/// Decode GBK/GB18030 bytes to UTF-8, replacing malformed sequences.
pub fn decode_gbk(bytes: &[u8]) -> String {
    let (out, _, _) = encoding_rs::GBK.decode(bytes);
    out.into_owned()
}

/// Percent-encode a query value the way `requests` does for `params=`.
pub fn encode_uri_component(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{:02X}", b)),
        }
    }
    out
}

/// Append `params` to `base` exactly like `requests` (`?a=1&b=2`).
pub fn with_query(base: &str, params: &[(&str, &str)]) -> String {
    if params.is_empty() {
        return base.to_string();
    }
    let mut out = String::from(base);
    out.push(if base.contains('?') { '&' } else { '?' });
    for (i, (k, v)) in params.iter().enumerate() {
        if i > 0 {
            out.push('&');
        }
        out.push_str(&encode_uri_component(k));
        out.push('=');
        out.push_str(&encode_uri_component(v));
    }
    out
}

fn agent(timeout_secs: u64) -> ureq::Agent {
    ureq::Agent::new_with_config(
        ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(timeout_secs.max(1))))
            .http_status_as_error(false)
            .user_agent(UA)
            .build(),
    )
}

/// GET with explicit headers. Never panics; transport failures become `Err`.
pub fn get(url: &str, headers: &[(&str, &str)], timeout_secs: u64) -> Result<Resp, String> {
    let mut req = agent(timeout_secs).get(url);
    for (k, v) in headers {
        req = req.header(*k, *v);
    }
    match req.call() {
        Ok(mut resp) => {
            let status = resp.status().as_u16();
            let body = resp
                .body_mut()
                .read_to_vec()
                .map_err(|e| format!("read body: {e}"))?;
            Ok(Resp { status, body })
        }
        Err(e) => Err(format!("{e}")),
    }
}

/// `requests.get(url, timeout=..., headers={"User-Agent": _UA})`.
pub fn get_plain(url: &str, timeout_secs: u64) -> Result<Resp, String> {
    get(url, &[], timeout_secs)
}

/// GET + parse JSON. Missing/non-JSON body is an error.
pub fn get_json(url: &str, headers: &[(&str, &str)], timeout_secs: u64) -> Result<Value, String> {
    let resp = get(url, headers, timeout_secs)?;
    if !resp.is_ok() {
        return Err(format!("HTTP {}", resp.status));
    }
    resp.json().ok_or_else(|| "invalid JSON".to_string())
}

/// GET with query params + JSON parse (`requests.get(url, params=..., timeout=...)`).
pub fn get_json_q(
    url: &str,
    params: &[(&str, &str)],
    headers: &[(&str, &str)],
    timeout_secs: u64,
) -> Result<Value, String> {
    get_json(&with_query(url, params), headers, timeout_secs)
}

/// `_retry(fn, attempts, sleep)` from `data_sources.py` — exponential backoff.
/// `fn` is invoked up to `attempts` times; the last error is returned.
pub fn retry<T, F>(attempts: usize, sleep: f64, mut f: F) -> Result<T, String>
where
    F: FnMut() -> Result<T, String>,
{
    let attempts = attempts.max(1);
    let mut last = String::new();
    for i in 0..attempts {
        match f() {
            Ok(v) => return Ok(v),
            Err(e) => {
                last = e;
                if i + 1 < attempts {
                    std::thread::sleep(Duration::from_secs_f64(sleep * (i as f64 + 1.0)));
                }
            }
        }
    }
    Err(last)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_encoding_matches_requests() {
        assert_eq!(with_query("http://x/a", &[]), "http://x/a");
        assert_eq!(
            with_query("http://x/a", &[("secid", "1.600519"), ("ut", "fa5fd")]),
            "http://x/a?secid=1.600519&ut=fa5fd"
        );
        assert_eq!(with_query("http://x/a?z=1", &[("k", "v")]), "http://x/a?z=1&k=v");
        assert_eq!(encode_uri_component("a b/c"), "a%20b%2Fc");
        assert_eq!(encode_uri_component("中文"), "%E4%B8%AD%E6%96%87");
    }

    #[test]
    fn gbk_decoding() {
        // B9F3 D6DD C3A9 CCA8 = 贵州茅台 in GBK
        let bytes = [0xB9u8, 0xF3, 0xD6, 0xDD, 0xC3, 0xA9, 0xCC, 0xA8];
        assert_eq!(decode_gbk(&bytes), "贵州茅台");
    }
}
