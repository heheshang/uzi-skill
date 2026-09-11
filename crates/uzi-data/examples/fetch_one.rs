//! Smoke harness for the data layer: fetch one ticker and print what came back.
//!
//! ```text
//! cargo run -p uzi-data --example fetch_one -- 600519.SH
//! cargo run -p uzi-data --example fetch_one -- --providers
//! cargo run -p uzi-data --example fetch_one -- --providers chain A kline
//! ```
//!
//! With a ticker it runs [`uzi_data::collect`] and prints the `0_basic` payload —
//! the quote endpoint — so a live check shows real numbers rather than a
//! pass/fail. `--providers` forwards the remaining arguments to
//! [`uzi_data::providers::cli::main`] (see that module's header), which prints
//! provider health and the per-market priority chain.
//!
//! The two modes exist because both answer "is the data layer actually working?"
//! on a machine where the sandbox may or may not reach the endpoints: one shows
//! parsed values, the other which provider answered.

use std::process::ExitCode;

use serde_json::Value;

/// Environment override for the fetch fan-out, matching the CLI's default.
const DEFAULT_MAX_WORKERS: usize = 6;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();

    if args.is_empty() {
        eprintln!("用法: fetch_one <ticker> | --providers [health|chain [market] [dim...]]");
        return ExitCode::from(2);
    }

    if args[0] == "--providers" {
        return ExitCode::from(uzi_data::providers::cli::main(&args[1..]) as u8);
    }

    let ticker = &args[0];
    let raw = uzi_data::collect(ticker, None, DEFAULT_MAX_WORKERS, None);
    print_basic(ticker, &raw)
}

/// Print the resolved ticker plus the `0_basic` dimension.
///
/// A dimension is reported as `{data, source, fallback}`; on an unreachable
/// endpoint `data` is empty and `fallback` is true, which is itself the useful
/// signal this harness exists to surface.
fn print_basic(ticker: &str, raw: &Value) -> ExitCode {
    let full = raw
        .get("ticker")
        .and_then(|v| v.as_str())
        .unwrap_or(ticker);
    println!("ticker: {ticker} → {full}");

    let Some(dim) = raw.get("dimensions").and_then(|d| d.get("0_basic")) else {
        eprintln!("0_basic 缺失: collect 未返回该维度");
        return ExitCode::FAILURE;
    };

    println!("source:   {}", dim.get("source").and_then(|v| v.as_str()).unwrap_or("—"));
    println!(
        "fallback: {}",
        dim.get("fallback").and_then(|v| v.as_bool()).unwrap_or(false)
    );
    if let Some(quality) = dim
        .get("_pipeline")
        .and_then(|p| p.get("quality"))
        .and_then(|v| v.as_str())
    {
        println!("quality:  {quality}");
    }

    let data = dim.get("data").cloned().unwrap_or(Value::Null);
    match data.as_object() {
        Some(map) if !map.is_empty() => {
            println!("data ({} 字段):", map.len());
            for (k, v) in map {
                println!("  {k}: {v}");
            }
            ExitCode::SUCCESS
        }
        _ => {
            println!("data: 空（端点不可达或返回为空）");
            ExitCode::FAILURE
        }
    }
}
