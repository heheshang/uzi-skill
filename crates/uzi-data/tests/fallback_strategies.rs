//! Differential test: `uzi_data::browser::fallback` vs upstream
//! `lib/playwright_fallback.py`.
//!
//! The fetching half needs a browser; the strategies' **parsing**, the quality
//! gate, and the network filter are pure and are what this port reimplements.
//! `tools/golden/dump_fallback.py` stubs `fetch_url` so the upstream parsers run
//! against canned HTML.

use serde_json::{json, Value};
use std::collections::BTreeSet;

use uzi_core::testkit::{assert_json_eq, load_golden};
use uzi_data::browser::fallback;

/// Reference pages, matching `tools/golden/dump_fallback.py` exactly.
fn page(dim: &str) -> &'static str {
    match dim {
        "4_peers" => {
            r#"
      <a href="/S/SH600519">贵州茅台</a>
      <a href="/S/SZ000582">北部湾港</a>
      <a href="/S/SH600519">贵州茅台</a>
      <a href="/S/SH600520">短</a>
      <a href="/S/HK00700">腾讯控股</a>
      <a href="/S/BJ430047">诺思兰德</a>
    "#
        }
        "8_materials" => "<div class='m_table'>主营业务：光学薄膜研发与制造</div>",
        "15_events" => r#"
      <div class="announcement-title">2026年第一季度报告</div>
      <div class="announcement-title">关于回购公司股份的公告</div>
    "#,
        "17_sentiment" => {
            r#"<script>{"title":"水晶光电还有机会吗"}{"title":"聊聊AR眼镜产业链"}</script>"#
        }
        "3_macro" => r#"
      <a href="/x">短</a>
      <a href="/y">1234567890</a>
      <a href="/z">2026年8月份国民经济运行情况</a>
      <a href="/w">国家统计局城市司首席统计师解读数据</a>
    "#,
        "7_industry" => r#"
      <h3><a href="/l">光学光电子行业景气度持续提升</a></h3>
      <span class="content-right_abc">市场规模预计达到 420 亿元，渗透率稳步提升</span>
    "#,
        "14_moat" => r#"
      <div class="lemma-summary J-summary"><b>水晶光电</b>是一家光学薄膜企业。</div>
      <dt class="basicInfo-item name">主营业务</dt><dd class="basicInfo-item value">光学元件</dd>
      <dt class="basicInfo-item name">所属行业</dt><dd class="basicInfo-item value">光学光电子</dd>
    "#,
        "13_policy" => r#"
      <a title="证监会发布关于资本市场的最新政策通知">x</a>
      <a title="首页">y</a>
      <a>关于进一步规范上市公司信息披露的公告</a>
    "#,
        "18_trap" => {
            r#"{"title":"水晶光电 老师推荐 必涨"}{"title":"其他公司分析"}{"title":"水晶光电财报解读"}"#
        }
        "19_contests" => {
            r#"<div>{"name":"稳健成长","total_gain":45.6}</div><div>{"name":"激进策略","total_gain":-12.3}</div>"#
        }
        other => panic!("no reference page for {other}"),
    }
}

fn raw() -> Value {
    json!({"dimensions": {
        "0_basic": {"data": {"name": "水晶光电", "industry": "光学光电子"}},
        "4_peers": {"data": {}}
    }})
}

/// Every strategy must parse its reference page to exactly upstream's payload.
#[test]
fn strategies_match_upstream_on_reference_pages() {
    let golden = load_golden("fallback", "strategies");
    let expected = golden["strategies"].as_object().expect("golden.strategies");
    let raw = raw();

    for (dim, want) in expected {
        let got = fallback::parse_for_dim(dim, page(dim), &raw);
        match got {
            Some(actual) => assert_json_eq(&actual, want, &format!("strategy/{dim}")),
            None => panic!("{dim}: port returned None, upstream produced {want}"),
        }
    }
}

/// A page without the strategy's markers must degrade to `None` upstream too, so
/// the caller records a failure rather than merging junk.
#[test]
fn empty_pages_degrade_exactly_like_upstream() {
    let golden = load_golden("fallback", "strategies");
    let expected = golden["empty_pages"].as_object().expect("golden.empty_pages");
    let raw = raw();

    for (dim, want) in expected {
        assert!(
            want.is_null(),
            "{dim}: golden unexpectedly holds {want}; update the assertions below"
        );
        assert!(
            fallback::parse_for_dim(dim, "", &raw).is_none(),
            "{dim} should return None on an empty page"
        );
    }
}

/// The quality gate drives whether a fetch happens at all, so pin every fixture.
#[test]
fn quality_gate_matches_upstream() {
    let golden = load_golden("fallback", "strategies");
    let cases = golden["quality"].as_array().expect("golden.quality");
    assert!(!cases.is_empty(), "golden holds no quality cases");

    for case in cases {
        let dim = &case["dim"];
        let want_needs = case["needs"].as_bool().unwrap();
        let want_score = case["score"].as_f64().unwrap();

        let (got_needs, reason) = fallback::dim_needs_fallback(dim);
        assert_eq!(got_needs, want_needs, "{dim} → {reason}");

        let data = dim.get("data").cloned().unwrap_or(Value::Null);
        let got_score = if data.is_object() {
            fallback::dim_quality_score(&data)
        } else {
            0.0
        };
        assert!(
            (got_score - want_score).abs() < 1e-9,
            "{dim}: score {got_score} != {want_score}"
        );
    }
}

/// The network filter must drop the same dimensions upstream drops, with the
/// same reason strings.
#[test]
fn network_filter_matches_upstream() {
    let golden = load_golden("fallback", "strategies");
    let cases = golden["network"].as_array().expect("golden.network");
    let all: BTreeSet<String> = fallback::DIM_STRATEGIES
        .iter()
        .map(|s| s.to_string())
        .collect();

    for case in cases {
        let profile = &case["profile"];
        let (effective, skipped) = fallback::filter_dims_by_network(&all, Some(profile));

        let want_effective: Vec<String> = case["effective"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap().to_string())
            .collect();
        let want_skipped: Vec<String> = case["skipped"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap().to_string())
            .collect();

        assert_eq!(
            effective.iter().cloned().collect::<Vec<_>>(),
            want_effective,
            "effective dims for {profile}"
        );
        assert_eq!(skipped, want_skipped, "skip reasons for {profile}");
    }
}

/// Each dimension's required capabilities must match upstream's declaration.
#[test]
fn network_requirements_match_upstream() {
    let golden = load_golden("fallback", "strategies");
    let expected = golden["dim_network_requirements"]
        .as_object()
        .expect("golden.dim_network_requirements");

    for (dim, want) in expected {
        let got: Vec<String> = fallback::dim_network_requirements(dim)
            .iter()
            .map(|s| s.to_string())
            .collect();
        let want: Vec<String> = want
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap().to_string())
            .collect();
        assert_eq!(got, want, "requirements for {dim}");
    }
}
