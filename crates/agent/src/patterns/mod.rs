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

pub mod coordinator;
pub mod rag;
pub mod react;

pub use coordinator::{CoordinationResult, CoordinatorAgent, SubTaskResult};
pub use rag::{RagAgent, RagResult};
pub use react::{ReactAgent, ReactResult};

#[cfg(test)]
pub(crate) mod test_helpers;
