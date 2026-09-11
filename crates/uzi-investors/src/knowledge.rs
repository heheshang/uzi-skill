//! Port of `lib/investor_knowledge.py` — market scope, known holdings and
//! industry affinity, combined by `reality_check`.
//!
//! `src/data/knowledge.json` holds `{market_scope, known_holdings,
//! industry_affinity}` dumped verbatim from the upstream module.

use serde_json::{json, Value};
use std::sync::LazyLock;

const KNOWLEDGE_JSON: &str = include_str!("data/knowledge.json");

fn tables() -> &'static Value {
    static TABLES: LazyLock<Value> =
        LazyLock::new(|| serde_json::from_str(KNOWLEDGE_JSON).expect("embedded knowledge json"));
    &TABLES
}

/// `investor_knowledge.market_match` — `scope == "all"` accepts everything.
pub fn market_match(investor_id: &str, market: &str) -> bool {
    let scope = tables()
        .get("market_scope")
        .and_then(|s| s.get(investor_id))
        .and_then(Value::as_str)
        .unwrap_or("all");
    if scope == "all" {
        return true;
    }
    scope.to_uppercase().contains(&market.to_uppercase())
}

/// `investor_knowledge.check_known_holdings` — first `(attitude, note)` whose
/// ticker pattern appears in the ticker or the name.
pub fn check_known_holdings(investor_id: &str, ticker: &str, name: &str) -> Option<(String, String)> {
    let list = tables().get("known_holdings")?.get(investor_id)?.as_array()?;
    for entry in list {
        let arr = entry.as_array()?;
        let pattern = arr.first()?.as_str()?;
        if ticker.contains(pattern) || name.contains(pattern) {
            return Some((arr.get(1)?.as_str()?.to_string(), arr.get(2)?.as_str()?.to_string()));
        }
    }
    None
}

/// `investor_knowledge.compute_affinity` — `min(10, love*4) - min(10, hate*5)`.
pub fn compute_affinity(investor_id: &str, industry: &str, name: &str) -> i64 {
    let Some(info) = tables().get("industry_affinity").and_then(|a| a.get(investor_id)) else {
        return 0;
    };
    let text = format!("{} {}", industry, name).to_lowercase();
    let hits = |key: &str| -> i64 {
        info.get(key)
            .and_then(Value::as_array)
            .map(|list| {
                list.iter()
                    .filter_map(Value::as_str)
                    .filter(|kw| text.contains(&kw.to_lowercase()))
                    .count() as i64
            })
            .unwrap_or(0)
    };
    std::cmp::min(10, hits("love") * 4) - std::cmp::min(10, hits("hate") * 5)
}

/// `investor_knowledge.reality_check`.
///
/// Key order matches upstream: `should_evaluate`, `skip_reason`,
/// `holding_match`, `affinity_adjust`, `override_signal`. `holding_match` is the
/// `(attitude, note)` pair rendered as a two-element JSON array so the evaluator
/// can unpack it across the crate boundary.
pub fn reality_check(investor_id: &str, market: &str, ticker: &str, name: &str, industry: &str) -> Value {
    let mut result = json!({
        "should_evaluate": true,
        "skip_reason": Value::Null,
        "holding_match": Value::Null,
        "affinity_adjust": 0,
        "override_signal": Value::Null,
    });

    if !market_match(investor_id, market) {
        result["should_evaluate"] = Value::Bool(false);
        // Only the crypto venue gets a localised label; other markets keep the
        // upstream `不看{market}市场` string verbatim.
        let reason = if market.eq_ignore_ascii_case("C") {
            "不看加密市场".to_string()
        } else {
            format!("不看{}市场", market)
        };
        result["skip_reason"] = Value::String(reason);
        return result;
    }

    if let Some((attitude, note)) = check_known_holdings(investor_id, ticker, name) {
        result["holding_match"] = json!([attitude, note]);
        if attitude == "held" || attitude == "bullish_known" {
            result["override_signal"] = Value::String("bullish".into());
            result["affinity_adjust"] = json!(15);
        }
    }

    let affinity = compute_affinity(investor_id, industry, name);
    let adj = result["affinity_adjust"].as_i64().unwrap_or(0) + affinity;
    result["affinity_adjust"] = json!(adj);
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn youzi_are_a_share_only() {
        assert!(market_match("zhao_lg", "A"));
        assert!(!market_match("zhao_lg", "US"));
        assert!(market_match("buffett", "US"));
        assert!(!market_match("thorp", "A")); // 索普 is US scope
        assert!(market_match("thorp", "US"));
        assert!(!market_match("thorp", "HK"));
    }

    #[test]
    fn reality_check_reproduces_holding_bonus() {
        let r = reality_check("buffett", "US", "AAPL", "苹果", "消费电子");
        assert_eq!(r["should_evaluate"], true);
        assert_eq!(r["override_signal"], "bullish");
        assert_eq!(r["holding_match"], json!(["held", "伯克希尔第一大持仓，2016年起买入，多次加仓"]));
        // holding bonus +15 plus the 消费 keyword in buffett's love list (+4)
        assert_eq!(r["affinity_adjust"], 19);

        // 段永平 × 周期化工: hate hit -5, and US-scope 游资 skipped
        assert_eq!(reality_check("duan", "A", "600028", "中国石化", "化工").get("should_evaluate").unwrap(), true);
        assert!(reality_check("duan", "A", "600028", "中国石化", "化工")["affinity_adjust"].as_i64().unwrap() < 0);
        let skip = reality_check("zhao_lg", "US", "AAPL", "苹果", "消费电子");
        assert_eq!(skip["should_evaluate"], false);
        assert_eq!(skip["skip_reason"], "不看US市场");
    }

    #[test]
    fn affinity_is_clamped_to_ten() {
        // Wood loves AI/半导体/软件/云: 4 love hits → capped at +10
        assert_eq!(compute_affinity("wood", "AI半导体软件云", ""), 10);
        assert_eq!(compute_affinity("buffett", "量子加密", ""), -10);
        assert_eq!(compute_affinity("nobody", "AI", ""), 0);
    }
}
