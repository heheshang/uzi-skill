//! Small feature-dict helpers shared by the modeling and scoring crates.

use serde_json::{Map, Value};

/// `stock_features.sanitize_features` — drop top-level `null` values.
///
/// Institutional workflows call `dict.get(key, conservative_default)`; a present
/// key whose value is `None` bypasses that default and can raise numeric
/// `TypeError`. Keep this at the modeling boundary only: the investor evaluator
/// deliberately uses `None` to mean "evidence unavailable, skip the rule".
pub fn sanitize_features(features: &Value) -> Value {
    match features {
        Value::Object(map) => Value::Object(
            map.iter()
                .filter(|(_, v)| !v.is_null())
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect::<Map<String, Value>>(),
        ),
        Value::Null => Value::Object(Map::new()),
        other => other.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn drops_only_top_level_nulls_and_preserves_order() {
        let input = json!({"a": 1, "b": null, "c": {"d": null}, "e": 0});
        let out = sanitize_features(&input);
        let keys: Vec<&String> = out.as_object().unwrap().keys().collect();
        assert_eq!(keys, vec!["a", "c", "e"]);
        assert_eq!(out["c"]["d"], Value::Null);
        assert_eq!(out["e"], json!(0));
    }
}
