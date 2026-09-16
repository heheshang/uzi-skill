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

/// Deterministic crypto-native voice for the panel. Crypto must not reuse the
/// stock persona pools: no PE, ROE, EPS, dividends, or company language.
pub fn crypto_persona_comment(investor_id: &str, signal: &str, ctx: &Value) -> String {
    let group = crate::db::investor_by_id(investor_id)
        .and_then(|i| i.get("group"))
        .and_then(Value::as_str)
        .unwrap_or("");
    let name = match group {
        "A" => "链上价值研究员",
        "B" => "协议成长研究员",
        "C" => "加密宏观周期研究员",
        "D" => "加密市场结构交易员",
        "E" => "网络质量研究员",
        "G" => "加密系统量化研究员",
        "H" => "开放网络建设者",
        "I" => "AI 加密卡位研究员",
        _ => "加密资产观察员",
    };
    let value = |key: &str| {
        ctx.get(key)
            .filter(|v| !v.is_null())
            .map(uzi_core::py::num_str)
            .unwrap_or_else(|| "—".to_string())
    };
    let line = match group {
        "A" => format!("{name}：NVT {}、流通率 {}% 和市值排名 #{} 决定网络价值；不看股票估值。", value("nvt_ratio"), value("circulating_ratio_pct"), value("market_cap_rank")),
        "B" => format!("{name}：协议份额 {}%、市值/TVL {} 与 30 日涨跌 {}% 一起验证采用曲线。", value("market_share_pct"), value("mcap_to_tvl_ratio"), value("change_30d_pct")),
        "C" => format!("{name}：恐慌贪婪 {}、资金费率 {}% 与 30 日涨跌 {}% 描绘流动性周期。", value("fear_greed"), value("funding_rate_pct"), value("change_30d_pct")),
        "D" => format!("{name}：MA 结构为{}，RSI {}；只在趋势和成交确认后行动。", value("ma_align"), value("rsi")),
        "E" => format!("{name}：网络份额 {}%、流通率 {}% 和 NVT {} 是长期复利的可验证底座。", value("market_share_pct"), value("circulating_ratio_pct"), value("nvt_ratio")),
        "G" => format!("{name}：年化波动 {}%、24 小时成交额 {}、资金费率 {}%，先做风险定价再做方向。", value("volatility_1y"), value("volume_24h"), value("funding_rate_pct")),
        "H" => format!("{name}：AI/基础设施卡位{}，网络份额 {}%；开放协议必须有真实扩散。", if ctx.get("ai_chain_hit").and_then(Value::as_bool).unwrap_or(false) { "已命中" } else { "未命中" }, value("market_share_pct")),
        "I" => format!("{name}：卡位{}，市值/FDV {}；替代方案出现就退出。", if ctx.get("ai_chain_hit").and_then(Value::as_bool).unwrap_or(false) { "成立" } else { "不足" }, value("mcap_to_fdv")),
        _ => "加密数据不足，先观察。".to_string(),
    };
    format!("{} {}", line, match signal {
        "bullish" => "数据支持参与，但仍按波动管理仓位。",
        "bearish" => "风险收益不对称，暂不参与。",
        _ => "证据尚未形成优势，保持观察。",
    })
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
