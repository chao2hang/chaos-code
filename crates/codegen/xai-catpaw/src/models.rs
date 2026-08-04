//! Seed model catalog and live-list merge.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ModelInfo {
    pub id: String,
    pub user_model_type_code: i32,
    pub label: String,
    pub upstream_name: String,
    #[serde(default)]
    pub support_agent: bool,
}

#[derive(Debug, Clone, Default)]
pub struct ModelMap {
    pub by_id: HashMap<String, ModelInfo>,
}

impl ModelMap {
    pub fn seeded() -> Self {
        let models = [
            ("longcat-flash", 22, "LongCat Flash", "longcat-flash", false),
            ("LongCat-2.0", 77, "LongCat 2.0", "LongCat-2.0", false),
            ("glm-5v-turbo", 60, "GLM 5V Turbo", "glm-5v-turbo", false),
            ("glm-5.2", 75, "GLM 5.2", "glm-5.2", true),
            ("glm-5.1", 59, "GLM 5.1", "glm-5.1", false),
            ("glm-5", 46, "GLM 5", "glm-5", false),
            ("kimi-k2.6", 62, "Kimi K2.6", "kimi-k2.6", false),
            ("kimi-k2.5", 41, "Kimi K2.5", "kimi-k2.5", false),
            ("MiniMax-M2.7", 56, "MiniMax M2.7", "MiniMax-M2.7", false),
            ("MiniMax-M2.5", 48, "MiniMax M2.5", "MiniMax-M2.5", false),
            (
                "deepseek-v3.2",
                9,
                "DeepSeek V3.2",
                "gpt-4o-2024-05-13",
                true,
            ),
        ];
        let mut by_id = HashMap::with_capacity(models.len());
        for (id, code, label, upstream_name, support_agent) in models {
            by_id.insert(
                id.to_string(),
                ModelInfo {
                    id: id.to_string(),
                    user_model_type_code: code,
                    label: label.to_string(),
                    upstream_name: upstream_name.to_string(),
                    support_agent,
                },
            );
        }
        Self { by_id }
    }

    pub fn merge_from_payload(&mut self, payload: &Value) {
        let list = payload
            .as_array()
            .or_else(|| payload.pointer("/data/list").and_then(Value::as_array))
            .or_else(|| payload.pointer("/data").and_then(Value::as_array));
        let Some(list) = list else { return };
        for entry in list {
            let Some(code) = entry
                .get("modelType")
                .or_else(|| entry.get("modelTypeCode"))
                .and_then(Value::as_i64)
                .and_then(|value| i32::try_from(value).ok())
                .filter(|value| *value >= 0)
            else {
                continue;
            };
            let Some(name) = entry
                .get("modelTypeName")
                .or_else(|| entry.get("modelName"))
                .and_then(Value::as_str)
                .filter(|name| !name.is_empty())
            else {
                continue;
            };
            let existing_id = self
                .by_id
                .values()
                .find(|model| model.user_model_type_code == code)
                .map(|model| model.id.clone());
            let id = existing_id.unwrap_or_else(|| name.to_string());
            let support_agent = entry
                .get("supportAgent")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let model = self.by_id.entry(id.clone()).or_insert_with(|| ModelInfo {
                id,
                user_model_type_code: code,
                label: name.to_string(),
                upstream_name: name.to_string(),
                support_agent,
            });
            model.label = name.to_string();
            model.upstream_name = name.to_string();
            model.support_agent = support_agent;
        }
    }

    pub fn get(&self, id: &str) -> Option<&ModelInfo> {
        self.by_id.get(id)
    }

    pub fn list(&self) -> Vec<&ModelInfo> {
        let mut models: Vec<_> = self.by_id.values().collect();
        models.sort_by(|left, right| left.id.cmp(&right.id));
        models
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merge_updates_by_code_without_changing_public_id() {
        let mut models = ModelMap::seeded();
        models.merge_from_payload(&serde_json::json!([
            {"modelType": 9, "modelTypeName": "deepseek-live", "supportAgent": true},
            {"modelType": 1001, "modelTypeName": "future", "supportAgent": true},
            {"modelType": -1, "modelTypeName": "invalid", "supportAgent": true}
        ]));
        assert_eq!(
            models.get("deepseek-v3.2").unwrap().upstream_name,
            "deepseek-live"
        );
        assert!(models.get("deepseek-v3.2").unwrap().support_agent);
        assert!(models.get("future").is_some());
        assert!(models.get("invalid").is_none());
    }
}
