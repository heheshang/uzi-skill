//! Shipped-asset contract — where the report assets live, and the marker the
//! renderer must find in the template.
//!
//! Upstream derived this path from `__file__` (`ASSETS_DIR = SCRIPTS_DIR.parent
//! / "assets"`); a compiled binary has no `__file__`. Resolution order:
//!
//! 1. `UZI_ASSETS_DIR`
//! 2. `UZI_REPO_ROOT/assets`
//! 3. walk up from the current directory until `assets/report-template.html` exists
//! 4. walk up from the executable's own directory (see [`repo_root`])
//!
//! Every step is resolved at runtime, so a binary copied anywhere still finds the
//! assets it ships beside. Only shipped files under `assets/` are read — a reader
//! without the Rust source tree resolves them like any other user. Keep it that
//! way: the self-review gate used to probe `crates/…/assemble_report.rs`, a file
//! that no longer exists, so the check silently no-opped everywhere while baking
//! the builder's checkout path into the binary.

use std::path::PathBuf;

/// Directory holding the report assets (`report-template.html`, `avatars/`, …).
pub fn assets_dir() -> PathBuf {
    if let Ok(p) = std::env::var("UZI_ASSETS_DIR") {
        if !p.is_empty() {
            return PathBuf::from(p);
        }
    }
    if let Ok(p) = std::env::var("UZI_REPO_ROOT") {
        if !p.is_empty() {
            return PathBuf::from(p).join("assets");
        }
    }
    let mut dir = std::env::current_dir().unwrap_or_default();
    loop {
        let cand = dir.join("assets");
        if cand.join("report-template.html").is_file() {
            return cand;
        }
        if !dir.pop() {
            break;
        }
    }
    if let Some(repo) = repo_root() {
        return repo.join("assets");
    }
    PathBuf::from("assets")
}

/// Repository root, derived at **runtime** from the executable's own location.
///
/// Marker: `assets/report-template.html`. A binary at `<repo>/uzi` (the shipped
/// one) or `<repo>/target/release/uzi` (a cargo build) resolves to `<repo>`
/// whichever cwd it is started from.
///
/// Deliberately not `env!("CARGO_MANIFEST_DIR")`: that bakes the *builder's*
/// absolute checkout path into the binary — useless on any other machine, and a
/// personal-path leak in a shipped artifact.
pub fn repo_root() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let mut dir = exe.parent()?.to_path_buf();
    loop {
        if dir.join("assets/report-template.html").is_file() {
            return Some(dir);
        }
        if !dir.pop() {
            return None;
        }
    }
}

/// The report template the assembly stage renders.
pub fn report_template() -> PathBuf {
    assets_dir().join("report-template.html")
}

/// Injection point in the template that the renderer replaces with the rendered
/// panel-insights block. Shared so the renderer and the self-review gate cannot
/// drift apart: the gate asserts the shipped template still carries it.
pub const PANEL_INSIGHTS_MARKER: &str = "<!-- INJECT_PANEL_INSIGHTS -->";
