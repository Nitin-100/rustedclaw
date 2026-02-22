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

pub mod error;
pub mod message;
pub mod provider;
pub mod channel;
pub mod tool;
pub mod memory;
pub mod agent;
pub mod identity;
pub mod event;

// Re-export key types at crate root for ergonomics
pub use error::{Error, Result};
pub use message::{Message, Role, Conversation, ConversationId};
pub use provider::{Provider, ProviderRequest, ProviderResponse, StreamChunk};
pub use channel::{Channel, ChannelMessage, ChannelId};
pub use tool::{Tool, ToolCall, ToolResult, ToolRegistry};
pub use memory::{MemoryBackend, MemoryEntry, MemoryQuery};
pub use agent::{AgentConfig, AgentState};
pub use identity::{Identity, ContextPaths};
pub use event::{DomainEvent, EventBus};
