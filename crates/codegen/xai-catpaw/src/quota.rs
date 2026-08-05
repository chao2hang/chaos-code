//! CatPaw model quota normalization.

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct QuotaInfo {
    pub raw: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub used: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub remaining: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage_percentage: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
}

impl QuotaInfo {
    pub fn from_value(value: Value) -> Self {
        let payload = value.get("data").cloned().unwrap_or(value);
        let (used, limit, remaining, model) = extract_canonical_fields(&payload);
        Self {
            usage_percentage: extract_top_percentage(&payload),
            raw: payload,
            used,
            limit,
            remaining,
            model,
        }
    }

    pub fn summary(&self) -> String {
        let model = self.model.as_deref().map(|name| format!("{name}: ")).unwrap_or_default();
        if let Some(percentage) = self.usage_percentage {
            let remaining = (100.0 - percentage.clamp(0.0, 100.0)).max(0.0);
            return format!("{model}已用 {percentage:.1}%，剩余 {remaining:.1}%");
        }
        match (self.used, self.limit, self.remaining) {
            (Some(used), Some(limit), _) => {
                format!("{model}已用 {used} / {limit}，剩余 {}", (limit - used).max(0))
            }
            (Some(used), None, Some(remaining)) => {
                format!("{model}已用 {used}，剩余 {remaining}")
            }
            (Some(used), None, None) => format!("{model}已用 {used}"),
            _ => format!("{model}上游已返回额度信息，但字段格式暂无法汇总"),
        }
    }
}

fn extract_top_percentage(value: &Value) -> Option<f64> {
    if let Some(entries) = value.as_array() {
        return entries
            .iter()
            .filter_map(|entry| entry.get("usagePercentage").and_then(Value::as_f64))
            .reduce(f64::max);
    }
    value.get("usagePercentage").and_then(Value::as_f64)
}

fn extract_canonical_fields(
    value: &Value,
) -> (Option<i64>, Option<i64>, Option<i64>, Option<String>) {
    if let Some(entries) = value.as_array() {
        let best = entries.iter().max_by(|left, right| {
            let left = left.get("usagePercentage").and_then(Value::as_f64).unwrap_or(0.0);
            let right = right.get("usagePercentage").and_then(Value::as_f64).unwrap_or(0.0);
            left.total_cmp(&right)
        });
        return best.map(extract_flat_fields).unwrap_or_default();
    }

    if let Some(object) = value.as_object() {
        if object.contains_key("modelRequestTotalCount") || object.contains_key("usageCount") {
            return extract_flat_fields(value);
        }
        for (model, entry) in object {
            if entry.get("modelRequestTotalCount").is_some() || entry.get("usageCount").is_some() {
                let (used, limit, remaining, _) = extract_flat_fields(entry);
                return (used, limit, remaining, Some(model.clone()));
            }
        }
    }
    (None, None, None, None)
}

fn extract_flat_fields(value: &Value) -> (Option<i64>, Option<i64>, Option<i64>, Option<String>) {
    let used = value
        .get("usageCount")
        .or_else(|| value.get("modelRequestTotalCount"))
        .and_then(Value::as_i64);
    let limit = value.get("modelRequestLimitCount").and_then(Value::as_i64);
    let remaining = value
        .get("remaining")
        .or_else(|| value.get("remainingCount"))
        .and_then(Value::as_i64)
        .or_else(|| match (used, limit) {
            (Some(used), Some(limit)) => Some((limit - used).max(0)),
            _ => None,
        });
    let model = value
        .get("modelName")
        .or_else(|| value.get("model"))
        .and_then(Value::as_str)
        .map(str::to_owned);
    (used, limit, remaining, model)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn array_uses_the_most_constrained_model() {
        let quota = QuotaInfo::from_value(serde_json::json!([
            {"modelName": "a", "usageCount": 3, "usagePercentage": 30.0},
            {"modelName": "b", "usageCount": 9, "usagePercentage": 90.0}
        ]));
        assert_eq!(quota.model.as_deref(), Some("b"));
        assert_eq!(quota.used, Some(9));
        assert_eq!(quota.usage_percentage, Some(90.0));
        assert!(quota.summary().contains("剩余 10.0%"));
    }

    #[test]
    fn flat_shape_derives_remaining() {
        let quota = QuotaInfo::from_value(serde_json::json!({
            "modelRequestTotalCount": 7,
            "modelRequestLimitCount": 20
        }));
        assert_eq!(quota.remaining, Some(13));
        assert_eq!(quota.summary(), "已用 7 / 20，剩余 13");
    }
}
