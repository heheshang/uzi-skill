//! Port of `lib/update_check.py` — GitHub release check for a newer version.
//!
//! Behaviour is upstream's, including the parts that exist to avoid nagging:
//!
//! * silently skipped when `UZI_NO_UPDATE_CHECK=1`, when stdin is not a TTY
//!   (CI / sandboxes / piped output), or when the GitHub API fails;
//! * the newest release is cached for 6h so the 60 req/h unauthenticated API
//!   limit cannot be hit by repeated runs;
//! * "skip this version" persists until a *different* newer release appears;
//! * the check never blocks or fails the main flow.
//!
//! The local version is this binary's own crate version, which tracks the
//! upstream release tags (the workspace and the plugin manifest agree on
//! `MAJOR.MINOR.PATCH`).

use std::io::{IsTerminal, Write};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{json, Value};

use uzi_core::cache::cache_root;

/// `GITHUB_REPO`.
pub const GITHUB_REPO: &str = "heheshang/uzi-skill";
/// `CACHE_TTL_SEC` — 6h, to stay under the unauthenticated GitHub API limit.
pub const CACHE_TTL_SEC: f64 = 6.0 * 3600.0;
/// `HTTP_TIMEOUT` — fail fast and let the flow continue.
pub const HTTP_TIMEOUT: u64 = 5;

/// This implementation's version, the analogue of reading `plugin.json`.
pub const LOCAL_VERSION: &str = env!("CARGO_PKG_VERSION");

/// `UpdateInfo` — a newer release worth telling the user about.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateInfo {
    pub current: String,
    pub latest: String,
    /// Release body, truncated to 600 chars like upstream.
    pub notes: String,
    pub url: String,
}

/// `_cache_path()` — `.cache/_global/update_check.json`.
pub fn state_path() -> PathBuf {
    cache_root().join("_global").join("update_check.json")
}

/// `_read_local_version()`.
pub fn local_version() -> String {
    LOCAL_VERSION.to_string()
}

/// `_parse_semver(v)` — `'2.13.7'` / `'v2.13.7'` → `(2, 13, 7)`.
///
/// Pre-release and malformed versions yield `None`, which makes the comparison
/// conservative (never "newer").
pub fn parse_semver(v: &str) -> Option<(u64, u64, u64)> {
    let v = v.trim();
    let v = v.strip_prefix('v').unwrap_or(v);
    // Each component must be `\d+` like upstream's regex: ASCII digits only, so
    // `+1`, `1e3`, and `0x1` are rejected the way Python rejects them.
    let digit_part = |s: Option<&str>| -> Option<u64> {
        let s = s?;
        if s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        s.parse::<u64>().ok()
    };
    let mut parts = v.split('.');
    let major = digit_part(parts.next())?;
    let minor = digit_part(parts.next())?;
    let patch = digit_part(parts.next())?;
    if parts.next().is_some() {
        return None;
    }
    Some((major, minor, patch))
}

/// `_newer(latest, current)`.
pub fn is_newer(latest: &str, current: &str) -> bool {
    match (parse_semver(latest), parse_semver(current)) {
        (Some(l), Some(c)) => l > c,
        _ => false,
    }
}

fn now_secs() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

/// `_load_state()` — a corrupt or absent state file reads as empty.
pub fn load_state() -> Value {
    std::fs::read_to_string(state_path())
        .ok()
        .and_then(|t| serde_json::from_str::<Value>(&t).ok())
        .unwrap_or_else(|| json!({}))
}

/// `_save_state(state)` — write failures are ignored, as upstream does.
pub fn save_state(state: &Value) {
    let path = state_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(text) = serde_json::to_string_pretty(state) {
        let _ = std::fs::write(path, text);
    }
}

/// `mark_skipped(version)` — stop prompting until a different release appears.
pub fn mark_skipped(version: &str) {
    let mut state = load_state();
    if let Some(obj) = state.as_object_mut() {
        obj.insert("skipped_version".into(), Value::from(version));
    }
    save_state(&state);
}

fn state_str(state: &Value, key: &str) -> String {
    state.get(key).and_then(|v| v.as_str()).unwrap_or("").to_string()
}

fn state_f64(state: &Value, key: &str) -> f64 {
    state.get(key).and_then(|v| v.as_f64()).unwrap_or(0.0)
}

/// `_fetch_latest_release()` — `GET /releases/latest`, `None` on any failure.
pub fn fetch_latest_release() -> Option<Value> {
    let url = format!("https://api.github.com/repos/{GITHUB_REPO}/releases/latest");
    let resp = uzi_data::http::get(
        &url,
        &[
            ("Accept", "application/vnd.github+json"),
            ("User-Agent", "UZI-Skill-update-check"),
        ],
        HTTP_TIMEOUT,
    )
    .ok()?;
    if !resp.is_ok() {
        return None;
    }
    serde_json::from_str::<Value>(&resp.text()).ok()
}

/// The main entry point. `None` means "nothing to tell the user".
///
/// `force` bypasses the 6h cache and the skip marker; the caller supplies the
/// clock so tests stay deterministic.
pub fn check_for_update_at(force: bool, now: f64) -> Option<UpdateInfo> {
    if std::env::var("UZI_NO_UPDATE_CHECK").map(|v| v == "1").unwrap_or(false) {
        return None;
    }

    let current = local_version();
    if current.is_empty() {
        return None;
    }

    let mut state = load_state();

    if !force {
        let last = state_f64(&state, "last_check_at");
        let cached_latest = state_str(&state, "cached_latest");
        if now - last < CACHE_TTL_SEC && !cached_latest.is_empty() {
            // Cache still fresh: answer from it when there is nothing to show.
            if !is_newer(&cached_latest, &current) {
                return None;
            }
            if state_str(&state, "skipped_version") == cached_latest {
                return None;
            }
            // Worth prompting, but the cached entry has no release body —
            // fall through and fetch it.
        }
    }

    let rel = fetch_latest_release();
    if let Some(obj) = state.as_object_mut() {
        obj.insert("last_check_at".into(), Value::from(now));
    }
    let Some(rel) = rel else {
        save_state(&state);
        return None;
    };

    let latest = rel
        .get("tag_name")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim_start_matches('v')
        .to_string();
    if latest.is_empty() {
        save_state(&state);
        return None;
    }

    if let Some(obj) = state.as_object_mut() {
        obj.insert("cached_latest".into(), Value::from(latest.clone()));
    }
    save_state(&state);

    if !is_newer(&latest, &current) {
        return None;
    }
    if !force && state_str(&state, "skipped_version") == latest {
        return None;
    }

    let mut notes = rel
        .get("body")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    if notes.chars().count() > 600 {
        notes = notes.chars().take(600).collect::<String>() + "…";
    }

    let url = rel
        .get("html_url")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| format!("https://github.com/{GITHUB_REPO}/releases/tag/v{latest}"));

    Some(UpdateInfo {
        current,
        latest,
        notes,
        url,
    })
}

/// [`check_for_update_at`] against the wall clock.
pub fn check_for_update(force: bool) -> Option<UpdateInfo> {
    check_for_update_at(force, now_secs())
}

/// `format_prompt(info)` — shared by `run.py` and the session-start hook.
pub fn format_prompt(info: &UpdateInfo) -> String {
    format!(
        "\n📦 UZI-Skill 有新版本可更新：v{} → v{}\n   {}\n\n更新内容（前 600 字）：\n{}\n\n选项：\n  [y] 是，我现在去更新（查看 README 安装章节的更新命令）\n  [s] 跳过本版（v{} 之后有更新再提示）\n  [n] 否，下次启动再问\n",
        info.current, info.latest, info.url, info.notes, info.latest
    )
}

/// `handle_answer(answer, latest)` — normalises the reply and returns feedback.
pub fn handle_answer(answer: &str, latest: &str) -> String {
    let a = answer.trim().to_lowercase();
    if a == "s" || a == "skip" || a == "跳过" {
        mark_skipped(latest);
        return format!("✓ 已跳过 v{latest}，后续有更新版本再提示");
    }
    if a == "y" || a == "yes" || a == "是" {
        return "→ 请按 README 里你当前 agent 的更新命令操作：\n  Claude Code: /plugin update stock-deep-analyzer\n  git clone: cd UZI-Skill && git pull\n  Hermes: hermes skills update heheshang/uzi-skill/skills/deep-analysis".to_string();
    }
    "→ 好的，下次启动再问".to_string()
}

/// `run.py::maybe_prompt_update()` — notify and ask, or stay quiet.
///
/// Returns the feedback line when the user answered, so callers (and tests) can
/// observe the outcome. Non-TTY callers return immediately, mirroring
/// upstream's `if not sys.stdin.isatty(): return`.
pub fn maybe_prompt_update() -> Option<String> {
    maybe_prompt_update_with(std::io::stdin().is_terminal())
}

/// Write (or remove) the update prompt file for `info`.
///
/// The pure half of [`write_update_prompt`]: `Some` writes the prompt, `None`
/// removes any existing file so a stale prompt cannot resurface next session.
pub fn write_prompt_file(
    path: &std::path::Path,
    info: Option<&UpdateInfo>,
) -> Option<String> {
    match info {
        Some(info) => {
            let body = format!(
                "{}\n[agent] 请把上面消息展示给用户，收到 y/s/n 后用 `uzi --update-answer <y|s|n> {}` 处理。\n",
                format_prompt(info),
                info.latest
            );
            if let Some(parent) = path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            let _ = std::fs::write(path, &body);
            Some(body)
        }
        None => {
            let _ = std::fs::remove_file(path);
            None
        }
    }
}

/// Check for an update and reflect it into the session-hook prompt file.
///
/// Returns the prompt text when one was written.
pub fn write_update_prompt(path: &std::path::Path) -> Option<String> {
    let info = check_for_update(false);
    write_prompt_file(path, info.as_ref())
}

/// `handle_answer` applied from the CLI, persisting a skip marker.
pub fn apply_answer(answer: &str, latest: &str) -> String {
    handle_answer(answer, latest)
}

/// Testable form of [`maybe_prompt_update`] with an explicit TTY answer.
pub fn maybe_prompt_update_with(interactive: bool) -> Option<String> {
    if !interactive {
        return None;
    }
    let info = check_for_update(false)?;
    println!("{}", format_prompt(&info));
    print!("请选择 [y/s/n]（回车默认 n）: ");
    let _ = std::io::stdout().flush();

    let mut line = String::new();
    if std::io::stdin().read_line(&mut line).is_err() {
        return None;
    }
    let feedback = handle_answer(&line, &info.latest);
    println!("{feedback}\n");
    Some(feedback)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn semver_parsing_accepts_plain_and_prefixed_versions() {
        assert_eq!(parse_semver("2.13.7"), Some((2, 13, 7)));
        assert_eq!(parse_semver("v2.13.7"), Some((2, 13, 7)));
        assert_eq!(parse_semver("  v3.9.4  "), Some((3, 9, 4)));
        assert_eq!(parse_semver("1.0.0"), Some((1, 0, 0)));
    }

    #[test]
    fn semver_parsing_rejects_prereleases_and_junk() {
        // Upstream's `re.match(r"v?\d+\.\d+\.\d+$")` anchors on a full triple.
        assert_eq!(parse_semver("2.13.7-rc1"), None);
        assert_eq!(parse_semver("2.13"), None);
        assert_eq!(parse_semver("2.13.7.1"), None);
        assert_eq!(parse_semver(""), None);
        assert_eq!(parse_semver("vv2.1.1"), None);
        assert_eq!(parse_semver("latest"), None);
        // `\d+` means ASCII digits: no sign, no exponent, no hex.
        assert_eq!(parse_semver("+2.1.1"), None);
        assert_eq!(parse_semver("2.1e3.1"), None);
        assert_eq!(parse_semver("2.0x1.1"), None);
        assert_eq!(parse_semver("2..1"), None);
    }

    #[test]
    fn comparison_is_pairwise_on_all_three_components() {
        assert!(is_newer("3.9.5", "3.9.4"));
        assert!(is_newer("3.10.0", "3.9.9"));
        assert!(is_newer("4.0.0", "3.99.99"));
        assert!(!is_newer("3.9.4", "3.9.4"));
        assert!(!is_newer("3.9.3", "3.9.4"));
    }

    #[test]
    fn unparseable_versions_are_never_newer() {
        // Conservative on both sides: a bad local or remote version means "no".
        assert!(!is_newer("2.0.0-rc1", "1.0.0"));
        assert!(!is_newer("2.0.0", "1.0.0-rc1"));
        assert!(!is_newer("", "1.0.0"));
    }

    #[test]
    fn prompt_template_matches_upstream_verbatim() {
        let info = UpdateInfo {
            current: "3.9.4".into(),
            latest: "3.10.0".into(),
            notes: "修复若干问题".into(),
            url: "https://example.invalid/r".into(),
        };
        let prompt = format_prompt(&info);
        assert!(prompt.contains("📦 UZI-Skill 有新版本可更新：v3.9.4 → v3.10.0"));
        assert!(prompt.contains("   https://example.invalid/r"));
        assert!(prompt.contains("修复若干问题"));
        assert!(prompt.contains("[y] 是，我现在去更新"));
        assert!(prompt.contains("[s] 跳过本版（v3.10.0 之后有更新再提示）"));
        assert!(prompt.contains("[n] 否，下次启动再问"));
    }

    #[test]
    fn answers_are_normalised_case_insensitively() {
        // "y"/"YES"/"是" all take the update branch; nothing is persisted.
        for a in ["y", "Y", "yes", "YES", "是", "  是  "] {
            let msg = handle_answer(a, "9.9.9");
            assert!(msg.contains("请按 README"), "{a:?} → {msg}");
        }
        // Anything else defers to next launch.
        for a in ["", "n", "no", "随便", "q"] {
            assert_eq!(handle_answer(a, "9.9.9"), "→ 好的，下次启动再问", "{a:?}");
        }
    }

    #[test]
    fn local_version_is_this_crate_version() {
        assert_eq!(local_version(), env!("CARGO_PKG_VERSION"));
        assert!(parse_semver(&local_version()).is_some());
    }

    #[test]
    fn non_interactive_runs_never_prompt() {
        // Mirrors `if not sys.stdin.isatty(): return` — CI and pipes stay quiet
        // without touching the network or the state file.
        assert!(maybe_prompt_update_with(false).is_none());
    }

    /// The session hook's contract: the file's existence *is* the signal.
    #[test]
    fn prompt_file_is_written_for_an_update_and_removed_without_one() {
        let dir = std::env::temp_dir().join(format!("uzi-upd-prompt-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("nested").join("update_prompt.md");

        let info = UpdateInfo {
            current: "3.9.4".into(),
            latest: "3.10.0".into(),
            notes: "修复若干问题".into(),
            url: "https://example.invalid/r".into(),
        };

        // A newer release writes the prompt, creating parents as needed.
        let body = write_prompt_file(&path, Some(&info)).expect("prompt written");
        assert!(path.exists());
        assert!(body.contains("v3.9.4 → v3.10.0"), "{body}");
        assert!(body.contains("--update-answer"), "{body}");
        // The version is embedded so the agent can pass it straight back.
        assert!(body.contains("--update-answer <y|s|n> 3.10.0"), "{body}");

        // No update must *remove* it, not leave a stale prompt behind.
        assert!(write_prompt_file(&path, None).is_none());
        assert!(
            !path.exists(),
            "a stale prompt would be shown again next session"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Removing a file that is already absent is not an error.
    #[test]
    fn prompt_file_removal_tolerates_a_missing_file() {
        let path = std::env::temp_dir().join(format!("uzi-upd-absent-{}.md", std::process::id()));
        let _ = std::fs::remove_file(&path);
        assert!(write_prompt_file(&path, None).is_none());
        assert!(!path.exists());
    }

    #[test]
    fn answer_application_persists_a_skip_marker() {
        let tmp = std::env::temp_dir().join(format!("uzi-upd-skip-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::env::set_var("UZI_CACHE_ROOT", &tmp);

        let msg = apply_answer("s", "9.9.9");
        assert!(msg.contains("已跳过"), "{msg}");
        assert_eq!(load_state()["skipped_version"], json!("9.9.9"));

        // "y" and "n" must not persist a skip.
        std::fs::remove_file(state_path()).ok();
        assert!(apply_answer("y", "9.9.9").contains("README"));
        assert!(load_state().get("skipped_version").is_none());

        std::env::remove_var("UZI_CACHE_ROOT");
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
