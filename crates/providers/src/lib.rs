//! LLM Provider implementations for RustedClaw.
//!
//! All providers implement the `rustedclaw_core::Provider` trait.
//! The router selects the correct provider based on configuration.

pub mod anthropic;
pub mod fallback;
pub mod openai_compat;
pub mod router;

pub use anthropic::AnthropicProvider;
pub use fallback::FallbackProvider;
pub use openai_compat::OpenAiCompatProvider;
pub use router::ProviderRouter;
