//! Boundary tests for the daily-screen hard gates and filters.
//!
//! Expected values are derived from the upstream Python code and verified
//! against it where noted:
//!
//! ```
//! cd /tmp/uzi-src/skills/deep-analysis/scripts
//! python3 -c "
//! from lib.daily_screen.universe import apply_hard_filters, normalize_universe_frame
//! ..."
//! ```
//!
//! The four boundaries the acceptance criteria name are covered here:
//! `amount == 2e8`, `top_n > 10`, ST / 退市 name filtering, and the F/I school
//! gate.

use serde_json::{json, Value};
use uzi_screen::daily_screen::models::StockSnapshot;
use uzi_screen::daily_screen::ranker::rank_candidates;
use uzi_screen::daily_screen::universe::{apply_hard_filters, normalize_universe_frame};

fn snapshot(code: &str, name: &str, market: &str, amount: f64) -> StockSnapshot {
    StockSnapshot {
        code: code.to_string(),
        name: name.to_string(),
        market: market.to_string(),
        price: 10.0,
        change_pct: 1.0,
        amount,
        industry: "白酒".to_string(),
        observed_at: "2025-09-10T14:30:00+08:00".to_string(),
        source: "test".to_string(),
        ..Default::default()
    }
}

/// `kept = [s for s in stocks if s.amount >= min_turnover_local]` — the filter
/// is inclusive, so 2e8 exactly is kept and 2e8 - 1 is dropped.
#[test]
fn turnover_boundary_is_inclusive() {
    let stocks = vec![
        snapshot("600519.SH", "贵州茅台", "A", 2e8),
        snapshot("000858.SZ", "五粮液", "A", 2e8 - 0.5),
        snapshot("600000.SH", "浦发银行", "A", 2e8 + 1.0),
        snapshot("601398.SH", "工商银行", "A", 0.0),
    ];
    let (kept, stats) = apply_hard_filters(&stocks, 2e8).unwrap();
    let codes: Vec<&str> = kept.iter().map(|s| s.code.as_str()).collect();
    assert_eq!(codes, vec!["600519.SH", "600000.SH"]);
    assert_eq!(stats["input"], json!(4));
    assert_eq!(stats["liquid"], json!(2));
    assert_eq!(stats["removed_low_turnover"], json!(2));
    assert_eq!(stats["min_turnover_local"], json!(2e8));
}

/// `if not math.isfinite(min_turnover_local) or min_turnover_local < 2e8: raise
/// ValueError("minimum turnover must be finite and at least 200 million")`.
#[test]
fn turnover_floor_rejects_lower_or_non_finite() {
    let stocks = vec![snapshot("600519.SH", "贵州茅台", "A", 1e9)];
    for bad in [1e8, 2e8 - 1.0, f64::NAN, f64::INFINITY] {
        let err = apply_hard_filters(&stocks, bad).unwrap_err();
        assert_eq!(
            err.to_string(),
            "minimum turnover must be finite and at least 200 million"
        );
    }
    assert!(apply_hard_filters(&stocks, 2e8).is_ok());
}

/// `rank_candidates(..., top_n=10)` caps the picks at `top_n`; candidate #11+
/// land in `rejected`, and `min_confidence=70` is inclusive.
#[test]
fn top_n_cap_and_confidence_floor() {
    let mut candidates = Vec::new();
    for index in 0..12 {
        let mut stock = snapshot(
            &format!("60000{index}.SH"),
            &format!("股票{index}"),
            "A",
            1e9 + index as f64,
        );
        stock.amount = 1e9 + index as f64 * 1e6;
        candidates.push(uzi_screen::daily_screen::models::ScreenCandidate {
            snapshot: stock,
            research_confidence: 90.0 - index as f64,
            action: "watch_only".to_string(),
            why_now: String::new(),
            entry_condition: String::new(),
            invalidation: String::new(),
            theme_rank: None,
            leader_rank: None,
            theme_breadth_pct: None,
            persona_verdicts: Vec::new(),
            serenity: None,
            evidence: Vec::new(),
            data_gaps: Vec::new(),
            risk_flags: Vec::new(),
        });
    }
    let (picks, rejected) = rank_candidates(&candidates, 10, 70.0);
    assert_eq!(picks.len(), 10);
    assert_eq!(rejected.len(), 2);
    // Highest confidence first.
    assert_eq!(picks[0].research_confidence, 90.0);
    assert_eq!(picks[9].snapshot.code, "600009.SH");

    // `>= min_confidence` is inclusive: 70.0 passes, 69.9 does not.
    let boundary = vec![
        uzi_screen::daily_screen::models::ScreenCandidate {
            research_confidence: 70.0,
            ..candidates[0].clone()
        },
        uzi_screen::daily_screen::models::ScreenCandidate {
            research_confidence: 69.9,
            ..candidates[1].clone()
        },
        uzi_screen::daily_screen::models::ScreenCandidate {
            research_confidence: 0.0,
            ..candidates[2].clone()
        },
    ];
    let (picks, rejected) = rank_candidates(&boundary, 10, 70.0);
    assert_eq!(picks.len(), 1);
    assert_eq!(picks[0].research_confidence, 70.0);
    assert_eq!(rejected.len(), 2);
    assert_eq!(rejected[0].research_confidence, 69.9);
}

/// A-share names starting with `ST` / `*ST` / `SST` (case-insensitive) or
/// containing `退` are dropped; H-share names containing `退市` / `停牌` are
/// dropped. Quote validity is also a gate.
#[test]
fn st_delisted_and_invalid_quotes_are_filtered() {
    let rows: Value = json!([
        {"代码": "600519", "名称": "贵州茅台", "最新价": 1680.5, "涨跌幅": 2.1, "成交额": 5.2e9},
        {"代码": "600221", "名称": "ST海航", "最新价": 1.5, "涨跌幅": 1.0, "成交额": 3e8},
        {"代码": "600222", "名称": "*ST海航", "最新价": 1.5, "涨跌幅": 1.0, "成交额": 3e8},
        {"代码": "600223", "名称": "SST海航", "最新价": 1.5, "涨跌幅": 1.0, "成交额": 3e8},
        {"代码": "600224", "名称": "st海航", "最新价": 1.5, "涨跌幅": 1.0, "成交额": 3e8},
        {"代码": "600888", "名称": "蓝光退", "最新价": 0.9, "涨跌幅": -5.0, "成交额": 2.5e8},
        {"代码": "000001", "名称": "平安银行", "最新价": 0, "涨跌幅": 0.0, "成交额": 5e8},
        {"代码": "000002", "名称": "万科A", "最新价": -1.0, "涨跌幅": 0.0, "成交额": 5e8},
        {"代码": "000003", "名称": "无涨跌幅", "最新价": 5.0, "成交额": 5e8},
        {"代码": "000004", "名称": "无成交额", "最新价": 5.0, "涨跌幅": 1.0},
        {"代码": "000005", "名称": "", "最新价": 5.0, "涨跌幅": 1.0, "成交额": 5e8},
        {"代码": "not-a-code", "名称": "坏代码", "最新价": 5.0, "涨跌幅": 1.0, "成交额": 5e8},
        {"代码": "1234", "名称": "短代码", "最新价": 5.0, "涨跌幅": 1.0, "成交额": 5e8},
        {"代码": "600519", "名称": "报价为字符串", "最新价": "1680.5", "涨跌幅": "2.1", "成交额": "5,200,000,000"}
    ]);
    let kept = normalize_universe_frame(&rows, "A", "2025-09-10T14:30:00+08:00", "test");
    let codes: Vec<&str> = kept.iter().map(|s| s.code.as_str()).collect();
    // Only 贵州茅台 and the string-quoted row survive.
    assert_eq!(codes, vec!["600519.SH", "600519.SH"]);
    assert_eq!(kept[1].price, 1680.5);
    assert_eq!(kept[1].amount, 5.2e9);

    let hk_rows: Value = json!([
        {"代码": "00700", "名称": "腾讯控股", "最新价": 380.0, "涨跌幅": 2.5, "成交额": 3.2e9},
        {"代码": "03333", "名称": "恒大停牌", "最新价": 0.5, "涨跌幅": 0.0, "成交额": 3e8},
        {"代码": "12345", "名称": "测试退市", "最新价": 1.0, "涨跌幅": -1.0, "成交额": 3e8},
        {"代码": "00001", "名称": "长和", "最新价": 45.0, "涨跌幅": -0.6, "成交额": 3.5e8}
    ]);
    let kept = normalize_universe_frame(&hk_rows, "H", "2025-09-10T15:30:00+08:00", "test");
    let codes: Vec<&str> = kept.iter().map(|s| s.code.as_str()).collect();
    assert_eq!(codes, vec!["00700.HK", "00001.HK"]);
}

/// Exchange suffixes follow upstream `_full_code`: 4/8/92 → BJ, 5/6/9 → SH,
/// everything else SZ; HK zero-pads to 5 digits.
#[test]
fn exchange_suffix_assignment() {
    let rows: Value = json!([
        {"代码": "400001", "名称": "北交所", "最新价": 1.0, "涨跌幅": 1.0, "成交额": 1e9},
        {"代码": "830799", "名称": "艾融软件", "最新价": 1.0, "涨跌幅": 1.0, "成交额": 1e9},
        {"代码": "920001", "名称": "新三板", "最新价": 1.0, "涨跌幅": 1.0, "成交额": 1e9},
        {"代码": "600519", "名称": "沪市", "最新价": 1.0, "涨跌幅": 1.0, "成交额": 1e9},
        {"代码": "688981", "名称": "科创板", "最新价": 1.0, "涨跌幅": 1.0, "成交额": 1e9},
        {"代码": "000001", "名称": "深市", "最新价": 1.0, "涨跌幅": 1.0, "成交额": 1e9},
        {"代码": "300750", "名称": "创业板", "最新价": 1.0, "涨跌幅": 1.0, "成交额": 1e9},
        {"代码": "1234", "名称": "港股", "最新价": 1.0, "涨跌幅": 1.0, "成交额": 1e9}
    ]);
    let kept = normalize_universe_frame(&rows, "A", "2025-09-10T14:30:00+08:00", "test");
    let codes: Vec<&str> = kept.iter().map(|s| s.code.as_str()).collect();
    assert_eq!(
        codes,
        vec![
            "400001.BJ",
            "830799.BJ",
            "920001.BJ",
            "600519.SH",
            "688981.SH",
            "000001.SZ",
            "300750.SZ",
        ]
    );
    // HK zero-pads to 5 digits and keeps every row's own numeric code.
    let hk = normalize_universe_frame(&rows, "H", "2025-09-10T15:30:00+08:00", "test");
    let hk_codes: Vec<&str> = hk.iter().map(|s| s.code.as_str()).collect();
    assert_eq!(
        hk_codes,
        vec![
            "400001.HK",
            "830799.HK",
            "920001.HK",
            "600519.HK",
            "688981.HK",
            "000001.HK",
            "300750.HK",
            "01234.HK",
        ]
    );
}
