//! Smoke test for the cache-reading glue: `build_friendly` and the
//! `compute_segmental` discover/validate commands round-trip through
//! `uzi_core::cache` in an isolated cache root.
//!
//! Lives in its own integration binary because it mutates `UZI_CACHE_ROOT`
//! process-wide; cargo runs each test file in a separate process.

use uzi_core::testkit::{load_fixture, load_golden};

#[test]
fn cache_glue_roundtrips() {
    let tmp = std::env::temp_dir().join(format!("uzi_features_glue_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    std::env::set_var("UZI_CACHE_ROOT", &tmp);

    let raw = load_fixture("raw_data_synthetic");
    uzi_core::cache::write_task_output("002273.SZ", "raw_data", &raw).unwrap();

    // build_friendly reproduces the golden friendly block from cached raw_data.
    let friendly = uzi_features::build_friendly("002273.SZ");
    let golden = load_golden("synthetic", "synthesis");
    assert_eq!(friendly["scenarios"], golden["friendly"]["scenarios"]);
    assert_eq!(friendly["exit_triggers"], golden["friendly"]["exit_triggers"]);
    assert_eq!(
        friendly["similar_stocks"],
        golden["friendly"]["similar_stocks"]
    );

    // discover writes the skeleton cache.
    assert_eq!(uzi_features::segmental::cmd_discover("002273.SZ"), 0);
    let skel = uzi_core::cache::read_task_output("002273.SZ", "segmental_skeleton").unwrap();
    assert_eq!(skel["total_revenue_latest_yi"], serde_json::json!(52.3));

    // validate fails (exit 1) when the agent-filled model is absent...
    assert_eq!(uzi_features::segmental::cmd_validate("002273.SZ"), 1);

    // ...and reports a gap when the model does not reconcile.
    let model = serde_json::json!({"segments": [
        {"name": "光学", "latest_revenue_yi": 10.0, "latest_share_pct": 19.1}
    ]});
    uzi_core::cache::write_task_output("002273.SZ", "segmental_model", &model).unwrap();
    assert_eq!(uzi_features::segmental::cmd_validate("002273.SZ"), 1);
    let report = uzi_core::cache::read_task_output("002273.SZ", "segmental_validation").unwrap();
    assert_eq!(report["passed"], serde_json::json!(false));
    assert!(report["errors"][0].as_str().unwrap().contains("阈值 10%"));

    std::env::remove_var("UZI_CACHE_ROOT");
    let _ = std::fs::remove_dir_all(&tmp);
}
