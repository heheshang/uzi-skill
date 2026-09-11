//! Port of `inline_assets.py` — bundle a generated full-report.html into a
//! single self-contained file by inlining `avatars/*.svg` as data URIs.

use anyhow::{anyhow, Context};
use regex::Regex;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

/// Base64 (standard alphabet, `=` padded).
fn base64_encode(data: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = *chunk.get(1).unwrap_or(&0) as u32;
        let b2 = *chunk.get(2).unwrap_or(&0) as u32;
        let n = (b0 << 16) | (b1 << 8) | b2;
        out.push(TABLE[((n >> 18) & 63) as usize] as char);
        out.push(TABLE[((n >> 12) & 63) as usize] as char);
        if chunk.len() > 1 {
            out.push(TABLE[((n >> 6) & 63) as usize] as char);
        } else {
            out.push('=');
        }
        if chunk.len() > 2 {
            out.push(TABLE[(n & 63) as usize] as char);
        } else {
            out.push('=');
        }
    }
    out
}

/// Reports output root (`UZI_REPORTS_DIR` overrides the upstream `reports`).
pub fn reports_dir() -> PathBuf {
    match std::env::var("UZI_REPORTS_DIR") {
        Ok(p) if !p.is_empty() => PathBuf::from(p),
        _ => PathBuf::from("reports"),
    }
}

fn today() -> String {
    chrono::Local::now().format("%Y%m%d").to_string()
}

/// Locate `reports/{ticker}_{YYYYMMDD}` (falling back to the newest match).
pub fn report_dir(ticker: &str) -> anyhow::Result<PathBuf> {
    let base = reports_dir();
    let today_dir = base.join(format!("{}_{}", ticker, today()));
    if today_dir.exists() {
        return Ok(today_dir);
    }
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&base) {
        for e in entries.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            if name.starts_with(&format!("{ticker}_")) {
                candidates.push(e.path());
            }
        }
    }
    if candidates.is_empty() {
        return Err(anyhow!("No report dir for {ticker}"));
    }
    candidates.sort();
    Ok(candidates.pop().unwrap())
}

/// Inline avatars into `full-report-standalone.html`; returns that path.
pub fn inline_assets(ticker: &str) -> anyhow::Result<PathBuf> {
    let report_dir = report_dir(ticker)?;
    let html_path = report_dir.join("full-report.html");
    if !html_path.exists() {
        return Err(anyhow!("{} missing", html_path.display()));
    }
    let avatars_dir = report_dir.join("avatars");
    let html = std::fs::read_to_string(&html_path)
        .with_context(|| format!("reading {}", html_path.display()))?;

    static RE: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r#"src="(avatars/[^"]+)""#).unwrap());
    let inlined = RE
        .replace_all(&html, |caps: &regex::Captures| {
            let src = &caps[1];
            if !src.starts_with("avatars/") {
                return caps[0].to_string();
            }
            let avatar_name = &src["avatars/".len()..];
            let avatar_path = avatars_dir.join(avatar_name);
            match std::fs::read(&avatar_path) {
                Ok(bytes) => {
                    format!(r#"src="data:image/svg+xml;base64,{}""#, base64_encode(&bytes))
                }
                Err(_) => caps[0].to_string(),
            }
        })
        .into_owned();

    let out = report_dir.join("full-report-standalone.html");
    std::fs::write(&out, &inlined).with_context(|| format!("writing {}", out.display()))?;
    let _ = (ticker, Path::new(&out));
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_matches_python() {
        // python3 -c "import base64;print(base64.b64encode(b'<svg/>').decode())"
        // -> PHN2Zy8+
        assert_eq!(base64_encode(b"<svg/>"), "PHN2Zy8+");
        // base64.b64encode(b'abc') -> YWJj
        assert_eq!(base64_encode(b"abc"), "YWJj");
        // base64.b64encode(b'ab') -> YWI=
        assert_eq!(base64_encode(b"ab"), "YWI=");
    }
}
