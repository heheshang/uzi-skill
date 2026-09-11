//! Chromium discovery, launch, and Chrome DevTools Protocol session handling.
//!
//! This replaces upstream's Playwright dependency. Playwright would have brought
//! a bundled browser and a Python runtime; instead this talks to any installed
//! Chromium over CDP, which is plain WebSocket JSON on loopback (see
//! [`super::ws`]).
//!
//! Lifecycle:
//!
//! 1. [`find_chromium`] locates a browser binary (system Chrome, Chrome for
//!    Testing, Chromium, Edge).
//! 2. [`Browser::launch`] spawns it headless with `--remote-debugging-port`, waits
//!    for the endpoint to answer, and opens a WebSocket to the **browser** target.
//! 3. [`Browser::new_page`] creates a tab and attaches with `flatten: true`, which
//!    returns a `sessionId`; every later command carries it.
//! 4. [`Browser::navigate`] returns the response status and the JS-rendered DOM —
//!    upstream's `page.content()`.
//!
//! Cookies live in `--user-data-dir`, so a persistent profile keeps a login
//! across runs, matching Playwright's `launch_persistent_context`.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

pub mod base64;
pub mod fallback;
pub mod xueqiu;
pub mod sha1;
pub mod ws;

use ws::{Ws, WsError};

/// Default navigation budget, matching upstream's `DEFAULT_TIMEOUT`.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Debug)]
pub enum BrowserError {
    /// No Chromium-family browser could be found.
    NotFound(String),
    /// The browser process started but never exposed CDP, or exited early.
    LaunchFailed(String),
    /// A CDP command returned an error object.
    Cdp(String),
    /// A read exceeded its budget. No bytes were consumed, so the session stays
    /// usable.
    Timeout,
    Ws(WsError),
    Io(std::io::Error),
}

impl std::fmt::Display for BrowserError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BrowserError::NotFound(s) => write!(f, "no chromium browser found: {s}"),
            BrowserError::LaunchFailed(s) => write!(f, "browser launch failed: {s}"),
            BrowserError::Cdp(s) => write!(f, "cdp error: {s}"),
            BrowserError::Timeout => write!(f, "cdp read timed out"),
            BrowserError::Ws(e) => write!(f, "{e}"),
            BrowserError::Io(e) => write!(f, "io: {e}"),
        }
    }
}

impl std::error::Error for BrowserError {}

impl From<WsError> for BrowserError {
    fn from(e: WsError) -> Self {
        BrowserError::Ws(e)
    }
}

impl From<std::io::Error> for BrowserError {
    fn from(e: std::io::Error) -> Self {
        BrowserError::Io(e)
    }
}

type Result<T, E = BrowserError> = std::result::Result<T, E>;

/// How to launch the browser.
#[derive(Debug, Clone)]
pub struct LaunchOptions {
    /// Headed mode is required for the interactive XueQiu login.
    pub headless: bool,
    /// Persistent profile directory. Cookies live here, so reusing it keeps a
    /// login across runs.
    pub profile_dir: Option<PathBuf>,
    pub user_agent: Option<String>,
    pub viewport: Option<(u32, u32)>,
    /// Explicit binary; otherwise [`find_chromium`] is consulted.
    pub executable: Option<PathBuf>,
}

impl Default for LaunchOptions {
    fn default() -> Self {
        LaunchOptions {
            headless: true,
            profile_dir: None,
            user_agent: None,
            viewport: None,
            executable: None,
        }
    }
}

/// Candidate browser locations, most specific first.
///
/// `CHROME_PATH`/`CHROMIUM_PATH` always win, which is how a caller points at a
/// browser without a system install (CI images, sandboxes).
pub fn chromium_candidates() -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = Vec::new();

    for var in ["UZI_CHROME_PATH", "CHROME_PATH", "CHROMIUM_PATH"] {
        if let Ok(p) = std::env::var(var) {
            if !p.is_empty() {
                out.push(PathBuf::from(p));
            }
        }
    }

    // macOS application bundles.
    for app in [
        "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
        "/Applications/Chromium.app/Contents/MacOS/Chromium",
        "/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge",
        "/Applications/Brave Browser.app/Contents/MacOS/Brave Browser",
    ] {
        out.push(PathBuf::from(app));
    }

    // Anything on PATH.
    for name in [
        "google-chrome",
        "google-chrome-stable",
        "chromium",
        "chromium-browser",
        "microsoft-edge",
    ] {
        if let Some(p) = which(name) {
            out.push(p);
        }
    }

    // Puppeteer's cache, including the harness's own Chrome for Testing.
    if let Some(home) = std::env::var_os("HOME") {
        let home = PathBuf::from(home);
        for root in [home.join(".cache/puppeteer"), home.join(".omp/puppeteer")] {
            out.extend(puppeteer_chromes(&root));
        }
    }

    out
}

/// Chrome-for-Testing builds under a Puppeteer cache root.
fn puppeteer_chromes(root: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let Ok(entries) = std::fs::read_dir(root) else {
        return found;
    };
    for entry in entries.flatten() {
        let dir = entry.path().join("chrome");
        let Ok(builds) = std::fs::read_dir(&dir) else {
            continue;
        };
        let mut builds: Vec<PathBuf> = builds.flatten().map(|e| e.path()).collect();
        // Newest build first.
        builds.sort();
        builds.reverse();
        for build in builds {
            for rel in [
                "chrome-mac-arm64/Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing",
                "chrome-mac-x64/Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing",
                "chrome-linux64/chrome",
                "chrome-headless-shell-linux64/chrome-headless-shell",
            ] {
                let candidate = build.join(rel);
                if candidate.is_file() {
                    found.push(candidate);
                }
            }
        }
    }
    found
}

/// First existing, executable candidate.
pub fn find_chromium() -> Result<PathBuf> {
    let candidates = chromium_candidates();
    for path in &candidates {
        if path.is_file() {
            return Ok(path.clone());
        }
    }
    Err(BrowserError::NotFound(format!(
        "looked in {} locations (set UZI_CHROME_PATH to override)",
        candidates.len()
    )))
}

/// Minimal `which` — resolves a bare name against `PATH`.
fn which(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|dir| dir.join(name))
        .find(|p| p.is_file())
}

/// An attached CDP page session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Session(pub String);

/// Outcome of a navigation.
#[derive(Debug, Clone)]
pub struct Navigation {
    /// HTTP status of the main-frame response, when the server reported one.
    /// `None` covers `about:blank`, `file://`, and aborted requests.
    pub status: Option<u16>,
    /// `document.documentElement.outerHTML` after load — upstream's
    /// `page.content()`.
    pub html: String,
}

/// A running browser with one CDP WebSocket.
pub struct Browser {
    child: Child,
    ws: Ws,
    /// Monotonic CDP command id.
    id: u64,
    /// Events read while awaiting a command reply.
    ///
    /// CDP interleaves events with replies, and the browser can emit a page's
    /// `Network.responseReceived` *before* replying to `Page.navigate`. Dropping
    /// those events would lose the response status, so they are queued here and
    /// drained by [`Browser::next_event`].
    pending_events: std::collections::VecDeque<Value>,
    /// Kept so an early return still cleans the profile up when the caller asked
    /// for a temporary one.
    _profile: Option<TempProfile>,
    default_timeout: Duration,
}

/// Guards a temporary profile directory, removing it on drop.
struct TempProfile {
    path: PathBuf,
    remove: bool,
}

impl Drop for TempProfile {
    fn drop(&mut self) {
        if self.remove {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }
}

impl Browser {
    /// Launch a browser and connect to its CDP endpoint.
    pub fn launch(options: &LaunchOptions) -> Result<Browser> {
        let exe = match &options.executable {
            Some(p) => p.clone(),
            None => find_chromium()?,
        };
        if !exe.is_file() {
            return Err(BrowserError::NotFound(exe.display().to_string()));
        }

        // Reserve a port by binding and immediately releasing it. The window is
        // tiny and the endpoint is verified below before use.
        let port = free_port()?;

        let (profile_path, remove) = match &options.profile_dir {
            Some(p) => (p.clone(), false),
            None => (
                std::env::temp_dir().join(format!("uzi-chrome-{}-{port}", std::process::id())),
                true,
            ),
        };
        std::fs::create_dir_all(&profile_path)?;

        let mut cmd = Command::new(&exe);
        cmd.arg(format!("--remote-debugging-port={port}"))
            .arg(format!("--user-data-dir={}", profile_path.display()))
            .arg("--no-first-run")
            .arg("--no-default-browser-check")
            .args(["--disable-background-networking", "--disable-sync"])
            .arg("--disable-features=Translate,MediaRouter")
            .arg("--disable-extensions")
            .arg("--window-size=1280,900");
        if options.headless {
            cmd.arg("--headless");
        }
        if let Some(ua) = &options.user_agent {
            cmd.arg(format!("--user-agent={ua}"));
        }
        cmd.arg("about:blank")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .stdin(Stdio::null());

        let mut child = cmd.spawn()?;

        // Wait for the endpoint *before* connecting: the socket is only created
        // once `/json/version` reports a WebSocket URL, so there is no placeholder
        // connection to invalidate.
        let ws_url = match wait_for_endpoint(&mut child, port, Duration::from_secs(30)) {
            Ok(url) => url,
            Err(e) => {
                let _ = child.kill();
                let _ = child.wait();
                if remove {
                    let _ = std::fs::remove_dir_all(&profile_path);
                }
                return Err(e);
            }
        };
        let ws = Ws::connect_ws_url(&ws_url, Duration::from_secs(30))?;

        Ok(Browser {
            child,
            ws,
            id: 1,
            pending_events: std::collections::VecDeque::new(),
            _profile: Some(TempProfile {
                path: profile_path,
                remove,
            }),
            default_timeout: DEFAULT_TIMEOUT,
        })
    }

    /// Create a tab and attach to it.
    pub fn new_page(&mut self) -> Result<Session> {
        let created = self.send(None, "Target.createTarget", json!({"url": "about:blank"}))?;
        let target_id = created
            .get("targetId")
            .and_then(|v| v.as_str())
            .ok_or_else(|| BrowserError::Cdp("createTarget returned no targetId".into()))?
            .to_string();
        let attached = self.send(
            None,
            "Target.attachToTarget",
            json!({"targetId": target_id, "flatten": true}),
        )?;
        let session_id = attached
            .get("sessionId")
            .and_then(|v| v.as_str())
            .ok_or_else(|| BrowserError::Cdp("attachToTarget returned no sessionId".into()))?
            .to_string();

        let session = Session(session_id);
        // Domains must be enabled before navigate so response events arrive.
        for domain in ["Page.enable", "Network.enable", "Runtime.enable"] {
            self.send(Some(&session), domain, json!({}))?;
        }
        Ok(session)
    }

    /// Navigate and return the status plus the rendered DOM.
    ///
    /// Waits for `Page.loadEventFired`, falling back to the DOM once `timeout`
    /// elapses — a page that never finishes loading still yields what rendered,
    /// matching Playwright's `domcontentloaded` tolerance.
    pub fn navigate(&mut self, session: &Session, url: &str, timeout: Duration) -> Result<Navigation> {
        let status = self.navigate_and_wait(session, url, timeout)?;
        let html = self.evaluate_str(
            session,
            "document.documentElement.outerHTML",
        )?;
        Ok(Navigation { status, html })
    }

    /// Fire the navigation and wait for the load event, returning the status.
    fn navigate_and_wait(
        &mut self,
        session: &Session,
        url: &str,
        timeout: Duration,
    ) -> Result<Option<u16>> {
        let nav = self.send(Some(session), "Page.navigate", json!({"url": url}))?;
        if let Some(err) = nav.get("errorText").and_then(|v| v.as_str()) {
            return Err(BrowserError::Cdp(format!("navigation to {url} failed: {err}")));
        }

        let deadline = Instant::now() + timeout;
        let mut status: Option<u16> = None;
        let mut current_doc: Option<String> = None;

        while Instant::now() < deadline {
            let remaining = deadline.saturating_duration_since(Instant::now());
            let msg = match self.next_event(remaining) {
                Ok(m) => m,
                // Read timeout: the page is slow. Stop waiting for the load event
                // but keep the connection — no bytes were consumed.
                Err(BrowserError::Timeout) => break,
                Err(e) => return Err(e),
            };

            match msg.get("method").and_then(|v| v.as_str()) {
                Some("Network.responseReceived") => {
                    // Track the main-frame response for the status code.
                    let params = &msg["params"];
                    let is_main = params
                        .get("type")
                        .and_then(|v| v.as_str())
                        .map(|t| t == "Document")
                        .unwrap_or(false);
                    let frame_id = params.get("frameId").and_then(|v| v.as_str());
                    if is_main && (current_doc.is_none() || frame_id == current_doc.as_deref()) {
                        status = params["response"]["status"].as_u64().map(|s| s as u16);
                        current_doc = frame_id.map(str::to_string);
                    }
                }
                Some("Page.frameNavigated") => {
                    // `about:blank` and error pages report no HTTP status.
                    let url = msg["params"]["frame"]["url"].as_str().unwrap_or("");
                    if url.starts_with("chrome-error://") {
                        status = None;
                    }
                }
                Some("Page.loadEventFired") => break,
                _ => {}
            }
        }
        Ok(status)
    }

    /// Evaluate an expression in the page and return its value.
    pub fn evaluate(&mut self, session: &Session, expression: &str) -> Result<Value> {
        let res = self.send(
            Some(session),
            "Runtime.evaluate",
            json!({
                "expression": expression,
                "returnByValue": true,
                "awaitPromise": true,
            }),
        )?;

        if let Some(details) = res.get("exceptionDetails") {
            let text = details
                .get("exception")
                .and_then(|e| e.get("description"))
                .and_then(|v| v.as_str())
                .unwrap_or("unknown exception");
            return Err(BrowserError::Cdp(format!("evaluate threw: {text}")));
        }
        Ok(res
            .get("result")
            .and_then(|r| r.get("value"))
            .cloned()
            .unwrap_or(Value::Null))
    }

    /// Evaluate an expression expected to produce a string.
    pub fn evaluate_str(&mut self, session: &Session, expression: &str) -> Result<String> {
        Ok(match self.evaluate(session, expression)? {
            Value::String(s) => s,
            Value::Null => String::new(),
            other => other.to_string(),
        })
    }

    /// Wait until `selector` matches an element. Returns whether it appeared;
    /// a timeout is not an error, since upstream keeps the HTML either way.
    pub fn wait_for_selector(
        &mut self,
        session: &Session,
        selector: &str,
        timeout: Duration,
    ) -> Result<bool> {
        let deadline = Instant::now() + timeout;
        let expr = format!("!!document.querySelector({})", json!(selector));
        loop {
            if matches!(self.evaluate(session, &expr)?, Value::Bool(true)) {
                return Ok(true);
            }
            if Instant::now() >= deadline {
                return Ok(false);
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }

    /// `Network.getAllCookies` — the equivalent of Playwright's
    /// `context.cookies()`.
    pub fn cookies(&mut self, session: &Session) -> Result<Vec<Value>> {
        let res = self.send(Some(session), "Network.getAllCookies", json!({}))?;
        Ok(res
            .get("cookies")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default())
    }

    /// `Network.setCookies` — the equivalent of Playwright restoring a saved
    /// cookie jar into a context.
    ///
    /// Each entry needs `name`/`value`; `domain`, `path`, `expires`, `secure`,
    /// and `httpOnly` are forwarded when present. Entries without a usable domain
    /// are skipped by CDP, so unparsable input degrades to "no cookies" rather
    /// than an error.
    pub fn set_cookies(&mut self, session: &Session, cookies: &[Value]) -> Result<()> {
        if cookies.is_empty() {
            return Ok(());
        }
        let params = json!({ "cookies": cookies });
        self.send(Some(session), "Network.setCookies", params)?;
        Ok(())
    }

    /// Send a CDP command and await its result object.
    fn send(&mut self, session: Option<&Session>, method: &str, params: Value) -> Result<Value> {
        let id = self.next_id();
        let payload = match session {
            Some(s) => json!({"id": id, "method": method, "params": params, "sessionId": s.0}),
            None => json!({"id": id, "method": method, "params": params}),
        };
        self.ws.send_text(&payload.to_string())?;

        // Events interleave with the reply; queue them for the caller instead of
        // discarding, since a page's `responseReceived` can precede the reply.
        let deadline = Instant::now() + self.default_timeout.max(Duration::from_secs(30));
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(BrowserError::Cdp(format!("{method}: timed out waiting for reply")));
            }
            let msg = self.next_message(remaining)?;
            if msg.get("id").and_then(|v| v.as_u64()) != Some(id as u64) {
                if msg.get("method").is_some() {
                    self.pending_events.push_back(msg);
                }
                continue;
            }
            if let Some(err) = msg.get("error") {
                return Err(BrowserError::Cdp(format!("{method}: {err}")));
            }
            return Ok(msg.get("result").cloned().unwrap_or(Value::Null));
        }
    }

    /// Next CDP event, preferring any buffered while awaiting a reply.
    ///
    /// `Err(Timeout)` means nothing arrived within `budget`; no bytes were
    /// consumed, so the session remains usable.
    fn next_event(&mut self, budget: Duration) -> std::result::Result<Value, BrowserError> {
        if let Some(event) = self.pending_events.pop_front() {
            return Ok(event);
        }
        loop {
            let msg = self.next_message(budget)?;
            // A stray reply (e.g. to a command we abandoned) is not an event.
            if msg.get("method").is_some() {
                return Ok(msg);
            }
        }
    }

    /// Read one CDP message, bounded by `budget`.
    fn next_message(&mut self, budget: Duration) -> std::result::Result<Value, BrowserError> {
        self.ws.set_read_timeout(budget)?;
        let text = self.ws.recv_text().map_err(|e| match e {
            WsError::Io(io)
                if io.kind() == std::io::ErrorKind::WouldBlock
                    || io.kind() == std::io::ErrorKind::TimedOut =>
            {
                BrowserError::Timeout
            }
            other => BrowserError::Ws(other),
        })?;
        serde_json::from_str(&text)
            .map_err(|e| BrowserError::Cdp(format!("invalid CDP json: {e}")))
    }

    fn next_id(&mut self) -> u64 {
        // Monotonic per connection; only uniqueness matters to CDP.
        let id = self.id;
        self.id += 1;
        id
    }
}

impl Drop for Browser {
    fn drop(&mut self) {
        self.ws.close();
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Poll `/json/version` until the browser answers, returning its WebSocket URL.
///
/// A browser that exits during startup is reported immediately rather than
/// waited on for the full timeout.
fn wait_for_endpoint(child: &mut Child, port: u16, timeout: Duration) -> Result<String> {
    let deadline = Instant::now() + timeout;
    let mut last = String::new();
    while Instant::now() < deadline {
        if let Ok(Some(status)) = child.try_wait() {
            return Err(BrowserError::LaunchFailed(format!(
                "browser exited early ({status}); last probe: {last}"
            )));
        }
        match probe_version(port) {
            Ok(ws) => return Ok(ws),
            Err(e) => last = e,
        }
        std::thread::sleep(Duration::from_millis(150));
    }
    Err(BrowserError::LaunchFailed(format!(
        "CDP endpoint on port {port} never became ready; last probe: {last}"
    )))
}

/// Reserve an ephemeral loopback port.
fn free_port() -> Result<u16> {
    let listener = std::net::TcpListener::bind("127.0.0.1:0")?;
    Ok(listener.local_addr()?.port())
}

/// `GET /json/version` → `webSocketDebuggerUrl`.
fn probe_version(port: u16) -> std::result::Result<String, String> {
    let url = format!("http://127.0.0.1:{port}/json/version");
    let resp = loopback_http_get(&url)?;
    let v: Value = serde_json::from_str(&resp).map_err(|e| format!("bad json: {e}"))?;
    v.get("webSocketDebuggerUrl")
        .and_then(|s| s.as_str())
        .map(str::to_string)
        .ok_or_else(|| "no webSocketDebuggerUrl".to_string())
}

/// One-shot HTTP GET against loopback, used only for the readiness probe.
///
/// Deliberately avoids the crate's HTTP layer: that one applies timeouts,
/// proxies, and retry policy meant for remote hosts, none of which should apply
/// to `127.0.0.1` during startup.
///
/// The body is read by `Content-Length` rather than until EOF: Chrome honours
/// keep-alive and will *not* close the socket, so reading to EOF would block
/// until the read timeout and then discard the bytes already buffered.
fn loopback_http_get(url: &str) -> std::result::Result<String, String> {
    let rest = url.strip_prefix("http://").ok_or("not http")?;
    let (authority, path) = match rest.find('/') {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, "/"),
    };
    let addr = authority
        .parse::<std::net::SocketAddr>()
        .map_err(|e| e.to_string())?;
    let mut stream = std::net::TcpStream::connect_timeout(&addr, Duration::from_secs(2))
        .map_err(|e| e.to_string())?;
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .map_err(|e| e.to_string())?;

    let req = format!(
        "GET {path} HTTP/1.1\r\nHost: {authority}\r\nAccept: application/json\r\n\r\n"
    );
    stream.write_all(req.as_bytes()).map_err(|e| e.to_string())?;
    stream.flush().map_err(|e| e.to_string())?;

    // Read until the header terminator, then exactly Content-Length bytes.
    let mut buf: Vec<u8> = Vec::with_capacity(1024);
    let mut chunk = [0u8; 512];
    let header_end = loop {
        if let Some(pos) = find_subslice(&buf, b"\r\n\r\n") {
            break pos + 4;
        }
        if buf.len() > 64 * 1024 {
            return Err("response headers too large".into());
        }
        let n = stream.read(&mut chunk).map_err(|e| e.to_string())?;
        if n == 0 {
            return Err("connection closed before headers completed".into());
        }
        buf.extend_from_slice(&chunk[..n]);
    };

    let headers = String::from_utf8_lossy(&buf[..header_end]).to_string();
    let status_ok = headers
        .lines()
        .next()
        .map(|l| l.contains(" 200 ") || l.ends_with(" 200"))
        .unwrap_or(false);
    if !status_ok {
        return Err(format!(
            "unexpected status: {}",
            headers.lines().next().unwrap_or("").trim()
        ));
    }

    let content_length: Option<usize> = headers.lines().find_map(|l| {
        let (name, value) = l.split_once(':')?;
        name.eq_ignore_ascii_case("content-length")
            .then(|| value.trim().parse::<usize>().ok())
            .flatten()
    });

    let mut body = buf[header_end..].to_vec();
    if let Some(len) = content_length {
        while body.len() < len {
            let n = stream.read(&mut chunk).map_err(|e| e.to_string())?;
            if n == 0 {
                break;
            }
            body.extend_from_slice(&chunk[..n]);
        }
        body.truncate(len);
    }

    String::from_utf8(body).map_err(|e| e.to_string())
}

/// Index of the first occurrence of `needle` in `haystack`.
fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || haystack.len() < needle.len() {
        return None;
    }
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chromium_candidates_are_ordered_with_env_override_first() {
        let dir = std::env::temp_dir().join(format!("uzi-chrome-cand-{}", std::process::id()));
        let fake = dir.join("chrome");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(&fake, b"#!/bin/sh\n").unwrap();

        std::env::set_var("UZI_CHROME_PATH", &fake);
        let candidates = chromium_candidates();
        assert_eq!(candidates[0], fake, "env override must come first");
        // The platform bundles are always considered.
        assert!(candidates.iter().any(|p| p.to_string_lossy().contains("Google Chrome.app")));

        std::env::remove_var("UZI_CHROME_PATH");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn find_chromium_prefers_a_known_file_and_reports_missing() {
        // A path that cannot exist must produce NotFound, not a panic.
        std::env::set_var("UZI_CHROME_PATH", "/definitely/not/a/browser");
        let previous: Vec<String> = ["CHROME_PATH", "CHROMIUM_PATH"]
            .iter()
            .map(|k| std::env::var(k).unwrap_or_default())
            .collect();
        for k in ["CHROME_PATH", "CHROMIUM_PATH"] {
            std::env::remove_var(k);
        }

        match find_chromium() {
            Ok(path) => assert!(path.is_file(), "must only return existing files"),
            Err(BrowserError::NotFound(msg)) => assert!(msg.contains("UZI_CHROME_PATH")),
            Err(e) => panic!("unexpected error: {e}"),
        }

        std::env::remove_var("UZI_CHROME_PATH");
        for (k, v) in ["CHROME_PATH", "CHROMIUM_PATH"].iter().zip(previous) {
            if !v.is_empty() {
                std::env::set_var(k, v);
            }
        }
    }

    #[test]
    fn launch_options_default_to_headless_without_a_profile() {
        let opts = LaunchOptions::default();
        assert!(opts.headless);
        assert!(opts.profile_dir.is_none());
        assert!(opts.executable.is_none());
    }

    #[test]
    fn free_port_returns_a_bindable_loopback_port() {
        let port = free_port().unwrap();
        assert!(port > 0);
        // It was released, so binding again should succeed.
        assert!(std::net::TcpListener::bind(("127.0.0.1", port)).is_ok());
    }

    #[test]
    fn version_probe_rejects_a_dead_endpoint() {
        let port = free_port().unwrap();
        assert!(
            probe_version(port).is_err(),
            "nothing is listening on an unused port"
        );
    }

    #[test]
    fn puppeteer_scan_tolerates_a_missing_root() {
        assert!(puppeteer_chromes(Path::new("/definitely/not/here")).is_empty());
    }
}
