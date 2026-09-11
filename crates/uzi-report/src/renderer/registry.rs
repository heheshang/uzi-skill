//! Port of `lib/pipeline/renderer/registry.py` — dim_key → SectionRenderer.

use super::base::SectionRenderer;
use super::{
    basic_header::BasicHeaderRenderer, capital_flow::CapitalFlowRenderer, chain::ChainRenderer,
    contests::ContestsRenderer, events::EventsRenderer, financials::FinancialsRenderer,
    fund::FundRenderer, futures::FuturesRenderer, governance::GovernanceRenderer,
    industry::IndustryRenderer, kline::KlineRenderer, lhb::LhbRenderer, macro_::MacroRenderer,
    materials::MaterialsRenderer, moat::MoatRenderer, peers::PeersRenderer,
    policy::PolicyRenderer, research::ResearchRenderer, sentiment::SentimentRenderer,
    trap::TrapRenderer, valuation::ValuationRenderer,
};

/// All 21 registered renderers, in registry insertion order.
pub const RENDERER_KEYS: &[&str] = &[
    "0_basic",
    "1_financials",
    "2_kline",
    "3_macro",
    "4_peers",
    "5_chain",
    "6_fund_holders",
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
];

/// `RENDERER_REGISTRY.get(dim_key)` — fresh boxed renderer or `None`.
pub fn get_renderer(dim_key: &str) -> Option<Box<dyn SectionRenderer>> {
    Some(match dim_key {
        "0_basic" => Box::new(BasicHeaderRenderer),
        "1_financials" => Box::new(FinancialsRenderer),
        "2_kline" => Box::new(KlineRenderer),
        "3_macro" => Box::new(MacroRenderer),
        "4_peers" => Box::new(PeersRenderer),
        "5_chain" => Box::new(ChainRenderer),
        "6_fund_holders" => Box::new(FundRenderer),
        "6_research" => Box::new(ResearchRenderer),
        "7_industry" => Box::new(IndustryRenderer),
        "8_materials" => Box::new(MaterialsRenderer),
        "9_futures" => Box::new(FuturesRenderer),
        "10_valuation" => Box::new(ValuationRenderer),
        "11_governance" => Box::new(GovernanceRenderer),
        "12_capital_flow" => Box::new(CapitalFlowRenderer),
        "13_policy" => Box::new(PolicyRenderer),
        "14_moat" => Box::new(MoatRenderer),
        "15_events" => Box::new(EventsRenderer),
        "16_lhb" => Box::new(LhbRenderer),
        "17_sentiment" => Box::new(SentimentRenderer),
        "18_trap" => Box::new(TrapRenderer),
        "19_contests" => Box::new(ContestsRenderer),
        _ => return None,
    })
}

/// `list_renderers()` — sorted dim keys.
pub fn list_renderers() -> Vec<String> {
    let mut keys: Vec<String> = RENDERER_KEYS.iter().map(|s| (*s).to_string()).collect();
    keys.sort();
    keys
}
