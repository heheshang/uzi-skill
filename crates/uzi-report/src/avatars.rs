//! Port of `gen_pixel_avatars.py` — deterministic pixel-art avatar generator.
//!
//! Upstream first tries the DiceBear v9 HTTP API and falls back to a hash-based
//! identicon when offline. The Rust render layer makes no network calls, so
//! [`fetch_one`] always returns `None` and [`fallback_svg`] (byte-identical to
//! the upstream offline identicon) is always used.

use md5::{Digest, Md5};
use serde_json::Value;
use std::path::Path;

/// Upstream DiceBear endpoint (documented; not called by the port).
pub const DICEBEAR_BASE: &str = "https://api.dicebear.com/9.x/pixel-art/svg";

/// Upstream performs the DiceBear request here; the port is offline-only.
pub fn fetch_one(_seed: &str, _timeout: u64) -> Option<String> {
    None
}

/// Tiny offline identicon — colored grid based on seed hash, deterministic.
pub fn fallback_svg(seed: &str) -> String {
    let digest = Md5::digest(seed.as_bytes());
    let h: [u8; 16] = digest.into();
    let hue = h[0] as i64 * 360 / 256;
    let sat = 60 + (h[1] % 30) as i64;
    let mut cells: Vec<String> = Vec::new();
    for y in 0..8 {
        for x in 0..4usize {
            if h[(y * 4 + x) % 16] & (1 << (x % 8)) != 0 {
                let color = format!("hsl({hue}, {sat}%, 55%)");
                cells.push(format!(
                    r##"<rect x="{}" y="{}" width="16" height="16" fill="{color}"/>"##,
                    x * 16,
                    y * 16
                ));
                cells.push(format!(
                    r##"<rect x="{}" y="{}" width="16" height="16" fill="{color}"/>"##,
                    (7 - x) * 16,
                    y * 16
                ));
            }
        }
    }
    format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 128 128" width="128" height="128">
<rect width="128" height="128" fill="#0a0e17"/>
{}
</svg>"##,
        cells.concat()
    )
}

/// Generate missing `{id}.svg` files for `investors`; returns the count written.
pub fn generate(investors: &Value, output_dir: &Path) -> std::io::Result<usize> {
    std::fs::create_dir_all(output_dir)?;
    let list = match investors.as_array() {
        Some(a) => a.clone(),
        None => Vec::new(),
    };
    let mut written = 0;
    for inv in list {
        let id = match inv.get("id").and_then(|v| v.as_str()) {
            Some(id) => id.to_string(),
            None => continue,
        };
        let out = output_dir.join(format!("{id}.svg"));
        if let Ok(meta) = std::fs::metadata(&out) {
            if meta.len() > 100 {
                continue;
            }
        }
        let seed = match inv.get("avatar_seed").and_then(|v| v.as_str()) {
            Some(s) if !s.is_empty() => s.to_string(),
            _ => id.clone(),
        };
        let svg = match fetch_one(&seed, 12) {
            Some(s) => s,
            None => fallback_svg(&seed),
        };
        std::fs::write(&out, svg)?;
        written += 1;
    }
    Ok(written)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fallback_is_deterministic_and_hashed() {
        let a = fallback_svg("zhangkun");
        assert_eq!(a, fallback_svg("zhangkun"));
        assert!(a.starts_with("<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 128 128\""));
        assert!(a.contains("fill=\"#0a0e17\""));
        // python3 -c "import hashlib;h=hashlib.md5(b'zhangkun').digest();print(h[0],h[1],h[0]*360//256,60+h[1]%30)"
        let d = Md5::digest(b"zhangkun");
        let hue = d[0] as i64 * 360 / 256;
        let sat = 60 + (d[1] % 30) as i64;
        assert!(a.contains(&format!("hsl({hue}, {sat}%, 55%)")));
    }
}
