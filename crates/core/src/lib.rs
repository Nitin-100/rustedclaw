//! # RustedClaw Core
//!
//! Domain types, traits, and error definitions for the RustedClaw AI agent runtime.
//! This crate has **zero framework dependencies** — it defines the domain model
//! that all other crates implement against.
//!
//! ## Design Philosophy
//!
//! Every subsystem is defined as a trait here. Implementations live in their
//! respective crates. This enables:
//! - Swapping implementations via configuration
//! - Easy testing with mock/stub implementations
//! - Clean dependency graph (all crates depend inward on core)

pub mod agent;
pub mod channel;
pub mod error;
pub mod event;
pub mod identity;
pub mod memory;
pub mod message;
pub mod provider;
pub mod tool;

// Re-export key types at crate root for ergonomics
pub use agent::{AgentConfig, AgentState};
pub use channel::{Channel, ChannelId, ChannelMessage};
pub use error::{Error, Result};
pub use event::{DomainEvent, EventBus};
pub use identity::{ContextPaths, Identity};
pub use memory::{MemoryBackend, MemoryEntry, MemoryQuery};
pub use message::{Conversation, ConversationId, Message, Role};
pub use provider::{Provider, ProviderRequest, ProviderResponse, StreamChunk};
pub use tool::{Tool, ToolCall, ToolRegistry, ToolResult};
