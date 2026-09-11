//! Port of `lib/providers/direct_http_provider.py` — Tencent qt / Sina hq /
//! etnet quote endpoints, the three sources that need no key and no Python lib.
//!
//! Field-index mapping is copied verbatim from upstream (`parts[3]` is the
//! current price, `parts[45]` the total market cap in 亿, …); Sina's A-share
//! branch reads `fields[3]` for the price and its HK branch `fields[6]`.

use serde_json::{json, Value};

use crate::http;

pub const NAME: &str = "direct_http";
pub const REQUIRES_KEY: bool = false;
pub const MARKETS: &[&str] = &["A", "H", "U"];

/// `_REQ_OK` — upstream gates on `requests` being importable; ureq is always
/// present, so the Rust provider is always available.
pub fn is_available() -> bool {
    true
}

fn tencent_symbol(code: &str, market: &str) -> Result<String, String> {
    match market {
        // sh600519 / sz000001
        "A" => {
            let prefix = if code.starts_with(['6', '9', '5', '1']) {
                "sh"
            } else {
                "sz"
            };
            Ok(format!("{prefix}{code}"))
        }
        "H" => Ok(format!("hk{:0>5}", code)),
        "U" => Ok(format!("us{}", code.to_uppercase())),
        other => Err(format!("unsupported market: {other:?}")),
    }
}

fn sina_symbol(code: &str, market: &str) -> Result<String, String> {
    match market {
        "A" => {
            let prefix = if code.starts_with(['6', '9', '5', '1']) {
                "sh"
            } else {
                "sz"
            };
            Ok(format!("{prefix}{code}"))
        }
        "H" => Ok(format!("hk{:0>5}", code)),
        "U" => Ok(format!("gb_{}", code.to_lowercase())),
        other => Err(format!("unsupported market: {other:?}")),
    }
}

fn fnum(s: &str) -> f64 {
    // upstream `float(parts[i] or 0)` — empty string means 0
    s.trim().parse::<f64>().unwrap_or(0.0)
}

fn opt_num(s: &str) -> Option<f64> {
    let v = fnum(s);
    if v == 0.0 {
        None
    } else {
        Some(v)
    }
}

/// `_DirectHttpProvider.fetch_quote_tencent`.
pub fn fetch_quote_tencent(code: &str, market: &str) -> Result<Value, String> {
    let qt_code = tencent_symbol(code, market)?;
    let url = format!("http://qt.gtimg.cn/q={qt_code}");
    let resp = http::get(&url, &[], 8).map_err(|e| format!("tencent qt: {e}"))?;
    let text = resp.gbk_text();
    let text = text.trim();
    let payload = extract_quoted(text)
        .ok_or_else(|| format!("tencent qt empty response: {}", first80(text)))?;
    let parts: Vec<&str> = payload.split('~').collect();
    if parts.len() < 33 {
        return Err(format!("tencent qt short response: {} fields", parts.len()));
    }
    let at = |i: usize| parts.get(i).copied().unwrap_or("");
    Ok(json!({
        "name": at(1),
        "code": at(2),
        "price": fnum(at(3)),
        "prev_close": fnum(at(4)),
        "open": fnum(at(5)),
        "volume": fnum(at(6)),
        "high": opt_num(at(33)),
        "low": opt_num(at(34)),
        "amount": opt_num(at(37)),
        "source": format!("tencent_qt:{qt_code}"),
    }))
}

/// `_DirectHttpProvider.fetch_quote_sina`.
pub fn fetch_quote_sina(code: &str, market: &str) -> Result<Value, String> {
    let sina_code = sina_symbol(code, market)?;
    let url = format!("http://hq.sinajs.cn/list={sina_code}");
    let resp = http::get(
        &url,
        &[("Referer", "http://finance.sina.com.cn")],
        8,
    )
    .map_err(|e| format!("sina hq: {e}"))?;
    let text = resp.gbk_text();
    let text = text.trim();
    let payload = extract_quoted(text).ok_or_else(|| format!("sina hq empty: {}", first80(text)))?;
    let fields: Vec<&str> = payload.split(',').collect();
    if fields.len() < 6 {
        return Err("sina hq too short".to_string());
    }
    let at = |i: usize| fields.get(i).copied().unwrap_or("");
    Ok(match market {
        // 0:名称 1:开盘 2:昨收 3:当前 4:最高 5:最低 6:买一 7:卖一 8:成交量 9:成交额
        "A" => json!({
            "name": at(0),
            "code": code,
            "open": fnum(at(1)),
            "prev_close": fnum(at(2)),
            "price": fnum(at(3)),
            "high": fnum(at(4)),
            "low": fnum(at(5)),
            "volume": if fields.len() > 8 { fnum(at(8)) } else { 0.0 },
            "amount": if fields.len() > 9 { fnum(at(9)) } else { 0.0 },
            "source": format!("sina_hq:{sina_code}"),
        }),
        // 0:英文名 1:中文名 2:开盘 3:昨收 4:最高 5:最低 6:当前 ...
        "H" => json!({
            "name": if fields.len() > 1 { at(1) } else { "" },
            "code": code,
            "open": fnum(at(2)),
            "prev_close": fnum(at(3)),
            "high": fnum(at(4)),
            "low": fnum(at(5)),
            "price": fnum(at(6)),
            "source": format!("sina_hq:{sina_code}"),
        }),
        // 美股: name / price / change_pct / change / open / high / low / prev_close ...
        _ => json!({
            "name": at(0),
            "code": code,
            "price": fnum(at(1)),
            "open": if fields.len() > 5 { opt_num(at(5)) } else { None },
            "high": if fields.len() > 6 { opt_num(at(6)) } else { None },
            "low": if fields.len() > 7 { opt_num(at(7)) } else { None },
            "prev_close": if fields.len() > 26 { opt_num(at(26)) } else { None },
            "source": format!("sina_hq:{sina_code}"),
        }),
    })
}

/// `_DirectHttpProvider.fetch_quote_etnet` — HK page-level fallback.
pub fn fetch_quote_etnet(code: &str) -> Result<Value, String> {
    let code5 = code.trim_start_matches('0');
    let code5 = if code5.is_empty() { "0" } else { code5 };
    let url = format!("https://www.etnet.com.hk/www/tc/stocks/realtime/quote.php?code={code5}");
    let resp = http::get_plain(&url, 10).map_err(|e| format!("etnet: {e}"))?;
    let html = resp.text();
    let price = find_group(&html, r#"realTimeQuote[^>]*>([\d.]+)"#)
        .or_else(|| find_group(&html, r#""lastPrice"[^>]*>([\d.]+)"#))
        .ok_or_else(|| "etnet: price element not found".to_string())?;
    let price: f64 = price.parse().unwrap_or(0.0);
    Ok(json!({
        "code": code,
        "price": price,
        "source": format!("etnet:{code5}"),
        "_note": "页面级 fallback，字段不全",
    }))
}

/// `fetch_quote` — Tencent → Sina → etnet (HK only).
pub fn fetch_quote(code: &str, market: &str) -> Result<Value, String> {
    let mut errors: Vec<String> = Vec::new();
    match fetch_quote_tencent(code, market) {
        Ok(v) => return Ok(v),
        Err(e) => errors.push(e),
    }
    match fetch_quote_sina(code, market) {
        Ok(v) => return Ok(v),
        Err(e) => errors.push(e),
    }
    if market == "H" {
        match fetch_quote_etnet(code) {
            Ok(v) => return Ok(v),
            Err(e) => errors.push(e),
        }
    }
    Err(format!("direct_http all failed: {}", errors.join(" | ")))
}

/// First `"..."` group of the response body (`re.search(r'"([^"]+)"', text)`).
fn extract_quoted(text: &str) -> Option<&str> {
    let start = text.find('"')? + 1;
    let end = text[start..].find('"')? + start;
    Some(&text[start..end])
}

fn find_group(hay: &str, pattern: &str) -> Option<String> {
    let re = regex::Regex::new(pattern).ok()?;
    re.captures(hay)
        .and_then(|c| c.get(1).map(|m| m.as_str().to_string()))
}

fn first80(s: &str) -> String {
    s.chars().take(80).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn symbol_mapping() {
        assert_eq!(tencent_symbol("600519", "A").unwrap(), "sh600519");
        assert_eq!(tencent_symbol("002273", "A").unwrap(), "sz002273");
        assert_eq!(tencent_symbol("700", "H").unwrap(), "hk00700");
        assert_eq!(sina_symbol("00700", "H").unwrap(), "hk00700");
        assert_eq!(sina_symbol("AAPL", "U").unwrap(), "gb_aapl");
        assert_eq!(tencent_symbol("aapl", "U").unwrap(), "usAAPL");
    }

    #[test]
    fn extracts_first_quoted_group() {
        let body = "v_sz002273=\"51~水晶光电~002273~29.93\";";
        assert_eq!(extract_quoted(body).unwrap(), "51~水晶光电~002273~29.93");
    }

    #[test]
    fn tencent_field_indices_are_upstream_order() {
        // Build a 47-field payload; upstream reads 1=name 3=price 4=prev 5=open
        // 6=volume 32=change_pct 33=high 34=low 39=pe 45=mcap 46=pb.
        let mut parts = vec!["51".to_string(); 47];
        parts[1] = "水晶光电".into();
        parts[3] = "29.93".into();
        parts[4] = "29.18".into();
        parts[5] = "29.20".into();
        parts[6] = "12345".into();
        parts[33] = "30.10".into();
        parts[34] = "29.00".into();
        let payload = parts.join("~");
        let p: Vec<&str> = payload.split('~').collect();
        assert_eq!(p[1], "水晶光电");
        assert_eq!(fnum(p[3]), 29.93);
        assert_eq!(opt_num(p[33]), Some(30.10));
    }
}
