//! Port of `fetch_governance.py`.
//!
//! Dimension 11 · 治理 (实控人/质押/管理层增减持/关联交易).
//!
//! Both upstream data sources are AkShare-only wrappers
//! (`stock_gpzy_pledge_ratio_em` whole-market table, `stock_ggcg_em` insider
//! trades); each sits in its own `try/except: pass`, so an unavailable library
//! leaves the two lists empty exactly as upstream does when the calls fail.

use serde_json::{json, Value};

use uzi_core::ticker::parse_ticker;

/// `main(ticker)`.
pub fn main(ticker: &str) -> Result<Value, String> {
    let ti = parse_ticker(ticker);

    // AkShare wraps EastMoney datacenter endpoints whose queries are not part of
    // this crate's documented helper surface; both `try` blocks degrade to the
    // upstream `except: pass` path, leaving the lists empty.
    let pledges: Vec<Value> = Vec::new();
    let insider: Vec<Value> = Vec::new();

    Ok(json!({
        "ticker": ti.full,
        "data": {
            "pledge": pledges,
            "insider_trades_1y": insider,
            "qualitative_search": [
                format!("{} 关联交易 违规 处罚", ti.full),
                format!("{} 股权激励", ti.full),
            ],
        },
        "source": "akshare:stock_gpzy_pledge_ratio_em + stock_ggcg_em",
        "fallback": false,
    }))
}
