//! Provider fallback — ordered retry chain with per-provider timeouts.
//!
//! When a provider fails (timeout, rate limit, error), automatically tries the next
//! provider in the configured fallback chain.

use async_trait::async_trait;
use rustedclaw_core::error::ProviderError;
use rustedclaw_core::provider::*;
use std::sync::Arc;
use std::time::Duration;
use tracing::{info, warn};

/// A provider that wraps an ordered list of providers and falls back on failure.
pub struct FallbackProvider {
    name: String,
    chain: Vec<FallbackEntry>,
}

/// A single entry in the fallback chain.
struct FallbackEntry {
    provider: Arc<dyn rustedclaw_core::Provider>,
    timeout: Duration,
}

impl FallbackProvider {
    /// Create a new fallback provider with no entries.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            chain: Vec::new(),
        }
    }

    /// Add a provider to the fallback chain with a custom timeout.
    pub fn add(mut self, provider: Arc<dyn rustedclaw_core::Provider>, timeout: Duration) -> Self {
        self.chain.push(FallbackEntry { provider, timeout });
        self
    }

    /// Add a provider with the default timeout (120s).
    pub fn add_default(self, provider: Arc<dyn rustedclaw_core::Provider>) -> Self {
        self.add(provider, Duration::from_secs(120))
    }

    /// Number of providers in the chain.
    pub fn len(&self) -> usize {
        self.chain.len()
    }

    /// Whether the chain is empty.
    pub fn is_empty(&self) -> bool {
        self.chain.is_empty()
    }
}

#[async_trait]
impl rustedclaw_core::Provider for FallbackProvider {
    fn name(&self) -> &str {
        &self.name
    }

    async fn complete(
        &self,
        request: ProviderRequest,
    ) -> std::result::Result<ProviderResponse, ProviderError> {
        let mut last_error = ProviderError::NotConfigured("No providers in fallback chain".into());

        for (i, entry) in self.chain.iter().enumerate() {
            let provider_name = entry.provider.name().to_string();

            info!(
                provider = %provider_name,
                attempt = i + 1,
                total = self.chain.len(),
                "Fallback: trying provider"
            );

            match tokio::time::timeout(entry.timeout, entry.provider.complete(request.clone()))
                .await
            {
                Ok(Ok(response)) => return Ok(response),
                Ok(Err(e)) => {
                    warn!(
                        provider = %provider_name,
                        error = %e,
                        "Fallback: provider failed, trying next"
                    );
                    last_error = e;
                }
                Err(_) => {
                    warn!(
                        provider = %provider_name,
                        timeout_secs = entry.timeout.as_secs(),
                        "Fallback: provider timed out, trying next"
                    );
                    last_error = ProviderError::Timeout(format!(
                        "Provider '{}' timed out after {}s",
                        provider_name,
                        entry.timeout.as_secs()
                    ));
                }
            }
        }

        Err(last_error)
    }

    async fn stream(
        &self,
        request: ProviderRequest,
    ) -> std::result::Result<
        tokio::sync::mpsc::Receiver<std::result::Result<StreamChunk, ProviderError>>,
        ProviderError,
    > {
        let mut last_error = ProviderError::NotConfigured("No providers in fallback chain".into());

        for (i, entry) in self.chain.iter().enumerate() {
            let provider_name = entry.provider.name().to_string();

            info!(
                provider = %provider_name,
                attempt = i + 1,
                total = self.chain.len(),
                "Fallback: trying provider (streaming)"
            );

            match tokio::time::timeout(entry.timeout, entry.provider.stream(request.clone())).await
            {
                Ok(Ok(rx)) => return Ok(rx),
                Ok(Err(e)) => {
                    warn!(
                        provider = %provider_name,
                        error = %e,
                        "Fallback: provider stream failed, trying next"
                    );
                    last_error = e;
                }
                Err(_) => {
                    warn!(
                        provider = %provider_name,
                        timeout_secs = entry.timeout.as_secs(),
                        "Fallback: provider stream timed out, trying next"
                    );
                    last_error = ProviderError::Timeout(format!(
                        "Provider '{}' stream timed out after {}s",
                        provider_name,
                        entry.timeout.as_secs()
                    ));
                }
            }
        }

        Err(last_error)
    }

    async fn list_models(&self) -> std::result::Result<Vec<String>, ProviderError> {
        let mut all_models = Vec::new();
        for entry in &self.chain {
            if let Ok(models) = entry.provider.list_models().await {
                all_models.extend(models);
            }
        }
        Ok(all_models)
    }

    async fn health_check(&self) -> std::result::Result<bool, ProviderError> {
        for entry in &self.chain {
            if let Ok(true) = entry.provider.health_check().await {
                return Ok(true);
            }
        }
        Ok(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustedclaw_core::message::Message;
    use std::sync::Mutex;

    /// A mock provider that always fails.
    struct FailingProvider {
        name: String,
        error: ProviderError,
        call_count: Mutex<usize>,
    }

    impl FailingProvider {
        fn new(name: &str, error: ProviderError) -> Self {
            Self {
                name: name.into(),
                error,
                call_count: Mutex::new(0),
            }
        }

        fn calls(&self) -> usize {
            *self.call_count.lock().unwrap()
        }
    }

    #[async_trait]
    impl rustedclaw_core::Provider for FailingProvider {
        fn name(&self) -> &str {
            &self.name
        }

        async fn complete(
            &self,
            _request: ProviderRequest,
        ) -> std::result::Result<ProviderResponse, ProviderError> {
            *self.call_count.lock().unwrap() += 1;
            Err(self.error.clone())
        }
    }

    /// A mock provider that always succeeds.
    struct SuccessProvider {
        name: String,
        call_count: Mutex<usize>,
    }

    impl SuccessProvider {
        fn new(name: &str) -> Self {
            Self {
                name: name.into(),
                call_count: Mutex::new(0),
            }
        }

        fn calls(&self) -> usize {
            *self.call_count.lock().unwrap()
        }
    }

    #[async_trait]
    impl rustedclaw_core::Provider for SuccessProvider {
        fn name(&self) -> &str {
            &self.name
        }

        async fn complete(
            &self,
            _request: ProviderRequest,
        ) -> std::result::Result<ProviderResponse, ProviderError> {
            *self.call_count.lock().unwrap() += 1;
            Ok(ProviderResponse {
                message: Message::assistant("success"),
                usage: None,
                model: "test-model".into(),
                metadata: serde_json::Map::new(),
            })
        }
    }

    /// A mock provider that hangs forever (for timeout testing).
    struct HangingProvider;

    #[async_trait]
    impl rustedclaw_core::Provider for HangingProvider {
        fn name(&self) -> &str {
            "hanging"
        }

        async fn complete(
            &self,
            _request: ProviderRequest,
        ) -> std::result::Result<ProviderResponse, ProviderError> {
            tokio::time::sleep(Duration::from_secs(3600)).await;
            unreachable!()
        }
    }

    fn test_request() -> ProviderRequest {
        ProviderRequest {
            model: "test".into(),
            messages: vec![Message::user("hello")],
            temperature: 0.7,
            max_tokens: None,
            tools: vec![],
            stream: false,
            stop: vec![],
        }
    }

    #[tokio::test]
    async fn first_provider_succeeds() {
        let p1 = Arc::new(SuccessProvider::new("primary"));
        let p2 = Arc::new(SuccessProvider::new("secondary"));

        let fallback = FallbackProvider::new("test")
            .add_default(p1.clone())
            .add_default(p2.clone());

        let result = fallback.complete(test_request()).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap().message.content, "success");

        // Only first provider should be called
        assert_eq!(p1.calls(), 1);
        assert_eq!(p2.calls(), 0);
    }

    #[tokio::test]
    async fn falls_back_on_failure() {
        let p1 = Arc::new(FailingProvider::new(
            "primary",
            ProviderError::ApiError {
                status_code: 500,
                message: "Internal Server Error".into(),
            },
        ));
        let p2 = Arc::new(SuccessProvider::new("secondary"));

        let fallback = FallbackProvider::new("test")
            .add_default(p1.clone())
            .add_default(p2.clone());

        let result = fallback.complete(test_request()).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap().message.content, "success");

        assert_eq!(p1.calls(), 1);
        assert_eq!(p2.calls(), 1);
    }

    #[tokio::test]
    async fn falls_back_on_rate_limit() {
        let p1 = Arc::new(FailingProvider::new(
            "primary",
            ProviderError::RateLimited {
                retry_after_secs: 60,
            },
        ));
        let p2 = Arc::new(SuccessProvider::new("secondary"));

        let fallback = FallbackProvider::new("test")
            .add_default(p1.clone())
            .add_default(p2.clone());

        let result = fallback.complete(test_request()).await;
        assert!(result.is_ok());
        assert_eq!(p1.calls(), 1);
        assert_eq!(p2.calls(), 1);
    }

    #[tokio::test]
    async fn all_providers_fail() {
        let p1 = Arc::new(FailingProvider::new(
            "primary",
            ProviderError::Network("conn refused".into()),
        ));
        let p2 = Arc::new(FailingProvider::new(
            "secondary",
            ProviderError::AuthenticationFailed("bad key".into()),
        ));

        let fallback = FallbackProvider::new("test")
            .add_default(p1.clone())
            .add_default(p2.clone());

        let result = fallback.complete(test_request()).await;
        assert!(result.is_err());

        // Last error should be from the last provider
        match result.unwrap_err() {
            ProviderError::AuthenticationFailed(_) => {} // expected
            other => panic!("Expected AuthenticationFailed, got: {other:?}"),
        }

        assert_eq!(p1.calls(), 1);
        assert_eq!(p2.calls(), 1);
    }

    #[tokio::test]
    async fn timeout_triggers_fallback() {
        let p1 = Arc::new(HangingProvider);
        let p2 = Arc::new(SuccessProvider::new("secondary"));

        let fallback = FallbackProvider::new("test")
            .add(p1, Duration::from_millis(50)) // Very short timeout
            .add_default(p2.clone());

        let result = fallback.complete(test_request()).await;
        assert!(result.is_ok());
        assert_eq!(p2.calls(), 1);
    }

    #[tokio::test]
    async fn empty_chain_returns_not_configured() {
        let fallback = FallbackProvider::new("empty");
        let result = fallback.complete(test_request()).await;
        assert!(result.is_err());
        match result.unwrap_err() {
            ProviderError::NotConfigured(_) => {}
            other => panic!("Expected NotConfigured, got: {other:?}"),
        }
    }

    #[test]
    fn chain_length() {
        let p1 = Arc::new(SuccessProvider::new("a"));
        let p2 = Arc::new(SuccessProvider::new("b"));

        let fallback = FallbackProvider::new("test")
            .add_default(p1)
            .add_default(p2);

        assert_eq!(fallback.len(), 2);
        assert!(!fallback.is_empty());
    }

    #[test]
    fn empty_chain() {
        let fallback = FallbackProvider::new("empty");
        assert!(fallback.is_empty());
        assert_eq!(fallback.len(), 0);
    }

    #[tokio::test]
    async fn health_check_any_healthy() {
        let p1 = Arc::new(FailingProvider::new(
            "bad",
            ProviderError::Network("down".into()),
        ));
        let p2 = Arc::new(SuccessProvider::new("good"));

        let fallback = FallbackProvider::new("test")
            .add_default(p1)
            .add_default(p2);

        // health_check should return true if any provider is healthy
        let healthy = fallback.health_check().await.unwrap();
        assert!(healthy);
    }
}
