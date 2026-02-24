//! LLM Provider implementations for RustedClaw.
//!
//! All providers implement the `rustedclaw_core::Provider` trait.
//! The router selects the correct provider based on configuration.

pub mod anthropic;
pub mod fallback;
#[cfg(feature = "local")]
pub mod local;
pub mod openai_compat;
pub mod router;

pub use anthropic::AnthropicProvider;
pub use fallback::FallbackProvider;
#[cfg(feature = "local")]
pub use local::LocalProvider;
pub use openai_compat::OpenAiCompatProvider;
pub use router::ProviderRouter;
