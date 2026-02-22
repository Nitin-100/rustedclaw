//! Agent patterns — structured reasoning strategies.
//!
//! Implements the three required agent patterns from the specification:
//!
//! 1. **ReAct** — Thought → Action → Observation loop with visible traces
//! 2. **RAG** — Retrieval-Augmented Generation grounded in knowledge chunks
//! 3. **Coordinator** — Multi-agent task decomposition and delegation
//!
//! All patterns use the context assembly pipeline (FR-2) and working
//! memory (FR-5) for structured reasoning.

pub mod react;
pub mod rag;
pub mod coordinator;

pub use react::{ReactAgent, ReactResult};
pub use rag::{RagAgent, RagResult};
pub use coordinator::{CoordinatorAgent, CoordinationResult, SubTaskResult};

#[cfg(test)]
pub(crate) mod test_helpers;
