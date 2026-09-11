//! Port of `lib/analysis_profile.py` — the three thinking-depth profiles that
//! decide which fetchers run, how many investors vote, and how strict the
//! self-review gate is.

use std::collections::BTreeSet;

pub const DEPTH_LITE: &str = "lite";
pub const DEPTH_MEDIUM: &str = "medium";
pub const DEPTH_DEEP: &str = "deep";

#[derive(Debug, Clone, PartialEq)]
pub struct AnalysisProfile {
    pub depth: String,
    pub label_cn: String,
    pub estimated_minutes: String,

    pub fetchers_enabled: BTreeSet<String>,
    pub ddg_budget: u32,
    pub industry_dynamic_lookup: bool,

    pub investors_count: u32,
    pub enable_bull_bear_debate: bool,

    pub institutional_methods: BTreeSet<String>,
    pub enable_segmental_model: bool,
    pub enable_owner_earnings: bool,
    pub enable_narrative_gap: bool,

    pub fund_stats_top_n: u32,
    pub fund_lite_list_enabled: bool,

    pub require_qualitative_deep_dive: bool,
    pub self_review_block_warnings: bool,

    pub playwright_mode: String,
    pub playwright_dims: BTreeSet<String>,
}

impl AnalysisProfile {
    /// Skip fetchers outside the profile's enable set.
    pub fn should_run_fetcher(&self, dim_key: &str) -> bool {
        self.fetchers_enabled.contains(dim_key)
    }
}

fn set(items: &[&str]) -> BTreeSet<String> {
    items.iter().map(|s| s.to_string()).collect()
}

fn playwright_medium_dims() -> BTreeSet<String> {
    set(&[
        "4_peers",
        "8_materials",
        "15_events",
        "17_sentiment",
        "7_industry",
        "14_moat",
    ])
}

fn playwright_deep_dims() -> BTreeSet<String> {
    let mut dims = playwright_medium_dims();
    for d in ["3_macro", "13_policy", "18_trap", "19_contests"] {
        dims.insert(d.to_string());
    }
    dims
}

fn core_fetchers() -> BTreeSet<String> {
    set(&[
        "0_basic",
        "1_financials",
        "2_kline",
        "10_valuation",
        "11_governance",
        "15_events",
        "16_lhb",
    ])
}

fn all_fetchers() -> BTreeSet<String> {
    set(&[
        "0_basic",
        "1_financials",
        "2_kline",
        "3_macro",
        "4_peers",
        "5_chain",
        "6_research",
        "7_industry",
        "8_materials",
        "9_futures",
        "10_valuation",
        "11_governance",
        "12_capital_flow",
        "13_policy",
        "14_moat",
        "15_events",
        "16_lhb",
        "17_sentiment",
        "18_trap",
        "19_contests",
    ])
}

fn full_institutional_methods() -> BTreeSet<String> {
    set(&[
        "dcf",
        "comps",
        "lbo",
        "three_statement",
        "merger",
        "initiating",
        "earnings",
        "catalysts",
        "thesis",
        "morning",
        "screen",
        "sector",
        "ic_memo",
        "porter_bcg",
        "dd",
        "unit_economics",
        "portfolio_rebalance",
    ])
}

fn lite() -> AnalysisProfile {
    AnalysisProfile {
        depth: DEPTH_LITE.into(),
        label_cn: "速判模式".into(),
        estimated_minutes: "1-2 分钟".into(),
        fetchers_enabled: core_fetchers(),
        ddg_budget: 0,
        industry_dynamic_lookup: false,
        investors_count: 10,
        enable_bull_bear_debate: false,
        institutional_methods: set(&["dcf"]),
        enable_segmental_model: false,
        enable_owner_earnings: false,
        enable_narrative_gap: false,
        fund_stats_top_n: 5,
        fund_lite_list_enabled: false,
        require_qualitative_deep_dive: false,
        self_review_block_warnings: false,
        playwright_mode: "off".into(),
        playwright_dims: BTreeSet::new(),
    }
}

fn medium() -> AnalysisProfile {
    AnalysisProfile {
        depth: DEPTH_MEDIUM.into(),
        label_cn: "标准分析".into(),
        estimated_minutes: "5-8 分钟".into(),
        fetchers_enabled: all_fetchers(),
        ddg_budget: 30,
        industry_dynamic_lookup: true,
        investors_count: 51,
        enable_bull_bear_debate: false,
        institutional_methods: full_institutional_methods(),
        enable_segmental_model: false,
        enable_owner_earnings: false,
        enable_narrative_gap: false,
        fund_stats_top_n: 20,
        fund_lite_list_enabled: true,
        require_qualitative_deep_dive: true,
        self_review_block_warnings: false,
        playwright_mode: "opt-in".into(),
        playwright_dims: playwright_medium_dims(),
    }
}

fn deep() -> AnalysisProfile {
    let mut methods = full_institutional_methods();
    methods.insert("segmental".into());
    AnalysisProfile {
        depth: DEPTH_DEEP.into(),
        label_cn: "深度研究".into(),
        estimated_minutes: "15-20 分钟".into(),
        fetchers_enabled: all_fetchers(),
        ddg_budget: 60,
        industry_dynamic_lookup: true,
        investors_count: 51,
        enable_bull_bear_debate: true,
        institutional_methods: methods,
        enable_segmental_model: true,
        enable_owner_earnings: true,
        enable_narrative_gap: true,
        fund_stats_top_n: 100,
        fund_lite_list_enabled: true,
        require_qualitative_deep_dive: true,
        self_review_block_warnings: true,
        playwright_mode: "default".into(),
        playwright_dims: playwright_deep_dims(),
    }
}

/// Resolve a profile. `depth = None` reads `UZI_DEPTH`, then the legacy
/// `UZI_LITE` flag, then defaults to `medium`.
pub fn get_profile(depth: Option<&str>) -> Result<AnalysisProfile, String> {
    let depth = match depth {
        Some(d) => d.to_string(),
        None => match std::env::var("UZI_DEPTH") {
            Ok(d) if !d.is_empty() => d,
            _ => {
                let lite_env = std::env::var("UZI_LITE")
                    .unwrap_or_else(|_| "auto".into())
                    .to_lowercase();
                if matches!(lite_env.as_str(), "1" | "true" | "yes" | "on") {
                    DEPTH_LITE.to_string()
                } else {
                    DEPTH_MEDIUM.to_string()
                }
            }
        },
    };
    let depth = if depth.is_empty() {
        DEPTH_MEDIUM.to_string()
    } else {
        depth.to_lowercase()
    };
    match depth.as_str() {
        DEPTH_LITE => Ok(lite()),
        DEPTH_MEDIUM => Ok(medium()),
        DEPTH_DEEP => Ok(deep()),
        other => Err(format!(
            "unknown depth {:?}; expected one of [\"lite\", \"medium\", \"deep\"]",
            other
        )),
    }
}

/// Mirror the profile into the env vars downstream subsystems read.
pub fn apply_profile_to_env(profile: &AnalysisProfile) {
    std::env::set_var("UZI_DEPTH", &profile.depth);
    std::env::set_var(
        "UZI_LITE",
        if profile.depth == DEPTH_LITE { "1" } else { "0" },
    );
    std::env::set_var(
        "UZI_DDG_BUDGET",
        if profile.ddg_budget > 0 {
            profile.ddg_budget.to_string()
        } else {
            "0".to_string()
        },
    );
    std::env::set_var("UZI_FUND_STATS_TOP", profile.fund_stats_top_n.to_string());
}

/// The startup banner.
pub fn format_banner(profile: &AnalysisProfile) -> String {
    let icon = match profile.depth.as_str() {
        DEPTH_LITE => "⚡",
        DEPTH_MEDIUM => "📊",
        DEPTH_DEEP => "🔬",
        _ => "·",
    };
    let debate = if profile.enable_bull_bear_debate {
        "（含 Bull-Bear 辩论）"
    } else {
        ""
    };
    let ddg = if profile.ddg_budget == 0 && profile.depth != DEPTH_LITE {
        "无限".to_string()
    } else {
        profile.ddg_budget.to_string()
    };
    [
        format!(
            "{} {} · depth={} · 预计 {}",
            icon, profile.label_cn, profile.depth, profile.estimated_minutes
        ),
        format!(
            "  · fetchers: {}/{} 维",
            profile.fetchers_enabled.len(),
            all_fetchers().len()
        ),
        format!("  · 评委: {} 位{}", profile.investors_count, debate),
        format!("  · 机构方法: {} 种", profile.institutional_methods.len()),
        format!("  · ddgs 预算: {}", ddg),
        format!("  · fund_holders: 头部 {} 家完整", profile.fund_stats_top_n),
    ]
    .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lite_profile_matches_upstream_table() {
        let p = get_profile(Some("lite")).unwrap();
        assert_eq!(p.fetchers_enabled.len(), 7);
        assert_eq!(p.investors_count, 10);
        assert_eq!(p.ddg_budget, 0);
        assert_eq!(p.fund_stats_top_n, 5);
        assert!(!p.should_run_fetcher("3_macro"));
        assert!(p.should_run_fetcher("0_basic"));
        assert!(!p.enable_bull_bear_debate);
    }

    #[test]
    fn deep_profile_has_debate_and_segmental() {
        let p = get_profile(Some("deep")).unwrap();
        assert_eq!(p.fetchers_enabled.len(), 20);
        assert!(p.enable_bull_bear_debate);
        assert!(p.enable_segmental_model);
        assert!(p.self_review_block_warnings);
        assert_eq!(p.institutional_methods.len(), 18);
        assert_eq!(p.playwright_dims.len(), 10);
        assert_eq!(p.fund_stats_top_n, 100);
    }

    #[test]
    fn medium_overrides_fetchers_and_playwright_scope() {
        let p = get_profile(Some("medium")).unwrap();
        assert_eq!(p.institutional_methods.len(), 17);
        assert_eq!(p.playwright_dims.len(), 6);
        assert_eq!(p.playwright_mode, "opt-in");
        assert_eq!(p.ddg_budget, 30);
    }

    #[test]
    fn unknown_depth_is_an_error() {
        assert!(get_profile(Some("turbo")).is_err());
        assert!(get_profile(Some("LITE")).is_ok(), "depth is lowercased");
    }

    #[test]
    fn banner_reports_all_six_lines_like_upstream() {
        let p = get_profile(Some("lite")).unwrap();
        let banner = format_banner(&p);
        let lines: Vec<&str> = banner.lines().collect();
        assert_eq!(lines.len(), 6);
        assert!(lines[0].starts_with("⚡ 速判模式 · depth=lite"));
        assert!(lines[1].contains("fetchers: 7/20 维"));
        assert!(lines[4].contains("ddgs 预算: 0"));
    }
}
