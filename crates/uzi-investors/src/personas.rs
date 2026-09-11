//! Port of `lib/investor_personas.py` — the per-investor signature lines used for
//! the panel `comment` field.
//!
//! Upstream picks the line with an unseeded `random.choice`; the Rust port is
//! deterministic:
//!
//! * [`persona_comment_seeded`] indexes the pool with `seed % len(lines)`.
//! * [`persona_comment`] derives that seed from `ctx` — FNV-1a 64 over the
//!   compact JSON encoding of the context dict, so the same stock/context always
//!   yields the same line and every produced line is still a member of the
//!   upstream pool (see `testkit::assert_persona_line_known`).
//!
//! `src/data/persona_pools.json` is `json.dumps(PERSONAS, ensure_ascii=False)`,
//! identical to `tools/golden/expected/*/persona_pools.json`.

use crate::pyhelp::{format_map, Missing};
use serde_json::{Map, Value};
use std::sync::LazyLock;

const POOLS_JSON: &str = include_str!("data/persona_pools.json");

/// `investor_personas._GENERIC_FALLBACK`.
const GENERIC_FALLBACK: &[(&str, &str)] = &[
    ("bullish", "数据支持买入。"),
    ("bearish", "数据不支持。"),
    ("neutral", "先观察。"),
    ("skip", "不在能力圈范围内，不做评价。"),
];

/// `investor_personas.PERSONAS` — `{id: {signal: [line, ...]}}`.
pub fn pools() -> &'static Map<String, Value> {
    static POOLS: LazyLock<Map<String, Value>> = LazyLock::new(|| {
        serde_json::from_str::<Value>(POOLS_JSON)
            .expect("embedded PERSONAS json")
            .as_object()
            .cloned()
            .expect("PERSONAS is a dict")
    });
    &POOLS
}

fn fallback_line(signal: &str) -> &'static str {
    GENERIC_FALLBACK
        .iter()
        .find(|(s, _)| *s == signal)
        .or_else(|| GENERIC_FALLBACK.iter().find(|(s, _)| *s == "neutral"))
        .map(|(_, l)| *l)
        .unwrap_or("先观察。")
}

/// The candidate lines for one `(investor, signal)` pair, or the generic
/// fallback when the investor/signal is unregistered.
pub fn lines_for(investor_id: &str, signal: &str) -> Vec<String> {
    let entry = pools().get(investor_id).and_then(Value::as_object);
    let lines: Vec<String> = entry
        .and_then(|e| e.get(signal))
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();
    if lines.is_empty() {
        return vec![fallback_line(signal).to_string()];
    }
    lines
}

/// `investor_personas.get_comment` with an explicit pool index.
///
/// `seed % len(lines)` selects the line (upstream: `random.choice`). The line is
/// then formatted with `ctx.get(key, default)` for
/// `roe/pe/price/name/industry/growth/stage`; a template that needs any other
/// field falls back to the raw line, exactly like upstream's `KeyError` path.
pub fn persona_comment_seeded(investor_id: &str, signal: &str, ctx: &Value, seed: u64) -> String {
    let lines = lines_for(investor_id, signal);
    let line = &lines[(seed % lines.len() as u64) as usize];

    let mut map = Map::new();
    for (key, default) in [
        ("roe", "—"),
        ("pe", "—"),
        ("price", "—"),
        ("name", "这只票"),
        ("industry", "该行业"),
        ("growth", "—"),
        ("stage", "—"),
    ] {
        let value = ctx
            .get(key)
            .cloned()
            .unwrap_or_else(|| Value::String(default.to_string()));
        map.insert(key.to_string(), value);
    }

    format_map(line, &|k| map.get(k).cloned(), Missing::Error)
        .unwrap_or_else(|_| line.clone())
}

/// `investor_personas.get_comment` — deterministic seed derived from `ctx`.
pub fn persona_comment(investor_id: &str, signal: &str, ctx: &Value) -> String {
    persona_comment_seeded(investor_id, signal, ctx, ctx_seed(ctx))
}

/// FNV-1a 64 over the compact JSON encoding of the context.
///
/// The upstream line is chosen by an unseeded RNG, so any stable function of the
/// inputs is a faithful, reproducible substitute. Hashing the serialized context
/// keeps the choice stable across runs and platforms.
fn ctx_seed(ctx: &Value) -> u64 {
    let bytes = uzi_core::json::to_compact(ctx);
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for b in bytes.as_bytes() {
        hash ^= *b as u64;
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn seeded_pick_is_stable_and_in_pool() {
        let ctx = json!({"roe": "18.2", "pe": "21.5", "name": "贵州茅台", "industry": "白酒"});
        let a = persona_comment_seeded("buffett", "neutral", &ctx, 7);
        let b = persona_comment_seeded("buffett", "neutral", &ctx, 7);
        assert_eq!(a, b);
        assert!(lines_for("buffett", "neutral").contains(&a));
        // different seeds are allowed to (but need not) differ
        let _ = persona_comment_seeded("buffett", "neutral", &ctx, 8);
    }

    #[test]
    fn substitution_uses_upstream_defaults_and_renders_numbers_like_python() {
        // graham neutral is a single line with no placeholders
        assert_eq!(
            persona_comment_seeded("graham", "neutral", &json!({}), 0),
            "数据不齐，严守不达标不买入的纪律。"
        );
        // buffett bullish/bearish templates reference roe/pe
        let line = persona_comment_seeded("buffett", "bullish", &json!({"roe": 18.5}), 0);
        assert_eq!(line, "在我们能力圈里的生意，ROE 18.5% 长期稳得住，就值得持有十年。");
        let line = persona_comment_seeded("buffett", "bearish", &json!({"pe": 42}), 1);
        assert_eq!(line, "PE 42 已经没有安全边际了，等别人恐惧时再看。");
        // unknown investor → generic fallback line
        assert_eq!(persona_comment_seeded("nobody", "bullish", &json!({}), 0), "数据支持买入。");
        assert_eq!(persona_comment_seeded("nobody", "weird", &json!({}), 0), "先观察。");
    }

    #[test]
    fn known_signal_pools_are_present_for_every_investor() {
        for inv in crate::db::investors() {
            let id = inv["id"].as_str().unwrap();
            for signal in ["bullish", "bearish", "neutral"] {
                let lines = lines_for(id, signal);
                assert!(!lines.is_empty(), "{id}/{signal} has no lines");
                let rendered = persona_comment_seeded(id, signal, &json!({"name": "测试"}), 3);
                assert!(!rendered.is_empty());
            }
        }
    }
}
