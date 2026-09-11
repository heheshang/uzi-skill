//! Port of `lib/report/security.py` — output-encoding helpers for report renderers.
//!
//! `escape_text` mirrors Python's `html.escape(html.unescape(str(value)), quote=True)`:
//! unescape first (so nested renderers can call it idempotently), then escape
//! `& < > " '`. `safe_url` mirrors `urllib.parse.urlparse` scheme/netloc checks and
//! `safe_asset_id` mirrors the `[^A-Za-z0-9_-]+` scrub + 80-char cap.

use regex::Regex;
use serde_json::{Map, Value};
use std::sync::LazyLock;

/// Python `html.escape(s, quote=True)`.
pub fn html_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 8);
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#x27;"),
            _ => out.push(c),
        }
    }
    out
}

/// HTML5 named character references (the subset Python's `html.unescape` table
/// contains that matters for report text; unknown names are left untouched).
fn named_entity(name: &str) -> Option<char> {
    let c = match name {
        "amp" | "AMP" => '&',
        "lt" | "LT" => '<',
        "gt" | "GT" => '>',
        "quot" | "QUOT" => '"',
        "apos" => '\'',
        "nbsp" | "NonBreakingSpace" => '\u{a0}',
        "iexcl" => '\u{a1}',
        "cent" => '\u{a2}',
        "pound" => '\u{a3}',
        "curren" => '\u{a4}',
        "yen" => '\u{a5}',
        "brvbar" => '\u{a6}',
        "sect" => '\u{a7}',
        "uml" | "die" | "Dot" => '\u{a8}',
        "copy" | "COPY" => '\u{a9}',
        "ordf" => '\u{aa}',
        "laquo" => '\u{ab}',
        "not" => '\u{ac}',
        "shy" => '\u{ad}',
        "reg" | "REG" | "circledR" => '\u{ae}',
        "macr" | "strns" => '\u{af}',
        "deg" => '\u{b0}',
        "plusmn" | "pm" | "PlusMinus" => '\u{b1}',
        "sup2" => '\u{b2}',
        "sup3" => '\u{b3}',
        "acute" | "DiacriticalAcute" => '\u{b4}',
        "micro" => '\u{b5}',
        "para" => '\u{b6}',
        "middot" | "centerdot" | "CenterDot" => '\u{b7}',
        "cedil" | "Cedilla" => '\u{b8}',
        "sup1" => '\u{b9}',
        "ordm" => '\u{ba}',
        "raquo" => '\u{bb}',
        "frac14" => '\u{bc}',
        "frac12" | "half" => '\u{bd}',
        "frac34" => '\u{be}',
        "iquest" => '\u{bf}',
        "times" => '\u{d7}',
        "divide" | "div" => '\u{f7}',
        "ndash" => '\u{2013}',
        "mdash" => '\u{2014}',
        "lsquo" | "OpenCurlyQuote" => '\u{2018}',
        "rsquo" | "rsquor" | "CloseCurlyQuote" => '\u{2019}',
        "sbquo" | "lsquor" => '\u{201a}',
        "ldquo" | "OpenCurlyDoubleQuote" => '\u{201c}',
        "rdquo" | "rdquor" | "CloseCurlyDoubleQuote" => '\u{201d}',
        "bdquo" | "ldquor" => '\u{201e}',
        "dagger" => '\u{2020}',
        "Dagger" | "ddagger" => '\u{2021}',
        "bull" | "bullet" => '\u{2022}',
        "hellip" | "mldr" => '\u{2026}',
        "permil" => '\u{2030}',
        "prime" => '\u{2032}',
        "Prime" => '\u{2033}',
        "lsaquo" => '\u{2039}',
        "rsaquo" => '\u{203a}',
        "oline" => '\u{203e}',
        "frasl" => '\u{2044}',
        "euro" => '\u{20ac}',
        "trade" | "TRADE" => '\u{2122}',
        "larr" | "leftarrow" | "LeftArrow" => '\u{2190}',
        "uarr" | "uparrow" | "UpArrow" => '\u{2191}',
        "rarr" | "rightarrow" | "RightArrow" => '\u{2192}',
        "darr" | "downarrow" | "DownArrow" => '\u{2193}',
        "harr" | "leftrightarrow" => '\u{2194}',
        "crarr" => '\u{21b5}',
        "forall" => '\u{2200}',
        "part" | "partiald" => '\u{2202}',
        "exist" | "Exists" => '\u{2203}',
        "empty" | "emptyset" | "emptyv" | "varnothing" => '\u{2205}',
        "nabla" => '\u{2207}',
        "isin" | "isinv" | "Element" | "in" => '\u{2208}',
        "notin" | "NotElement" | "notinva" => '\u{2209}',
        "ni" | "niv" | "ReverseElement" | "SuchThat" => '\u{220b}',
        "prod" | "Product" => '\u{220f}',
        "sum" | "Sum" => '\u{2211}',
        "minus" => '\u{2212}',
        "lowast" => '\u{2217}',
        "radic" | "Sqrt" => '\u{221a}',
        "prop" | "propto" | "Proportional" | "vprop" | "varpropto" => '\u{221d}',
        "infin" => '\u{221e}',
        "ang" | "angle" => '\u{2220}',
        "and" | "wedge" => '\u{2227}',
        "or" | "vee" => '\u{2228}',
        "cap" | "Intersection" => '\u{2229}',
        "cup" | "Union" => '\u{222a}',
        "int" | "Integral" => '\u{222b}',
        "there4" | "therefore" | "Therefore" => '\u{2234}',
        "sim" | "Tilde" | "thksim" | "thicksim" => '\u{223c}',
        "cong" | "TildeFullEqual" => '\u{2245}',
        "asymp" | "ap" | "TildeTilde" | "approx" | "thkap" | "thickapprox" => '\u{2248}',
        "ne" | "NotEqual" => '\u{2260}',
        "equiv" | "Congruent" => '\u{2261}',
        "le" | "leq" | "LessEqual" => '\u{2264}',
        "ge" | "geq" | "GreaterEqual" => '\u{2265}',
        "sub" | "subset" => '\u{2282}',
        "sup" | "supset" | "Superset" => '\u{2283}',
        "nsub" => '\u{2284}',
        "sube" | "subseteq" | "SubsetEqual" => '\u{2286}',
        "supe" | "supseteq" | "SupersetEqual" => '\u{2287}',
        "oplus" | "CirclePlus" => '\u{2295}',
        "otimes" | "CircleTimes" => '\u{2297}',
        "perp" | "bot" | "bottom" | "UpTee" => '\u{22a5}',
        "sdot" => '\u{22c5}',
        "loz" | "lozenge" | "diams" | "diamond" | "Diamond" => '\u{25ca}',
        "spades" | "spadesuit" => '\u{2660}',
        "clubs" | "clubsuit" => '\u{2663}',
        "hearts" | "heartsuit" => '\u{2665}',
        "diamondsuit" => '\u{2666}',
        "star" | "starf" | "bigstar" => '\u{2605}',
        "check" | "checkmark" | "cross" => '\u{2713}',
        "alefsym" | "aleph" => '\u{2135}',
        "alpha" => '\u{3b1}',
        "beta" => '\u{3b2}',
        "gamma" => '\u{3b3}',
        "delta" => '\u{3b4}',
        "epsilon" | "epsi" => '\u{3b5}',
        "pi" => '\u{3c0}',
        "sigma" => '\u{3c3}',
        "tau" => '\u{3c4}',
        "phi" => '\u{3c6}',
        "omega" => '\u{3c9}',
        "Alpha" => '\u{391}',
        "Beta" => '\u{392}',
        "Gamma" => '\u{393}',
        "Delta" => '\u{394}',
        "Pi" => '\u{3a0}',
        "Sigma" => '\u{3a3}',
        "Omega" => '\u{3a9}',
        _ => return None,
    };
    Some(c)
}

/// Python's Windows-1252 fixups for numeric references in 0x80..=0x9f.
fn invalid_charref(cp: u32) -> Option<char> {
    let c = match cp {
        0x00 => '\u{fffd}',
        0x0d => '\r',
        0x80 => '\u{20ac}',
        0x81 => '\u{81}',
        0x82 => '\u{201a}',
        0x83 => '\u{192}',
        0x84 => '\u{201e}',
        0x85 => '\u{2026}',
        0x86 => '\u{2020}',
        0x87 => '\u{2021}',
        0x88 => '\u{2c6}',
        0x89 => '\u{2030}',
        0x8a => '\u{160}',
        0x8b => '\u{2039}',
        0x8c => '\u{152}',
        0x8d => '\u{8d}',
        0x8e => '\u{17d}',
        0x8f => '\u{8f}',
        0x90 => '\u{90}',
        0x91 => '\u{2018}',
        0x92 => '\u{2019}',
        0x93 => '\u{201c}',
        0x94 => '\u{201d}',
        0x95 => '\u{2022}',
        0x96 => '\u{2013}',
        0x97 => '\u{2014}',
        0x98 => '\u{2dc}',
        0x99 => '\u{2122}',
        0x9a => '\u{161}',
        0x9b => '\u{203a}',
        0x9c => '\u{153}',
        0x9d => '\u{9d}',
        0x9e => '\u{17e}',
        0x9f => '\u{178}',
        _ => return None,
    };
    Some(c)
}

/// Is `cp` an invalid Unicode codepoint per Python's `html._invalid_codepoints`?
fn is_invalid_codepoint(cp: u32) -> bool {
    matches!(cp, 0x1..=0x8 | 0xb | 0xe..=0x1f | 0x7f | 0xfdd0..=0xfdef)
        || (cp >= 0xfffe && (cp & 0xffff == 0xfffe || cp & 0xffff == 0xffff))
}

/// Python `html.unescape(s)`.
pub fn html_unescape(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    static RE: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"&(#[0-9]+;?|#[xX][0-9a-fA-F]+;?|[^\t\n\x0c <&#;]{1,32};?)").unwrap()
    });
    RE.replace_all(s, |caps: &regex::Captures| {
        let ent = &caps[1];
        if let Some(num) = ent.strip_prefix('#') {
            let (digits, hex) = match num.strip_prefix(['x', 'X']) {
                Some(h) => (h, true),
                None => (num, false),
            };
            let digits = digits.strip_suffix(';').unwrap_or(digits);
            let cp = if hex {
                u32::from_str_radix(digits, 16).ok()
            } else {
                digits.parse::<u32>().ok()
            };
            if let Some(cp) = cp {
                if cp != 0 && cp < 0x11_0000 {
                    if let Some(c) = char::from_u32(cp) {
                        if let Some(fixed) = invalid_charref(cp) {
                            return fixed.to_string();
                        }
                        if is_invalid_codepoint(cp) {
                            return "\u{fffd}".to_string();
                        }
                        return c.to_string();
                    }
                }
            }
            return caps[0].to_string();
        }
        let name = ent.strip_suffix(';').unwrap_or(ent);
        match named_entity(name) {
            Some(c) => c.to_string(),
            None => caps[0].to_string(),
        }
    })
    .into_owned()
}

/// `str(value)` for the escape helpers (Python `str()`).
fn to_text(value: &Value) -> String {
    uzi_core::py::py_str(value)
}

/// HTML-escape text idempotently so nested renderers may call it safely.
pub fn escape_text(value: &Value) -> String {
    html_escape(&html_unescape(&to_text(value)))
}

/// Recursively escape the strings inside a JSON payload (dict keys untouched).
pub fn escape_payload(value: &Value) -> Value {
    match value {
        Value::String(_) => Value::String(escape_text(value)),
        Value::Array(items) => Value::Array(items.iter().map(escape_payload).collect()),
        Value::Object(map) => {
            let mut out = Map::with_capacity(map.len());
            for (k, v) in map {
                out.insert(k.clone(), escape_payload(v));
            }
            Value::Object(out)
        }
        other => other.clone(),
    }
}

/// Parse the scheme/netloc the way `urllib.parse.urlparse` does.
fn urlparse(raw: &str) -> Option<(String, String)> {
    // find scheme separator; Python requires scheme char set and a non-digit
    // first char after ':'.
    let bytes = raw.as_bytes();
    let mut colon = None;
    for (i, b) in bytes.iter().enumerate() {
        if *b == b':' {
            colon = Some(i);
            break;
        }
        let c = *b as char;
        let ok = if i == 0 {
            c.is_ascii_alphabetic()
        } else {
            c.is_ascii_alphanumeric() || c == '+' || c == '.' || c == '-'
        };
        if !ok {
            break;
        }
    }
    match colon {
        Some(i) => {
            let scheme = &raw[..i];
            if scheme.is_empty() {
                return None;
            }
            let rest = &raw[i + 1..];
            if rest.starts_with(|c: char| c.is_ascii_digit()) {
                return None;
            }
            let netloc = if let Some(after) = rest.strip_prefix("//") {
                let end = after
                    .find(|c| c == '/' || c == '?' || c == '#')
                    .unwrap_or(after.len());
                after[..end].to_string()
            } else {
                String::new()
            };
            Some((scheme.to_lowercase(), netloc))
        }
        None => None,
    }
}

/// Internal allow-list of `http`/`https` URLs; everything else becomes `default`.
pub fn safe_url(value: &Value, default: &str) -> String {
    let src = if uzi_core::py::truthy(value) {
        to_text(value)
    } else {
        String::new()
    };
    let raw = html_unescape(&src).trim().to_string();
    let parsed = std::panic::catch_unwind(|| urlparse(&raw)).ok().flatten();
    match parsed {
        Some((scheme, netloc))
            if (scheme == "http" || scheme == "https") && !netloc.is_empty() =>
        {
            escape_text(&Value::String(raw))
        }
        _ => default.to_string(),
    }
}

/// Idempotent, caller-supplied default.
pub fn safe_url_default(value: &Value) -> String {
    safe_url(value, "#")
}

/// Scrub a value into a DOM/CSS-safe asset id (`[^A-Za-z0-9_-]+` removed, 80 cap).
pub fn safe_asset_id(value: &Value, default: &str) -> String {
    static RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[^A-Za-z0-9_-]+").unwrap());
    let src = if uzi_core::py::truthy(value) {
        to_text(value)
    } else {
        String::new()
    };
    let cleaned: String = RE.replace_all(&src, "").into_owned();
    let capped: String = cleaned.chars().take(80).collect();
    if capped.is_empty() {
        default.to_string()
    } else {
        capped
    }
}

/// `safe_asset_id(value)` with the upstream `_placeholder` default.
pub fn safe_asset_id_default(value: &Value) -> String {
    safe_asset_id(value, "_placeholder")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // Expected strings derived from the upstream Python helpers:
    //   cd /tmp/uzi-src/skills/deep-analysis/scripts
    //   python3 -c "from lib.report.security import escape_text as e; print(repr(e('<script>alert(1)</script>')))"
    //   -> '&lt;script&gt;alert(1)&lt;/script&gt;'
    //   print(repr(e('A & B \"C\" <d>')))   -> 'A &amp; B &quot;C&quot; &lt;d&gt;'
    //   print(repr(e(\"it's\")))             -> 'it&#x27;s'
    //   print(repr(e('贵州茅台')))           -> '贵州茅台'
    #[test]
    fn escape_text_matches_upstream_v3_7_2() {
        assert_eq!(
            escape_text(&json!("<script>alert(1)</script>")),
            "&lt;script&gt;alert(1)&lt;/script&gt;"
        );
        assert_eq!(
            escape_text(&json!("A & B \"C\" <d>")),
            "A &amp; B &quot;C&quot; &lt;d&gt;"
        );
        assert_eq!(escape_text(&json!("it's")), "it&#x27;s");
        assert_eq!(escape_text(&json!("贵州茅台")), "贵州茅台");
    }

    //   print(repr(e('<img src=x onerror="alert(1)">')))
    //   -> '&lt;img src=x onerror=&quot;alert(1)&quot;&gt;'
    #[test]
    fn escape_boundary_payload_matches_upstream() {
        let payload = json!("<img src=x onerror=\"alert(1)\">");
        let out = escape_text(&payload);
        assert_eq!(out, "&lt;img src=x onerror=&quot;alert(1)&quot;&gt;");
        assert!(!out.contains("<img"));
        assert!(out.contains("&lt;img"));
    }

    //   print(repr(escape_text(escape_text('<b>&')))) -> '&lt;b&gt;&amp;'
    //   print(repr(escape_text('&amp;lt;')))          -> '&amp;lt;'
    //   print(repr(escape_text('&#x27;')))            -> '&#x27;'
    #[test]
    fn escape_text_is_idempotent() {
        let once = escape_text(&json!("<b>&"));
        assert_eq!(escape_text(&Value::String(once)), "&lt;b&gt;&amp;");
        assert_eq!(escape_text(&json!("&amp;lt;")), "&amp;lt;");
        assert_eq!(escape_text(&json!("&#x27;")), "&#x27;");
    }

    //   print(escape_payload({'msg':'<b>x</b>','n':1,'l':['<i>','&']}))
    //   -> {'msg': '&lt;b&gt;x&lt;/b&gt;', 'n': 1, 'l': ['&lt;i&gt;', '&amp;']}
    #[test]
    fn escape_payload_recurses_without_touching_keys() {
        let out = escape_payload(&json!({"msg": "<b>x</b>", "n": 1, "l": ["<i>", "&"]}));
        assert_eq!(
            out,
            json!({"msg": "&lt;b&gt;x&lt;/b&gt;", "n": 1, "l": ["&lt;i&gt;", "&amp;"]})
        );
        assert_eq!(out.as_object().unwrap().keys().next().unwrap(), "msg");
    }

    //   print(repr(safe_asset_id('../../evil" onerror=alert(1)'))) -> 'evilonerroralert1'
    //   print(repr(safe_asset_id(None)), repr(safe_asset_id('')))  -> '_placeholder' '_placeholder'
    #[test]
    fn safe_asset_id_scrubs_path_and_quotes() {
        assert_eq!(
            safe_asset_id_default(&json!("../../evil\" onerror=alert(1)")),
            "evilonerroralert1"
        );
        assert_eq!(safe_asset_id_default(&json!(null)), "_placeholder");
        assert_eq!(safe_asset_id_default(&json!("")), "_placeholder");
    }

    //   print(repr(safe_url('javascript:alert(1)')))                  -> '#'
    //   print(repr(safe_url('https://xueqiu.com/S/00700?q=1&b=2')))   -> 'https://xueqiu.com/S/00700?q=1&amp;b=2'
    //   print(repr(safe_url('HTTP://Example.com/a')))                 -> 'HTTP://Example.com/a'
    #[test]
    fn safe_url_allow_lists_http() {
        assert_eq!(safe_url_default(&json!("javascript:alert(1)")), "#");
        assert_eq!(
            safe_url_default(&json!("https://xueqiu.com/S/00700?q=1&b=2")),
            "https://xueqiu.com/S/00700?q=1&amp;b=2"
        );
        assert_eq!(
            safe_url_default(&json!("HTTP://Example.com/a")),
            "HTTP://Example.com/a"
        );
        assert_eq!(safe_url_default(&json!("not a url")), "#");
    }
}
