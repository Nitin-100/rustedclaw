//! Multi-layer context assembly pipeline.
//!
//! Assembles a structured prompt from six distinct context layers,
//! enforcing a configurable token budget with priority-based filling
//! and drop tracking. Implements FR-2 and FR-5 from the specification.
//!
//! # Context Layers (in priority order)
//!
//! | Layer | Source | Trim Strategy |
//! |-------|--------|---------------|
//! | 1. System | Identity config | Never trimmed |
//! | 2. Long-Term Memory | Persistent facts | Oldest dropped first |
//! | 3. Working Memory | Scratchpad | Oldest entries dropped, plan kept |
//! | 4. Knowledge/RAG | Retrieved chunks | Lowest-similarity dropped |
//! | 5. Tool Schemas | Tool registry | Least-relevant dropped |
//! | 6. Conversation History | Recent turns | Oldest turns dropped |

pub mod assembler;
pub mod token;
pub mod working_memory;

pub use assembler::{
    AssembledContext, AssemblyError, AssemblyInput, AssemblyMetadata, ContextAssembler, DropInfo,
    KnowledgeChunk, LayerStats, PerLayerBudget, TokenBudget,
};
pub use working_memory::WorkingMemory;
