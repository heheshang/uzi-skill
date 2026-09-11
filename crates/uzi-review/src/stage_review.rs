//! Port of `review_stage_output.py` — self-review CLI wrapper.

use crate::pyfmt;
use crate::self_review::{format_human, review_all, write_review};
use serde_json::Value;

/// Run the self-review CLI. `args` includes argv\[0\] (the program name).
/// Returns the process exit code: 0 pass, 1 critical, 2 warning, 64 usage.
pub fn run(args: &[String]) -> i32 {
    if args.len() < 2 {
        eprintln!("用法: uzi <ticker> --stage-review");
        return 64;
    }
    let ticker = &args[1];

    let report = review_all(ticker, None);
    let path = write_review(ticker, &report);
    println!("{}", format_human(&report));
    println!("\n→ Issues JSON: {}", path.display());
    println!(
        "→ passed: {}  (critical={}, warning={}, info={})",
        pyfmt::str_exact(report.get("passed").unwrap_or(&Value::Null)),
        pyfmt::str_exact(report.get("critical_count").unwrap_or(&Value::Null)),
        pyfmt::str_exact(report.get("warning_count").unwrap_or(&Value::Null)),
        pyfmt::str_exact(report.get("info_count").unwrap_or(&Value::Null)),
    );

    let crit = report.get("critical_count").and_then(|v| v.as_i64()).unwrap_or(0);
    let warn = report.get("warning_count").and_then(|v| v.as_i64()).unwrap_or(0);
    if crit > 0 {
        println!("\n⛔ BLOCKED: 必须修 critical 后重跑 review 再出 HTML。");
        1
    } else if warn > 0 {
        println!("\n⚠  WARN: warning 级问题存在，建议 agent 在 agent_analysis.review_acknowledged 写明");
        2
    } else {
        println!("\n✓ OK: 全部通过，可以进 HTML 生成。");
        0
    }
}

/// Entry point mirroring `if __name__ == "__main__": main()`.
pub fn main() {
    let args: Vec<String> = std::env::args().collect();
    std::process::exit(run(&args));
}
