//! Port of `lib/junk_filter.py` — autofill noise filter shared by the MX/ddgs and
//! Playwright fallbacks.

/// Prompt residue / template placeholders / LLM apologies.
pub const JUNK_PATTERNS: &[&str] = &[
    "类型；类型",
    "XXX",
    "TODO",
    "null",
    "undefined",
    "None",
    "抱歉，",
    "无法回答",
    "我不知道",
    "不清楚",
    "暂无数据",
    "（示例）",
    "（待补）",
];

/// Detect noise returned by MX / ddgs / Playwright fallbacks.
///
/// Triggers when the text is shorter than 5 characters, contains a blacklisted
/// phrase, or is semicolon-separated into identical parts.
pub fn is_junk_autofill_text(text: &str) -> bool {
    let t = text.trim();
    // Python `len()` counts characters, not bytes.
    if t.chars().count() < 5 {
        return true;
    }
    if JUNK_PATTERNS.iter().any(|j| t.contains(j)) {
        return true;
    }
    let parts: Vec<&str> = t
        .split('；')
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .collect();
    if parts.len() >= 2 {
        let first = parts[0];
        if parts.iter().all(|p| *p == first) {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_short_placeholder_and_repeated_payloads() {
        assert!(is_junk_autofill_text(""));
        assert!(is_junk_autofill_text("abc"));
        assert!(is_junk_autofill_text("类型；类型"));
        assert!(is_junk_autofill_text("抱歉，我无法回答"));
        assert!(is_junk_autofill_text("暂无数据"));
        assert!(is_junk_autofill_text("增长；增长；增长"));
        // character count, not byte count: 4 CJK chars is still too short
        assert!(is_junk_autofill_text("行业景气"));
        assert!(!is_junk_autofill_text("铜价同比上涨 12%"));
        assert!(!is_junk_autofill_text("营收增长；毛利改善"));
    }
}
