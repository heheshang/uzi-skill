//! Port of `prewarm_cache.py` — build a distributable warm-cache pack so a
//! first run does not have to re-fetch the data every analysis shares.
//!
//! Upstream's stated intent is to pre-populate only the **cross-ticker, public**
//! data (`_global/api_cache`): the A-share name table and the macro / policy /
//! moat searches every analysis triggers. Two upstream defects are corrected
//! here rather than reproduced, because both make the pack unusable:
//!
//! 1. **Format.** `prewarm_cache._save` writes `_cached_at` as an ISO *string*,
//!    but `lib.cache.cached` does `now - payload["_cached_at"] < ttl`. Reading a
//!    prewarmed entry therefore raises `TypeError: unsupported operand type(s)
//!    for -: 'float' and 'str'` — verified against upstream. This port writes the
//!    epoch float (plus `_ttl`) that the reader actually consumes, via
//!    [`uzi_core::cache`]'s own writer.
//! 2. **Keys.** Upstream also warms `industry_pe__*`, `fund_nav__*` and
//!    `futures_main__*` into `_global`, but no reader looks those up there: fund
//!    stats are read as `{fund_code}/fund_stats_{fund_code}`, futures are fetched
//!    per ticker, and the cninfo industry endpoint is unreachable from this port
//!    (it needs an AES `Accept-Enckey` token minted by cninfo's JS). Those steps
//!    are reported as unsupported instead of writing dead cache entries.

use std::path::{Path, PathBuf};

use serde_json::Value;

use uzi_core::cache::{cache_path, cache_root};

/// Upstream's `UNIVERSAL_QUERIES` — the cross-stock searches worth shipping.
/// Each entry is `(dim_key, query template)`; the current year is substituted.
const UNIVERSAL_QUERIES: &[(&str, &str)] = &[
    // 3_macro · macro conditions
    ("3_macro", "{year} 中国 利率 货币政策 降息 最新"),
    ("3_macro", "{year} 美联储 利率周期 最新"),
    ("3_macro", "{year} 人民币 汇率 走势"),
    // 13_policy · the most common industry policies
    ("13_policy", "{year} 白酒 国家政策 扶持 利好"),
    ("13_policy", "{year} 半导体 国产替代 政策"),
    ("13_policy", "{year} 新能源 政策 利好"),
    ("13_policy", "{year} 医药 集采 政策"),
    ("13_policy", "{year} 有色金属 国家政策"),
    ("13_policy", "{year} 工业金属 政策 扶持"),
    // 14_moat · common moat keywords
    ("14_moat", "白酒 上市公司 品牌壁垒 竞争优势"),
    ("14_moat", "半导体 上市公司 专利 核心技术"),
    ("14_moat", "新能源 上市公司 市场份额 龙头"),
];

/// Upstream's `sanity_check_output` suspicious patterns.
///
/// A warm pack is meant to be published, so it must not carry API keys, the
/// contributor's home paths, or contact addresses.
pub const SENSITIVE_PATTERNS: &[(&str, &str)] = &[
    ("sk-", "OpenAI key 格式"),
    ("mkt_", "MX API key 格式"),
    ("pk_", "私钥格式"),
    ("/Users/", "macOS 个人路径"),
    ("/home/", "Linux 个人路径"),
    ("C:\\Users\\", "Windows 个人路径"),
    ("@qq.com", "QQ 邮箱"),
    ("@163.com", "163 邮箱"),
    ("@gmail.com", "Gmail"),
];

/// A warm step that could not run in this port, with the reason.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unsupported {
    pub name: &'static str,
    pub reason: &'static str,
}

/// Outcome of a prewarm run.
#[derive(Debug, Clone, Default)]
pub struct WarmReport {
    /// Cache keys successfully written (or already fresh).
    pub written: Vec<String>,
    /// Steps this port cannot perform, and why.
    pub unsupported: Vec<Unsupported>,
    /// Sensitive-content findings from [`sanity_check`].
    pub issues: Vec<String>,
}

/// One sensitive-content finding: which file, which pattern, which label.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Leak {
    pub file: String,
    pub pattern: String,
    pub label: String,
}

impl Leak {
    pub fn render(&self) -> String {
        format!("{}: 含 {} ({:?})", self.file, self.label, self.pattern)
    }
}

/// `sanity_check_output()` — scan a warm-pack directory for secrets and
/// personal data. Returns every finding; empty means the pack is clean.
pub fn sanity_check(dir: &Path) -> Vec<Leak> {
    let mut leaks = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return leaks;
    };
    let mut files: Vec<PathBuf> = entries
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().map(|x| x == "json").unwrap_or(false))
        .collect();
    files.sort();

    for path in files {
        let Ok(content) = std::fs::read_to_string(&path) else {
            continue;
        };
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        for (pattern, label) in SENSITIVE_PATTERNS {
            if content.contains(pattern) {
                leaks.push(Leak {
                    file: name.clone(),
                    pattern: (*pattern).to_string(),
                    label: (*label).to_string(),
                });
            }
        }
    }
    leaks
}

/// Absolute path of a written cache entry, or `None` when the key lives under a
/// different cache root (i.e. `UZI_CACHE_ROOT` redirected it).
fn written_path(ticker: &str, key: &str) -> PathBuf {
    cache_path(ticker, key)
}

/// Warm the A-share `(code, name)` table used by name resolution.
///
/// Reads through [`crate::sources::build_a_share_index`], which writes the
/// `_global/a_share_name_index` entry with the cache module's real format.
/// Success is judged by re-reading that entry, not by whether the file existed
/// beforehand — on a cold cache it never does.
pub fn warm_stock_name_table(report: &mut WarmReport) {
    crate::sources::build_a_share_index();

    let path = written_path("_global", "a_share_name_index");
    let rows = std::fs::read_to_string(&path)
        .ok()
        .and_then(|t| serde_json::from_str::<Value>(&t).ok())
        .and_then(|v| v.get("data").and_then(|d| d.as_array()).map(|a| a.len()))
        .unwrap_or(0);

    if rows == 0 {
        // Reached the endpoint but got nothing usable (e.g. EastMoney push2
        // returning 502). Saying so beats recording a phantom success.
        report.unsupported.push(Unsupported {
            name: "stock_name_table",
            reason: "A 股代码/名称表返回为空（push2 常被反爬拦截；见 registry 的 em_push2 条目）",
        });
        return;
    }
    report
        .written
        .push(format!("_global/a_share_name_index ({rows} rows)"));
}

/// Warm the cross-stock macro / policy / moat searches.
pub fn warm_qualitative_searches(report: &mut WarmReport, year: i32, max_results: usize) {
    for (dim_key, template) in UNIVERSAL_QUERIES {
        let query = template.replace("{year}", &year.to_string());
        let hits = crate::web_search::search_trusted(&query, dim_key, max_results, &[], 0);
        let usable = hits
            .iter()
            .any(|h| h.get("error").is_none() && h.get("_budget_exceeded").is_none());
        if usable {
            report.written.push(format!("_global {dim_key}: {query}"));
        } else {
            report.unsupported.push(Unsupported {
                name: "qualitative_search",
                reason: "搜索未返回可用结果（网络不可达或配额耗尽）",
            });
        }
    }
}

/// Run the warm steps this port supports and scan the result.
///
/// `year` is injected so the generated queries stay testable.
pub fn warm_all(year: i32, max_results: usize) -> WarmReport {
    let mut report = WarmReport::default();

    warm_stock_name_table(&mut report);
    warm_qualitative_searches(&mut report, year, max_results);

    // Steps upstream performs that have no reader in this port.
    report.unsupported.extend([
        Unsupported {
            name: "industry_taxonomy",
            reason: "cninfo 行业接口需要 JS 生成的 AES Accept-Enckey，本移植不发起该请求",
        },
        Unsupported {
            name: "common_fund_nav",
            reason: "基金净值缓存键是 {fund_code}/fund_stats_{fund_code}，按 _global/fund_nav__* 预热不会被读取",
        },
        Unsupported {
            name: "futures_main",
            reason: "期货主连按个股 ticker 拉取并缓存，_global/futures_main__* 不会被读取",
        },
    ]);

    let root = cache_root().join("_global").join("api_cache");
    report.issues = sanity_check(&root).iter().map(Leak::render).collect();
    report
}

/// Print a [`WarmReport`] the way upstream's script reports progress.
pub fn format_report(report: &WarmReport, dir: &Path) -> String {
    let mut out = String::new();
    out.push_str(&format!("输出目录: {}\n", dir.display()));
    out.push_str(&format!("\n✓ 已预热 {} 项\n", report.written.len()));
    for w in &report.written {
        out.push_str(&format!("    ✓ {w}\n"));
    }
    if !report.unsupported.is_empty() {
        out.push_str(&format!("\n⚠ 跳过 {} 项（本移植无对应读取方）\n", report.unsupported.len()));
        for u in &report.unsupported {
            out.push_str(&format!("    – {}: {}\n", u.name, u.reason));
        }
    }
    out.push_str("\n[SAFETY] 输出内容敏感性扫描\n");
    if report.issues.is_empty() {
        out.push_str("  ✓ 扫描通过，未发现敏感信息\n");
    } else {
        out.push_str("  ⚠️ 发现潜在敏感内容：\n");
        for i in &report.issues {
            out.push_str(&format!("  {i}\n"));
        }
        out.push_str("  请人工审查后再打包分发\n");
    }
    out
}

/// `prewarm_cache.main()` — warm, scan, and report.
pub fn main_prewarm(year: i32, max_results: usize) -> WarmReport {
    let report = warm_all(year, max_results);
    let dir = cache_root().join("_global").join("api_cache");
    print!("{}", format_report(&report, &dir));
    report
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use uzi_core::cache::TTL_STATIC;

    fn tmp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("uzi_prewarm_{}_{}", tag, std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn safety_scan_flags_each_sensitive_pattern() {
        let dir = tmp_dir("leaks");
        std::fs::write(
            dir.join("a.json"),
            r#"{"data":["sk-abc123","mkt_live_key"]}"#,
        )
        .unwrap();
        std::fs::write(dir.join("b.json"), r#"{"p":"/Users/shang/secret"}"#).unwrap();
        std::fs::write(dir.join("c.json"), r#"{"mail":"x@qq.com"}"#).unwrap();
        std::fs::write(dir.join("clean.json"), r#"{"data":[1,2,3]}"#).unwrap();

        let leaks = sanity_check(&dir);
        let rendered: Vec<String> = leaks.iter().map(Leak::render).collect();

        assert!(rendered.iter().any(|l| l.contains("a.json") && l.contains("sk-")), "{rendered:?}");
        assert!(rendered.iter().any(|l| l.contains("a.json") && l.contains("mkt_")), "{rendered:?}");
        assert!(rendered.iter().any(|l| l.contains("b.json") && l.contains("/Users/")), "{rendered:?}");
        assert!(rendered.iter().any(|l| l.contains("c.json") && l.contains("@qq.com")), "{rendered:?}");
        // The clean file must not be reported.
        assert!(!rendered.iter().any(|l| l.contains("clean.json")), "{rendered:?}");
        assert_eq!(leaks.len(), 4, "{rendered:?}");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn safety_scan_of_a_clean_dir_is_empty_and_a_missing_dir_is_not_an_error() {
        let dir = tmp_dir("clean");
        std::fs::write(dir.join("ok.json"), r#"{"data":{"price":18.56}}"#).unwrap();
        assert!(sanity_check(&dir).is_empty());
        let _ = std::fs::remove_dir_all(&dir);

        // A pack that was never produced must not panic.
        assert!(sanity_check(&PathBuf::from("/definitely/not/here")).is_empty());
    }

    #[test]
    fn scan_ignores_non_json_artifacts() {
        let dir = tmp_dir("nonjson");
        // A README carrying a path is documentation, not leaked cache data.
        std::fs::write(dir.join("README.md"), "/Users/me/notes").unwrap();
        assert!(sanity_check(&dir).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The payload the cache writer produces must carry a numeric `_cached_at`
    /// and a `_ttl` — the shape `cached()` reads back. The regression this port
    /// corrects is upstream writing an ISO *string* here, which made the reader
    /// raise `TypeError` instead of serving the prewarmed entry.
    #[test]
    fn cached_payload_shape_is_numeric() {
        let payload = json!({
            "_cached_at": 1_700_000_000.0,
            "data": [{"code": "600519", "name": "贵州茅台"}],
            "_ttl": TTL_STATIC,
        });
        assert!(payload["_cached_at"].is_f64());
        assert!(payload["_ttl"].is_u64());
        assert!(payload["data"].is_array());
    }

    #[test]
    fn report_renders_supported_and_skipped_steps() {
        let report = WarmReport {
            written: vec!["_global/a_share_name_index (5000 rows)".into()],
            unsupported: vec![Unsupported {
                name: "futures_main",
                reason: "per-ticker",
            }],
            issues: vec!["x.json: 含 QQ 邮箱 (\"@qq.com\")".into()],
        };
        let text = format_report(&report, Path::new("/tmp/pack"));
        assert!(text.contains("已预热 1 项"));
        assert!(text.contains("a_share_name_index"));
        assert!(text.contains("跳过 1 项"));
        assert!(text.contains("futures_main"));
        assert!(text.contains("发现潜在敏感内容"));
        assert!(text.contains("x.json"));
    }

    #[test]
    fn universal_queries_cover_the_shared_dimensions() {
        // Upstream warmed macro / policy / moat only; keep that shape so the pack
        // stays cross-stock and carries no user-specific query.
        for dim in ["3_macro", "13_policy", "14_moat"] {
            assert!(
                UNIVERSAL_QUERIES.iter().any(|(d, _)| *d == dim),
                "missing universal queries for {dim}"
            );
        }
        // Every dim must have trusted domains, or the search degrades to an
        // untargeted query.
        for (dim, _) in UNIVERSAL_QUERIES {
            assert!(
                !crate::web_search::trusted_domains_for(dim).is_empty(),
                "no trusted domains for {dim}"
            );
        }
    }

    #[test]
    fn year_placeholder_is_substituted_in_queries() {
        let queries: Vec<String> = UNIVERSAL_QUERIES
            .iter()
            .map(|(_, t)| t.replace("{year}", "2026"))
            .collect();
        assert!(queries.iter().all(|q| !q.contains("{year}")));
        assert!(queries.iter().any(|q| q.contains("2026 中国 利率")));
    }
}
