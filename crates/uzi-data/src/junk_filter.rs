//! Port of `lib/junk_filter.py` — autofill noise filter shared by the MX,
//! ddgs and Playwright fallbacks.

/// `JUNK_PATTERNS` — prompt residue / template placeholders / LLM apologies.
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

/// `is_junk_autofill_text(text)`.
///
/// Triggers when the text is empty, shorter than 5 chars, contains a junk
/// phrase, or is a sole repeated `；`-separated token (`类型；类型；类型`).
pub fn is_junk_autofill_text(text: &str) -> bool {
    if text.is_empty() {
        return true;
    }
    let t = text.trim();
    if t.chars().count() < 5 {
        return true;
    }
    if JUNK_PATTERNS.iter().any(|j| t.contains(*j)) {
        return true;
    }
    let parts: Vec<&str> = t
        .split('；')
        .map(|p| p.trim())
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

/// `is_junk_autofill` alias used by `score_fns`.
pub fn is_junk_autofill(text: &str) -> bool {
    is_junk_autofill_text(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_and_empty_are_junk() {
        assert!(is_junk_autofill_text(""));
        assert!(is_junk_autofill_text("abc"));
        assert!(!is_junk_autofill_text("贵州茅台最新公告"));
    }

    #[test]
    fn prompt_residue_is_junk() {
        assert!(is_junk_autofill_text("类型；类型"));
        assert!(is_junk_autofill_text("抱歉，我无法回答这个问题"));
        assert!(is_junk_autofill_text("这里的值 TODO 待补"));
    }

    #[test]
    fn repeated_segments_are_junk_but_distinct_are_not() {
        assert!(is_junk_autofill_text("白酒；白酒；白酒"));
        assert!(!is_junk_autofill_text("利率下行；汇率走稳；大宗反弹"));
    }
}
