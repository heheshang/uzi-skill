//! Port of `preview_with_mock.py` — build a fully-populated mock report to
//! preview the HTML template without touching the network.
//!
//! Upstream constructs four task artifacts (`raw_data`, `dimensions`, `panel`,
//! `synthesis`), writes them to `.cache/MOCK.SZ/`, and calls
//! `assemble_report.assemble`.
//!
//! Split of responsibilities here, chosen so the *logic* is ported and the
//! *prose* is not re-typed:
//!
//! * `raw_data` / `dimensions` are pure literal fixtures → checked in under
//!   `assets/mock/` (captured from upstream by `tools/golden/dump_mock.py`).
//! * `panel` is **generated here**: upstream seeds CPython's `random` with 42
//!   and draws per investor, so [`uzi_core::pyrandom`] reproduces it exactly.
//! * `synthesis` is the literal fixture, except for the five fields upstream
//!   derives from the panel (consensus, the bull/bear debate pairing, and the
//!   great-divide scores), which are recomputed so the mock stays self-consistent.

use serde_json::{json, Map, Value};

use uzi_core::cache::write_task_output;
use uzi_core::pyrandom::PyRandom;

/// Upstream `TICKER`.
pub const MOCK_TICKER: &str = "MOCK.SZ";

/// Upstream's `random.seed(42)`.
pub const MOCK_SEED: u64 = 42;

/// Upstream divides the bullish share by a hardcoded 50 even though the roster
/// now holds 66 investors. That is an upstream quirk, reproduced deliberately:
/// "fixing" it would change every generated mock.
const CONSENSUS_DIVISOR: f64 = 50.0;

const TICKER_JSON: &str = include_str!("../../../assets/mock/raw_data.json");
const DIMENSIONS_JSON: &str = include_str!("../../../assets/mock/dimensions.json");
const SYNTHESIS_JSON: &str = include_str!("../../../assets/mock/synthesis.json");
const PANEL_TABLES_JSON: &str = include_str!("../../../assets/mock/panel_tables.json");

fn asset(text: &str, what: &str) -> Value {
    serde_json::from_str(text)
        .unwrap_or_else(|e| panic!("assets/mock/{what}.json is not valid JSON: {e}"))
}

/// `SAMPLE_COMMENTS` / `VERDICTS`.
fn panel_tables() -> Value {
    asset(PANEL_TABLES_JSON, "panel_tables")
}

/// `pick_comment(sig, group)`.
///
/// Neutral signals draw from the shared `all` pool; every other signal draws
/// from its group's pool, falling back to group `A` and finally `["—"]` — which
/// is what happens for groups `H` / `I`, absent from the table.
fn pick_comment(rng: &mut PyRandom, tables: &Value, sig: &str, group: &str) -> String {
    let pool = if sig == "neutral" {
        tables["comments"]["neutral"]["all"].clone()
    } else {
        let by_group = &tables["comments"][sig];
        let chosen = by_group
            .get(group)
            .or_else(|| by_group.get("A"))
            .cloned()
            .unwrap_or_else(|| json!(["—"]));
        chosen
    };
    let options: Vec<&str> = pool
        .as_array()
        .map(|a| a.iter().filter_map(|v| v.as_str()).collect())
        .unwrap_or_default();
    if options.is_empty() {
        return "—".to_string();
    }
    rng.choice_str(&options).to_string()
}

/// Verdict → the `vote_distribution` bucket upstream counts it under.
fn vote_key(verdict: &str) -> &'static str {
    match verdict {
        "强烈买入" => "strongly_buy",
        "买入" => "buy",
        "关注" => "watch",
        "观望" => "wait",
        "回避" => "avoid",
        _ => "n_a",
    }
}

/// `dict[key] += 1` for an integer-valued JSON counter.
fn bump(map: &mut Map<String, Value>, key: &str, what: &str) {
    let slot = map
        .get_mut(key)
        .unwrap_or_else(|| panic!("{what}: unexpected key {key:?}"));
    *slot = json!(slot.as_i64().unwrap_or(0) + 1);
}

/// Generate `panel.json` exactly as upstream's seeded loop does.
///
/// Per investor the draw order is fixed by Python's evaluation order:
/// `random()`, `randint`, `randint`, `choice(verdict)`, `choice(comment)`,
/// `random()` (ideal price), `choice(period)`. Any reordering would desynchronise
/// the stream.
pub fn build_panel() -> Value {
    let tables = panel_tables();
    let investors = uzi_investors::db::investors();
    let mut rng = PyRandom::seed_u64(MOCK_SEED);

    // Inserted with upstream's key order so the emitted JSON matches.
    let mut vote_dist = Map::new();
    for k in [
        "strongly_buy",
        "buy",
        "watch",
        "wait",
        "avoid",
        "n_a",
        "skip",
    ] {
        vote_dist.insert(k.to_string(), json!(0));
    }
    let mut sig_dist = Map::new();
    for k in ["bullish", "neutral", "bearish", "skip"] {
        sig_dist.insert(k.to_string(), json!(0));
    }

    let periods = ["3-5 年", "1-3 年", "半年", "1-3 月"];
    let mut panel_investors: Vec<Value> = Vec::with_capacity(investors.len());

    for inv in investors {
        let id = inv.get("id").and_then(|v| v.as_str()).unwrap_or("");
        let name = inv.get("name").and_then(|v| v.as_str()).unwrap_or("");
        let group = inv.get("group").and_then(|v| v.as_str()).unwrap_or("");

        let r = rng.random();
        let sig = if r < 0.42 {
            "bullish"
        } else if r < 0.78 {
            "neutral"
        } else {
            "bearish"
        };

        let conf = if sig != "neutral" {
            rng.randint(55, 95)
        } else {
            rng.randint(30, 60)
        };
        let score = (conf - rng.randint(-8, 5)).clamp(10, 98);

        let verdicts: Vec<&str> = tables["verdicts"][sig]
            .as_array()
            .map(|a| a.iter().filter_map(|v| v.as_str()).collect())
            .unwrap_or_default();
        let verdict = rng.choice_str(&verdicts).to_string();

        let reason = pick_comment(&mut rng, &tables, sig, group);
        let ideal_price = uzi_core::py::round(16.0 + rng.random() * 4.0, 2);
        let period = rng.choice_str(&periods).to_string();

        bump(&mut vote_dist, vote_key(&verdict), "vote_distribution");
        bump(&mut sig_dist, sig, "signal_distribution");

        panel_investors.push(json!({
            "investor_id": id,
            "name": name,
            "group": group,
            "avatar": format!("avatars/{id}.svg"),
            "signal": sig,
            "confidence": conf,
            "score": score,
            "verdict": verdict,
            "reasoning": reason,
            "comment": reason,
            "pass": [],
            "fail": [],
            "ideal_price": ideal_price,
            "period": period,
        }));
    }

    let bullish = sig_dist["bullish"].as_f64().unwrap_or(0.0);
    let consensus = uzi_core::py::round(bullish / CONSENSUS_DIVISOR * 100.0, 1);

    json!({
        "ticker": MOCK_TICKER,
        "panel_consensus": consensus,
        "vote_distribution": vote_dist,
        "signal_distribution": sig_dist,
        "investors": panel_investors,
    })
}

/// The strongest `signal` voice: highest confidence, ties broken by panel order
/// (Python's stable `sorted`).
fn strongest(panel_investors: &[Value], signal: &str, fallback_first: bool) -> Value {
    let mut best: Option<&Value> = None;
    for inv in panel_investors {
        if inv.get("signal").and_then(|v| v.as_str()) != Some(signal) {
            continue;
        }
        let better = match best {
            None => true,
            Some(cur) => {
                let a = inv.get("confidence").and_then(|v| v.as_i64()).unwrap_or(0);
                let b = cur.get("confidence").and_then(|v| v.as_i64()).unwrap_or(0);
                a > b
            }
        };
        if better {
            best = Some(inv);
        }
    }
    let chosen = if fallback_first {
        best.or_else(|| panel_investors.first())
    } else {
        best.or_else(|| panel_investors.last())
    };
    chosen.cloned().unwrap_or(Value::Null)
}

fn voice(inv: &Value) -> Value {
    json!({
        "investor_id": inv.get("investor_id").cloned().unwrap_or(Value::Null),
        "name": inv.get("name").cloned().unwrap_or(Value::Null),
        "group": inv.get("group").cloned().unwrap_or(Value::Null),
    })
}

/// Refresh the fields upstream derives from the generated panel.
///
/// With the roster and seed unchanged these are no-ops (the checked-in synthesis
/// already holds the same values) — they exist so the mock cannot drift out of
/// sync if the roster changes.
pub fn sync_synthesis(synthesis: &mut Value, panel: &Value) {
    let investors = panel["investors"].as_array().cloned().unwrap_or_default();
    let bull = strongest(&investors, "bullish", true);
    let bear = strongest(&investors, "bearish", false);

    synthesis["panel_consensus"] = panel["panel_consensus"].clone();
    synthesis["debate"]["bull"] = voice(&bull);
    synthesis["debate"]["bear"] = voice(&bear);
    synthesis["great_divide"]["bull_avatar"] =
        bull.get("investor_id").cloned().unwrap_or(Value::Null);
    synthesis["great_divide"]["bear_avatar"] =
        bear.get("investor_id").cloned().unwrap_or(Value::Null);
    synthesis["great_divide"]["bull_score"] =
        bull.get("confidence").cloned().unwrap_or(Value::Null);
    synthesis["great_divide"]["bear_score"] =
        bear.get("confidence").cloned().unwrap_or(Value::Null);
}

/// The four artifacts upstream writes, in its own write order.
pub fn build_artifacts() -> Vec<(&'static str, Value)> {
    let mut synthesis = asset(SYNTHESIS_JSON, "synthesis");
    let panel = build_panel();
    sync_synthesis(&mut synthesis, &panel);

    vec![
        ("raw_data", asset(TICKER_JSON, "raw_data")),
        ("dimensions", asset(DIMENSIONS_JSON, "dimensions")),
        ("panel", panel),
        ("synthesis", synthesis),
    ]
}

/// `preview_with_mock.py`'s tail: write the artifacts, then assemble the report.
///
/// Returns the report path reported by [`uzi_report::assemble::assemble`].
pub fn main_preview() -> anyhow::Result<String> {
    for (name, payload) in build_artifacts() {
        write_task_output(MOCK_TICKER, name, &payload)?;
    }
    println!("📝 已写入 mock 缓存: {}", MOCK_TICKER);
    let report = uzi_report::assemble::assemble(MOCK_TICKER)?;
    println!("\n[ok] Mock report ready. Open: {report}");
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn panel_reproduces_the_upstream_seeded_generation() {
        // The heaviest assertion in this module: the seeded draw order and every
        // bounded draw must match CPython's stream, or the whole mock shifts.
        let expected = asset(
            include_str!("../../../assets/mock/panel.json"),
            "panel",
        );
        let actual = build_panel();
        assert_eq!(
            uzi_core::testkit::first_difference(&expected, &actual, "panel"),
            None,
            "generated panel diverged from the upstream fixture"
        );
    }

    #[test]
    fn panel_counts_are_consistent_with_the_roster() {
        let panel = build_panel();
        let investors = panel["investors"].as_array().unwrap();
        assert_eq!(investors.len(), uzi_investors::db::investors().len());

        // signal_distribution must total the roster size.
        let sig = panel["signal_distribution"].as_object().unwrap();
        let sig_total: i64 = sig.values().map(|v| v.as_i64().unwrap()).sum();
        assert_eq!(sig_total as usize, investors.len());

        // vote_distribution must total it too, and the seeded buckets must exist.
        let vote = panel["vote_distribution"].as_object().unwrap();
        let vote_total: i64 = vote.values().map(|v| v.as_i64().unwrap()).sum();
        assert_eq!(vote_total as usize, investors.len());
        assert_eq!(vote.len(), 7);
        assert_eq!(sig.len(), 4);

        // Scores stay inside the clamp and match their signal's confidence band.
        for inv in investors {
            let conf = inv["confidence"].as_i64().unwrap();
            let score = inv["score"].as_i64().unwrap();
            assert!((10..=98).contains(&score), "{inv}");
            match inv["signal"].as_str().unwrap() {
                "neutral" => assert!((30..=60).contains(&conf), "{inv}"),
                _ => assert!((55..=95).contains(&conf), "{inv}"),
            }
        }
    }

    #[test]
    fn consensus_uses_upstreams_hardcoded_divisor() {
        let panel = build_panel();
        let bullish = panel["signal_distribution"]["bullish"].as_f64().unwrap();
        let expected = uzi_core::py::round(bullish / 50.0 * 100.0, 1);
        assert_eq!(panel["panel_consensus"].as_f64().unwrap(), expected);
    }

    #[test]
    fn synthesis_derived_fields_agree_with_the_panel() {
        let artifacts = build_artifacts();
        let panel = &artifacts.iter().find(|(n, _)| *n == "panel").unwrap().1;
        let synthesis = &artifacts.iter().find(|(n, _)| *n == "synthesis").unwrap().1;

        assert_eq!(synthesis["panel_consensus"], panel["panel_consensus"]);

        // The debate pairing must be the highest-confidence voice per side.
        let investors = panel["investors"].as_array().unwrap();
        for (side, signal) in [("bull", "bullish"), ("bear", "bearish")] {
            let want = investors
                .iter()
                .filter(|i| i["signal"] == signal)
                .max_by_key(|i| i["confidence"].as_i64().unwrap())
                .unwrap();
            assert_eq!(
                synthesis["debate"][side]["investor_id"], want["investor_id"],
                "{side}"
            );
            assert_eq!(
                synthesis["great_divide"][format!("{side}_score")],
                want["confidence"],
                "{side} score"
            );
        }
    }

    #[test]
    fn all_four_artifacts_are_present_and_well_formed() {
        let artifacts = build_artifacts();
        let names: Vec<&str> = artifacts.iter().map(|(n, _)| *n).collect();
        assert_eq!(names, vec!["raw_data", "dimensions", "panel", "synthesis"]);
        for (name, payload) in &artifacts {
            assert!(payload.is_object(), "{name} should be an object");
            assert_eq!(payload["ticker"], json!(MOCK_TICKER), "{name}");
        }
    }

    #[test]
    fn group_falls_back_to_the_a_pool_when_absent() {
        // Groups H / I have no comments of their own; upstream falls back to A
        // and would otherwise raise KeyError.
        let tables = panel_tables();
        let mut rng = PyRandom::seed_u64(7);
        let h = pick_comment(&mut rng, &tables, "bullish", "H");
        let mut rng = PyRandom::seed_u64(7);
        let a = pick_comment(&mut rng, &tables, "bullish", "A");
        assert_eq!(h, a);
        assert!(!h.is_empty());
    }
}
