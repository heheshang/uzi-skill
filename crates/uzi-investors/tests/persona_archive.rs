//! The shipped persona archive under `skills/deep-analysis/personas/`.
//!
//! `persona_yaml::personas_dir()` resolves that directory relative to the current
//! working directory and returns an **empty map** when it is absent — no error,
//! no warning. So if the archive is moved or deleted, the flagship-persona rules
//! silently stop applying and `role-play` quietly falls back to the rule engine.
//! This guards the asset the same way the golden fixtures are guarded.
//!
//! Resolution uses `CARGO_MANIFEST_DIR`, not the cwd, so it holds regardless of
//! where cargo runs the test from.

use std::path::PathBuf;

fn personas_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("skills/deep-analysis/personas")
}

#[test]
fn shipped_persona_archive_loads() {
    let dir = personas_dir();
    assert!(
        dir.is_dir(),
        "persona archive missing at {} — flagship role-play would silently degrade",
        dir.display()
    );

    let all = uzi_investors::persona_yaml::load_all_personas_from(&dir);
    assert_eq!(
        all.len(),
        51,
        "expected the 51 shipped personas (12 flagship + 39 stub), got {}",
        all.len()
    );

    // Every shipped roster id that has an archive must be a real investor, and
    // every archive must parse to a non-empty name/school — a YAML that fails to
    // parse would otherwise be skipped without a trace.
    let roster: std::collections::HashSet<&str> =
        uzi_investors::db::investors()
            .iter()
            .filter_map(|i| i.get("id").and_then(|v| v.as_str()))
            .collect();

    for (id, persona) in &all {
        assert!(
            roster.contains(id.as_str()),
            "persona {id} is not in the investor roster"
        );
        assert!(!persona.name.is_empty(), "persona {id} has no name");
        assert!(!persona.school.is_empty(), "persona {id} has no school");
        assert!(
            !persona.avoids.is_empty(),
            "persona {id} has no avoids — flagship rules need them"
        );
    }

    // `key_metrics` is the field the role-play rules cite, but three of the 12
    // flagship archives name it differently. Upstream reads only `key_metrics`,
    // so all three load with an empty list there too — pinned here so the
    // divergence stays visible instead of silently weakening the rules.
    let mut alternate_frame: Vec<&str> = all
        .iter()
        .filter(|(_, p)| p.key_metrics.is_empty())
        .map(|(id, _)| id.as_str())
        .collect();
    alternate_frame.sort();
    assert_eq!(
        alternate_frame,
        vec!["dalio", "fisher", "soros"],
        "the set of personas without `key_metrics` changed; update the SKILL.md guidance"
    );

    // Each carries its criteria under a different key — and `fisher` under none
    // at all, which is why the SKILL.md tells the agent to read the raw YAML.
    for (id, field) in [("dalio", "key_framework"), ("soros", "key_signals")] {
        assert!(
            all[id]
                .raw
                .get(field)
                .and_then(|v| v.as_array())
                .map(|a| !a.is_empty())
                .unwrap_or(false),
            "persona {id} should carry its criteria under `{field}`"
        );
    }
    assert!(
        all["fisher"].raw.get("key_framework").is_none()
            && all["fisher"].raw.get("key_signals").is_none(),
        "fisher is expected to carry no criteria list; adjust the SKILL.md if that changed"
    );
    assert!(
        !all["fisher"].famous_positions.is_empty(),
        "fisher must still be citable via famous_positions"
    );
}

/// The flagship / stub split drives whether YAML outranks the rule engine, so the
/// counts are part of the contract the SKILL.md documents.
#[test]
fn flagship_and_stub_split_matches_the_documented_counts() {
    let all = uzi_investors::persona_yaml::load_all_personas_from(&personas_dir());
    let flagship = all.values().filter(|p| p.is_flagship).count();
    let stubs = all.len() - flagship;

    assert_eq!(flagship, 12, "documented flagship count");
    assert_eq!(stubs, 39, "documented stub count");

    // The 12 hand-written flagships the SKILL.md names must all be present.
    for id in [
        "buffett", "munger", "graham", "fisher", "lynch", "wood", "soros", "dalio", "duan",
        "zhangkun", "zhao_lg", "zhang_mz",
    ] {
        let persona = all
            .get(id)
            .unwrap_or_else(|| panic!("flagship persona {id} missing from the archive"));
        assert!(persona.is_flagship, "{id} should be a flagship, not a stub");
    }
}

/// An absent archive must degrade quietly rather than panic — the loader is
/// called on paths that may not exist.
#[test]
fn a_missing_persona_dir_loads_empty() {
    let all = uzi_investors::persona_yaml::load_all_personas_from(&PathBuf::from(
        env!("CARGO_MANIFEST_DIR"),
    ).join("definitely/not/here"));
    assert!(all.is_empty());
    assert!(uzi_investors::persona_yaml::load_persona_from(
        &PathBuf::from(env!("CARGO_MANIFEST_DIR")),
        "buffett"
    )
    .is_none());
}
