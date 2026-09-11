//! Port of `lib/daily_screen/themes.py` — cross-sectional theme strength derived
//! from the same observable snapshot.

use serde_json::{Map, Value};

use super::events::{evidence_time, iso_auto};
use super::models::StockSnapshot;

struct Row<'a> {
    market: String,
    avg_change_pct: f64,
    breadth_pct: f64,
    amount: f64,
    members: Vec<&'a StockSnapshot>,
    observed_at: Option<chrono::DateTime<chrono::FixedOffset>>,
}

/// `build_theme_context(stocks)` keyed by stock code.
pub fn build_theme_context(stocks: &[StockSnapshot]) -> Value {
    // Python `defaultdict(list)` keeps first-insertion order; a Vec preserves it.
    let mut groups: Vec<((String, String), Vec<&StockSnapshot>)> = Vec::new();
    for stock in stocks {
        let industry = stock.industry.trim().to_string();
        let lower = industry.to_lowercase();
        if matches!(lower.as_str(), "" | "未分类" | "未知" | "—" | "nan" | "none") {
            continue;
        }
        let key = (stock.market.clone(), industry);
        match groups.iter_mut().find(|(k, _)| *k == key) {
            Some((_, members)) => members.push(stock),
            None => groups.push((key, vec![stock])),
        }
    }

    let mut rows: Vec<Row<'_>> = Vec::new();
    for ((market, _industry), members) in groups {
        if members.len() < 2 {
            continue;
        }
        let count = members.len() as f64;
        let avg = members.iter().map(|s| s.change_pct).sum::<f64>() / count;
        let breadth = members.iter().filter(|s| s.change_pct > 0.0).count() as f64 / count * 100.0;
        let amount = members.iter().map(|s| s.amount).sum::<f64>();
        // `sorted(..., key=(change_pct, amount), reverse=True)` — stable.
        let mut sorted: Vec<&StockSnapshot> = members.clone();
        sorted.sort_by(|a, b| {
            (b.change_pct, b.amount)
                .partial_cmp(&(a.change_pct, a.amount))
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        let observed_at = members
            .iter()
            .filter_map(|s| evidence_time(&Value::String(s.observed_at.clone())))
            .min();
        rows.push(Row {
            market,
            avg_change_pct: avg,
            breadth_pct: breadth,
            amount,
            members: sorted,
            observed_at,
        });
    }

    // `rows.sort(key=(avg_change_pct, breadth_pct, amount), reverse=True)`.
    rows.sort_by(|a, b| {
        (b.avg_change_pct, b.breadth_pct, b.amount)
            .partial_cmp(&(a.avg_change_pct, a.breadth_pct, a.amount))
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let mut context = Map::new();
    let mut market_ranks: Vec<(String, i64)> = Vec::new();
    for row in &rows {
        let theme_rank = match market_ranks.iter_mut().find(|(m, _)| *m == row.market) {
            Some((_, rank)) => {
                *rank += 1;
                *rank
            }
            None => {
                market_ranks.push((row.market.clone(), 1));
                1
            }
        };
        for (index, stock) in row.members.iter().enumerate() {
            let leader_rank = (index + 1) as i64;
            let mut entry = Map::new();
            entry.insert("theme_rank".into(), Value::from(theme_rank));
            entry.insert("leader_rank".into(), Value::from(leader_rank));
            entry.insert(
                "breadth_pct".into(),
                Value::from(uzi_core::py::round(row.breadth_pct, 1)),
            );
            entry.insert(
                "theme_avg_change_pct".into(),
                Value::from(uzi_core::py::round(row.avg_change_pct, 2)),
            );
            entry.insert("theme_amount".into(), Value::from(row.amount));
            entry.insert(
                "observed_at".into(),
                match row.observed_at {
                    Some(dt) => Value::String(iso_auto(&dt)),
                    None => Value::Null,
                },
            );
            entry.insert("sample_size".into(), Value::from(row.members.len()));
            context.insert(stock.code.clone(), Value::Object(entry));
        }
    }
    Value::Object(context)
}
