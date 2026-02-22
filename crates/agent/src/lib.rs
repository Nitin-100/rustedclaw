//! The core agent loop — the heart of RustedClaw.
//!
//! The agent follows a **Plan → Act → Observe** cycle:
//!
//! 1. **Receive** a user message (from any channel)
//! 2. **Build context** (system prompt + conversation + memory recall)
//! 3. **Send to LLM** via the configured provider
//! 4. **If tool calls**: execute tools, append results, loop back to step 3
//! 5. **If text response**: return to the user via the channel
//!
//! The loop continues until the LLM responds with text only (no tool calls)
//! or the max iteration limit is reached.

pub mod context;
pub mod loop_runner;
pub mod patterns;
pub mod stream_event;

pub use context::{
    AssembledContext, AssemblyError, AssemblyInput, AssemblyMetadata, ContextAssembler, DropInfo,
    KnowledgeChunk, LayerStats, PerLayerBudget, TokenBudget, WorkingMemory,
};
pub use loop_runner::AgentLoop;
pub use patterns::{CoordinationResult, CoordinatorAgent, SubTaskResult};
pub use patterns::{RagAgent, RagResult, ReactAgent, ReactResult};
pub use stream_event::AgentStreamEvent;
