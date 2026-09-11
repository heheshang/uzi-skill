//! Port of `render_share_card.py` / `render_war_report.py`.
//!
//! Upstream screenshots `#share-card` / `#war-report` with Playwright +
//! headless Chromium to produce 1080×1920 / 1920×1080 PNGs. The Rust port has
//! no browser dependency: it emits the report's **HTML source** to disk instead
//! (the same document the browser would have rendered), with a `.html`
//! extension in place of the `.png` name.

use anyhow::{anyhow, Context};
use std::path::{Path, PathBuf};

/// Render `out_name` for `ticker` by writing the source HTML.
pub fn render(
    ticker: &str,
    selector: &str,
    out_name: &str,
    scale: u32,
) -> anyhow::Result<PathBuf> {
    let _ = (selector, scale);
    let report_dir = crate::inline::report_dir(ticker)?;
    let html_path = report_dir.join("full-report.html");
    if !html_path.exists() {
        return Err(anyhow!(
            "{} not found. Run `uzi <ticker> --stage2` first.",
            html_path.display()
        ));
    }
    let html = std::fs::read_to_string(&html_path)
        .with_context(|| format!("reading {}", html_path.display()))?;
    let out_path = report_dir.join(html_out_name(out_name));
    std::fs::write(&out_path, html)?;
    Ok(out_path)
}

fn html_out_name(out_name: &str) -> String {
    match Path::new(out_name).file_stem() {
        Some(stem) => format!("{}.html", stem.to_string_lossy()),
        None => format!("{out_name}.html"),
    }
}

/// `render_share_card.render` alias (upstream also exposes `main`).
pub fn main_share(ticker: &str) -> anyhow::Result<PathBuf> {
    render(ticker, "#share-card", "share-card.png", 2)
}

/// `render_war_report.main` — the 1920×1080 horizontal card.
pub fn main_war(ticker: &str) -> anyhow::Result<PathBuf> {
    render(ticker, "#war-report", "war-report.png", 2)
}
