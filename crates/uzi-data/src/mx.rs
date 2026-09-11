//! Port of `lib/mx_api.py` — thin client for the 东方财富妙想 Skills Hub API
//! (`mkapi2.dfcfs.com/finskillshub`), authenticated with `MX_APIKEY`.
//!
//! Without the key both endpoints short-circuit to the upstream failure shapes
//! (`{"error": "MX_APIKEY not set"}`, empty entity/tag lists), so callers can
//! gate on [`MXClient::available`] exactly as upstream does.

use serde_json::{json, Map, Value};
use std::sync::LazyLock;

use uzi_core::cache::cached;

pub const BASE: &str = "https://mkapi2.dfcfs.com/finskillshub/api/claw";
pub const QUERY_URL: &str = "https://mkapi2.dfcfs.com/finskillshub/api/claw/query";
pub const NEWS_URL: &str = "https://mkapi2.dfcfs.com/finskillshub/api/claw/news-search";

pub const MX_TTL: u64 = 30 * 60;

/// `_post(url, body, api_key, timeout, attempts)` — returns parsed JSON or
/// `{"error": ...}`.
pub fn post(url: &str, body: &Value, api_key: &str, timeout: u64, attempts: usize) -> Value {
    let body_text = serde_json::to_string(body).unwrap_or_else(|_| "{}".to_string());
    let mut last_err = String::from("unknown");
    for i in 0..attempts.max(1) {
        let result = ureq::Agent::new_with_config(
            ureq::Agent::config_builder()
                .timeout_global(Some(std::time::Duration::from_secs(timeout.max(1))))
                .http_status_as_error(false)
                .build(),
        )
        .post(url)
        .header("Content-Type", "application/json")
        .header("apikey", api_key)
        .send(&body_text);
        match result {
            Ok(mut resp) => {
                let status = resp.status().as_u16();
                let text = resp
                    .body_mut()
                    .read_to_string()
                    .unwrap_or_default();
                if status != 200 {
                    last_err = format!("HTTP {status}: {}", first200(&text));
                    if status == 401 || status == 403 {
                        break;
                    }
                    std::thread::sleep(std::time::Duration::from_secs(i as u64 + 1));
                    continue;
                }
                return serde_json::from_str(&text).unwrap_or_else(|_| json!({"error": "invalid JSON"}));
            }
            Err(e) => {
                last_err = format!("ureq: {}", first200(&e.to_string()));
                std::thread::sleep(std::time::Duration::from_secs(i as u64 + 1));
            }
        }
    }
    json!({"error": last_err})
}

/// `MXClient`.
pub struct MXClient {
    pub api_key: String,
    pub available: bool,
}

impl Default for MXClient {
    fn default() -> Self {
        Self::new(None)
    }
}

impl MXClient {
    pub fn new(api_key: Option<String>) -> Self {
        let api_key = api_key
            .or_else(|| std::env::var("MX_APIKEY").ok())
            .unwrap_or_default();
        let available = !api_key.is_empty();
        MXClient { api_key, available }
    }

    /// `query(tool_query)` — cached 30 min.
    pub fn query(&self, tool_query: &str) -> Value {
        if !self.available {
            return json!({"error": "MX_APIKEY not set"});
        }
        let key = format!("mx_query__{}", first80(tool_query));
        let body = json!({"toolQuery": tool_query});
        let api_key = self.api_key.clone();
        cached::<_, anyhow::Error>("_global", &key, MX_TTL, move || {
            Ok(post(QUERY_URL, &body, &api_key, 30, 2))
        })
        .unwrap_or_else(|_| json!({"error": "cache failure"}))
    }

    /// `news_search(query)` — cached 30 min.
    pub fn news_search(&self, query: &str) -> Value {
        if !self.available {
            return json!({"error": "MX_APIKEY not set"});
        }
        let key = format!("mx_news__{}", first80(query));
        let body = json!({"query": query});
        let api_key = self.api_key.clone();
        cached::<_, anyhow::Error>("_global", &key, MX_TTL, move || {
            Ok(post(NEWS_URL, &body, &api_key, 30, 2))
        })
        .unwrap_or_else(|_| json!({"error": "cache failure"}))
    }

    /// `resolve_entity(name)`.
    pub fn resolve_entity(&self, name: &str) -> Vec<Value> {
        if !self.available {
            return Vec::new();
        }
        let result = self.query(&format!("{name} 股票代码 所属行业"));
        extract_entity_tags(&result)
    }

    /// `fetch_snapshot(code_or_name)`.
    pub fn fetch_snapshot(&self, code_or_name: &str) -> Value {
        if !self.available {
            return json!({});
        }
        let result = self.query(&format!(
            "{code_or_name} 最新价 总市值 PE PB 所属行业 主营业务"
        ));
        extract_first_table_row(&result)
    }
}

/// Module-level `resolve_entity` using the env key (used by `data_sources`).
pub fn resolve_entity(name: &str) -> Option<Vec<Value>> {
    let client = MXClient::default();
    if !client.available {
        return None;
    }
    let hits = client.resolve_entity(name);
    if hits.is_empty() {
        None
    } else {
        Some(hits)
    }
}

/// `_extract_entity_tags(result)`.
pub fn extract_entity_tags(result: &Value) -> Vec<Value> {
    let Some(obj) = result.as_object() else {
        return Vec::new();
    };
    if obj.contains_key("error") {
        return Vec::new();
    }
    if let Some(status) = obj.get("status") {
        if status.as_i64() != Some(0) && !status.is_null() {
            return Vec::new();
        }
    }
    let sr = result
        .get("data")
        .and_then(|d| d.get("data"))
        .and_then(|d| d.get("searchDataResultDTO"))
        .cloned()
        .unwrap_or_else(|| json!({}));

    let mut out: Vec<Value> = Vec::new();
    let mut seen: Vec<String> = Vec::new();

    // Source 1: entityTagDTOList
    for t in sr
        .get("entityTagDTOList")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default()
    {
        let Some(t) = t.as_object() else { continue };
        let full_name = t
            .get("fullName")
            .or_else(|| t.get("shortName"))
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let code = t
            .get("secuCode")
            .or_else(|| t.get("code"))
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let etype = t
            .get("entityTypeName")
            .or_else(|| t.get("entityType"))
            .and_then(|v| v.as_str())
            .unwrap_or("");
        if !full_name.is_empty() && !code.is_empty() && !seen.iter().any(|s| s == code) {
            seen.push(code.to_string());
            out.push(json!({"fullName": full_name, "secuCode": code, "entityType": etype}));
        }
    }

    // Source 2: dataTableDTOList[].code / .entityName
    static PAREN: LazyLock<regex::Regex> = LazyLock::new(|| {
        regex::Regex::new(r"^(.+?)\s*[（(][^)）]+[)）]\s*$").unwrap()
    });
    for dto in sr
        .get("dataTableDTOList")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default()
    {
        let Some(dto) = dto.as_object() else { continue };
        let code = dto.get("code").and_then(|v| v.as_str()).unwrap_or("").trim();
        let entity_name = dto
            .get("entityName")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim();
        if code.is_empty() || seen.iter().any(|s| s == code) {
            continue;
        }
        let clean_name = PAREN
            .captures(entity_name)
            .and_then(|c| c.get(1).map(|m| m.as_str().trim().to_string()))
            .unwrap_or_else(|| entity_name.to_string());
        seen.push(code.to_string());
        out.push(json!({
            "fullName": clean_name,
            "secuCode": code,
            // upstream: `dto.get("dataType", "") or "股票"`
            "entityType": dto.get("dataType").and_then(|v| v.as_str()).filter(|s| !s.is_empty()).unwrap_or("股票"),
        }));
    }
    out
}

/// `_extract_first_table_row(result)`.
pub fn extract_first_table_row(result: &Value) -> Value {
    let Some(obj) = result.as_object() else {
        return json!({});
    };
    if obj.contains_key("error") {
        return json!({});
    }
    let dto = result
        .get("data")
        .and_then(|d| d.get("data"))
        .and_then(|d| d.get("searchDataResultDTO"))
        .and_then(|s| s.get("dataTableDTOList"))
        .and_then(|l| l.as_array())
        .and_then(|l| l.first())
        .cloned()
        .unwrap_or(Value::Null);
    let Some(dto) = dto.as_object() else {
        return json!({});
    };
    let table = dto.get("table").and_then(|t| t.as_object()).cloned();
    let Some(table) = table else {
        return json!({});
    };
    let name_map: Map<String, Value> = match dto.get("nameMap") {
        Some(Value::Object(m)) => m.clone(),
        Some(Value::Array(a)) => a
            .iter()
            .enumerate()
            .map(|(i, v)| (i.to_string(), v.clone()))
            .collect(),
        _ => Map::new(),
    };

    let mut out = Map::new();
    out.insert(
        "_mx_entity".into(),
        dto.get("entityName").cloned().unwrap_or(json!("")),
    );
    for (key, values) in &table {
        if key == "headName" {
            continue;
        }
        let label = name_map
            .get(key)
            .or_else(|| name_map.get(key.as_str()))
            .cloned()
            .unwrap_or_else(|| json!(key));
        let label = label.as_str().map(|s| s.to_string()).unwrap_or_else(|| label.to_string());
        let value = match values.as_array() {
            Some(arr) if !arr.is_empty() => arr[arr.len() - 1].clone(),
            _ => values.clone(),
        };
        out.insert(label, value);
    }
    Value::Object(out)
}

/// `_extract_mx_text(result)` — the helper `score_fns` uses for autofill.
pub fn extract_mx_text(result: &Value) -> String {
    let Some(obj) = result.as_object() else {
        return String::new();
    };
    if obj.contains_key("error") {
        return String::new();
    }
    let inner = result
        .get("data")
        .and_then(|d| d.get("data"))
        .cloned()
        .unwrap_or_else(|| json!({}));
    let dto_list = inner
        .get("searchDataResultDTO")
        .and_then(|s| s.get("dataTableDTOList"))
        .and_then(|l| l.as_array())
        .cloned()
        .unwrap_or_default();
    if dto_list.is_empty() {
        let entity = inner
            .get("entityName")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        return first_n(entity, 200);
    }
    let mut parts: Vec<String> = Vec::new();
    for dto in dto_list.iter().take(2) {
        let Some(dto) = dto.as_object() else { continue };
        let title = dto
            .get("title")
            .or_else(|| dto.get("entityName"))
            .and_then(|v| v.as_str())
            .unwrap_or("");
        if !title.is_empty() {
            parts.push(first_n(title, 120));
        }
    }
    if parts.is_empty() {
        String::new()
    } else {
        first_n(&parts.join("；"), 300)
    }
}

fn first80(s: &str) -> String {
    s.chars().take(80).collect()
}

fn first_n(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

fn first200(s: &str) -> String {
    s.chars().take(200).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unavailable_client_returns_upstream_failure_shapes() {
        std::env::remove_var("MX_APIKEY");
        let c = MXClient::new(Some(String::new()));
        assert!(!c.available);
        assert_eq!(c.query("x")["error"], json!("MX_APIKEY not set"));
        assert_eq!(c.news_search("x")["error"], json!("MX_APIKEY not set"));
        assert_eq!(c.resolve_entity("北部湾港"), Vec::<Value>::new());
        assert_eq!(c.fetch_snapshot("000582"), json!({}));
    }

    #[test]
    fn entity_tags_parse_cleans_parenthetical_name() {
        let result = json!({
            "status": 0,
            "data": {"data": {"searchDataResultDTO": {
                "dataTableDTOList": [
                    {"code": "000582.SZ", "entityName": "北部湾港(000582.SZ)", "dataType": ""}
                ]
            }}}
        });
        let tags = extract_entity_tags(&result);
        assert_eq!(tags.len(), 1);
        assert_eq!(tags[0]["fullName"], json!("北部湾港"));
        assert_eq!(tags[0]["secuCode"], json!("000582.SZ"));
        assert_eq!(tags[0]["entityType"], json!("股票"));
    }

    #[test]
    fn first_table_row_takes_last_value() {
        let result = json!({
            "data": {"data": {"searchDataResultDTO": {"dataTableDTOList": [
                {"entityName": "贵州茅台(600519.SH)",
                 "table": {"headName": ["2024", "2025"], "1000000000001": ["1.0", "2.0"]},
                 "nameMap": {"1000000000001": "最新价"}}
            ]}}}
        });
        let row = extract_first_table_row(&result);
        assert_eq!(row["最新价"], json!("2.0"));
        assert_eq!(row["_mx_entity"], json!("贵州茅台(600519.SH)"));
    }

    #[test]
    fn mx_text_falls_back_to_entity_name() {
        let result = json!({"data": {"data": {"entityName": "贵州茅台"}}});
        assert_eq!(extract_mx_text(&result), "贵州茅台");
        assert_eq!(extract_mx_text(&json!({"error": "x"})), "");
    }
}
