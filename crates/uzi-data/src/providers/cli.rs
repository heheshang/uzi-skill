//! Port of `lib/providers/__main__.py` — provider health / chain inspector.
//!
//! Usage: `python -m lib.providers [health|chain [market] [dim...]]`; the Rust
//! entry point is [`main`], called by the `fetch_one` example with argument
//! `--providers`.

use super::{health_check, provider_chain};

/// `cmd_health()` — printed table of every provider.
pub fn cmd_health() -> String {
    let h = health_check();
    let mut out = String::new();
    out.push('\n');
    out.push_str(&"─".repeat(60));
    out.push_str("\n  Provider 健康度 (v2.10.6)\n");
    out.push_str(&"─".repeat(60));
    out.push('\n');
    out.push_str(&format!(
        "\n  {:<12} {:<8} {:<8} {:<10}  status\n",
        "name", "avail", "key req", "markets"
    ));
    out.push_str(&format!(
        "  {:<12} {:<8} {:<8} {:<10}  {}\n",
        "-".repeat(12),
        "-".repeat(8),
        "-".repeat(8),
        "-".repeat(10),
        "-".repeat(30)
    ));
    let mut names: Vec<&String> = h.as_object().map(|o| o.keys().collect()).unwrap_or_default();
    names.sort();
    for name in names {
        let info = &h[name];
        let markets = info["markets"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str())
                    .collect::<Vec<_>>()
                    .join(",")
            })
            .unwrap_or_default();
        let req = if info["requires_key"].as_bool().unwrap_or(false) {
            "yes"
        } else {
            "no"
        };
        let avail = if info["available"].as_bool().unwrap_or(false) {
            "✓"
        } else {
            "✗"
        };
        out.push_str(&format!(
            "  {:<12} {:<8} {:<8} {:<10}  {}\n",
            name,
            avail,
            req,
            markets,
            info["status"].as_str().unwrap_or("?")
        ));
    }
    out.push('\n');
    out.push_str(&"─".repeat(60));
    out.push_str("\n  启用建议\n");
    out.push_str(&"─".repeat(60));
    out.push('\n');
    if h.get("tushare")
        .and_then(|t| t.get("available"))
        .and_then(|a| a.as_bool())
        != Some(true)
    {
        out.push_str("  · Tushare (A 股最稳官方源) 未启用：\n");
        out.push_str("    1. https://tushare.pro 注册 → 复制 token\n");
        out.push_str("    2. export TUSHARE_TOKEN=<your>\n");
    }
    out
}

/// `cmd_chain(market, dims)`.
pub fn cmd_chain(market: &str, dims: &[String]) -> String {
    let mut out = String::new();
    out.push('\n');
    out.push_str(&"─".repeat(60));
    out.push_str(&format!("\n  Provider 优先级链 · market={market}\n"));
    out.push_str(&"─".repeat(60));
    out.push('\n');
    for d in dims {
        let chain = provider_chain(d, market);
        let env_override = std::env::var(format!("UZI_PROVIDERS_{}", d.to_uppercase())).ok();
        let hint = env_override
            .map(|v| format!("  [UZI_PROVIDERS_{}={v}]", d.to_uppercase()))
            .unwrap_or_default();
        let names = if chain.is_empty() {
            "(无可用 provider)".to_string()
        } else {
            chain
                .iter()
                .map(|p| p.name)
                .collect::<Vec<_>>()
                .join(" → ")
        };
        out.push_str(&format!("  {d:<14} {names}{hint}\n"));
    }
    out
}

/// `main()` — returns the CLI exit code and prints to stdout.
pub fn main(argv: &[String]) -> i32 {
    if argv.is_empty() || argv[0] == "health" || argv[0] == "-h" {
        print!("{}", cmd_health());
        return 0;
    }
    if argv[0] == "chain" {
        let market = argv.get(1).map(String::as_str).unwrap_or("A");
        let dims: Vec<String> = if argv.len() > 2 {
            argv[2..].to_vec()
        } else {
            vec![
                "kline".into(),
                "financials".into(),
                "basic".into(),
                "lhb".into(),
            ]
        };
        print!("{}", cmd_chain(market, &dims));
        return 0;
    }
    println!("未知子命令: {}", argv[0]);
    println!("用法: uzi --providers [health|chain [market] [dim...]]");
    2
}
