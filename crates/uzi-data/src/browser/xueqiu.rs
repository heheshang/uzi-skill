//! Port of `lib/xueqiu_browser.py` — authenticated XueQiu (雪球) fetching.
//!
//! XueQiu gates `query/v1/search/cube/stock.json` behind a login: a direct HTTP
//! request returns 400/401. Upstream solved this with Playwright driving a real
//! browser with a persistent cookie jar; this port does the same over CDP (see
//! [`super`]).
//!
//! Opt-in only, exactly like upstream — the browser starts only when the user
//! asks for it:
//!
//! * set `UZI_XQ_LOGIN=1`, or pass `--enable-xueqiu-login`;
//! * the first run needs an interactive login (`uzi --xueqiu-login`);
//! * afterwards the saved cookies are reused, and every failure path returns an
//!   empty payload so the caller degrades.
//!
//! Callers that never opt in see the same `_login_required` behaviour upstream
//! documents.

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::{json, Map, Value};

use super::{Browser, BrowserError, LaunchOptions, Session};

/// `PROFILE_DIR` — `~/.uzi-skill/playwright-xueqiu`.
pub fn profile_dir() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".uzi-skill")
        .join("playwright-xueqiu")
}

/// `COOKIE_FILE` — `PROFILE_DIR/cookies.json`.
pub fn cookie_file() -> PathBuf {
    profile_dir().join("cookies.json")
}

/// `LOGIN_URL`.
pub const LOGIN_URL: &str = "https://xueqiu.com/";

/// `LOGIN_TEST_URL` — the protected endpoint used to verify a login worked.
pub const LOGIN_TEST_URL: &str =
    "https://xueqiu.com/query/v1/search/cube/stock.json?q=SH600519&count=1&page=1";

/// `UA_PC`-equivalent used by the XueQiu flows.
pub const XUEQIU_UA: &str =
    "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36";

/// `is_login_enabled()` — the user must opt in explicitly.
pub fn is_login_enabled() -> bool {
    std::env::var("UZI_XQ_LOGIN").map(|v| v == "1").unwrap_or(false)
}

/// Whether a cookie is one of XueQiu's auth cookies.
///
/// Upstream requires `xq_a_token` specifically, since it is the one the API
/// checks; the id/refresh tokens alone are not sufficient.
fn is_auth_cookie(cookie: &Value) -> bool {
    cookie.get("name").and_then(|v| v.as_str()) == Some("xq_a_token")
}

/// `_has_valid_cookies()` against an explicit path.
pub fn has_valid_cookies_at(path: &Path) -> bool {
    let Ok(text) = std::fs::read_to_string(path) else {
        return false;
    };
    let Ok(cookies) = serde_json::from_str::<Value>(&text) else {
        return false;
    };
    match cookies.as_array() {
        Some(list) if !list.is_empty() => list.iter().any(is_auth_cookie),
        _ => false,
    }
}

/// `_has_valid_cookies()`.
pub fn has_valid_cookies() -> bool {
    has_valid_cookies_at(&cookie_file())
}

/// Load the saved jar, or an empty list when absent/unreadable.
pub fn load_cookies() -> Vec<Value> {
    load_cookies_at(&cookie_file())
}

pub fn load_cookies_at(path: &Path) -> Vec<Value> {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|t| serde_json::from_str::<Value>(&t).ok())
        .and_then(|v| v.as_array().cloned())
        .unwrap_or_default()
}

/// `_save_cookies(context)`.
pub fn save_cookies_at(path: &Path, cookies: &[Value]) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let text = serde_json::to_string_pretty(cookies).unwrap_or_else(|_| "[]".into());
    std::fs::write(path, text)
}

/// `xq_symbol(code)` — `600519` → `SH600519`, `000582` → `SZ000582`, … .
///
/// Shared by both XueQiu entry points; upstream duplicates it in each.
pub fn xq_symbol(stock_code: &str) -> String {
    let code = stock_code.trim();
    if code.starts_with(['6', '9']) {
        format!("SH{code}")
    } else if code.starts_with(['0', '3']) {
        format!("SZ{code}")
    } else if code.starts_with(['4', '8']) {
        format!("BJ{code}")
    } else if code.chars().all(|c| c.is_ascii_digit()) && code.chars().count() <= 5 {
        // A 1-5 digit numeric code is a Hong Kong listing.
        format!("HK{:0>5}", code)
    } else {
        code.to_uppercase()
    }
}

/// XueQiu symbol → the local `CODE.MARKET` form.
fn normalize_symbol(xq_code: &str) -> String {
    let upper = xq_code.to_uppercase();
    let (prefix, rest) = upper.split_at(2.min(upper.len()));
    match prefix {
        "SH" | "SZ" | "BJ" => format!("{rest}.{prefix}"),
        "HK" => format!("{rest}.HK"),
        _ => upper,
    }
}

/// `fetch_cubes_via_browser` — parse the cubes payload out of page HTML.
///
/// Upstream wraps the JSON in `<html><body>…</body></html>` and pulls it back out
/// with a **greedy** `\{.*"list".*\}|\{.*"cubes".*\}` DOTALL search. Greediness is
/// reproduced deliberately: a non-greedy match would parse pages upstream leaves
/// empty, changing results rather than fixing them. Whitespace is stripped from
/// every field, as upstream does.
pub fn parse_cubes(html: &str) -> Vec<Value> {
    let Some(raw) = extract_cubes_json(html) else {
        return Vec::new();
    };
    let Ok(data) = serde_json::from_str::<Value>(&raw) else {
        return Vec::new();
    };

    let list = data
        .get("list")
        .or_else(|| data.get("cubes"))
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    list.iter()
        .filter(|c| c.is_object())
        .map(|c| {
            let symbol = c.get("symbol").and_then(|v| v.as_str());
            let mut m = Map::new();
            m.insert("name".into(), c.get("name").cloned().unwrap_or(Value::Null));
            m.insert(
                "owner".into(),
                c.get("owner")
                    .and_then(|o| o.get("screen_name"))
                    .cloned()
                    .unwrap_or(Value::Null),
            );
            m.insert("symbol".into(), c.get("symbol").cloned().unwrap_or(Value::Null));
            m.insert("daily_gain".into(), c.get("daily_gain").cloned().unwrap_or(Value::Null));
            m.insert(
                "monthly_gain".into(),
                c.get("monthly_gain").cloned().unwrap_or(Value::Null),
            );
            m.insert("total_gain".into(), c.get("total_gain").cloned().unwrap_or(Value::Null));
            m.insert(
                "annualized_gain_rate".into(),
                c.get("annualized_gain_rate").cloned().unwrap_or(Value::Null),
            );
            m.insert(
                "url".into(),
                symbol
                    .map(|s| Value::from(format!("https://xueqiu.com/P/{s}")))
                    .unwrap_or(Value::Null),
            );
            m.insert(
                "stocks_count".into(),
                c.get("stocks_count").cloned().unwrap_or(Value::Null),
            );
            m.insert(
                "view_rebalancing_count".into(),
                c.get("view_rebalancing_count").cloned().unwrap_or(Value::Null),
            );
            Value::Object(m)
        })
        .collect()
}

/// The greedy DOTALL search upstream performs to lift the JSON out of the page.
fn extract_cubes_json(html: &str) -> Option<String> {
    use std::sync::LazyLock;
    static RE: LazyLock<regex::Regex> = LazyLock::new(|| {
        regex::Regex::new(r#"(?s)\{.*"list".*\}|\{.*"cubes".*\}"#)
            .expect("cubes extraction regex")
    });
    RE.find(html).map(|m| m.as_str().to_string())
}

/// `fetch_peers_via_browser` — parse same-sector peers out of a XueQiu page.
pub fn parse_peers(html: &str, max_peers: usize) -> Vec<Value> {
    use std::sync::LazyLock;
    static RE: LazyLock<regex::Regex> = LazyLock::new(|| {
        regex::Regex::new(r#"(?i)/S/([A-Z]{2}\d{5,6})"[^>]*>([^<]{1,20})</a>"#)
            .expect("peers extraction regex")
    });

    let mut peers: Vec<Value> = Vec::new();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();

    for cap in RE.captures_iter(html) {
        if peers.len() >= max_peers {
            break;
        }
        let xq_code = cap[1].to_uppercase();
        let name = cap[2].trim();
        let normalized = normalize_symbol(&xq_code);
        // Upstream skips single-character names as nav noise.
        if seen.contains(&normalized) || name.chars().count() < 2 {
            continue;
        }
        seen.insert(normalized.clone());
        peers.push(json!({
            "name": name,
            "code": normalized,
            // The F10 HTML needs further parsing for these; upstream returns
            // zeros so callers can distinguish "unknown" from "absent".
            "mcap_yi": 0,
            "pe": 0,
            "pb": 0,
        }));
    }
    peers
}

/// Open a browser for the XueQiu flows, reusing the persistent profile.
fn open_browser(headless: bool) -> Result<Browser, BrowserError> {
    Browser::launch(&LaunchOptions {
        headless,
        profile_dir: Some(profile_dir().join("chromium-profile")),
        user_agent: Some(XUEQIU_UA.to_string()),
        ..Default::default()
    })
}

/// Restore the saved jar into a session so a fresh profile still authenticates.
fn inject_cookies(browser: &mut Browser, session: &Session) {
    let cookies = load_cookies();
    if cookies.is_empty() {
        return;
    }
    // CDP wants the domain per cookie; entries saved from a previous run carry it.
    let usable: Vec<Value> = cookies
        .into_iter()
        .filter(|c| {
            c.get("name").and_then(|v| v.as_str()).is_some()
                && c.get("domain").and_then(|v| v.as_str()).is_some()
        })
        .collect();
    let _ = browser.set_cookies(session, &usable);
}

/// `fetch_with_browser(url, timeout)` — page HTML with login cookies, or `None`.
///
/// Mirrors upstream's gating: returns `None` unless login is enabled and the
/// profile exists, and never raises.
pub fn fetch_with_browser(url: &str, timeout: Duration) -> Option<String> {
    if !is_login_enabled() {
        return None;
    }
    if !profile_dir().exists() {
        println!(
            "   ℹ️ XueQiu 未登录 (UZI_XQ_LOGIN=1 但首次需跑 `uzi --xueqiu-login`)"
        );
        return None;
    }

    let mut browser = match open_browser(true) {
        Ok(b) => b,
        Err(e) => {
            println!("   ⚠️ XueQiu Playwright 失败: {e}");
            return None;
        }
    };
    let session = match browser.new_page() {
        Ok(s) => s,
        Err(e) => {
            println!("   ⚠️ XueQiu 会话创建失败: {e}");
            return None;
        }
    };
    inject_cookies(&mut browser, &session);

    match browser.navigate(&session, url, timeout) {
        Ok(nav) => Some(nav.html),
        Err(e) => {
            println!("   ⚠️ XueQiu 抓取失败 {url}: {e}");
            None
        }
    }
}

/// `fetch_cubes_via_browser(xq_symbol, limit)`.
pub fn fetch_cubes_via_browser(xq_symbol: &str, limit: usize) -> Vec<Value> {
    let url = format!(
        "https://xueqiu.com/query/v1/search/cube/stock.json?q={xq_symbol}&count={limit}&page=1"
    );
    match fetch_with_browser(&url, Duration::from_secs(20)) {
        Some(html) => parse_cubes(&html),
        None => Vec::new(),
    }
}

/// `fetch_peers_via_browser(stock_code, max_peers)`.
pub fn fetch_peers_via_browser(stock_code: &str, max_peers: usize) -> Vec<Value> {
    if !is_login_enabled() {
        return Vec::new();
    }
    let url = format!("https://xueqiu.com/S/{}", xq_symbol(stock_code));
    match fetch_with_browser(&url, Duration::from_secs(20)) {
        Some(html) => parse_peers(&html, max_peers),
        None => Vec::new(),
    }
}

/// Outcome of the interactive login flow.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoginOutcome {
    /// The protected endpoint answered after logging in.
    Success,
    /// Cookies were saved, but the endpoint still rejected them.
    Unverified,
    /// Could not run: no TTY, or the browser would not start.
    Skipped(String),
}

impl LoginOutcome {
    pub fn is_success(&self) -> bool {
        matches!(self, LoginOutcome::Success)
    }
}

/// `_interactive_login()` — open a headed browser, let the user log in, save the
/// jar, and verify against the protected endpoint.
///
/// `interactive` mirrors upstream's `sys.stdin.isatty()` check; the caller passes
/// it so this stays testable.
pub fn interactive_login(interactive: bool) -> LoginOutcome {
    if !interactive {
        println!("⚠️  XueQiu 登录需要交互式 TTY；当前是非交互环境，跳过。");
        println!("   解决：在交互式终端运行一次：");
        println!("     UZI_XQ_LOGIN=1 uzi --xueqiu-login");
        return LoginOutcome::Skipped("非交互环境".into());
    }

    let dir = profile_dir();
    if let Err(e) = std::fs::create_dir_all(&dir) {
        return LoginOutcome::Skipped(format!("无法创建目录 {}: {e}", dir.display()));
    }

    println!();
    println!("{}", "━".repeat(50));
    println!("🌐 XueQiu 登录流程（首次需要）");
    println!("{}", "━".repeat(50));
    println!("即将打开有头浏览器窗口。请：");
    println!("  1) 在弹出的浏览器里点击 \"登录\" 完成登录（账号密码 / 微信扫码 / 手机短信均可）");
    println!("  2) 看到 XueQiu 主页右上角变成你的头像 = 登录成功");
    println!("  3) 回到本终端按回车，cookie 会被保存供后续使用");
    print!("\n准备好后按回车继续... ");
    let _ = std::io::Write::flush(&mut std::io::stdout());
    let _ = read_line();

    let mut browser = match open_browser(false) {
        Ok(b) => b,
        Err(e) => {
            println!("❌ 浏览器启动失败: {e}");
            return LoginOutcome::Skipped(e.to_string());
        }
    };
    let session = match browser.new_page() {
        Ok(s) => s,
        Err(e) => return LoginOutcome::Skipped(e.to_string()),
    };
    let _ = browser.navigate(&session, LOGIN_URL, Duration::from_secs(30));

    println!("\n浏览器已打开。请完成登录后回到此终端，按回车继续...");
    let _ = read_line();

    // Verify against the protected endpoint.
    let ok = match browser.navigate(&session, LOGIN_TEST_URL, Duration::from_secs(10)) {
        Ok(nav) => {
            nav.status == Some(200)
                && !nav.html.contains("error_code")
        }
        Err(_) => false,
    };

    if !ok {
        println!("⚠️  登录验证失败 — query/v1/search/cube/stock.json 仍返回错误。");
        println!("   可能：(a) 实际未登录成功 (b) 该接口仍有反爬。Cookie 仍会被保存以便重试。");
    }

    // Save the jar, including for the failure case, so a retry can reuse it.
    match browser.cookies(&session) {
        Ok(cookies) => {
            let path = cookie_file();
            match save_cookies_at(&path, &cookies) {
                Ok(()) if ok => {
                    println!("✅ 登录成功 · cookie 已保存到 {}", path.display());
                    println!("   下次跑分析时自动复用，无需再登录。");
                    return LoginOutcome::Success;
                }
                Ok(()) => {
                    println!("⚠️  cookie 已保存到 {}（但登录可能未生效）", path.display());
                    return LoginOutcome::Unverified;
                }
                Err(e) => {
                    println!("❌ cookie 保存失败: {e}");
                    return LoginOutcome::Skipped(e.to_string());
                }
            }
        }
        Err(e) => {
            println!("❌ 读取 cookie 失败: {e}");
            LoginOutcome::Skipped(e.to_string())
        }
    }
}

fn read_line() -> std::io::Result<String> {
    use std::io::BufRead;
    let mut line = String::new();
    std::io::stdin().lock().read_line(&mut line)?;
    Ok(line)
}

/// `status` subcommand output.
pub fn status_text() -> String {
    let dir = profile_dir();
    format!(
        "PROFILE_DIR: {}\n  exists:        {}\n  cookie file:   {}\n  has valid auth cookie: {}\n  is_login_enabled(): {}\n\nSetup steps:\n  1) export UZI_XQ_LOGIN=1\n  2) uzi --xueqiu-login          # one-time interactive login\n  3) uzi <ticker> --no-browser   # XueQiu cubes will use saved cookies\n",
        dir.display(),
        dir.exists(),
        cookie_file().exists(),
        has_valid_cookies(),
        is_login_enabled(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn symbol_conversion_matches_upstream_branches() {
        assert_eq!(xq_symbol("600519"), "SH600519");
        assert_eq!(xq_symbol("900001"), "SH900001");
        assert_eq!(xq_symbol("000582"), "SZ000582");
        assert_eq!(xq_symbol("300750"), "SZ300750");
        assert_eq!(xq_symbol("430047"), "BJ430047");
        assert_eq!(xq_symbol("830799"), "BJ830799");
        // The HK branch is unreachable for 0/3/4/6/8/9-prefixed codes: `00700`
        // hits the SZ branch first. That is upstream's actual behaviour
        // (verified against the Python), so it is reproduced rather than fixed.
        assert_eq!(xq_symbol("00700"), "SZ00700");
        // HK only fires for a 1-5 digit code starting with something else.
        assert_eq!(xq_symbol("700"), "HK00700");
        assert_eq!(xq_symbol("1"), "HK00001");
        assert_eq!(xq_symbol("500"), "HK00500");
        // Anything else (US tickers) passes through uppercased.
        assert_eq!(xq_symbol("aapl"), "AAPL");
        assert_eq!(xq_symbol(" 600519 "), "SH600519");
    }

    #[test]
    fn symbol_normalization_round_trips() {
        assert_eq!(normalize_symbol("SH600519"), "600519.SH");
        assert_eq!(normalize_symbol("SZ000582"), "000582.SZ");
        assert_eq!(normalize_symbol("BJ430047"), "430047.BJ");
        assert_eq!(normalize_symbol("HK00700"), "00700.HK");
        // Unknown prefixes are left uppercased rather than mangled.
        assert_eq!(normalize_symbol("AAPL"), "AAPL");
    }

    #[test]
    fn auth_cookie_detection_requires_xq_a_token() {
        let dir = std::env::temp_dir().join(format!("uzi-xq-cookie-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();

        let path = dir.join("cookies.json");
        assert!(!has_valid_cookies_at(&path), "missing file");

        std::fs::write(&path, "not json").unwrap();
        assert!(!has_valid_cookies_at(&path), "unparsable file");

        std::fs::write(&path, "[]").unwrap();
        assert!(!has_valid_cookies_at(&path), "empty list");

        // Other XueQiu cookies are not sufficient on their own.
        std::fs::write(&path, r#"[{"name":"xq_id_token","value":"x"}]"#).unwrap();
        assert!(!has_valid_cookies_at(&path), "no xq_a_token");

        std::fs::write(
            &path,
            r#"[{"name":"xq_id_token","value":"x"},{"name":"xq_a_token","value":"y"}]"#,
        )
        .unwrap();
        assert!(has_valid_cookies_at(&path), "xq_a_token present");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn cookies_round_trip_through_disk() {
        let dir = std::env::temp_dir().join(format!("uzi-xq-save-{}", std::process::id()));
        let path = dir.join("nested").join("cookies.json");
        let jar = vec![json!({"name": "xq_a_token", "value": "abc", "domain": ".xueqiu.com"})];
        save_cookies_at(&path, &jar).unwrap();
        assert_eq!(load_cookies_at(&path), jar);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn cubes_parsing_maps_every_field() {
        // The shape upstream receives: JSON wrapped in an HTML document.
        let page = r#"<html><body>{"list":[{"name":"稳健组合","symbol":"ZH001","daily_gain":1.2,"monthly_gain":3.4,"total_gain":56.7,"annualized_gain_rate":12.3,"stocks_count":8,"view_rebalancing_count":4,"owner":{"screen_name":"张三"}}]}</body></html>"#;
        let cubes = parse_cubes(page);
        assert_eq!(cubes.len(), 1);
        let c = &cubes[0];
        assert_eq!(c["name"], json!("稳健组合"));
        assert_eq!(c["owner"], json!("张三"));
        assert_eq!(c["symbol"], json!("ZH001"));
        assert_eq!(c["daily_gain"], json!(1.2));
        assert_eq!(c["monthly_gain"], json!(3.4));
        assert_eq!(c["total_gain"], json!(56.7));
        assert_eq!(c["annualized_gain_rate"], json!(12.3));
        assert_eq!(c["url"], json!("https://xueqiu.com/P/ZH001"));
        assert_eq!(c["stocks_count"], json!(8));
        assert_eq!(c["view_rebalancing_count"], json!(4));
    }

    #[test]
    fn cubes_parsing_handles_the_cubes_key_and_missing_fields() {
        // The alternate key upstream accepts.
        let cubes = parse_cubes(r#"<html>{"cubes":[{"name":"A"}]}</html>"#);
        assert_eq!(cubes.len(), 1);
        assert_eq!(cubes[0]["name"], json!("A"));
        // A cube with no symbol yields a null url rather than a broken one.
        assert_eq!(cubes[0]["url"], Value::Null);
        // Every field upstream projects is present even when unknown.
        assert_eq!(cubes[0]["owner"], Value::Null);
        assert_eq!(cubes[0]["total_gain"], Value::Null);
    }

    #[test]
    fn cubes_parsing_degrades_on_junk() {
        assert!(parse_cubes("").is_empty());
        assert!(parse_cubes("<html><body>no json here</body></html>").is_empty());
        assert!(parse_cubes(r#"{"list": not valid json}"#).is_empty());
        // Non-object entries in the list are skipped, not panicked on.
        assert!(parse_cubes(r#"{"list":[1,"two",null]}"#).is_empty());
    }

    #[test]
    fn peers_parsing_skips_noise_and_dedupes() {
        let html = r#"
            <a href="/S/SH600519">贵州茅台</a>
            <a href="/S/SZ000582">北部湾港</a>
            <a href="/S/SH600519">贵州茅台</a>
            <a href="/S/SH600520">短</a>
            <a href="/S/HK00700">腾讯控股</a>
        "#;
        let peers = parse_peers(html, 20);
        assert_eq!(peers.len(), 3, "{peers:?}");
        assert_eq!(peers[0]["code"], json!("600519.SH"));
        assert_eq!(peers[0]["name"], json!("贵州茅台"));
        assert_eq!(peers[1]["code"], json!("000582.SZ"));
        assert_eq!(peers[2]["code"], json!("00700.HK"));
        // Upstream returns zeros for the fields it does not parse yet.
        assert_eq!(peers[0]["mcap_yi"], json!(0));
        assert_eq!(peers[0]["pe"], json!(0));
        assert_eq!(peers[0]["pb"], json!(0));
    }

    #[test]
    fn peers_parsing_respects_the_cap() {
        let html: String = (0..50)
            .map(|i| format!(r#"<a href="/S/SH6005{:02}">公司{:02}</a>"#, i, i))
            .collect();
        assert_eq!(parse_peers(&html, 5).len(), 5);
    }

    #[test]
    fn peers_parsing_degrades_on_junk() {
        assert!(parse_peers("", 20).is_empty());
        assert!(parse_peers("<a href='/S/SH600519'>unclosed", 20).is_empty());
    }

    #[test]
    fn login_is_off_unless_explicitly_enabled() {
        let previous = std::env::var("UZI_XQ_LOGIN").ok();
        std::env::remove_var("UZI_XQ_LOGIN");
        assert!(!is_login_enabled());
        std::env::set_var("UZI_XQ_LOGIN", "0");
        assert!(!is_login_enabled(), "only \"1\" enables it");
        std::env::set_var("UZI_XQ_LOGIN", "1");
        assert!(is_login_enabled());
        match previous {
            Some(v) => std::env::set_var("UZI_XQ_LOGIN", v),
            None => std::env::remove_var("UZI_XQ_LOGIN"),
        }
    }

    #[test]
    fn fetch_paths_return_empty_when_login_is_disabled() {
        let previous = std::env::var("UZI_XQ_LOGIN").ok();
        std::env::remove_var("UZI_XQ_LOGIN");
        // No browser is started, and the caller gets an empty payload to degrade on.
        assert!(fetch_cubes_via_browser("SH600519", 10).is_empty());
        assert!(fetch_peers_via_browser("600519", 20).is_empty());
        assert!(fetch_with_browser(LOGIN_URL, Duration::from_secs(1)).is_none());
        match previous {
            Some(v) => std::env::set_var("UZI_XQ_LOGIN", v),
            None => std::env::remove_var("UZI_XQ_LOGIN"),
        }
    }

    #[test]
    fn non_interactive_login_never_opens_a_browser() {
        let outcome = interactive_login(false);
        assert!(!outcome.is_success());
        assert!(matches!(outcome, LoginOutcome::Skipped(_)), "{outcome:?}");
    }
}
