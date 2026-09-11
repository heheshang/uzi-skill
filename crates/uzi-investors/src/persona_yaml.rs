//! Port of `lib/personas.py` — the YAML persona loader and the prefix-stable
//! system message builder.
//!
//! `PERSONAS_DIR` is derived from `__file__` upstream; the Rust port resolves the
//! same directory from `UZI_PERSONAS_DIR` (falling back to
//! `<cwd>/skills/deep-analysis/personas`), and every loader also takes an
//! explicit directory.
//!
//! `build_system_message` upstream calls `lib.i18n.language_instruction`. That
//! module has no crate of its own in this workspace, so the two instruction
//! strings are inlined below, verbatim from `lib/i18n.py` (`get_language` reads
//! `UZI_LANG`, default `zh`).

use serde_json::{Map, Value};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// `personas.FRAMEWORK_INSTRUCTIONS_ZH`.
pub const FRAMEWORK_INSTRUCTIONS_ZH: &str = include_str!("data/framework_zh.txt");

/// `personas.Persona`.
#[derive(Debug, Clone, PartialEq)]
pub struct Persona {
    pub id: String,
    pub name: String,
    pub school: String,
    pub group: String,
    pub philosophy: String,
    pub key_metrics: Vec<String>,
    pub avoids: Vec<String>,
    pub a_share_view: String,
    pub voice: String,
    pub famous_positions: Vec<String>,
    pub is_flagship: bool,
    pub raw: Value,
}

impl Persona {
    /// `Persona.to_prompt_block` — compressed persona block for the LLM prompt.
    pub fn to_prompt_block(&self) -> String {
        let mut lines: Vec<String> = vec![
            format!("# PERSONA · {} ({})", self.name, self.id),
            format!("School: {} · Group: {}", self.school, self.group),
            String::new(),
        ];
        if !self.philosophy.is_empty() {
            lines.push(format!(
                "## Philosophy\n{}",
                take_chars(self.philosophy.trim(), 400)
            ));
        }
        if !self.key_metrics.is_empty() {
            let items: Vec<String> = self
                .key_metrics
                .iter()
                .take(8)
                .map(|m| format!("- {}", m))
                .collect();
            lines.push(format!("\n## Key Metrics / Signals\n{}", items.join("\n")));
        }
        if !self.avoids.is_empty() {
            let items: Vec<String> = self
                .avoids
                .iter()
                .take(6)
                .map(|a| format!("- {}", a))
                .collect();
            lines.push(format!("\n## Avoids\n{}", items.join("\n")));
        }
        if !self.a_share_view.is_empty() {
            lines.push(format!(
                "\n## A-Share View\n{}",
                take_chars(self.a_share_view.trim(), 300)
            ));
        }
        if !self.voice.is_empty() {
            lines.push(format!("\n## Voice / Tone\n{}", take_chars(self.voice.trim(), 200)));
        }
        if !self.famous_positions.is_empty() {
            let items: Vec<String> = self
                .famous_positions
                .iter()
                .take(5)
                .map(|p| format!("- {}", p))
                .collect();
            lines.push(format!("\n## Famous Positions\n{}", items.join("\n")));
        }
        lines.join("\n")
    }
}

fn take_chars(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

/// The directory upstream calls `PERSONAS_DIR`.
pub fn personas_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("UZI_PERSONAS_DIR") {
        if !dir.is_empty() {
            return PathBuf::from(dir);
        }
    }
    PathBuf::from("skills/deep-analysis/personas")
}

/// `personas._parse_minimal_yaml` — dependency-free parser for the simplified
/// YAML used by `personas/`.
pub fn parse_minimal_yaml(text: &str) -> Map<String, Value> {
    let lines: Vec<&str> = text.split('\n').collect();
    let mut result = Map::new();
    let mut i = 0usize;

    while i < lines.len() {
        let line = lines[i];
        let stripped = line.trim();
        if stripped.is_empty() || stripped.starts_with('#') {
            i += 1;
            continue;
        }
        // top-level key — must be flush left
        if !line.starts_with(' ') && line.contains(':') {
            let (key, value) = line.split_once(':').map(|(k, v)| (k.trim(), v.trim())).unwrap();

            if value == "|" {
                // multi-line string (all following indented lines)
                let mut block: Vec<String> = Vec::new();
                i += 1;
                while i < lines.len() {
                    let nxt = lines[i];
                    if nxt.trim().is_empty()
                        && (i + 1 >= lines.len() || !lines[i + 1].starts_with(' '))
                    {
                        break;
                    }
                    if nxt.starts_with("  ") {
                        block.push(nxt[2..].to_string());
                        i += 1;
                    } else if nxt.trim().is_empty() {
                        block.push(String::new());
                        i += 1;
                    } else {
                        break;
                    }
                }
                result.insert(key.to_string(), Value::String(block.join("\n").trim_end().to_string()));
                continue;
            }

            if value.is_empty() {
                // either a list or a nested dict
                let mut items: Vec<String> = Vec::new();
                let mut child = Map::new();
                i += 1;
                while i < lines.len() {
                    let nxt = lines[i];
                    if nxt.starts_with("  - ") {
                        items.push(nxt[4..].trim().to_string());
                        i += 1;
                    } else if nxt.starts_with("  ") && nxt.contains(':') && !nxt.starts_with("    ") {
                        let (sub_key, sub_val) =
                            nxt.trim().split_once(':').map(|(k, v)| (k.trim(), v.trim())).unwrap();
                        child.insert(sub_key.to_string(), Value::String(sub_val.to_string()));
                        i += 1;
                    } else if nxt.trim().is_empty() {
                        i += 1;
                    } else {
                        break;
                    }
                }
                if !items.is_empty() {
                    result.insert(
                        key.to_string(),
                        Value::Array(items.into_iter().map(Value::String).collect()),
                    );
                } else if !child.is_empty() {
                    result.insert(key.to_string(), Value::Object(child));
                } else {
                    result.insert(key.to_string(), Value::String(String::new()));
                }
                continue;
            }

            // simple scalar · strip quotes
            let scalar = value.trim_matches(|c| c == '"' || c == '\'');
            result.insert(key.to_string(), Value::String(scalar.to_string()));
            i += 1;
            continue;
        }
        i += 1;
    }
    result
}

fn str_field(d: &Map<String, Value>, key: &str, default: &str) -> String {
    match d.get(key) {
        Some(Value::String(s)) => s.clone(),
        _ => default.to_string(),
    }
}

fn list_field(d: &Map<String, Value>, key: &str) -> Vec<String> {
    match d.get(key) {
        Some(Value::Array(a)) => a.iter().filter_map(Value::as_str).map(str::to_string).collect(),
        _ => Vec::new(),
    }
}

/// `personas.load_persona` from an explicit directory.
pub fn load_persona_from(dir: &Path, investor_id: &str) -> Option<Persona> {
    let path = dir.join(format!("{}.yaml", investor_id));
    if !path.exists() {
        return None;
    }
    let text = std::fs::read_to_string(&path).ok()?;
    let d = parse_minimal_yaml(&text);

    let mut is_stub = false;
    if let Some(Value::Object(meta)) = d.get("_meta").filter(|m| !matches!(m, Value::Null)) {
        is_stub = meta.get("status").and_then(Value::as_str) == Some("auto_generated_stub");
    }

    Some(Persona {
        id: str_field(&d, "id", investor_id),
        name: str_field(&d, "name", ""),
        school: str_field(&d, "school", ""),
        group: str_field(&d, "group", ""),
        philosophy: str_field(&d, "philosophy", ""),
        key_metrics: list_field(&d, "key_metrics"),
        avoids: list_field(&d, "avoids"),
        a_share_view: str_field(&d, "a_share_view", ""),
        voice: str_field(&d, "voice", ""),
        famous_positions: list_field(&d, "famous_positions"),
        is_flagship: !is_stub,
        raw: Value::Object(d),
    })
}

/// `personas.load_persona` using [`personas_dir`].
pub fn load_persona(investor_id: &str) -> Option<Persona> {
    load_persona_from(&personas_dir(), investor_id)
}

/// `personas.load_all_personas` — sorted by id (upstream iterates `glob`, whose
/// order is filesystem-dependent).
pub fn load_all_personas_from(dir: &Path) -> BTreeMap<String, Persona> {
    let mut result = BTreeMap::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return result;
    };
    let mut stems: Vec<String> = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("yaml") {
            continue;
        }
        if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
            stems.push(stem.to_string());
        }
    }
    stems.sort();
    for stem in stems {
        if let Some(p) = load_persona_from(dir, &stem) {
            if !p.id.is_empty() {
                result.insert(p.id.clone(), p);
            }
        }
    }
    result
}

/// `personas.load_all_personas` using [`personas_dir`].
pub fn load_all_personas() -> BTreeMap<String, Persona> {
    load_all_personas_from(&personas_dir())
}

/// `lib.i18n.get_language` (inlined; see module docs).
pub fn get_language() -> String {
    let lang = std::env::var("UZI_LANG").unwrap_or_default().to_lowercase();
    if lang == "zh" || lang == "en" {
        lang
    } else {
        "zh".to_string()
    }
}

/// `lib.i18n.language_instruction` (inlined; see module docs).
pub fn language_instruction(lang: &str) -> String {
    let lang = if lang.is_empty() { get_language() } else { lang.to_string() };
    if lang == "en" {
        return "OUTPUT LANGUAGE: All reasoning, headline, verdict and commentary must be \
written in English. Keep persona-specific terms (e.g., '赵老哥', '段永平') \
in their original Chinese as they are proper nouns."
            .to_string();
    }
    "输出语言：所有 reasoning / headline / verdict / commentary 必须用中文。\
外国投资者名（Buffett / Lynch / Wood）可混用中英文，以上下文自然为准。\
金融术语首选中文（净利率 / 毛利率 / 市盈率 / 护城河），技术/量化术语可保留英文（DCF / PEG / EPS）。"
        .to_string()
}

/// `personas.build_system_message` — prefix-stable system prompt shared by every
/// persona (prompt-cache friendly).
pub fn build_system_message(snapshot_json: &str, lang: &str, _include_flagship_tips: bool) -> String {
    [
        FRAMEWORK_INSTRUCTIONS_ZH.to_string(),
        String::new(),
        language_instruction(lang),
        String::new(),
        "# MARKET SNAPSHOT（全体 persona 共享，请勿重复提取）".to_string(),
        snapshot_json.to_string(),
    ]
    .join("\n")
}

/// `personas.build_persona_user_message`.
pub fn build_persona_user_message(persona: &Persona, ticker: &str, _task: &str) -> String {
    format!(
        "{}\n\n---\n\n# TASK\n现在请你以 {}（{}）的身份分析股票 {}，\
严格按照上面的 philosophy / key_metrics / voice。\
输出 JSON 格式的 PersonaVote（见 system message 末尾的格式约束）。",
        persona.to_prompt_block(),
        persona.name,
        persona.id,
        ticker
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "\
# comment
id: buffett
name: 巴菲特
school: 经典价值
group: A
_meta:
  status: auto_generated_stub
philosophy: |
  第一行
  第二行

  第四行
key_metrics:
  - ROE > 15%
  - 护城河
avoids:
  - 半导体
a_share_view: 看白酒
famous_positions:
  - 茅台
";

    #[test]
    fn minimal_yaml_parses_the_documented_shapes() {
        let d = parse_minimal_yaml(SAMPLE);
        assert_eq!(d["id"], "buffett");
        assert_eq!(d["name"], "巴菲特");
        assert_eq!(d["philosophy"], "第一行\n第二行\n\n第四行");
        assert_eq!(d["key_metrics"], serde_json::json!(["ROE > 15%", "护城河"]));
        assert_eq!(d["avoids"], serde_json::json!(["半导体"]));
        assert_eq!(d["_meta"]["status"], "auto_generated_stub");
        // keys keep source order
        let keys: Vec<&str> = d.keys().map(String::as_str).collect();
        assert_eq!(keys[0], "id");
        assert!(keys.contains(&"famous_positions"));
    }

    #[test]
    fn prompt_block_compresses_like_upstream() {
        let d = parse_minimal_yaml(SAMPLE);
        let p = Persona {
            id: str_field(&d, "id", "buffett"),
            name: str_field(&d, "name", ""),
            school: str_field(&d, "school", ""),
            group: str_field(&d, "group", ""),
            philosophy: str_field(&d, "philosophy", ""),
            key_metrics: list_field(&d, "key_metrics"),
            avoids: list_field(&d, "avoids"),
            a_share_view: str_field(&d, "a_share_view", ""),
            voice: str_field(&d, "voice", ""),
            famous_positions: list_field(&d, "famous_positions"),
            is_flagship: false,
            raw: Value::Object(d),
        };
        let block = p.to_prompt_block();
        assert!(block.starts_with("# PERSONA · 巴菲特 (buffett)\nSchool: 经典价值 · Group: A\n\n"));
        assert!(block.contains("## Philosophy\n第一行\n第二行\n\n第四行"));
        assert!(block.contains("\n## Key Metrics / Signals\n- ROE > 15%\n- 护城河"));
        assert!(block.contains("\n## Avoids\n- 半导体"));
        assert!(block.contains("\n## Famous Positions\n- 茅台"));
        assert!(!p.is_flagship);
    }

    #[test]
    fn system_message_matches_upstream_layout() {
        let msg = build_system_message("{\"a\": 1}", "zh", true);
        assert!(msg.starts_with(FRAMEWORK_INSTRUCTIONS_ZH));
        assert!(msg.ends_with("# MARKET SNAPSHOT（全体 persona 共享，请勿重复提取）\n{\"a\": 1}"));
        assert!(language_instruction("en").starts_with("OUTPUT LANGUAGE:"));
        assert!(language_instruction("zh").starts_with("输出语言："));
    }

    #[test]
    fn loads_real_persona_directory_when_present() {
        // The upstream checkout ships personas/ next to the scripts.
        let dir = Path::new("/tmp/uzi-src/skills/deep-analysis/personas");
        if !dir.exists() {
            return;
        }
        let all = load_all_personas_from(dir);
        assert!(all.len() >= 50, "loaded {} personas", all.len());
        let b = &all["buffett"];
        assert_eq!(b.name, "沃伦·巴菲特");
        assert!(b.is_flagship);
        assert!(b.philosophy.len() > 100);
        assert!(!b.key_metrics.is_empty());
        // bj_cj carries `_meta.status: auto_generated_stub`
        assert!(!all["bj_cj"].is_flagship);
        assert!(all["bj_cj"].raw["_meta"]["status"]
            .as_str()
            .is_some_and(|s| s == "auto_generated_stub"));
    }
}
