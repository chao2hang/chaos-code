//! 本地模型定价表
//!
//! 当服务器不报告费用时（如 BYOK 模式），使用本地定价表估算费用。
//! 定价数据来源：各厂商官方定价页面（2025年7月数据）。
//! 费用以 1e10 ticks/USD 精度存储，与服务器报告的 cost_usd_ticks 一致。

/// 每百万 Token 的定价（美元）
#[derive(Debug, Clone, Copy)]
pub struct ModelPricing {
    /// 输入 Token 单价（美元/百万 Token）
    pub input_price_per_mtok: f64,
    /// 输出 Token 单价（美元/百万 Token）
    pub output_price_per_mtok: f64,
    /// 缓存读取 Token 单价（美元/百万 Token）
    /// 通常为输入单价的 0.1 倍
    pub cached_read_price_per_mtok: f64,
}

impl ModelPricing {
    /// 从输入单价推导缓存读取单价（0.1 倍）
    const fn with_cache_0_1x(input: f64, output: f64) -> Self {
        Self {
            input_price_per_mtok: input,
            output_price_per_mtok: output,
            cached_read_price_per_mtok: input * 0.1,
        }
    }
}

/// 定价表条目：模型前缀 + 定价
const PRICING_TABLE: &[(&str, ModelPricing)] = &[
    // === OpenAI 模型 ===
    ("o4-mini", ModelPricing { input_price_per_mtok: 1.10, output_price_per_mtok: 4.40, cached_read_price_per_mtok: 0.275 }),
    ("o3-mini", ModelPricing { input_price_per_mtok: 1.10, output_price_per_mtok: 4.40, cached_read_price_per_mtok: 0.55 }),
    ("o3", ModelPricing { input_price_per_mtok: 10.0, output_price_per_mtok: 40.0, cached_read_price_per_mtok: 2.50 }),
    ("o1-pro", ModelPricing { input_price_per_mtok: 60.0, output_price_per_mtok: 240.0, cached_read_price_per_mtok: 6.0 }),
    ("o1-mini", ModelPricing { input_price_per_mtok: 1.10, output_price_per_mtok: 4.40, cached_read_price_per_mtok: 0.55 }),
    ("o1", ModelPricing { input_price_per_mtok: 15.0, output_price_per_mtok: 60.0, cached_read_price_per_mtok: 7.50 }),
    ("gpt-4o-mini", ModelPricing { input_price_per_mtok: 0.15, output_price_per_mtok: 0.60, cached_read_price_per_mtok: 0.075 }),
    ("gpt-4o", ModelPricing { input_price_per_mtok: 2.50, output_price_per_mtok: 10.0, cached_read_price_per_mtok: 1.25 }),
    ("gpt-4-turbo", ModelPricing { input_price_per_mtok: 10.0, output_price_per_mtok: 30.0, cached_read_price_per_mtok: 1.0 }),
    ("gpt-4", ModelPricing::with_cache_0_1x(30.0, 60.0)),
    ("gpt-3.5", ModelPricing::with_cache_0_1x(0.50, 1.50)),
    // === Anthropic 模型 ===
    ("claude-opus-4", ModelPricing::with_cache_0_1x(15.0, 75.0)),
    ("claude-sonnet-4", ModelPricing::with_cache_0_1x(3.0, 15.0)),
    ("claude-haiku-4", ModelPricing::with_cache_0_1x(0.80, 4.0)),
    ("claude-3-5-sonnet", ModelPricing::with_cache_0_1x(3.0, 15.0)),
    ("claude-3-5-haiku", ModelPricing::with_cache_0_1x(0.80, 4.0)),
    ("claude-3-opus", ModelPricing::with_cache_0_1x(15.0, 75.0)),
    ("claude-3-sonnet", ModelPricing::with_cache_0_1x(3.0, 15.0)),
    ("claude-3-haiku", ModelPricing::with_cache_0_1x(0.25, 1.25)),
    // === xAI 模型 ===
    ("grok-4", ModelPricing::with_cache_0_1x(3.0, 15.0)),
    ("grok-3", ModelPricing::with_cache_0_1x(3.0, 15.0)),
    // === DeepSeek 模型 ===
    ("deepseek-chat", ModelPricing::with_cache_0_1x(0.27, 1.10)),
    ("deepseek-reasoner", ModelPricing::with_cache_0_1x(0.55, 2.19)),
    // === 通义千问 模型 ===
    ("qwen-max", ModelPricing::with_cache_0_1x(2.88, 11.52)),
    ("qwen-plus", ModelPricing::with_cache_0_1x(0.36, 1.44)),
    ("qwen-turbo", ModelPricing::with_cache_0_1x(0.09, 0.36)),
];

/// 模糊匹配模型定价
///
/// 按 PRICING_TABLE 顺序，找到第一个前缀匹配的条目。
/// 模型 ID 不区分大小写。
fn lookup_pricing(model_id: &str) -> Option<ModelPricing> {
    let model_lower = model_id.to_lowercase();
    for (prefix, pricing) in PRICING_TABLE {
        if model_lower.starts_with(prefix) {
            return Some(*pricing);
        }
    }
    None
}

/// 费用精度：1e10 ticks = 1 美元
const COST_TICKS_PER_USD: f64 = 1e10;

/// 根据本地定价表估算费用（ticks）
///
/// 参数：
/// - `model_id`: 模型 ID（如 "gpt-4o-2024-08-06"）
/// - `prompt_tokens`: 完整提示 Token 数（含缓存）
/// - `completion_tokens`: 输出 Token 数
/// - `cached_prompt_tokens`: 缓存命中 Token 数
///
/// 返回 `Option<i64>`：匹配到定价表则返回 ticks，否则返回 None
pub fn estimate_cost_usd_ticks(
    model_id: &str,
    prompt_tokens: u64,
    completion_tokens: u64,
    cached_prompt_tokens: u64,
) -> Option<i64> {
    let pricing = lookup_pricing(model_id)?;

    // 未缓存输入 = 完整提示 - 缓存命中
    let uncached_input = prompt_tokens.saturating_sub(cached_prompt_tokens);

    // 费用 = 未缓存输入 * 输入单价 + 缓存命中 * 缓存单价 + 输出 * 输出单价
    let cost_usd = (uncached_input as f64 * pricing.input_price_per_mtok
        + cached_prompt_tokens as f64 * pricing.cached_read_price_per_mtok
        + completion_tokens as f64 * pricing.output_price_per_mtok)
        / 1_000_000.0;

    let ticks = (cost_usd * COST_TICKS_PER_USD).round() as i64;
    if ticks > 0 {
        Some(ticks)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lookup_gpt4o() {
        let p = lookup_pricing("gpt-4o-2024-08-06").unwrap();
        assert_eq!(p.input_price_per_mtok, 2.50);
        assert_eq!(p.output_price_per_mtok, 10.0);
    }

    #[test]
    fn test_lookup_claude() {
        let p = lookup_pricing("claude-3-5-sonnet-20241022").unwrap();
        assert_eq!(p.input_price_per_mtok, 3.0);
        assert_eq!(p.output_price_per_mtok, 15.0);
    }

    #[test]
    fn test_lookup_unknown() {
        assert!(lookup_pricing("unknown-model").is_none());
    }

    #[test]
    fn test_estimate_cost() {
        // gpt-4o: 1000 输入, 500 输出, 0 缓存
        // 费用 = 1000 * 2.50 / 1M + 500 * 10.0 / 1M = 0.0025 + 0.005 = 0.0075 美元
        let ticks = estimate_cost_usd_ticks("gpt-4o", 1000, 500, 0).unwrap();
        let usd = ticks as f64 / COST_TICKS_PER_USD;
        assert!((usd - 0.0075).abs() < 1e-6);
    }

    #[test]
    fn test_estimate_cost_with_cache() {
        // gpt-4o: 1000 提示(500缓存), 500 输出
        // 未缓存 = 500, 缓存 = 500
        // 费用 = 500 * 2.50 / 1M + 500 * 1.25 / 1M + 500 * 10.0 / 1M
        //      = 0.00125 + 0.000625 + 0.005 = 0.006875 美元
        let ticks = estimate_cost_usd_ticks("gpt-4o", 1000, 500, 500).unwrap();
        let usd = ticks as f64 / COST_TICKS_PER_USD;
        assert!((usd - 0.006875).abs() < 1e-6);
    }

    #[test]
    fn test_estimate_cost_unknown_model() {
        assert!(estimate_cost_usd_ticks("unknown-model", 1000, 500, 0).is_none());
    }
}
