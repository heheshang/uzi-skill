//! Port of `lib/network_preflight.py` — 3-group (domestic / overseas / search)
//! TCP reachability probe plus structured `NetworkProfile` and diagnostics.
//!
//! The port keeps the probe list, group counts, severity thresholds,
//! recommendation text and the per-group `diagnose_source` fix lists verbatim,
//! and writes the same `.cache/_global/network_profile.json` cache.

use serde_json::{json, Value};
use std::net::{TcpStream, ToSocketAddrs};
use std::time::{Duration, Instant};

pub const DOMESTIC_TARGETS: &[(&str, &str)] = &[
    ("push2.eastmoney.com", "东财 push2 · A 股行情主源"),
    ("www.cninfo.com.cn", "巨潮 cninfo · 公告/行业 PE"),
    ("stock.xueqiu.com", "雪球数据"),
];

pub const OVERSEAS_TARGETS: &[(&str, &str)] = &[
    ("query1.finance.yahoo.com", "Yahoo Finance · 美股数据"),
    ("api.coingecko.com", "CoinGecko · 加密市场流动性"),
    ("baike.baidu.com", "百度百科 · 公司词条（国内直连但海外代理可能反被拦）"),
];

pub const SEARCH_TARGETS: &[(&str, &str)] = &[
    ("duckduckgo.com", "DuckDuckGo · ddgs"),
    ("www.baidu.com", "百度搜索"),
    ("api.github.com", "GitHub API · 通用网络健康度"),
];

/// Local proxy ports upstream probes (`_LOCAL_PROXY_PORTS`).
pub const LOCAL_PROXY_PORTS: &[(u16, &str)] = &[
    (7890, "Clash (HTTP)"),
    (7891, "Clash (SOCKS)"),
    (7897, "Clash Verge"),
    (10808, "V2rayN"),
    (1080, "Shadowsocks / SOCKS5 通用"),
    (8888, "Charles / Fiddler"),
];

/// `_probe(domain, port, timeout)` — a TCP connect from a fixed host.
pub fn probe(domain: &str, port: u16, timeout: f64) -> Value {
    let t0 = Instant::now();
    let addr = (domain, port).to_socket_addrs().map(|mut a| a.next());
    match addr {
        Err(e) => json!({
            "domain": domain,
            "group": "",
            "reachable": false,
            "latency_ms": (t0.elapsed().as_secs_f64() * 1000.0) as i64,
            "error": format!("DNS fail: {e}"),
            "purpose": "",
        }),
        Ok(None) => json!({
            "domain": domain,
            "group": "",
            "reachable": false,
            "latency_ms": (t0.elapsed().as_secs_f64() * 1000.0) as i64,
            "error": "DNS fail: no address",
            "purpose": "",
        }),
        Ok(Some(sock)) => {
            let dur = Duration::from_secs_f64(timeout);
            match TcpStream::connect_timeout(&sock, dur) {
                Ok(_) => json!({
                    "domain": domain,
                    "group": "",
                    "reachable": true,
                    "latency_ms": (t0.elapsed().as_secs_f64() * 1000.0) as i64,
                    "error": "",
                    "purpose": "",
                }),
                Err(e) => json!({
                    "domain": domain,
                    "group": "",
                    "reachable": false,
                    "latency_ms": (t0.elapsed().as_secs_f64() * 1000.0) as i64,
                    "error": format!("{}: {}", err_kind(&e), truncate(&e.to_string(), 80)),
                    "purpose": "",
                }),
            }
        }
    }
}

fn err_kind(e: &std::io::Error) -> &'static str {
    match e.kind() {
        std::io::ErrorKind::TimedOut => "timeout",
        std::io::ErrorKind::ConnectionRefused => "ConnectionRefusedError",
        std::io::ErrorKind::NotFound => "gaierror",
        _ => "OSError",
    }
}

/// `_detect_proxy()` → `(has_proxy, proxy_url)`.
pub fn detect_proxy() -> (bool, String) {
    for var in [
        "HTTPS_PROXY",
        "HTTP_PROXY",
        "ALL_PROXY",
        "https_proxy",
        "http_proxy",
        "all_proxy",
    ] {
        let v = std::env::var(var).unwrap_or_default().trim().to_string();
        if !v.is_empty() {
            let lower = v.to_lowercase();
            if !["off", "no", "false"].contains(&lower.as_str()) {
                return (true, format!("{var}={v}"));
            }
        }
    }
    (false, String::new())
}

/// `_detect_local_proxy()`.
pub fn detect_local_proxy() -> Value {
    let mut detected: Vec<Value> = Vec::new();
    for (port, name) in LOCAL_PROXY_PORTS {
        if TcpStream::connect_timeout(
            &([127, 0, 0, 1], *port).into(),
            Duration::from_millis(300),
        )
        .is_ok()
        {
            detected.push(json!({"port": port, "name": name}));
        }
    }
    let (has_env_proxy, _) = detect_proxy();
    let hint = if !detected.is_empty() && !has_env_proxy {
        let names = detected
            .iter()
            .filter_map(|d| d.get("name").and_then(|n| n.as_str()))
            .collect::<Vec<_>>()
            .join(", ");
        let port = detected[0].get("port").and_then(|p| p.as_u64()).unwrap_or(7890);
        format!(
            "⚠ 检测到本地代理运行（{names}）但 env 未设 HTTPS_PROXY · \
             脚本默认不走代理 · 若海外源不通请：\n   export HTTPS_PROXY=http://127.0.0.1:{port} && export HTTP_PROXY=http://127.0.0.1:{port}"
        )
    } else if detected.is_empty() && has_env_proxy {
        "⚠ HTTPS_PROXY 已设但本地代理端口未响应 · 代理可能没启动 · unset 后重试".to_string()
    } else {
        String::new()
    };
    json!({"has_local_proxy": !detected.is_empty(), "detected": detected, "hint": hint})
}

/// `_build_recommendation(profile)` → `(recommendation, severity)`.
pub fn build_recommendation(p: &Value) -> (String, String) {
    let g = |k: &str| p.get(k).and_then(|v| v.as_u64()).unwrap_or(0);
    let (dc, oc, sc) = (g("domestic_count"), g("overseas_count"), g("search_count"));
    let sev = if dc >= 2 && oc >= 2 && sc >= 2 {
        "ok"
    } else if dc >= 2 && sc >= 1 {
        "warning"
    } else if dc >= 1 {
        "degraded"
    } else {
        "critical"
    };
    let domestic_ok = p.get("domestic_ok").and_then(|v| v.as_bool()).unwrap_or(false);
    let overseas_ok = p.get("overseas_ok").and_then(|v| v.as_bool()).unwrap_or(false);
    let search_ok = p.get("search_ok").and_then(|v| v.as_bool()).unwrap_or(false);
    let rec = if domestic_ok && overseas_ok && search_ok {
        "✓ 全网通畅 · Playwright 可抓境内+境外所有源".to_string()
    } else if domestic_ok && !overseas_ok && search_ok {
        "✓ 国内网络正常 · ✗ 境外受限 · Playwright 只抓国内源（东财 F10 / cninfo / 雪球 public / 百度搜索）· 跳过 Yahoo / CoinGecko / 英文 Wikipedia".to_string()
    } else if domestic_ok && !overseas_ok && !search_ok {
        "⚠ 国内通 · 搜索受限 · 境外不通 · Playwright 只抓国内无搜索依赖的源（东财 F10 / cninfo / 雪球 page）· 跳 baidu/百度搜索/Yahoo/CoinGecko".to_string()
    } else if !domestic_ok && overseas_ok {
        "⚠ 境外 VPN 环境 · 国内源受限 · Playwright 可抓 Yahoo/CoinGecko · 但不抓东财 push2（国内限外 IP）· 建议 akshare 配合用 xueqiu fallback".to_string()
    } else if dc >= 1 {
        "⚠ 网络不稳 · 建议 --depth lite + 跳过 Playwright 省时间".to_string()
    } else {
        "🔴 网络严重不通 · 建议退出修网络 · lite 模式也会失败".to_string()
    };
    (rec, sev.to_string())
}

/// `diagnose_source(profile)`.
pub fn diagnose_source(p: &Value) -> Vec<Value> {
    let mut out = Vec::new();
    let dc = p.get("domestic_count").and_then(|v| v.as_u64()).unwrap_or(0);
    if p.get("domestic_ok").and_then(|v| v.as_bool()) != Some(true) {
        out.push(json!({
            "group": "domestic",
            "status": "🔴 不通",
            "affected_fetchers": ["0_basic", "1_financials", "7_industry", "4_peers",
                                  "15_events", "12_capital_flow", "16_lhb", "6_fund_holders"],
            "affected_count": 8,
            "fix": "主要问题：push2.eastmoney.com / cninfo / xueqiu 全挂 · 绝大多数 fetcher 无法工作\n  1. 检查 VPN 是否指向国内 IP（海外 VPN 会被东财反向 GFW）\n  2. 尝试：unset HTTPS_PROXY HTTP_PROXY ALL_PROXY\n  3. 如用 Clash · 检查规则是否把 *.eastmoney.com / cninfo / xueqiu.com 走 direct",
        }));
    } else if dc < 3 {
        out.push(json!({
            "group": "domestic",
            "status": "⚠ 部分不通",
            "affected_fetchers": ["受影响具体域名见 checks 明细"],
            "affected_count": 3 - dc,
            "fix": "多数 fetcher 仍可工作 · 查 checks 明细看具体是哪个域名不通",
        }));
    }
    if p.get("overseas_ok").and_then(|v| v.as_bool()) != Some(true) {
        out.push(json!({
            "group": "overseas",
            "status": "🔴 不通",
            "affected_fetchers": ["2_kline（美股/港股链）", "4_peers（全球同行）", "19_contests"],
            "affected_count": 3,
            "fix": "Yahoo / CoinGecko 等海外源挂 · 美股 / 港股 / 加密数据会降级\n  1. 确认 VPN 能访问海外（浏览器访问 finance.yahoo.com 测试）\n  2. 中国大陆不需要海外源就能分析 A 股 · 此项可忽略\n  3. 若必须：开 Clash 全局模式 + export HTTPS_PROXY=http://127.0.0.1:7890",
        }));
    }
    if p.get("search_ok").and_then(|v| v.as_bool()) != Some(true) {
        out.push(json!({
            "group": "search",
            "status": "🔴 不通",
            "affected_fetchers": ["14_moat", "7_industry", "13_policy",
                                  "17_sentiment", "18_trap"],
            "affected_count": 5,
            "fix": "DuckDuckGo / 百度搜索 全挂 · 5 个定性维度将降级\n  1. 百度搜索被挡：中国大陆一般能通 · 检查是否 DNS 污染\n  2. DDGS 被挡：走 VPN 或切百度搜索作为唯一源\n  3. 实在不通：设 UZI_SKIP_WS=1 跳过 web search 部分（报告 5 个定性维度标 gap）",
        }));
    }
    out
}

/// `run_preflight(verbose, timeout)` — returns the `NetworkProfile` as JSON and
/// writes `.cache/_global/network_profile.json`.
pub fn run_preflight(verbose: bool, timeout: f64) -> Value {
    let (has_proxy, proxy_url) = detect_proxy();
    let mut results: Vec<Value> = Vec::new();
    for (group, targets) in [
        ("domestic", DOMESTIC_TARGETS),
        ("overseas", OVERSEAS_TARGETS),
        ("search", SEARCH_TARGETS),
    ] {
        for (domain, purpose) in targets {
            let mut check = probe(domain, 443, timeout);
            if let Some(o) = check.as_object_mut() {
                o.insert("group".into(), json!(group));
                o.insert("purpose".into(), json!(purpose));
            }
            results.push(check);
        }
    }

    let count = |g: &str| -> u64 {
        results
            .iter()
            .filter(|r| r.get("group").and_then(|v| v.as_str()) == Some(g))
            .filter(|r| r.get("reachable").and_then(|v| v.as_bool()) == Some(true))
            .count() as u64
    };
    let (dom_ok, ovs_ok, sch_ok) = (count("domestic"), count("overseas"), count("search"));
    let latencies: Vec<i64> = results
        .iter()
        .filter(|r| r.get("reachable").and_then(|v| v.as_bool()) == Some(true))
        .filter_map(|r| r.get("latency_ms").and_then(|v| v.as_i64()))
        .collect();
    let avg_lat = if latencies.is_empty() {
        0
    } else {
        (latencies.iter().sum::<i64>() as f64 / latencies.len() as f64) as i64
    };

    let local_proxy = detect_local_proxy();
    let mut prof = json!({
        "domestic_ok": dom_ok >= 2,
        "overseas_ok": ovs_ok >= 2,
        "search_ok": sch_ok >= 2,
        "has_proxy": has_proxy,
        "proxy_url": proxy_url,
        "domestic_count": dom_ok,
        "overseas_count": ovs_ok,
        "search_count": sch_ok,
        "avg_latency_ms": avg_lat,
        "recommendation": "",
        "severity": "ok",
        "probed_at": now_secs(),
        "checks": results,
        "local_proxy": local_proxy,
        "diagnostics": [],
    });
    let (rec, sev) = build_recommendation(&prof);
    let diags = diagnose_source(&prof);
    if let Some(o) = prof.as_object_mut() {
        o.insert("recommendation".into(), json!(rec));
        o.insert("severity".into(), json!(sev));
        o.insert("diagnostics".into(), json!(diags));
    }

    if verbose {
        // Upstream prints a human report; the Rust port keeps the same summary
        // on stderr so library consumers keep stdout clean.
        eprintln!(
            "\n🌐 网络预检 ({}/9 通 · 均延迟 {avg_lat}ms · proxy={})\n  {}",
            results
                .iter()
                .filter(|r| r.get("reachable").and_then(|v| v.as_bool()) == Some(true))
                .count(),
            if has_proxy { "yes" } else { "no" },
            prof["recommendation"].as_str().unwrap_or("")
        );
    }

    let dir = uzi_core::cache::cache_root().join("_global");
    let _ = std::fs::create_dir_all(&dir);
    let _ = std::fs::write(
        dir.join("network_profile.json"),
        serde_json::to_string_pretty(&prof).unwrap_or_default(),
    );
    prof
}

/// `get_network_profile(max_age_sec)`.
pub fn get_network_profile(max_age_sec: u64) -> Value {
    let file = uzi_core::cache::cache_root()
        .join("_global")
        .join("network_profile.json");
    if let Ok(raw) = std::fs::read_to_string(&file) {
        if let Ok(data) = serde_json::from_str::<Value>(&raw) {
            let probed_at = data.get("probed_at").and_then(|v| v.as_f64()).unwrap_or(0.0);
            if now_secs() - probed_at < max_age_sec as f64 {
                return data;
            }
        }
    }
    run_preflight(false, 3.0)
}

/// `run_preflight_legacy_dict(verbose, timeout)`.
pub fn run_preflight_legacy_dict(verbose: bool, timeout: f64) -> Value {
    let prof = run_preflight(verbose, timeout);
    let ok = prof.get("domestic_count").and_then(|v| v.as_u64()).unwrap_or(0)
        + prof.get("overseas_count").and_then(|v| v.as_u64()).unwrap_or(0)
        + prof.get("search_count").and_then(|v| v.as_u64()).unwrap_or(0);
    json!({
        "reachable": ok,
        "failures": 9 - ok,
        "critical_failures": 9 - ok,
        "avg_latency_ms": prof.get("avg_latency_ms").cloned().unwrap_or(json!(0)),
        "advisory": prof.get("recommendation").cloned().unwrap_or(json!("")),
        "severity": prof.get("severity").cloned().unwrap_or(json!("ok")),
        "results": prof.get("checks").cloned().unwrap_or(json!([])),
    })
}

fn now_secs() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

fn truncate(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

/// Marker used by tests to assert the profile is well formed.
pub fn profile_field_order() -> &'static [&'static str] {
    &[
        "domestic_ok",
        "overseas_ok",
        "search_ok",
        "has_proxy",
        "proxy_url",
        "domestic_count",
        "overseas_count",
        "search_count",
        "avg_latency_ms",
        "recommendation",
        "severity",
        "probed_at",
        "checks",
        "local_proxy",
        "diagnostics",
    ]
}

/// Build an all-unreachable profile without touching the network (test helper).
#[doc(hidden)]
pub fn offline_profile() -> Value {
    let mut p = json!({
        "domestic_ok": false,
        "overseas_ok": false,
        "search_ok": false,
        "has_proxy": false,
        "proxy_url": "",
        "domestic_count": 0,
        "overseas_count": 0,
        "search_count": 0,
        "avg_latency_ms": 0,
    });
    let (rec, sev) = build_recommendation(&p);
    let diags = diagnose_source(&p);
    if let Some(o) = p.as_object_mut() {
        o.insert("recommendation".into(), json!(rec));
        o.insert("severity".into(), json!(sev));
        o.insert("diagnostics".into(), json!(diags));
    }
    p
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Map;

    #[test]
    fn target_groups_have_nine_domains() {
        assert_eq!(
            DOMESTIC_TARGETS.len() + OVERSEAS_TARGETS.len() + SEARCH_TARGETS.len(),
            9
        );
    }

    #[test]
    fn severity_and_recommendation_thresholds() {
        let mut p = Map::new();
        p.insert("domestic_count".into(), json!(3));
        p.insert("overseas_count".into(), json!(3));
        p.insert("search_count".into(), json!(3));
        p.insert("domestic_ok".into(), json!(true));
        p.insert("overseas_ok".into(), json!(true));
        p.insert("search_ok".into(), json!(true));
        let (rec, sev) = build_recommendation(&Value::Object(p.clone()));
        assert_eq!(sev, "ok");
        assert!(rec.starts_with('✓'));

        let mut bad = p.clone();
        bad.insert("domestic_count".into(), json!(0));
        bad.insert("domestic_ok".into(), json!(false));
        let (_, sev) = build_recommendation(&Value::Object(bad));
        assert_eq!(sev, "critical");
    }

    #[test]
    fn offline_profile_diagnostics_cover_all_groups() {
        let p = offline_profile();
        let diags = p["diagnostics"].as_array().unwrap();
        assert_eq!(diags.len(), 3);
        assert_eq!(p["severity"], json!("critical"));
    }
}
