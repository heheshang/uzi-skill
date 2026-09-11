//! Port of `lib/playwright_fallback.py` — browser fallback for dimensions whose
//! primary chain came back empty or low quality.
//!
//! Upstream drives Playwright; this port drives Chromium over CDP (see [`super`]).
//! The parts that decide *whether* to fetch are ported exactly, since they are
//! what keeps the fallback from being a blunt "always scrape" switch:
//!
//! * **profile gating** — `lite` never uses a browser, `medium` requires
//!   `UZI_PLAYWRIGHT_ENABLE=1`, `deep` enables it by default;
//! * **quality gating** — a dimension is only retried when its data is empty,
//!   marked `fallback`, or has fewer than 50% usable public fields;
//! * **network gating** — dimensions needing domestic or search reachability are
//!   skipped when the preflight says that capability is down;
//! * **junk filtering** — parsed output matching the junk filter is discarded.
//!
//! Every fetch failure degrades: strategies return `None`, which counts as
//! `failed` and never aborts the run.

use std::collections::BTreeSet;
use std::time::Duration;

use serde_json::{json, Map, Value};

use super::xueqiu;
use super::{Browser, LaunchOptions};

/// `DEFAULT_TIMEOUT`.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(15);
/// `UA_PC`.
pub const UA_PC: &str =
    "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36";

/// `QUALITY_THRESHOLD` — below this share of usable fields, a dimension is
/// considered effectively empty.
pub const QUALITY_THRESHOLD: f64 = 0.5;

/// Values upstream treats as "no data" when scoring quality.
const EMPTY_SENTINELS: &[&str] = &["", "—", "-", "--", "N/A", "n/a", "None", "null", "TBD"];

/// `DIM_NETWORK_REQUIREMENTS` — each dimension's required network capabilities.
pub fn dim_network_requirements(dim_key: &str) -> &'static [&'static str] {
    match dim_key {
        "4_peers" => &["domestic"],
        "8_materials" => &["domestic"],
        "15_events" => &["domestic"],
        "17_sentiment" => &["domestic"],
        "3_macro" => &["domestic"],
        "7_industry" => &["domestic", "search"],
        "14_moat" => &["domestic"],
        "13_policy" => &["domestic"],
        "18_trap" => &["domestic", "search"],
        "19_contests" => &["domestic"],
        _ => &[],
    }
}

/// `DIM_STRATEGIES` — which dimensions have a browser strategy.
pub const DIM_STRATEGIES: &[&str] = &[
    "4_peers",
    "8_materials",
    "15_events",
    "17_sentiment",
    "3_macro",
    "7_industry",
    "14_moat",
    "13_policy",
    "18_trap",
    "19_contests",
];

/// `_is_empty_value(v)`.
pub fn is_empty_value(v: &Value) -> bool {
    match v {
        Value::Null => true,
        Value::String(s) => EMPTY_SENTINELS.contains(&s.trim()),
        Value::Array(a) => a.is_empty(),
        Value::Object(m) => m.is_empty(),
        _ => false,
    }
}

/// `_dim_quality_score(data)` — share of non-empty public fields.
///
/// Only keys not starting with `_` count; diagnostics are excluded so a
/// dimension cannot look healthy because of its own metadata.
pub fn dim_quality_score(data: &Value) -> f64 {
    let Some(map) = data.as_object() else {
        return 0.0;
    };
    let public: Vec<&Value> = map
        .iter()
        .filter(|(k, _)| !k.starts_with('_'))
        .map(|(_, v)| v)
        .collect();
    if public.is_empty() {
        return 0.0;
    }
    let valid = public.iter().filter(|v| !is_empty_value(v)).count();
    valid as f64 / public.len() as f64
}

/// `_dim_needs_fallback(dim)` → `(needs, reason)`.
pub fn dim_needs_fallback(dim: &Value) -> (bool, String) {
    let Some(map) = dim.as_object() else {
        return (true, "dim 非 dict".to_string());
    };
    // Upstream's guard is `if not data or not isinstance(data, dict)`. Python's
    // `not data` is true for an *empty* dict as well as for null, so `{}` must be
    // reported as empty rather than scored as zero quality.
    let data = map.get("data");
    let usable = match data {
        Some(d) => d.is_object() && uzi_core::py::truthy(d),
        None => false,
    };
    if !usable {
        return (true, "data 为空或非 dict".to_string());
    }
    if map.get("fallback").and_then(|f| f.as_bool()) == Some(true) {
        return (true, "主链标 fallback=True".to_string());
    }
    let q = dim_quality_score(data.unwrap_or(&Value::Null));
    if q < QUALITY_THRESHOLD {
        return (
            true,
            format!(
                "有效字段占比 {:.0}% < {:.0}%",
                q * 100.0,
                QUALITY_THRESHOLD * 100.0
            ),
        );
    }
    (false, format!("有效字段占比 {:.0}% 已达标", q * 100.0))
}

/// `_filter_dims_by_network(dims)` → `(effective, skipped)`.
///
/// `network` is the preflight profile; `None` means it could not be obtained, in
/// which case upstream filters nothing.
pub fn filter_dims_by_network(
    dims: &BTreeSet<String>,
    network: Option<&Value>,
) -> (BTreeSet<String>, Vec<String>) {
    let Some(net) = network else {
        return (dims.clone(), Vec::new());
    };
    let ok = |k: &str| net.get(k).and_then(|v| v.as_bool()).unwrap_or(false);

    let mut effective = BTreeSet::new();
    let mut skipped = Vec::new();
    for dim in dims {
        let mut satisfied = true;
        let mut why: Vec<&str> = Vec::new();
        for req in dim_network_requirements(dim) {
            let reachable = match *req {
                "domestic" => ok("domestic_ok"),
                "search" => ok("search_ok"),
                "overseas" => ok("overseas_ok"),
                _ => true,
            };
            if !reachable {
                satisfied = false;
                why.push(match *req {
                    "domestic" => "domestic 不通",
                    "search" => "search 不通",
                    _ => "overseas 不通",
                });
            }
        }
        if satisfied {
            effective.insert(dim.clone());
        } else {
            skipped.push(format!("{dim}({})", why.join(",")));
        }
    }
    (effective, skipped)
}

/// `_strategy_4_peers` — peers from the XueQiu stock page.
pub fn strategy_4_peers(html: &str) -> Option<Value> {
    let peers: Vec<Value> = xueqiu::parse_peers(html, 20)
        .into_iter()
        .map(|p| json!({"name": p["name"], "code": p["code"]}))
        .collect();
    if peers.is_empty() {
        return None;
    }
    Some(json!({"peer_table_playwright": peers}))
}

/// `_strategy_8_materials` — EastMoney F10 business analysis.
pub fn strategy_8_materials(html: &str) -> Option<Value> {
    let re = regex::Regex::new(r"主营业务[：:]\s*([^\n<]{5,200})").ok()?;
    let cap = re.captures(html)?;
    let text: String = cap[1].trim().chars().take(200).collect();
    Some(json!({"core_business_playwright": text}))
}

/// `_strategy_15_events` — cninfo announcement titles.
pub fn strategy_15_events(html: &str) -> Option<Value> {
    let re = regex::Regex::new(r"announcement-title[^>]*>([^<]{5,80})<").ok()?;
    let titles: Vec<Value> = re
        .captures_iter(html)
        .map(|c| Value::from(c[1].to_string()))
        .take(10)
        .collect();
    if titles.is_empty() {
        return None;
    }
    Some(json!({"event_titles_playwright": titles}))
}

/// `_strategy_17_sentiment` — XueQiu discussion titles.
pub fn strategy_17_sentiment(html: &str) -> Option<Value> {
    let re = regex::Regex::new(r#""title":"([^"]{5,100})""#).ok()?;
    let posts: Vec<Value> = re
        .captures_iter(html)
        .map(|c| Value::from(c[1].to_string()))
        .take(8)
        .collect();
    if posts.is_empty() {
        return None;
    }
    Some(json!({"xueqiu_posts_playwright": posts}))
}

/// `_strategy_3_macro` — National Bureau of Statistics headlines.
pub fn strategy_3_macro(html: &str) -> Option<Value> {
    let re = regex::Regex::new(r#"<a[^>]*href="[^"]*"[^>]*>([^<]{5,60})</a>"#).ok()?;
    let clean: Vec<Value> = re
        .captures_iter(html)
        .map(|c| c[1].trim().to_string())
        .filter(|t| t.chars().count() >= 10 && !t.chars().all(|c| c.is_ascii_digit()))
        .take(10)
        .map(Value::from)
        .collect();
    if clean.is_empty() {
        return None;
    }
    Some(json!({"macro_headlines_playwright": clean}))
}

/// `_strategy_7_industry` — Baidu search snippets for the industry.
pub fn strategy_7_industry(html: &str) -> Option<Value> {
    let titles = regex::Regex::new(r#"<h3[^>]*>\s*<a[^>]*>([^<]{5,80})</a>"#).ok()?;
    let descs = regex::Regex::new(r#"<span class="content-right_[^"]*">([^<]{10,200})</span>"#).ok()?;

    let t: Vec<Value> = titles
        .captures_iter(html)
        .map(|c| Value::from(c[1].trim().to_string()))
        .take(10)
        .collect();
    let d: Vec<Value> = descs
        .captures_iter(html)
        .map(|c| Value::from(c[1].trim().to_string()))
        .take(5)
        .collect();
    if t.is_empty() && d.is_empty() {
        return None;
    }
    Some(json!({
        "baidu_search_titles_playwright": t,
        "baidu_search_descs_playwright": d,
    }))
}

/// `_strategy_14_moat` — Baidu Baike company entry.
pub fn strategy_14_moat(html: &str) -> Option<Value> {
    use std::sync::LazyLock;
    static SUMMARY: LazyLock<regex::Regex> = LazyLock::new(|| {
        regex::Regex::new(r#"(?s)<div class="lemma-summary[^"]*"[^>]*>(.*?)</div>"#).unwrap()
    });
    static TAGS: LazyLock<regex::Regex> =
        LazyLock::new(|| regex::Regex::new(r"<[^>]+>").unwrap());
    static PAIRS: LazyLock<regex::Regex> = LazyLock::new(|| {
        regex::Regex::new(
            r#"<dt[^>]*class="basicInfo-item[^"]*">([^<]+)</dt>\s*<dd[^>]*class="basicInfo-item[^"]*">([^<]+)</dd>"#,
        )
        .unwrap()
    });
    static WS: LazyLock<regex::Regex> = LazyLock::new(|| regex::Regex::new(r"\s+").unwrap());

    let intro = SUMMARY
        .captures(html)
        .map(|c| {
            let stripped = TAGS.replace_all(&c[1], "");
            let trimmed = stripped.trim();
            trimmed.chars().take(500).collect::<String>()
        })
        .unwrap_or_default();

    let mut info = Map::new();
    for cap in PAIRS.captures_iter(html).take(10) {
        let key = WS.replace_all(&cap[1], "").to_string();
        let value: String = cap[2].trim().chars().take(100).collect();
        info.insert(key, Value::from(value));
    }

    if intro.is_empty() && info.is_empty() {
        return None;
    }
    Some(json!({
        "baike_intro_playwright": intro,
        "baike_basic_info_playwright": info,
    }))
}

/// `_strategy_13_policy` — CSRC news titles.
pub fn strategy_13_policy(html: &str) -> Option<Value> {
    let titled = regex::Regex::new(r#"<a[^>]*title="([^"]{10,100})""#).ok()?;
    let bare = regex::Regex::new(r"<a[^>]*>([^<]{10,80})</a>").ok()?;

    let mut titles: Vec<String> = titled
        .captures_iter(html)
        .map(|c| c[1].trim().to_string())
        .collect();
    if titles.is_empty() {
        titles = bare
            .captures_iter(html)
            .map(|c| c[1].trim().to_string())
            .collect();
    }
    let clean: Vec<Value> = titles
        .into_iter()
        .filter(|t| !t.chars().all(|c| c.is_ascii_digit()) && !t.contains("首页"))
        .take(15)
        .map(Value::from)
        .collect();
    if clean.is_empty() {
        return None;
    }
    Some(json!({"csrc_policy_titles_playwright": clean}))
}

/// `_strategy_18_trap` — Xiaohongshu posts matching the trap keywords.
///
/// The result is the count plus the matching titles; the caller treats a high
/// count as elevated risk. `name` is the company being searched for.
pub fn strategy_18_trap(html: &str, name: &str) -> Option<Value> {
    let re = regex::Regex::new(r#""title":"([^"]{5,80})""#).ok()?;
    let clean: Vec<Value> = re
        .captures_iter(html)
        .map(|c| c[1].to_string())
        .filter(|t| t.contains(name) || t.contains("推荐") || t.contains("老师"))
        .take(10)
        .map(Value::from)
        .collect();
    if clean.is_empty() {
        return None;
    }
    Some(json!({
        "xhs_trap_post_count_playwright": clean.len(),
        "xhs_trap_titles_playwright": clean,
    }))
}

/// `_strategy_19_contests` — XueQiu cube leaderboard.
pub fn strategy_19_contests(html: &str) -> Option<Value> {
    let re = regex::Regex::new(r#""name":"([^"]{3,40})"[^}]*?"total_gain":([\-\d.]+)"#).ok()?;
    let mut top10: Vec<Value> = Vec::new();
    for cap in re.captures_iter(html).take(10) {
        let gain = cap[2].parse::<f64>().unwrap_or(0.0);
        top10.push(json!({"name": cap[1].to_string(), "total_gain_pct": gain}));
    }
    if top10.is_empty() {
        return None;
    }
    Some(json!({"xueqiu_contest_top10_playwright": top10}))
}

/// The URL a strategy should visit, or `None` when the strategy cannot build one
/// (e.g. `7_industry` needs an industry name that is absent).
pub fn strategy_url(dim_key: &str, ticker: &str, raw: &Value) -> Option<String> {
    let code = ticker.split('.').next().unwrap_or("").to_string();
    let basic = |field: &str| -> String {
        raw.get("dimensions")
            .and_then(|d| d.get("0_basic"))
            .and_then(|d| d.get("data"))
            .and_then(|d| d.get(field))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string()
    };

    Some(match dim_key {
        "4_peers" | "17_sentiment" => {
            let sym = xueqiu::xq_symbol(&code);
            if dim_key == "4_peers" {
                format!("https://xueqiu.com/S/{sym}")
            } else {
                format!("https://xueqiu.com/S/{sym}/POST")
            }
        }
        "8_materials" => format!(
            "https://emweb.securities.eastmoney.com/PC_HSF10/BusinessAnalysis/Index?type=web&code={code}"
        ),
        "15_events" => {
            format!("http://www.cninfo.com.cn/new/disclosure/stock?stockCode={code}")
        }
        "3_macro" => "https://www.stats.gov.cn/sj/sjjd/".to_string(),
        "13_policy" => "http://www.csrc.gov.cn/csrc/c100028/common_list.shtml".to_string(),
        "19_contests" => "https://xueqiu.com/cube/rank/list".to_string(),
        "7_industry" => {
            let industry = basic("industry");
            if industry.is_empty() {
                return None;
            }
            let q = uzi_core::py::py_url_quote(&format!(
                "{industry} 行业景气度 增速 市场规模 2026"
            ));
            format!("https://www.baidu.com/s?wd={q}")
        }
        "14_moat" => {
            let name = basic("name");
            if name.chars().count() < 2 {
                return None;
            }
            format!("https://baike.baidu.com/item/{}", uzi_core::py::py_url_quote(&name))
        }
        "18_trap" => {
            let name = basic("name");
            if name.chars().count() < 2 {
                return None;
            }
            let q = uzi_core::py::py_url_quote(&format!("{name} 老师 推荐"));
            format!("https://www.xiaohongshu.com/search_result?keyword={q}")
        }
        _ => return None,
    })
}

/// Apply a strategy's parser to fetched HTML.
///
/// `18_trap` additionally needs the company name, which is why `raw` is threaded
/// through rather than baked into the HTML.
pub fn parse_for_dim(dim_key: &str, html: &str, raw: &Value) -> Option<Value> {
    match dim_key {
        "4_peers" => strategy_4_peers(html),
        "8_materials" => strategy_8_materials(html),
        "15_events" => strategy_15_events(html),
        "17_sentiment" => strategy_17_sentiment(html),
        "3_macro" => strategy_3_macro(html),
        "7_industry" => strategy_7_industry(html),
        "14_moat" => strategy_14_moat(html),
        "13_policy" => strategy_13_policy(html),
        "18_trap" => {
            let name = raw
                .get("dimensions")
                .and_then(|d| d.get("0_basic"))
                .and_then(|d| d.get("data"))
                .and_then(|d| d.get("name"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            strategy_18_trap(html, name)
        }
        "19_contests" => strategy_19_contests(html),
        _ => None,
    }
}

/// How the fallback is gated for the current run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Gate {
    Enabled,
    /// `lite` never uses a browser.
    DisabledByProfile(String),
    /// `medium` requires an explicit opt-in.
    DisabledByOptIn(String),
    /// The browser could not be started.
    DisabledByBrowser(String),
}

/// Decide whether the fallback may run, without starting anything.
///
/// `mode` is the profile's `playwright_mode`: `off` | `opt-in` | `default`.
/// `enabled_env` mirrors `UZI_PLAYWRIGHT_ENABLE=1`.
///
/// Upstream's shape is `off` → false, `default` → true, **everything else** →
/// the env check. An unrecognised mode therefore behaves like `opt-in`, which is
/// the conservative reading; treating it as enabled would silently start a
/// browser for a profile that never asked for one.
pub fn gate(mode: &str, depth: &str, enabled_env: bool) -> Gate {
    match mode {
        "off" => Gate::DisabledByProfile(format!(
            "profile={depth} · playwright_mode=off（lite 档不用浏览器）"
        )),
        "default" => Gate::Enabled,
        _ if enabled_env => Gate::Enabled,
        _ => Gate::DisabledByOptIn(format!(
            "profile={depth} · opt-in 未启用 · export UZI_PLAYWRIGHT_ENABLE=1 启用后重跑"
        )),
    }
}

/// `autofill_via_playwright(raw, ticker)` summary, mirroring upstream's shape.
#[derive(Debug, Clone, Default)]
pub struct Summary {
    pub enabled: bool,
    pub attempted: usize,
    pub succeeded: usize,
    pub failed: usize,
    pub skipped: usize,
    pub skip_reasons: Map<String, Value>,
    pub disabled_reason: String,
}

impl Summary {
    pub fn to_value(&self) -> Value {
        json!({
            "enabled": self.enabled,
            "attempted": self.attempted,
            "succeeded": self.succeeded,
            "failed": self.failed,
            "skipped": self.skipped,
            "skip_reasons": self.skip_reasons,
            "disabled_reason": self.disabled_reason,
        })
    }
}

/// Runs the fallback for every eligible dimension, mutating `raw` in place.
///
/// `browser` is injected so the decision logic is testable without a browser;
/// `fetch` maps a URL to page HTML and returns `None` on failure.
pub fn autofill_with<F>(
    raw: &mut Value,
    ticker: &str,
    dims: &BTreeSet<String>,
    network: Option<&Value>,
    force: bool,
    mut fetch: F,
) -> Summary
where
    F: FnMut(&str, Duration) -> Option<String>,
{
    let mut summary = Summary::default();

    let (effective, network_skipped) = filter_dims_by_network(dims, network);
    for s in &network_skipped {
        let dim_name = s.split('(').next().unwrap_or(s).to_string();
        summary.skipped += 1;
        summary
            .skip_reasons
            .insert(dim_name, Value::from(format!("网络能力不足 · {s}")));
    }
    if !network_skipped.is_empty() {
        println!("   🌐 网络过滤 · 跳过 {} 维: {}", network_skipped.len(), network_skipped.join(", "));
    }

    for dim_key in &effective {
        let dim = raw
            .get("dimensions")
            .and_then(|d| d.get(dim_key))
            .cloned()
            .unwrap_or_else(|| json!({}));

        if !force {
            let (needs, reason) = dim_needs_fallback(&dim);
            if !needs {
                summary.skipped += 1;
                summary.skip_reasons.insert(dim_key.clone(), Value::from(reason));
                continue;
            }
        }

        if !DIM_STRATEGIES.contains(&dim_key.as_str()) {
            summary.skipped += 1;
            summary
                .skip_reasons
                .insert(dim_key.clone(), Value::from("DIM_STRATEGIES 未定义"));
            continue;
        }

        let Some(url) = strategy_url(dim_key, ticker, raw) else {
            summary.skipped += 1;
            summary
                .skip_reasons
                .insert(dim_key.clone(), Value::from("无法构造 URL（缺少行业/公司名）"));
            continue;
        };

        summary.attempted += 1;
        let Some(html) = fetch(&url, DEFAULT_TIMEOUT) else {
            summary.failed += 1;
            continue;
        };
        let Some(result) = parse_for_dim(dim_key, &html, raw) else {
            summary.failed += 1;
            continue;
        };
        if crate::junk_filter::is_junk_autofill_text(&result.to_string()) {
            summary.failed += 1;
            continue;
        }

        // Merge into the dimension, appending the source marker upstream uses.
        if let Some(dims_map) = raw.get_mut("dimensions").and_then(|d| d.as_object_mut()) {
            let entry = dims_map
                .entry(dim_key.clone())
                .or_insert_with(|| json!({}));
            if let Some(obj) = entry.as_object_mut() {
                let data = obj.entry("data").or_insert_with(|| json!({}));
                if let (Some(dst), Some(src)) = (data.as_object_mut(), result.as_object()) {
                    for (k, v) in src {
                        dst.insert(k.clone(), v.clone());
                    }
                }
                let existing = obj
                    .get("source")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let combined = if existing.is_empty() {
                    "playwright_fallback".to_string()
                } else {
                    format!("{existing} + playwright_fallback")
                };
                obj.insert("source".into(), Value::from(combined));
            }
        }
        summary.succeeded += 1;
    }

    println!(
        "   📊 Playwright 兜底 · 尝试 {} · 成功 {} · 失败 {} · 跳过 {}（数据已足）",
        summary.attempted, summary.succeeded, summary.failed, summary.skipped
    );
    summary
}

/// `autofill_via_playwright(raw, ticker)` — gate, launch, and run.
pub fn autofill_via_browser(
    raw: &mut Value,
    ticker: &str,
    mode: &str,
    depth: &str,
    dims: &BTreeSet<String>,
    network: Option<&Value>,
    enabled_env: bool,
    force: bool,
) -> Summary {
    let mut summary = Summary::default();

    match gate(mode, depth, enabled_env) {
        Gate::Enabled => {}
        Gate::DisabledByProfile(reason) | Gate::DisabledByOptIn(reason) => {
            summary.disabled_reason = reason.clone();
            println!("   ℹ️  Playwright skip · {reason}");
            return summary;
        }
        Gate::DisabledByBrowser(reason) => {
            summary.disabled_reason = reason.clone();
            return summary;
        }
    }

    // One browser is shared across dimensions; upstream launches per page, but a
    // single context is both faster and closer to a normal user session.
    let mut browser = match Browser::launch(&LaunchOptions {
        headless: true,
        user_agent: Some(UA_PC.to_string()),
        ..Default::default()
    }) {
        Ok(b) => b,
        Err(e) => {
            summary.disabled_reason = format!("浏览器不可用 · 已降级跳过: {e}");
            println!("   ℹ️  Playwright skip · {}", summary.disabled_reason);
            return summary;
        }
    };
    let session = match browser.new_page() {
        Ok(s) => s,
        Err(e) => {
            summary.disabled_reason = format!("会话创建失败 · 已降级跳过: {e}");
            return summary;
        }
    };

    summary.enabled = true;
    println!(
        "   🎭 profile={depth} · playwright_dims={} · FORCE={force}",
        dims.len()
    );

    let summary = autofill_with(raw, ticker, dims, network, force, |url, timeout| {
        match browser.navigate(&session, url, timeout) {
            Ok(nav) => Some(nav.html),
            Err(e) => {
                println!("   ⚠️  Playwright fetch 失败 {}: {e}", &url[..url.len().min(50)]);
                None
            }
        }
    });
    summary
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_value_detection_covers_every_sentinel() {
        for s in ["", "—", "-", "--", "N/A", "n/a", "None", "null", "TBD", "  —  "] {
            assert!(is_empty_value(&json!(s)), "{s:?} should be empty");
        }
        for s in ["0", "0.0", "贵州茅台", "false"] {
            assert!(!is_empty_value(&json!(s)), "{s:?} should count as data");
        }
        assert!(is_empty_value(&Value::Null));
        assert!(is_empty_value(&json!([])));
        assert!(is_empty_value(&json!({})));
        assert!(!is_empty_value(&json!([1])));
        assert!(!is_empty_value(&json!({"a": 1})));
        // Zero is data, not absence.
        assert!(!is_empty_value(&json!(0)));
        assert!(!is_empty_value(&json!(false)));
    }

    /// The quality score is what suppresses the "12 keys but all em-dashes" case.
    #[test]
    fn quality_score_counts_only_populated_public_fields() {
        assert_eq!(dim_quality_score(&json!({})), 0.0);
        assert_eq!(dim_quality_score(&json!({"a": "—", "b": Value::Null})), 0.0);
        assert_eq!(dim_quality_score(&json!({"a": 1, "b": 2})), 1.0);
        assert_eq!(
            dim_quality_score(&json!({"a": 1, "b": "—", "c": Value::Null, "d": 4})),
            0.5
        );
        // Underscore-prefixed keys are diagnostics and must not count.
        assert_eq!(dim_quality_score(&json!({"_src": "x", "a": 1})), 1.0);
        assert_eq!(dim_quality_score(&json!({"_src": "x", "_e": "y"})), 0.0);
    }

    #[test]
    fn fallback_triggers_only_for_empty_low_quality_or_marked_dims() {
        let (needs, reason) = dim_needs_fallback(&json!(null));
        assert!(needs && reason.contains("非 dict"));

        let (needs, reason) = dim_needs_fallback(&json!({"data": {}}));
        assert!(needs && reason.contains("为空"), "{reason}");

        let (needs, reason) = dim_needs_fallback(&json!({"data": {"a": 1}, "fallback": true}));
        assert!(needs && reason.contains("fallback=True"), "{reason}");

        // 1 of 4 public fields populated → 25% < 50%.
        let (needs, reason) =
            dim_needs_fallback(&json!({"data": {"a": 1, "b": "—", "c": Value::Null, "d": ""}}));
        assert!(needs && reason.contains("25%"), "{reason}");

        // Exactly at the threshold is "good enough" (upstream uses `<`).
        let (needs, reason) = dim_needs_fallback(&json!({"data": {"a": 1, "b": "—"}}));
        assert!(!needs, "{reason}");

        let (needs, _) = dim_needs_fallback(&json!({"data": {"a": 1, "b": 2}}));
        assert!(!needs);
    }

    #[test]
    fn network_filtering_skips_dims_whose_capability_is_down() {
        let dims: BTreeSet<String> = ["4_peers", "7_industry", "13_policy"]
            .iter()
            .map(|s| s.to_string())
            .collect();

        // Everything reachable → nothing skipped.
        let all_ok = json!({"domestic_ok": true, "search_ok": true, "overseas_ok": true});
        let (effective, skipped) = filter_dims_by_network(&dims, Some(&all_ok));
        assert_eq!(effective.len(), 3);
        assert!(skipped.is_empty());

        // Search down → only the search-dependent dim is dropped.
        let no_search = json!({"domestic_ok": true, "search_ok": false});
        let (effective, skipped) = filter_dims_by_network(&dims, Some(&no_search));
        assert_eq!(effective.len(), 2);
        assert!(skipped.iter().any(|s| s.contains("7_industry") && s.contains("search 不通")));

        // Domestic down → everything goes, since all these dims need it.
        let no_domestic = json!({"domestic_ok": false, "search_ok": true});
        let (effective, skipped) = filter_dims_by_network(&dims, Some(&no_domestic));
        assert!(effective.is_empty());
        assert_eq!(skipped.len(), 3);

        // No profile → upstream filters nothing.
        let (effective, skipped) = filter_dims_by_network(&dims, None);
        assert_eq!(effective.len(), 3);
        assert!(skipped.is_empty());
    }

    #[test]
    fn gating_follows_the_profile_mode() {
        assert!(matches!(gate("off", "lite", false), Gate::DisabledByProfile(_)));
        assert!(matches!(gate("off", "lite", true), Gate::DisabledByProfile(_)));
        assert!(matches!(gate("opt-in", "medium", false), Gate::DisabledByOptIn(_)));
        assert_eq!(gate("opt-in", "medium", true), Gate::Enabled);
        assert_eq!(gate("default", "deep", false), Gate::Enabled);
        // An unknown mode behaves like opt-in, the conservative default.
        assert!(matches!(gate("weird", "x", false), Gate::DisabledByOptIn(_)));
    }

    #[test]
    fn strategies_parse_their_reference_pages() {
        // 8_materials
        let v = strategy_8_materials("<div>主营业务：光学薄膜研发与制造</div>").unwrap();
        assert_eq!(v["core_business_playwright"], json!("光学薄膜研发与制造"));
        assert!(strategy_8_materials("<div>nothing</div>").is_none());

        // 15_events
        let v = strategy_15_events(r#"<div class="announcement-title">2026年第一季度报告</div>"#).unwrap();
        assert_eq!(v["event_titles_playwright"], json!(["2026年第一季度报告"]));

        // 17_sentiment
        let v = strategy_17_sentiment(r#"<script>{"title":"水晶光电还有机会吗"}</script>"#).unwrap();
        assert_eq!(v["xueqiu_posts_playwright"], json!(["水晶光电还有机会吗"]));

        // 3_macro filters short and numeric-only anchors.
        let v = strategy_3_macro(
            r#"<a href="/x">短</a><a href="/y">1234567890</a><a href="/z">2026年8月份国民经济运行情况</a>"#,
        )
        .unwrap();
        assert_eq!(v["macro_headlines_playwright"], json!(["2026年8月份国民经济运行情况"]));

        // 13_policy prefers the title attribute, and drops nav noise.
        let v = strategy_13_policy(
            r#"<a title="证监会发布关于资本市场的最新政策通知">x</a><a title="首页">y</a>"#,
        )
        .unwrap();
        assert_eq!(v["csrc_policy_titles_playwright"], json!(["证监会发布关于资本市场的最新政策通知"]));

        // 19_contests
        let v = strategy_19_contests(r#"<div>{"name":"稳健成长","total_gain":45.6}</div>"#).unwrap();
        assert_eq!(v["xueqiu_contest_top10_playwright"][0]["name"], json!("稳健成长"));
        assert_eq!(v["xueqiu_contest_top10_playwright"][0]["total_gain_pct"], json!(45.6));
    }

    #[test]
    fn trap_strategy_counts_only_keyword_matches() {
        let html = r#"{"title":"水晶光电 老师推荐 必涨"}{"title":"其他公司分析"}{"title":"水晶光电财报解读"}"#;
        let v = strategy_18_trap(html, "水晶光电").unwrap();
        // Both the keyword match and the name match count; the unrelated post does not.
        assert_eq!(v["xhs_trap_post_count_playwright"], json!(2));
        // No matches → None, so the caller records a failure rather than a zero.
        assert!(strategy_18_trap(r#"{"title":"无关内容"}"#, "水晶光电").is_none());
    }

    #[test]
    fn strategies_degrade_on_pages_without_their_markers() {
        for empty in ["", "<html><body>nothing here</body></html>"] {
            assert!(strategy_4_peers(empty).is_none(), "4_peers");
            assert!(strategy_8_materials(empty).is_none(), "8_materials");
            assert!(strategy_15_events(empty).is_none(), "15_events");
            assert!(strategy_17_sentiment(empty).is_none(), "17_sentiment");
            assert!(strategy_3_macro(empty).is_none(), "3_macro");
            assert!(strategy_7_industry(empty).is_none(), "7_industry");
            assert!(strategy_14_moat(empty).is_none(), "14_moat");
            assert!(strategy_13_policy(empty).is_none(), "13_policy");
            assert!(strategy_18_trap(empty, "水晶光电").is_none(), "18_trap");
            assert!(strategy_19_contests(empty).is_none(), "19_contests");
        }
    }

    #[test]
    fn moat_strategy_extracts_intro_and_info_pairs() {
        let html = r#"
          <div class="lemma-summary J-summary"><b>公司</b>是一家光学薄膜企业。</div>
          <dt class="basicInfo-item name">主营业务</dt><dd class="basicInfo-item value">光学元件</dd>
        "#;
        let v = strategy_14_moat(html).unwrap();
        // Tags are stripped from the intro.
        assert_eq!(v["baike_intro_playwright"], json!("公司是一家光学薄膜企业。"));
        assert_eq!(v["baike_basic_info_playwright"]["主营业务"], json!("光学元件"));
    }

    #[test]
    fn strategy_urls_are_built_per_dimension() {
        let raw =
            json!({"dimensions": {"0_basic": {"data": {"name": "水晶光电", "industry": "光学光电子"}}}});
        assert_eq!(
            strategy_url("4_peers", "002273.SZ", &raw).unwrap(),
            "https://xueqiu.com/S/SZ002273"
        );
        assert_eq!(
            strategy_url("17_sentiment", "002273.SZ", &raw).unwrap(),
            "https://xueqiu.com/S/SZ002273/POST"
        );
        assert!(strategy_url("8_materials", "002273.SZ", &raw)
            .unwrap()
            .contains("code=002273"));
        assert!(strategy_url("15_events", "002273.SZ", &raw)
            .unwrap()
            .contains("stockCode=002273"));
        assert!(strategy_url("14_moat", "002273.SZ", &raw).unwrap().contains("baike.baidu.com"));
        assert!(strategy_url("7_industry", "002273.SZ", &raw).unwrap().contains("baidu.com/s"));
        // Dimensions with no strategy, and name-dependent strategies with no name.
        assert!(strategy_url("99_nope", "002273.SZ", &raw).is_none());
        let bare = json!({"dimensions": {"0_basic": {"data": {}}}});
        assert!(strategy_url("14_moat", "002273.SZ", &bare).is_none());
        assert!(strategy_url("7_industry", "002273.SZ", &bare).is_none());
    }

    /// The end-to-end decision path: only unhealthy dims are fetched, healthy ones
    /// are skipped without a request, and results merge into `raw`.
    #[test]
    fn autofill_fetches_only_unhealthy_dims_and_merges_results() {
        let mut raw = json!({"dimensions": {
            "15_events": {"data": {}},
            "8_materials": {"data": {"a": 1, "b": 2}},
            "13_policy": {"data": {"c": 3, "d": 4}}
        }});
        let dims: BTreeSet<String> = ["15_events", "8_materials", "13_policy"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let net = json!({"domestic_ok": true, "search_ok": true});

        let mut fetched: Vec<String> = Vec::new();
        let summary = autofill_with(&mut raw, "002273.SZ", &dims, Some(&net), false, |url, _| {
            fetched.push(url.to_string());
            Some(r#"<div class="announcement-title">2026年第一季度报告</div>"#.to_string())
        });

        // Only 15_events was empty; the two healthy dims were skipped.
        assert_eq!(summary.attempted, 1, "{summary:?}");
        assert_eq!(summary.succeeded, 1, "{summary:?}");
        assert_eq!(summary.skipped, 2, "{summary:?}");
        assert_eq!(fetched.len(), 1);
        assert!(fetched[0].contains("cninfo"), "{fetched:?}");

        // The parsed field landed in the dimension, with the source marker.
        assert_eq!(
            raw["dimensions"]["15_events"]["data"]["event_titles_playwright"],
            json!(["2026年第一季度报告"])
        );
        assert_eq!(
            raw["dimensions"]["15_events"]["source"],
            json!("playwright_fallback")
        );
        // Untouched dims kept their data and gained no source marker.
        assert_eq!(raw["dimensions"]["8_materials"]["data"]["a"], json!(1));
        assert!(raw["dimensions"]["8_materials"].get("source").is_none());
    }

    #[test]
    fn autofill_force_ignores_quality_and_records_fetch_failures() {
        let mut raw = json!({"dimensions": {"13_policy": {"data": {"a": 1, "b": 2}}}});
        let dims: BTreeSet<String> = ["13_policy"].iter().map(|s| s.to_string()).collect();
        let net = json!({"domestic_ok": true, "search_ok": true});

        // FORCE overrides the healthy-data skip...
        let s = autofill_with(&mut raw, "002273.SZ", &dims, Some(&net), true, |_, _| {
            Some(r#"<a title="证监会发布关于资本市场的最新政策通知">x</a>"#.to_string())
        });
        assert_eq!((s.attempted, s.succeeded), (1, 1), "{s:?}");

        // ...and a fetch failure is counted, not fatal.
        let mut raw2 = json!({"dimensions": {"13_policy": {"data": {}}}});
        let s = autofill_with(&mut raw2, "002273.SZ", &dims, Some(&net), false, |_, _| None);
        assert_eq!((s.attempted, s.failed, s.succeeded), (1, 1, 0), "{s:?}");
        // Nothing was merged on failure.
        assert!(raw2["dimensions"]["13_policy"]["data"]
            .get("csrc_policy_titles_playwright")
            .is_none());
    }

    #[test]
    fn autofill_skips_network_unreachable_dims_without_fetching() {
        let mut raw = json!({"dimensions": {"7_industry": {"data": {}}}});
        let dims: BTreeSet<String> = ["7_industry"].iter().map(|s| s.to_string()).collect();
        let net = json!({"domestic_ok": true, "search_ok": false});

        let mut calls = 0;
        let s = autofill_with(&mut raw, "002273.SZ", &dims, Some(&net), false, |_, _| {
            calls += 1;
            Some(String::new())
        });
        assert_eq!(calls, 0, "an unreachable dim must not be fetched");
        assert_eq!(s.skipped, 1);
        assert!(s.skip_reasons["7_industry"].as_str().unwrap().contains("search 不通"), "{s:?}");
    }

    #[test]
    fn junk_autofill_output_is_rejected() {
        let mut raw = json!({"dimensions": {"13_policy": {"data": {}}}});
        let dims: BTreeSet<String> = ["13_policy"].iter().map(|s| s.to_string()).collect();
        let net = json!({"domestic_ok": true, "search_ok": true});

        // "类型；类型" is in the junk filter, so the result is discarded.
        let s = autofill_with(&mut raw, "002273.SZ", &dims, Some(&net), true, |_, _| {
            Some(r#"<a title="类型；类型">类型；类型</a>"#.to_string())
        });
        assert_eq!((s.succeeded, s.failed), (0, 1), "{s:?}");
    }

    #[test]
    fn summary_serialises_with_upstream_keys() {
        let s = Summary {
            enabled: true,
            attempted: 3,
            succeeded: 2,
            failed: 1,
            skipped: 4,
            ..Default::default()
        };
        let v = s.to_value();
        for key in [
            "enabled",
            "attempted",
            "succeeded",
            "failed",
            "skipped",
            "skip_reasons",
            "disabled_reason",
        ] {
            assert!(v.get(key).is_some(), "missing {key}");
        }
    }

    #[test]
    fn every_declared_strategy_has_a_url_and_a_parser() {
        let raw =
            json!({"dimensions": {"0_basic": {"data": {"name": "水晶光电", "industry": "光学光电子"}}}});
        for dim in DIM_STRATEGIES {
            assert!(
                strategy_url(dim, "002273.SZ", &raw).is_some(),
                "{dim} has no URL"
            );
            // A page without the strategy's markers must degrade, not panic.
            assert!(
                parse_for_dim(dim, "<html></html>", &raw).is_none(),
                "{dim} should return None on an empty page"
            );
            assert!(
                !dim_network_requirements(dim).is_empty(),
                "{dim} has no network requirement declared"
            );
        }
    }
}
