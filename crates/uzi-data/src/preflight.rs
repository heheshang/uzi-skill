//! Port of `lib/pipeline/preflight_helpers.py` — stage-1 preflight, ticker
//! resolution, non-stock guards, and of `lib/pipeline/score_fns.py`'s
//! `_autofill_qualitative_via_mx` / `_extract_mx_text` (which the CLI uses to
//! patch the six qualitative dims when fetchers come back empty).
//!
//! `prepare_target` reproduces the early-exit payloads verbatim — uzi-cli
//! returns them straight to the caller and skips stage 2.

use serde_json::{json, Map, Value};

use uzi_core::ticker::{classify_security_type, is_chinese_name, parse_ticker, TickerInfo};

use crate::junk_filter::is_junk_autofill_text;
use crate::mx::{extract_mx_text, MXClient};
use crate::sources;
use crate::web_search;

/// `_NON_STOCK_GUIDANCE` (preflight variant, with the `label`/`why`/`what_to_do`
/// triple used for the early-exit payload).
fn non_stock_guidance(sec_type: &str) -> Option<(&'static str, &'static str, &'static str)> {
    match sec_type {
        "etf" => Some((
            "ETF",
            "51 评委跑 ROE / 护城河 / 管理层 / 分红 这些个股财务指标，ETF 没这些字段",
            "建议：分析该 ETF 前 3-5 大持仓股（用 akshare.fund_portfolio_hold_em 查持仓），对每只成分股单独跑 /analyze-stock",
        )),
        "lof" => Some((
            "LOF 基金",
            "基金没有企业基本面字段",
            "基金评估用专门的 fund-analyze 工具",
        )),
        "mutual_fund" => Some((
            "开放式基金",
            "基金没有企业基本面字段（个股评委不适用）",
            "已自动改为分析该基金的前 10 大重仓股（akshare.fund_portfolio_hold_em）",
        )),
        "convertible_bond" => Some((
            "可转债",
            "可转债看转股价/溢价率/到期收益率，不是 ROE",
            "分析正股或用集思录的可转债工具",
        )),
        _ => None,
    }
}

fn write_resolve_error(ticker_dir: &str, payload: &Value) {
    let dir = uzi_core::cache::cache_root().join(ticker_dir);
    let _ = std::fs::create_dir_all(&dir);
    let _ = uzi_core::json::write_json(&dir.join("_resolve_error.json"), payload);
}

/// `prepare_target(ticker, detect_lite_fn)`.
///
/// Returns `{"ok": true, "ticker_info": {...}}` or
/// `{"ok": false, "early_exit": "name_not_resolved"|"non_stock_security", "payload": {...}}`.
pub fn prepare_target(ticker: &str, detect_lite: Option<bool>) -> Value {
    // v2.10.2 · network preflight (自动切 lite)
    let skip_preflight = std::env::var("UZI_SKIP_PREFLIGHT").as_deref() == Ok("1");
    if !skip_preflight {
        let pre = crate::network_preflight::run_preflight(true, 3.0);
        let severity = pre.get("severity").and_then(|s| s.as_str()).unwrap_or("ok");
        if (severity == "critical" || severity == "degraded")
            && std::env::var("UZI_LITE").as_deref() != Ok("0")
        {
            std::env::set_var("UZI_LITE", "1");
        }
    }

    // Lite mode detection
    if detect_lite == Some(true) {
        std::env::set_var("UZI_LITE", "1");
        if std::env::var("UZI_DDG_BUDGET").is_err() {
            std::env::set_var("UZI_DDG_BUDGET", "15");
        }
    }

    // 中文名解析 — 无法明确解析时早退并返回候选
    let ti: TickerInfo = if is_chinese_name(ticker) {
        let r = sources::resolve_chinese_name_rich(ticker);
        let resolved = r.get("resolved").cloned().unwrap_or(Value::Null);
        if !resolved.is_null() {
            let full = resolved
                .get("full")
                .and_then(|v| v.as_str())
                .unwrap_or(ticker);
            parse_ticker(full)
        } else {
            let candidates = r
                .get("candidates")
                .and_then(|c| c.as_array())
                .cloned()
                .unwrap_or_default();
            if !candidates.is_empty() {
                let message = format!(
                    "未能确认 '{ticker}' 对应的股票。最接近的候选: {}",
                    candidates
                        .iter()
                        .take(3)
                        .map(|c| format!(
                            "{}({})",
                            c.get("name").and_then(|v| v.as_str()).unwrap_or(""),
                            c.get("code").and_then(|v| v.as_str()).unwrap_or("")
                        ))
                        .collect::<Vec<_>>()
                        .join(", ")
                );
                let payload = json!({
                    "status": "name_not_resolved",
                    "user_input": ticker,
                    "candidates": candidates.iter().take(5).cloned().collect::<Vec<_>>(),
                    "message": message,
                });
                write_resolve_error(ticker, &payload);
                return json!({
                    "ok": false,
                    "early_exit": "name_not_resolved",
                    "payload": payload,
                });
            }
            parse_ticker(ticker)
        }
    } else {
        parse_ticker(ticker)
    };

    // v2.9.2 · ETF / LOF / 可转债识别
    if let Some(guard) = check_non_stock_security(&ti) {
        return json!({
            "ok": false,
            "early_exit": "non_stock_security",
            "payload": guard,
        });
    }

    json!({
        "ok": true,
        "ticker_info": serde_json::to_value(&ti).unwrap_or_else(|_| json!({})),
    })
}

/// `_check_non_stock_security(ti)` — returns the error payload or `None`.
pub fn check_non_stock_security(ti: &TickerInfo) -> Option<Value> {
    if ti.market != "A" {
        return None;
    }
    let sec_type = classify_security_type(&ti.code).as_str().to_string();
    let (label, why, what_to_do) = non_stock_guidance(&sec_type)?;

    // ETF / LOF / mutual_fund pull the top-10 holdings for the user to pick.
    let mut top_holdings: Vec<Value> = Vec::new();
    if matches!(sec_type.as_str(), "etf" | "lof" | "mutual_fund") {
        if let Some(rows) = sources::fetch_fund_portfolio_hold(&ti.code).as_array() {
            for (i, row) in rows.iter().take(10).enumerate() {
                let stock_code = row
                    .get("股票代码")
                    .or_else(|| row.get("code"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .trim()
                    .to_string();
                let stock_name = row
                    .get("股票名称")
                    .or_else(|| row.get("name"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .trim()
                    .to_string();
                if stock_code.is_empty() || stock_name.is_empty() {
                    continue;
                }
                let full_code = parse_ticker(&stock_code).full;
                let pct_raw = row
                    .get("占净值比例")
                    .or_else(|| row.get("比例"))
                    .or_else(|| row.get("weight"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let pct = pct_raw.replace('%', "").parse::<f64>().unwrap_or(0.0);
                top_holdings.push(json!({
                    "rank": i + 1,
                    "code": full_code,
                    "name": stock_name,
                    "weight_pct": if pct != 0.0 { json!(uzi_core::py::round(pct, 2)) } else { Value::Null },
                }));
            }
        }
    }

    let payload = json!({
        "status": "non_stock_security",
        "security_type": sec_type,
        "ticker": ti.full,
        "label": label,
        "why": why,
        "what_to_do": what_to_do,
        "top_holdings": top_holdings,
        "message": format!(
            "{} 是 {}，不是个股 — 本插件未设计支持这类标的。\n原因: {}\n{}",
            ti.full, label, why, what_to_do
        ),
        "user_prompt": if top_holdings.is_empty() {
            "".to_string()
        } else {
            "请选择要分析的成分股（输入编号或代码），例如：`/analyze-stock 1` 或 `/analyze-stock 601899`".to_string()
        },
    });
    write_resolve_error(&ti.full, &payload);
    Some(payload)
}

// ─────────────────────────────────────────────────────────────
// score_fns._autofill_qualitative_via_mx
// ─────────────────────────────────────────────────────────────

/// The six qualitative dims and their emptiness predicates + query builders.
struct Target {
    dim_key: &'static str,
    is_empty: fn(&Value) -> bool,
    query: fn(&str, &str, &str) -> String,
}

/// `_is_default_or_empty(v)`.
fn is_default_or_empty(v: &Value) -> bool {
    if matches!(v, Value::Null) {
        return true;
    }
    if let Value::String(s) = v {
        if s.is_empty() || s == "—" || s == "-" || s == "n/a" || s == "N/A" {
            return true;
        }
        if ["中性（", "中性(", "未拉取", "未命中", "无直接关联"]
            .iter()
            .any(|kw| s.contains(kw))
        {
            return true;
        }
        return false;
    }
    match v {
        Value::Array(a) => a.is_empty(),
        Value::Object(o) => o.is_empty(),
        _ => false,
    }
}

fn g<'a>(d: &'a Value, k: &str) -> &'a Value {
    d.get(k).unwrap_or(&Value::Null)
}

fn targets() -> [Target; 6] {
    [
        Target {
            dim_key: "3_macro",
            is_empty: |d| {
                ["rate_cycle", "fx_trend", "geo_risk", "commodity"]
                    .iter()
                    .all(|k| is_default_or_empty(g(d, k)))
            },
            // v3.9.4 · 聚焦宏观 · 不带行业名
            query: |_n, _c, _i| "2026 中国 利率 货币政策 降息 汇率 大宗商品 宏观环境".to_string(),
        },
        Target {
            dim_key: "7_industry",
            is_empty: |d| {
                is_default_or_empty(g(d, "growth"))
                    && !g(d, "cninfo_metrics")
                        .get("industry_pe_weighted")
                        .map(|v| !v.is_null())
                        .unwrap_or(false)
            },
            query: |_n, _c, i| format!("{i} 2026 行业增速 TAM 市场规模 渗透率"),
        },
        Target {
            dim_key: "8_materials",
            is_empty: |d| is_default_or_empty(g(d, "core_material")),
            query: |n, c, _i| format!("{n} {c} 主营业务 主要原材料 成本构成"),
        },
        Target {
            dim_key: "9_futures",
            is_empty: |d| {
                is_default_or_empty(g(d, "linked_contract"))
                    || g(d, "linked_contract")
                        .as_str()
                        .map(|s| s.contains("无直接"))
                        .unwrap_or(false)
            },
            query: |_n, _c, i| format!("{i} 行业 上下游 期货品种 套保 大宗"),
        },
        Target {
            dim_key: "13_policy",
            is_empty: |d| {
                !["policy_dir", "subsidy", "monitoring", "anti_trust"]
                    .iter()
                    .any(|k| uzi_core::py::truthy(g(g(d, "snippets"), k)))
            },
            query: |_n, _c, i| format!("{i} 2026 国家政策 监管动态 补贴 税收 影响"),
        },
        Target {
            dim_key: "15_events",
            is_empty: |d| {
                !uzi_core::py::truthy(g(d, "event_timeline"))
                    && !uzi_core::py::truthy(g(d, "recent_news"))
                    && !uzi_core::py::truthy(g(d, "recent_notices"))
            },
            query: |n, c, _i| format!("{n} {c} 最新公告 重大事件 业绩 合同"),
        },
    ]
}

/// `_autofill_qualitative_via_mx(raw, ticker)` — mutates `raw["dimensions"]`
/// in place and returns the mutated raw.
pub fn autofill_qualitative_via_mx(raw: &mut Value, ticker: &str) -> Value {
    // Crypto dims are populated from crypto-native sources; the MX / DDG queries
    // below are A-share industry prompts and would inject equity noise.
    if uzi_core::ticker::parse_ticker(ticker).market == uzi_core::ticker::CRYPTO_MARKET {
        return raw.clone();
    }

    let client = MXClient::default();
    let mx_ok = client.available;

    let dims = match raw.get_mut("dimensions").and_then(|d| d.as_object_mut()) {
        Some(d) => d,
        None => return raw.clone(),
    };

    let basic = dims
        .get("0_basic")
        .and_then(|d| d.get("data"))
        .cloned()
        .unwrap_or_else(|| json!({}));
    let name = basic
        .get("name")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .unwrap_or(ticker)
        .to_string();
    let industry = basic
        .get("industry")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .unwrap_or("综合")
        .to_string();
    let code_raw = match ticker.split_once('.') {
        Some((c, _)) => c.to_string(),
        None => ticker.to_string(),
    };

    for target in targets() {
        let dim = dims.get(target.dim_key).cloned().unwrap_or_else(|| json!({}));
        let data = dim.get("data").cloned().unwrap_or_else(|| json!({}));
        // Upstream: `if not is_empty_fn(data): skipped_full; continue`
        let is_empty = (target.is_empty)(&data);
        if !is_empty {
            continue;
        }

        let query = (target.query)(&name, &code_raw, &industry);
        let mut text = String::new();
        let mut source_used: Option<&'static str> = None;

        if mx_ok {
            let r = client.query(&query);
            let candidate = extract_mx_text(&r);
            if !is_junk_autofill_text(&candidate) && !candidate.is_empty() {
                text = candidate;
                source_used = Some("mx_api");
            }
        }
        if text.is_empty() {
            let results = web_search::search(&query, 3, "ws");
            let mut snippets: Vec<String> = Vec::new();
            for r in results.iter().take(3) {
                let title = r.get("title").and_then(|v| v.as_str()).unwrap_or("").trim();
                let body = r.get("body").and_then(|v| v.as_str()).unwrap_or("").trim();
                if !title.is_empty() || !body.is_empty() {
                    let body80: String = body.chars().take(80).collect();
                    snippets.push(format!("{title} — {body80}").trim_matches([' ', '—']).to_string());
                }
            }
            let candidate: String = snippets.join("；").chars().take(300).collect();
            if !is_junk_autofill_text(&candidate) && !candidate.is_empty() {
                text = candidate;
                source_used = Some("ddgs");
            }
        }

        let mut new_data = data.as_object().cloned().unwrap_or_default();
        let prev_source = dim.get("source").and_then(|v| v.as_str()).unwrap_or("");
        if !text.is_empty() {
            let used = source_used.unwrap_or("ddgs");
            new_data.insert(
                "_autofill".into(),
                json!({"query": query, "snippet": text, "source": used}),
            );
            match target.dim_key {
                "3_macro" => insert_truncated(&mut new_data, "rate_cycle", &text, 80),
                "7_industry" => insert_truncated(&mut new_data, "growth", &text, 80),
                "8_materials" => insert_truncated(&mut new_data, "core_material", &text, 60),
                "9_futures" => insert_truncated(&mut new_data, "contract_trend", &text, 60),
                "13_policy" => {
                    let snippets = new_data
                        .entry("snippets".to_string())
                        .or_insert_with(|| json!({}));
                    if let Some(obj) = snippets.as_object_mut() {
                        let entry = obj
                            .entry("policy_dir".to_string())
                            .or_insert_with(|| json!([]));
                        if let Some(arr) = entry.as_array_mut() {
                            let title: String = text.chars().take(120).collect();
                            arr.push(json!({"title": title, "url": "", "source": used}));
                        }
                    }
                }
                "15_events" => {
                    let first: String = text.chars().take(120).collect();
                    new_data.insert("event_timeline".into(), json!([first]));
                }
                _ => {}
            }
            dims.insert(
                target.dim_key.to_string(),
                json!({
                    "ticker": ticker,
                    "data": Value::Object(new_data),
                    "source": format!("{prev_source}+autofill:{used}").trim_start_matches('+'),
                    "fallback": true,
                }),
            );
        } else {
            new_data.insert(
                "_autofill_failed".into(),
                json!({"query": query, "reason": "MX/ddgs 都没有返回内容"}),
            );
            dims.insert(
                target.dim_key.to_string(),
                json!({
                    "ticker": ticker,
                    "data": Value::Object(new_data),
                    "source": format!("{prev_source}+autofill_failed").trim_start_matches('+'),
                    "fallback": true,
                }),
            );
        }
    }

    raw.clone()
}

fn insert_truncated(data: &mut Map<String, Value>, key: &str, text: &str, limit: usize) {
    let value: String = if text.chars().count() > limit {
        format!("{}…", text.chars().take(limit).collect::<String>())
    } else {
        text.to_string()
    };
    data.insert(key.to_string(), json!(value));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prepare_target_parses_plain_code_offline() {
        std::env::set_var("UZI_SKIP_PREFLIGHT", "1");
        let r = prepare_target("600519.SH", Some(true));
        assert_eq!(r["ok"], json!(true));
        assert_eq!(r["ticker_info"]["full"], json!("600519.SH"));
        assert_eq!(r["ticker_info"]["market"], json!("A"));
        std::env::remove_var("UZI_SKIP_PREFLIGHT");
    }

    #[test]
    fn non_stock_guard_payload_shape() {
        // 510300 is an ETF by prefix
        std::env::set_var("UZI_SKIP_PREFLIGHT", "1");
        let r = prepare_target("510300.SH", Some(false));
        std::env::remove_var("UZI_SKIP_PREFLIGHT");
        assert_eq!(r["ok"], json!(false));
        assert_eq!(r["early_exit"], json!("non_stock_security"));
        let p = &r["payload"];
        assert_eq!(p["status"], json!("non_stock_security"));
        assert_eq!(p["label"], json!("ETF"));
        assert!(p["top_holdings"].is_array());
        assert!(p["what_to_do"].is_string());
    }

    #[test]
    fn default_or_empty_predicate_covers_placeholders() {
        assert!(is_default_or_empty(&json!("")));
        assert!(is_default_or_empty(&json!("—")));
        assert!(is_default_or_empty(&json!("中性（2026 货币政策）")));
        assert!(is_default_or_empty(&json!([])));
        assert!(!is_default_or_empty(&json!(0.0)));
        assert!(!is_default_or_empty(&json!("利好")));
    }

    #[test]
    fn autofill_skips_full_dims_and_marks_failures() {
        std::env::set_var("UZI_SKIP_PREFLIGHT", "1");
        std::env::remove_var("MX_APIKEY");
        let mut raw = json!({
            "dimensions": {
                "0_basic": {"data": {"name": "水晶光电", "industry": "光学光电子"}},
                "3_macro": {"data": {"rate_cycle": "利好（2026 货币政策）", "fx_trend": "中性",
                                     "geo_risk": "中性", "commodity": "中性"}, "source": "x"},
                "15_events": {"data": {}, "source": "legacy:fetch_events"}
            }
        });
        // 3_macro has real values → skipped; 15_events is empty → marked failed
        let out = autofill_qualitative_via_mx(&mut raw, "002273.SZ");
        assert_eq!(out["dimensions"]["3_macro"]["source"], json!("x"));
        let ev = &out["dimensions"]["15_events"];
        assert_eq!(ev["fallback"], json!(true));
        assert!(ev["data"]["_autofill_failed"].is_object());
        assert!(ev["source"].as_str().unwrap().ends_with("+autofill_failed"));
    }
}
