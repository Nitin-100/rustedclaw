//! Provider router — selects the correct LLM provider based on config.
//!
//! Handles provider creation, caching, and routing requests to the right backend.

use std::collections::HashMap;
use std::sync::Arc;
use rustedclaw_core::provider::Provider;
use crate::anthropic::AnthropicProvider;
use crate::openai_compat::OpenAiCompatProvider;

/// Routes LLM requests to the correct provider.
pub struct ProviderRouter {
    providers: HashMap<String, Arc<dyn Provider>>,
    default_provider: String,
}

impl ProviderRouter {
    /// Create a new router with a default provider.
    pub fn new(default_provider: impl Into<String>) -> Self {
        Self {
            providers: HashMap::new(),
            default_provider: default_provider.into(),
        }
    }

    /// Register a provider.
    pub fn register(&mut self, name: impl Into<String>, provider: Arc<dyn Provider>) {
        self.providers.insert(name.into(), provider);
    }

    /// Get the default provider.
    pub fn default(&self) -> Option<Arc<dyn Provider>> {
        self.providers.get(&self.default_provider).cloned()
    }

    /// Get a specific provider by name.
    pub fn get(&self, name: &str) -> Option<Arc<dyn Provider>> {
        self.providers.get(name).cloned()
    }

    /// Resolve a provider from a model string like "openrouter/anthropic/claude-sonnet-4".
    pub fn resolve(&self, model_or_provider: &str) -> Option<(Arc<dyn Provider>, String)> {
        // If it contains a provider prefix like "custom:https://...", extract it
        if let Some(rest) = model_or_provider.strip_prefix("custom:") {
            // Create an ad-hoc custom provider
            let provider = Arc::new(OpenAiCompatProvider::new(
                "custom",
                rest,
                "", // API key from env or config
            ));
            return Some((provider, model_or_provider.to_string()));
        }

        // Otherwise, use the default provider with the model string as-is
        self.default().map(|p| (p, model_or_provider.to_string()))
    }

    /// List all registered provider names.
    pub fn list(&self) -> Vec<&str> {
        self.providers.keys().map(|s| s.as_str()).collect()
    }
}

/// Build providers from configuration.
pub fn build_from_config(
    config: &rustedclaw_config::AppConfig,
) -> ProviderRouter {
    let mut router = ProviderRouter::new(&config.default_provider);

    // Build providers from config
    for (name, provider_config) in &config.providers {
        let api_key = provider_config
            .api_key
            .clone()
            .or_else(|| config.api_key.clone())
            .unwrap_or_default();

        let base_url = provider_config
            .api_url
            .clone()
            .unwrap_or_else(|| default_base_url(name));

        let provider: Arc<dyn Provider> = if name == "anthropic" {
            // Use native Anthropic provider for direct API access
            let mut p = AnthropicProvider::new(&api_key);
            if provider_config.api_url.is_some() {
                p = p.with_base_url(&base_url);
            }
            Arc::new(p)
        } else {
            Arc::new(OpenAiCompatProvider::new(name, &base_url, &api_key))
        };

        router.register(name.clone(), provider);
    }

    // Ensure the default provider exists (even if not explicitly configured)
    if router.get(&config.default_provider).is_none() {
        let api_key = config.api_key.clone().unwrap_or_default();
        let base_url = default_base_url(&config.default_provider);

        let provider: Arc<dyn Provider> = if config.default_provider == "anthropic" {
            Arc::new(AnthropicProvider::new(&api_key))
        } else {
            Arc::new(OpenAiCompatProvider::new(
                &config.default_provider,
                &base_url,
                &api_key,
            ))
        };

        router.register(config.default_provider.clone(), provider);
    }

    router
}

/// Get the default base URL for well-known providers.
fn default_base_url(provider_name: &str) -> String {
    match provider_name {
        "openrouter" => "https://openrouter.ai/api/v1".into(),
        "openai" => "https://api.openai.com/v1".into(),
        "anthropic" => "https://api.anthropic.com/v1".into(),
        "ollama" => "http://localhost:11434/v1".into(),
        "deepseek" => "https://api.deepseek.com/v1".into(),
        "groq" => "https://api.groq.com/openai/v1".into(),
        "together" => "https://api.together.xyz/v1".into(),
        "fireworks" => "https://api.fireworks.ai/inference/v1".into(),
        "vllm" => "http://localhost:8000/v1".into(),
        "llamacpp" | "llama.cpp" => "http://localhost:8080/v1".into(),
        _ => format!("https://{provider_name}.api.example.com/v1"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn router_register_and_lookup() {
        let mut router = ProviderRouter::new("openrouter");
        let provider = Arc::new(OpenAiCompatProvider::openrouter("sk-test"));
        router.register("openrouter", provider);

        assert!(router.get("openrouter").is_some());
        assert!(router.get("nonexistent").is_none());
        assert!(router.default().is_some());
    }

    #[test]
    fn default_base_urls() {
        assert!(default_base_url("openrouter").contains("openrouter.ai"));
        assert!(default_base_url("openai").contains("api.openai.com"));
        assert!(default_base_url("ollama").contains("localhost:11434"));
    }

    #[test]
    fn build_from_default_config() {
        let config = rustedclaw_config::AppConfig::default();
        let router = build_from_config(&config);
        assert!(router.default().is_some());
    }
}
