//! Built-in pricing table for common LLM models.
//!
//! Prices are in USD per 1 million tokens. Each model has an input and
//! output price. Custom pricing can be added at runtime via TOML config.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::RwLock;

/// Per-million-token pricing for a model.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelPricing {
    /// Price per 1M input tokens in USD.
    pub input_per_m: f64,
    /// Price per 1M output tokens in USD.
    pub output_per_m: f64,
}

impl ModelPricing {
    /// Create a new pricing entry.
    pub fn new(input_per_m: f64, output_per_m: f64) -> Self {
        Self {
            input_per_m,
            output_per_m,
        }
    }

    /// Compute cost for the given token counts.
    pub fn cost(&self, input_tokens: u32, output_tokens: u32) -> f64 {
        (input_tokens as f64 * self.input_per_m + output_tokens as f64 * self.output_per_m)
            / 1_000_000.0
    }
}

/// Thread-safe pricing table with built-in defaults and custom overrides.
pub struct PricingTable {
    prices: RwLock<HashMap<String, ModelPricing>>,
}

impl PricingTable {
    /// Create a pricing table with built-in model prices.
    pub fn with_defaults() -> Self {
        let mut prices = HashMap::new();

        // ── Anthropic ──────────────────────────────────────────────
        prices.insert(
            "anthropic/claude-sonnet-4".into(),
            ModelPricing::new(3.0, 15.0),
        );
        prices.insert(
            "anthropic/claude-opus-4".into(),
            ModelPricing::new(15.0, 75.0),
        );
        prices.insert(
            "anthropic/claude-3.5-sonnet".into(),
            ModelPricing::new(3.0, 15.0),
        );
        prices.insert(
            "anthropic/claude-3.5-haiku".into(),
            ModelPricing::new(0.8, 4.0),
        );
        prices.insert(
            "anthropic/claude-3-haiku".into(),
            ModelPricing::new(0.25, 1.25),
        );

        // ── OpenAI ─────────────────────────────────────────────────
        prices.insert("openai/gpt-4o".into(), ModelPricing::new(2.5, 10.0));
        prices.insert("openai/gpt-4o-mini".into(), ModelPricing::new(0.15, 0.6));
        prices.insert("openai/gpt-4-turbo".into(), ModelPricing::new(10.0, 30.0));
        prices.insert("openai/o1".into(), ModelPricing::new(15.0, 60.0));
        prices.insert("openai/o1-mini".into(), ModelPricing::new(3.0, 12.0));
        prices.insert("openai/o3-mini".into(), ModelPricing::new(1.1, 4.4));

        // ── Google ─────────────────────────────────────────────────
        prices.insert(
            "google/gemini-2.0-flash".into(),
            ModelPricing::new(0.1, 0.4),
        );
        prices.insert(
            "google/gemini-2.0-pro".into(),
            ModelPricing::new(1.25, 10.0),
        );
        prices.insert("google/gemini-1.5-pro".into(), ModelPricing::new(1.25, 5.0));
        prices.insert(
            "google/gemini-1.5-flash".into(),
            ModelPricing::new(0.075, 0.3),
        );

        // ── Meta (via OpenRouter) ──────────────────────────────────
        prices.insert(
            "meta-llama/llama-3.1-405b".into(),
            ModelPricing::new(2.7, 2.7),
        );
        prices.insert(
            "meta-llama/llama-3.1-70b".into(),
            ModelPricing::new(0.52, 0.75),
        );
        prices.insert(
            "meta-llama/llama-3.1-8b".into(),
            ModelPricing::new(0.055, 0.055),
        );

        // ── Mistral ────────────────────────────────────────────────
        prices.insert("mistral/mistral-large".into(), ModelPricing::new(2.0, 6.0));
        prices.insert("mistral/mistral-small".into(), ModelPricing::new(0.2, 0.6));
        prices.insert("mistral/codestral".into(), ModelPricing::new(0.3, 0.9));

        // ── DeepSeek ───────────────────────────────────────────────
        prices.insert("deepseek/deepseek-v3".into(), ModelPricing::new(0.27, 1.1));
        prices.insert("deepseek/deepseek-r1".into(), ModelPricing::new(0.55, 2.19));

        Self {
            prices: RwLock::new(prices),
        }
    }

    /// Create an empty pricing table.
    pub fn empty() -> Self {
        Self {
            prices: RwLock::new(HashMap::new()),
        }
    }

    /// Look up pricing for a model. Returns None if not found.
    pub fn get(&self, model: &str) -> Option<ModelPricing> {
        let prices = self.prices.read().unwrap();
        prices.get(model).cloned()
    }

    /// Add or update pricing for a model.
    pub fn set(&self, model: impl Into<String>, pricing: ModelPricing) {
        let mut prices = self.prices.write().unwrap();
        prices.insert(model.into(), pricing);
    }

    /// Compute cost for a model call, returning 0.0 if model is not in table.
    ///
    /// Supports flexible matching: tries exact match first, then strips
    /// provider prefix (`openai/gpt-4o` → `gpt-4o`), then tries prefix
    /// matching (`gpt-4o-mini-2024-07-18` matches `gpt-4o-mini`).
    pub fn compute_cost(&self, model: &str, input_tokens: u32, output_tokens: u32) -> f64 {
        let prices = self.prices.read().unwrap();

        // 1. Exact match
        if let Some(p) = prices.get(model) {
            return p.cost(input_tokens, output_tokens);
        }

        // 2. Try with common provider prefixes
        let prefixed_names = [
            format!("openai/{}", model),
            format!("anthropic/{}", model),
            format!("google/{}", model),
            format!("mistral/{}", model),
            format!("deepseek/{}", model),
            format!("meta-llama/{}", model),
        ];
        for name in &prefixed_names {
            if let Some(p) = prices.get(name.as_str()) {
                return p.cost(input_tokens, output_tokens);
            }
        }

        // 3. Prefix match — model response often includes version suffix
        //    e.g. "gpt-4o-mini-2024-07-18" should match "gpt-4o-mini"
        //    or "openai/gpt-4o-mini"
        let model_lower = model.to_lowercase();
        let bare_model = model_lower
            .split('/')
            .last()
            .unwrap_or(&model_lower);

        // Find the longest matching key whose bare name is a prefix of the model
        let mut best: Option<(&str, &ModelPricing)> = None;
        for (key, pricing) in prices.iter() {
            let bare_key = key.split('/').last().unwrap_or(key);
            if bare_model.starts_with(&bare_key.to_lowercase()) {
                if best.is_none() || bare_key.len() > best.unwrap().0.len() {
                    best = Some((key.as_str(), pricing));
                }
            }
        }

        if let Some((_, p)) = best {
            return p.cost(input_tokens, output_tokens);
        }

        0.0
    }

    /// List all known model names.
    pub fn models(&self) -> Vec<String> {
        let prices = self.prices.read().unwrap();
        let mut names: Vec<String> = prices.keys().cloned().collect();
        names.sort();
        names
    }

    /// Number of models in the pricing table.
    pub fn len(&self) -> usize {
        self.prices.read().unwrap().len()
    }

    /// Whether the table is empty.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl Default for PricingTable {
    fn default() -> Self {
        Self::with_defaults()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_table_has_models() {
        let table = PricingTable::with_defaults();
        assert!(table.len() >= 20);
        assert!(!table.is_empty());
    }

    #[test]
    fn known_model_cost() {
        let table = PricingTable::with_defaults();

        // Claude Sonnet 4: $3/M input, $15/M output
        let cost = table.compute_cost("anthropic/claude-sonnet-4", 1000, 500);
        // Expected: (1000 * 3.0 + 500 * 15.0) / 1_000_000 = (3000 + 7500) / 1M = 0.0105
        assert!((cost - 0.0105).abs() < 1e-10);
    }

    #[test]
    fn unknown_model_returns_zero() {
        let table = PricingTable::with_defaults();
        let cost = table.compute_cost("unknown/model-xyz", 1000, 500);
        assert!((cost - 0.0).abs() < 1e-10);
    }

    #[test]
    fn custom_pricing() {
        let table = PricingTable::empty();
        assert!(table.is_empty());

        table.set("custom/model", ModelPricing::new(1.0, 2.0));
        assert_eq!(table.len(), 1);

        let cost = table.compute_cost("custom/model", 1_000_000, 1_000_000);
        // (1M * 1.0 + 1M * 2.0) / 1M = 3.0
        assert!((cost - 3.0).abs() < 1e-10);
    }

    #[test]
    fn model_pricing_cost() {
        let p = ModelPricing::new(5.0, 15.0);
        // 500 input, 200 output → (500*5 + 200*15) / 1M = (2500+3000)/1M = 0.0055
        let c = p.cost(500, 200);
        assert!((c - 0.0055).abs() < 1e-10);
    }

    #[test]
    fn list_models() {
        let table = PricingTable::with_defaults();
        let models = table.models();
        assert!(models.contains(&"openai/gpt-4o".to_string()));
        assert!(models.contains(&"anthropic/claude-sonnet-4".to_string()));
        // Should be sorted
        assert!(models.windows(2).all(|w| w[0] <= w[1]));
    }

    #[test]
    fn set_overrides_existing() {
        let table = PricingTable::with_defaults();
        let old = table.compute_cost("openai/gpt-4o", 1_000_000, 0);
        assert!((old - 2.5).abs() < 1e-10);

        // Override with custom pricing
        table.set("openai/gpt-4o", ModelPricing::new(5.0, 20.0));
        let new_cost = table.compute_cost("openai/gpt-4o", 1_000_000, 0);
        assert!((new_cost - 5.0).abs() < 1e-10);
    }
}
