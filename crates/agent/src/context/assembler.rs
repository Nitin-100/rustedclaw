//! Context assembly pipeline — the core architectural component.
//!
//! Assembles a structured prompt from six distinct context layers:
//!
//! 1. **System** (identity, personality, rules) — highest priority, never trimmed
//! 2. **Long-Term Memory** (persistent facts) — oldest dropped first
//! 3. **Working Memory** (current plan, traces) — oldest dropped, plan kept
//! 4. **Knowledge / RAG** (retrieved chunks) — lowest-similarity dropped
//! 5. **Tool Schemas** (tool definitions) — least relevant dropped
//! 6. **Conversation History** (recent turns) — oldest turns dropped
//!
//! Implements FR-2 from the specification.
//!
//! # Determinism
//!
//! Context assembly is deterministic: identical inputs always produce
//! identical outputs (FR-2.5). No random or time-dependent logic is
//! used during assembly.

use crate::context::token;
use crate::context::working_memory::WorkingMemory;
use rustedclaw_core::identity::Identity;
use rustedclaw_core::memory::MemoryEntry;
use rustedclaw_core::message::{Conversation, Message};
use rustedclaw_core::provider::ToolDefinition;
use serde::{Deserialize, Serialize};

// ── Types ─────────────────────────────────────────────────────────────────

/// A retrieved knowledge chunk for the RAG layer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KnowledgeChunk {
    /// Source document identifier.
    pub document_id: String,
    /// Sequential chunk index within the document.
    pub chunk_index: usize,
    /// The text content of this chunk.
    pub content: String,
    /// Human-readable source label (filename, URL, etc.).
    pub source: String,
    /// Cosine similarity score from vector search (0.0–1.0).
    pub similarity: f32,
}

/// Token budget configuration.
#[derive(Debug, Clone)]
pub struct TokenBudget {
    /// Total token budget for the entire assembled context.
    pub total: usize,
    /// Optional per-layer maximum allocations.
    pub per_layer: PerLayerBudget,
}

/// Per-layer token budget caps. `None` means "use whatever remains".
#[derive(Debug, Clone, Default)]
pub struct PerLayerBudget {
    pub long_term_memory: Option<usize>,
    pub working_memory: Option<usize>,
    pub knowledge: Option<usize>,
    pub tool_schemas: Option<usize>,
    pub conversation_history: Option<usize>,
}

impl Default for TokenBudget {
    fn default() -> Self {
        Self {
            total: 4096,
            per_layer: PerLayerBudget::default(),
        }
    }
}

/// All inputs required by the assembler for a single LLM call.
pub struct AssemblyInput<'a> {
    /// Agent identity (system prompt source).
    pub identity: &'a Identity,
    /// Long-term memory entries, pre-sorted by relevance/recency.
    pub memories: &'a [MemoryEntry],
    /// Working memory state for the current task.
    pub working_memory: &'a WorkingMemory,
    /// RAG knowledge chunks, pre-sorted by similarity (descending).
    pub knowledge_chunks: &'a [KnowledgeChunk],
    /// Available tool definitions.
    pub tool_definitions: &'a [ToolDefinition],
    /// Conversation history.
    pub conversation: &'a Conversation,
    /// The current user message to process.
    pub user_message: &'a str,
}

/// The assembled context, ready for an LLM API call.
#[derive(Debug, Clone)]
pub struct AssembledContext {
    /// System message (identity + injected context sections).
    pub system_message: String,
    /// Conversation messages (history window + current user message).
    pub messages: Vec<Message>,
    /// Tool definitions to include in the API request.
    pub tool_definitions: Vec<ToolDefinition>,
    /// Assembly metadata (token counts, drops, utilization).
    pub metadata: AssemblyMetadata,
}

/// Detailed metadata about the assembly process.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssemblyMetadata {
    /// Total tokens in the assembled context.
    pub total_tokens: usize,
    /// Configured token budget.
    pub budget: usize,
    /// Budget utilization percentage (0.0–100.0).
    pub utilization_pct: f32,
    /// Per-layer statistics.
    pub per_layer: Vec<LayerStats>,
    /// Items dropped from each layer.
    pub drops: Vec<DropInfo>,
}

/// Statistics for a single context layer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LayerStats {
    /// Layer name.
    pub name: String,
    /// Tokens consumed by this layer.
    pub tokens: usize,
    /// Items included after budget trimming.
    pub items_included: usize,
    /// Total items available before trimming.
    pub items_total: usize,
}

/// Information about items dropped from a layer during budget enforcement.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DropInfo {
    /// Which layer.
    pub layer: String,
    /// Number of items dropped.
    pub items_dropped: usize,
    /// Estimated tokens of dropped content.
    pub tokens_dropped: usize,
    /// Reason for dropping.
    pub reason: String,
}

/// Errors from context assembly.
#[derive(Debug, Clone)]
pub enum AssemblyError {
    /// System prompt + current user message alone exceed the budget.
    BudgetExceeded {
        system_tokens: usize,
        user_tokens: usize,
        budget: usize,
    },
}

impl std::fmt::Display for AssemblyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::BudgetExceeded {
                system_tokens,
                user_tokens,
                budget,
            } => write!(
                f,
                "System prompt ({} tokens) + user message ({} tokens) exceed budget ({} tokens)",
                system_tokens, user_tokens, budget
            ),
        }
    }
}

impl std::error::Error for AssemblyError {}

// ── Assembler ─────────────────────────────────────────────────────────────

/// The context assembler. Stateless — create one and reuse it.
pub struct ContextAssembler {
    budget: TokenBudget,
}

impl ContextAssembler {
    /// Create a new assembler with the given token budget.
    pub fn new(budget: TokenBudget) -> Self {
        Self { budget }
    }

    /// Create an assembler with the default budget (4096 tokens).
    pub fn with_default_budget() -> Self {
        Self::new(TokenBudget::default())
    }

    /// Assemble context from all six layers.
    ///
    /// # Algorithm
    ///
    /// 1. Compute tokens for system prompt + user message (always included)
    /// 2. If those exceed the budget → return error
    /// 3. Fill remaining budget in priority order:
    ///    Long-Term Memory → Working Memory → Knowledge/RAG → Tool Schemas → Conversation History
    /// 4. Return assembled context + metadata
    pub fn assemble(&self, input: &AssemblyInput<'_>) -> Result<AssembledContext, AssemblyError> {
        let mut stats: Vec<LayerStats> = Vec::new();
        let mut drops: Vec<DropInfo> = Vec::new();

        // ── Layer 1: System prompt (always included, never trimmed) ────────
        let system_prompt = &input.identity.system_prompt;
        let system_tokens = token::estimate_tokens(system_prompt);
        stats.push(LayerStats {
            name: "system".into(),
            tokens: system_tokens,
            items_included: 1,
            items_total: 1,
        });

        // User message tokens (always included)
        let user_tokens = token::estimate_tokens(input.user_message) + 4; // +4 message overhead

        // Guard: system + user must fit
        let reserved = system_tokens + user_tokens;
        if reserved > self.budget.total {
            return Err(AssemblyError::BudgetExceeded {
                system_tokens,
                user_tokens,
                budget: self.budget.total,
            });
        }

        let mut remaining = self.budget.total - reserved;
        let mut context_sections: Vec<String> = Vec::new();

        // ── Layer 2: Long-Term Memory ──────────────────────────────────────
        let (mem_section, mem_stats, mem_drop) = Self::render_memory_layer(
            input.memories,
            self.effective_budget(self.budget.per_layer.long_term_memory, remaining),
        );
        remaining -= mem_stats.tokens;
        if !mem_section.is_empty() {
            context_sections.push(mem_section);
        }
        stats.push(mem_stats);
        if let Some(d) = mem_drop {
            drops.push(d);
        }

        // ── Layer 3: Working Memory ────────────────────────────────────────
        let (wm_section, wm_stats, wm_drop) = Self::render_working_memory_layer(
            input.working_memory,
            self.effective_budget(self.budget.per_layer.working_memory, remaining),
        );
        remaining -= wm_stats.tokens;
        if !wm_section.is_empty() {
            context_sections.push(wm_section);
        }
        stats.push(wm_stats);
        if let Some(d) = wm_drop {
            drops.push(d);
        }

        // ── Layer 4: Knowledge / RAG ───────────────────────────────────────
        let (rag_section, rag_stats, rag_drop) = Self::render_knowledge_layer(
            input.knowledge_chunks,
            self.effective_budget(self.budget.per_layer.knowledge, remaining),
        );
        remaining -= rag_stats.tokens;
        if !rag_section.is_empty() {
            context_sections.push(rag_section);
        }
        stats.push(rag_stats);
        if let Some(d) = rag_drop {
            drops.push(d);
        }

        // ── Layer 5: Tool Schemas ──────────────────────────────────────────
        let (tools_included, tool_stats, tool_drop) = Self::render_tool_layer(
            input.tool_definitions,
            self.effective_budget(self.budget.per_layer.tool_schemas, remaining),
        );
        remaining -= tool_stats.tokens;
        stats.push(tool_stats);
        if let Some(d) = tool_drop {
            drops.push(d);
        }

        // ── Layer 6: Conversation History ──────────────────────────────────
        let (history_messages, hist_stats, hist_drop) = Self::render_history_layer(
            input.conversation,
            self.effective_budget(self.budget.per_layer.conversation_history, remaining),
        );
        // remaining -= hist_stats.tokens; // last layer, not needed
        stats.push(hist_stats);
        if let Some(d) = hist_drop {
            drops.push(d);
        }

        // ── Assemble final system message ──────────────────────────────────
        let full_system = if context_sections.is_empty() {
            system_prompt.clone()
        } else {
            format!("{}\n\n{}", system_prompt, context_sections.join("\n\n"))
        };

        // ── Build message list: history + current user message ─────────────
        let mut messages = history_messages;
        messages.push(Message::user(input.user_message));

        // ── Compute final metadata ─────────────────────────────────────────
        // Add user message to stats
        stats.push(LayerStats {
            name: "user_message".into(),
            tokens: user_tokens,
            items_included: 1,
            items_total: 1,
        });

        let total_tokens: usize = stats.iter().map(|s| s.tokens).sum();
        let utilization_pct = (total_tokens as f32 / self.budget.total as f32) * 100.0;

        Ok(AssembledContext {
            system_message: full_system,
            messages,
            tool_definitions: tools_included,
            metadata: AssemblyMetadata {
                total_tokens,
                budget: self.budget.total,
                utilization_pct,
                per_layer: stats,
                drops,
            },
        })
    }

    // ── Private layer renderers ───────────────────────────────────────────

    fn render_memory_layer(
        memories: &[MemoryEntry],
        budget: usize,
    ) -> (String, LayerStats, Option<DropInfo>) {
        let layer = "long_term_memory";
        if memories.is_empty() {
            return (String::new(), Self::empty_stats(layer, 0), None);
        }

        let header = "[Long-Term Memory]\n";
        let header_tokens = token::estimate_tokens(header);
        if header_tokens >= budget {
            let dropped_tokens: usize = memories
                .iter()
                .map(|m| token::estimate_tokens(&m.content) + 2)
                .sum();
            return (
                String::new(),
                Self::empty_stats(layer, memories.len()),
                Some(DropInfo {
                    layer: layer.into(),
                    items_dropped: memories.len(),
                    tokens_dropped: dropped_tokens,
                    reason: "No budget available for memory layer".into(),
                }),
            );
        }

        let mut used = header_tokens;
        let mut lines = Vec::new();
        let mut dropped = 0;
        let mut dropped_tokens = 0;

        for entry in memories {
            let line = format!("- {}\n", entry.content);
            let line_tokens = token::estimate_tokens(&line);
            if used + line_tokens <= budget {
                lines.push(line);
                used += line_tokens;
            } else {
                dropped += 1;
                dropped_tokens += line_tokens;
            }
        }

        let section = if lines.is_empty() {
            String::new()
        } else {
            format!("{}{}", header, lines.join(""))
        };

        (
            section,
            LayerStats {
                name: layer.into(),
                tokens: used,
                items_included: lines.len(),
                items_total: memories.len(),
            },
            Self::maybe_drop(layer, dropped, dropped_tokens, "Oldest entries dropped"),
        )
    }

    fn render_working_memory_layer(
        wm: &WorkingMemory,
        budget: usize,
    ) -> (String, LayerStats, Option<DropInfo>) {
        let layer = "working_memory";
        if wm.is_empty() {
            return (String::new(), Self::empty_stats(layer, 0), None);
        }

        let header = "[Working Memory]\n";
        let full_render = format!("{}{}", header, wm.render());
        let full_tokens = token::estimate_tokens(&full_render);
        let item_count = wm.item_count();

        if full_tokens <= budget {
            // Everything fits
            return (
                full_render,
                LayerStats {
                    name: layer.into(),
                    tokens: full_tokens,
                    items_included: item_count,
                    items_total: item_count,
                },
                None,
            );
        }

        // Doesn't fit — trim. Keep plan (always), drop oldest trace entries.
        let mut out = String::from(header);
        let mut used = token::estimate_tokens(header);
        let mut included = 0;
        let mut dropped_count = 0;
        let mut dropped_tokens = 0;

        // Always include plan if present
        if let Some(plan) = &wm.plan {
            let plan_text = format!("Goal: {}\n", plan.goal);
            let plan_tokens = token::estimate_tokens(&plan_text);
            if used + plan_tokens <= budget {
                out.push_str(&plan_text);
                used += plan_tokens;
                included += 1;
            }
        }

        // Include trace entries from newest first (oldest dropped first)
        for entry in wm.trace.iter().rev() {
            let label = match entry.kind {
                super::working_memory::TraceKind::Thought => "Thought",
                super::working_memory::TraceKind::Action => "Action",
                super::working_memory::TraceKind::Observation => "Observation",
                super::working_memory::TraceKind::Reflection => "Reflection",
            };
            let line = format!("[{}] {}\n", label, entry.content);
            let line_tokens = token::estimate_tokens(&line);
            if used + line_tokens <= budget {
                out.push_str(&line);
                used += line_tokens;
                included += 1;
            } else {
                dropped_count += 1;
                dropped_tokens += line_tokens;
            }
        }

        (
            out,
            LayerStats {
                name: layer.into(),
                tokens: used,
                items_included: included,
                items_total: item_count,
            },
            Self::maybe_drop(
                layer,
                dropped_count,
                dropped_tokens,
                "Oldest trace entries dropped, plan kept",
            ),
        )
    }

    fn render_knowledge_layer(
        chunks: &[KnowledgeChunk],
        budget: usize,
    ) -> (String, LayerStats, Option<DropInfo>) {
        let layer = "knowledge";
        if chunks.is_empty() {
            return (String::new(), Self::empty_stats(layer, 0), None);
        }

        let header = "[Retrieved Knowledge]\n";
        let header_tokens = token::estimate_tokens(header);
        if header_tokens >= budget {
            let dropped_tokens: usize = chunks
                .iter()
                .map(|c| token::estimate_tokens(&c.content) + 4)
                .sum();
            return (
                String::new(),
                Self::empty_stats(layer, chunks.len()),
                Some(DropInfo {
                    layer: layer.into(),
                    items_dropped: chunks.len(),
                    tokens_dropped: dropped_tokens,
                    reason: "No budget available for knowledge layer".into(),
                }),
            );
        }

        let mut used = header_tokens;
        let mut lines = Vec::new();
        let mut dropped = 0;
        let mut dropped_tokens = 0;

        // Chunks are pre-sorted by similarity (highest first)
        for chunk in chunks {
            let entry = format!("[Source: {}] {}\n", chunk.source, chunk.content);
            let entry_tokens = token::estimate_tokens(&entry);
            if used + entry_tokens <= budget {
                lines.push(entry);
                used += entry_tokens;
            } else {
                dropped += 1;
                dropped_tokens += entry_tokens;
            }
        }

        let section = if lines.is_empty() {
            String::new()
        } else {
            format!("{}{}", header, lines.join(""))
        };

        (
            section,
            LayerStats {
                name: layer.into(),
                tokens: used,
                items_included: lines.len(),
                items_total: chunks.len(),
            },
            Self::maybe_drop(
                layer,
                dropped,
                dropped_tokens,
                "Lowest-similarity chunks dropped",
            ),
        )
    }

    fn render_tool_layer(
        tools: &[ToolDefinition],
        budget: usize,
    ) -> (Vec<ToolDefinition>, LayerStats, Option<DropInfo>) {
        let layer = "tool_schemas";
        if tools.is_empty() {
            return (Vec::new(), Self::empty_stats(layer, 0), None);
        }

        let mut used = 0;
        let mut included = Vec::new();
        let mut dropped = 0;
        let mut dropped_tokens = 0;

        for tool in tools {
            let tool_tokens = token::estimate_tool_tokens(tool);
            if used + tool_tokens <= budget {
                included.push(tool.clone());
                used += tool_tokens;
            } else {
                dropped += 1;
                dropped_tokens += tool_tokens;
            }
        }

        (
            included,
            LayerStats {
                name: layer.into(),
                tokens: used,
                items_included: tools.len() - dropped,
                items_total: tools.len(),
            },
            Self::maybe_drop(
                layer,
                dropped,
                dropped_tokens,
                "Least-relevant tools dropped",
            ),
        )
    }

    fn render_history_layer(
        conversation: &Conversation,
        budget: usize,
    ) -> (Vec<Message>, LayerStats, Option<DropInfo>) {
        let layer = "conversation_history";
        let messages = &conversation.messages;
        if messages.is_empty() {
            return (Vec::new(), Self::empty_stats(layer, 0), None);
        }

        let mut used = 0;
        let mut included = Vec::new();
        let mut dropped = 0;
        let mut dropped_tokens = 0;

        // Sliding window: include from newest (end) → oldest.
        // Skip system messages (Layer 1 handles that).
        for msg in messages.iter().rev() {
            if msg.role == rustedclaw_core::message::Role::System {
                continue;
            }
            let msg_tokens = token::estimate_message_tokens(msg);
            if used + msg_tokens <= budget {
                included.push(msg.clone());
                used += msg_tokens;
            } else {
                dropped += 1;
                dropped_tokens += msg_tokens;
            }
        }

        // Reverse to restore chronological order.
        included.reverse();

        let included_count = included.len();
        (
            included,
            LayerStats {
                name: layer.into(),
                tokens: used,
                items_included: included_count,
                items_total: messages.len(),
            },
            Self::maybe_drop(
                layer,
                dropped,
                dropped_tokens,
                "Oldest turns dropped (sliding window)",
            ),
        )
    }

    // ── Helpers ────────────────────────────────────────────────────────────

    fn effective_budget(&self, per_layer_limit: Option<usize>, remaining: usize) -> usize {
        match per_layer_limit {
            Some(limit) => limit.min(remaining),
            None => remaining,
        }
    }

    fn empty_stats(layer: &str, total: usize) -> LayerStats {
        LayerStats {
            name: layer.into(),
            tokens: 0,
            items_included: 0,
            items_total: total,
        }
    }

    fn maybe_drop(layer: &str, count: usize, tokens: usize, reason: &str) -> Option<DropInfo> {
        if count > 0 {
            Some(DropInfo {
                layer: layer.into(),
                items_dropped: count,
                tokens_dropped: tokens,
                reason: reason.into(),
            })
        } else {
            None
        }
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    // ── Helpers ────────────────────────────────────────────────────────

    fn test_identity() -> Identity {
        Identity::default()
    }

    fn test_memory(content: &str) -> MemoryEntry {
        MemoryEntry {
            id: "mem_1".into(),
            content: content.to_string(),
            tags: vec![],
            source: None,
            created_at: Utc::now(),
            last_accessed: Utc::now(),
            score: 0.0,
            embedding: None,
        }
    }

    fn test_chunk(content: &str, similarity: f32) -> KnowledgeChunk {
        KnowledgeChunk {
            document_id: "doc_1".into(),
            chunk_index: 0,
            content: content.to_string(),
            source: "test.md".into(),
            similarity,
        }
    }

    fn test_tool(name: &str) -> ToolDefinition {
        ToolDefinition {
            name: name.to_string(),
            description: format!("A {} tool", name),
            parameters: serde_json::json!({"type": "object", "properties": {}}),
        }
    }

    fn default_input<'a>(
        identity: &'a Identity,
        wm: &'a WorkingMemory,
        conv: &'a Conversation,
    ) -> AssemblyInput<'a> {
        AssemblyInput {
            identity,
            memories: &[],
            working_memory: wm,
            knowledge_chunks: &[],
            tool_definitions: &[],
            conversation: conv,
            user_message: "Hello",
        }
    }

    // ── Tests ──────────────────────────────────────────────────────────

    #[test]
    fn system_prompt_always_included() {
        let asm = ContextAssembler::with_default_budget();
        let id = test_identity();
        let wm = WorkingMemory::default();
        let conv = Conversation::new();

        let result = asm.assemble(&default_input(&id, &wm, &conv)).unwrap();
        assert!(!result.system_message.is_empty());

        let sys_layer = result
            .metadata
            .per_layer
            .iter()
            .find(|l| l.name == "system")
            .unwrap();
        assert!(sys_layer.tokens > 0);
        assert_eq!(sys_layer.items_included, 1);
    }

    #[test]
    fn user_message_always_in_messages() {
        let asm = ContextAssembler::with_default_budget();
        let id = test_identity();
        let wm = WorkingMemory::default();
        let conv = Conversation::new();

        let result = asm.assemble(&default_input(&id, &wm, &conv)).unwrap();
        assert_eq!(result.messages.len(), 1);
        assert_eq!(result.messages[0].content, "Hello");
    }

    #[test]
    fn budget_exceeded_returns_error() {
        let asm = ContextAssembler::new(TokenBudget {
            total: 5, // impossibly small
            per_layer: PerLayerBudget::default(),
        });
        let id = test_identity();
        let wm = WorkingMemory::default();
        let conv = Conversation::new();

        let err = asm.assemble(&default_input(&id, &wm, &conv)).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("exceed budget"));
    }

    #[test]
    fn memories_injected_into_system_message() {
        let asm = ContextAssembler::with_default_budget();
        let id = test_identity();
        let wm = WorkingMemory::default();
        let conv = Conversation::new();
        let memories = vec![
            test_memory("User prefers metric units"),
            test_memory("User's name is Alice"),
        ];

        let input = AssemblyInput {
            identity: &id,
            memories: &memories,
            working_memory: &wm,
            knowledge_chunks: &[],
            tool_definitions: &[],
            conversation: &conv,
            user_message: "Hello",
        };

        let result = asm.assemble(&input).unwrap();
        assert!(result.system_message.contains("[Long-Term Memory]"));
        assert!(result.system_message.contains("metric units"));
        assert!(result.system_message.contains("Alice"));

        let mem_layer = result
            .metadata
            .per_layer
            .iter()
            .find(|l| l.name == "long_term_memory")
            .unwrap();
        assert_eq!(mem_layer.items_included, 2);
    }

    #[test]
    fn working_memory_injected() {
        let asm = ContextAssembler::with_default_budget();
        let id = test_identity();
        let mut wm = WorkingMemory::default();
        wm.add_thought("I need to check the weather");
        let conv = Conversation::new();

        let input = AssemblyInput {
            identity: &id,
            memories: &[],
            working_memory: &wm,
            knowledge_chunks: &[],
            tool_definitions: &[],
            conversation: &conv,
            user_message: "Hello",
        };

        let result = asm.assemble(&input).unwrap();
        assert!(result.system_message.contains("[Working Memory]"));
        assert!(result.system_message.contains("check the weather"));
    }

    #[test]
    fn knowledge_chunks_injected() {
        let asm = ContextAssembler::with_default_budget();
        let id = test_identity();
        let wm = WorkingMemory::default();
        let conv = Conversation::new();
        let chunks = vec![
            test_chunk("Rust is a systems programming language", 0.95),
            test_chunk("WASM allows running code in browsers", 0.80),
        ];

        let input = AssemblyInput {
            identity: &id,
            memories: &[],
            working_memory: &wm,
            knowledge_chunks: &chunks,
            tool_definitions: &[],
            conversation: &conv,
            user_message: "Tell me about Rust",
        };

        let result = asm.assemble(&input).unwrap();
        assert!(result.system_message.contains("[Retrieved Knowledge]"));
        assert!(result.system_message.contains("systems programming"));
        assert!(result.system_message.contains("[Source: test.md]"));
    }

    #[test]
    fn tool_definitions_included() {
        let asm = ContextAssembler::with_default_budget();
        let id = test_identity();
        let wm = WorkingMemory::default();
        let conv = Conversation::new();
        let tools = vec![test_tool("web_search"), test_tool("calculator")];

        let input = AssemblyInput {
            identity: &id,
            memories: &[],
            working_memory: &wm,
            knowledge_chunks: &[],
            tool_definitions: &tools,
            conversation: &conv,
            user_message: "Hello",
        };

        let result = asm.assemble(&input).unwrap();
        assert_eq!(result.tool_definitions.len(), 2);

        let tool_layer = result
            .metadata
            .per_layer
            .iter()
            .find(|l| l.name == "tool_schemas")
            .unwrap();
        assert_eq!(tool_layer.items_included, 2);
    }

    #[test]
    fn conversation_history_sliding_window() {
        let asm = ContextAssembler::new(TokenBudget {
            total: 300,
            per_layer: PerLayerBudget {
                conversation_history: Some(50),
                ..Default::default()
            },
        });
        let id = test_identity();
        let wm = WorkingMemory::default();
        let mut conv = Conversation::new();

        // Add many messages — some should be dropped due to budget
        for i in 0..20 {
            conv.push(Message::user(format!("Message number {}", i)));
            conv.push(Message::assistant(format!("Response to message {}", i)));
        }

        let input = AssemblyInput {
            identity: &id,
            memories: &[],
            working_memory: &wm,
            knowledge_chunks: &[],
            tool_definitions: &[],
            conversation: &conv,
            user_message: "Latest message",
        };

        let result = asm.assemble(&input).unwrap();

        // Should have fewer messages than the original 40
        // (last message is "Latest message" added by assembler)
        assert!(result.messages.len() < 41);

        // The last non-user message in history should be recent, not old
        let hist_layer = result
            .metadata
            .per_layer
            .iter()
            .find(|l| l.name == "conversation_history")
            .unwrap();
        assert!(hist_layer.items_included < hist_layer.items_total);

        // Should have a drop record
        assert!(
            result
                .metadata
                .drops
                .iter()
                .any(|d| d.layer == "conversation_history")
        );
    }

    #[test]
    fn empty_layers_produce_no_sections() {
        let asm = ContextAssembler::with_default_budget();
        let id = test_identity();
        let wm = WorkingMemory::default();
        let conv = Conversation::new();

        let result = asm.assemble(&default_input(&id, &wm, &conv)).unwrap();

        // System message should NOT contain any layer headers
        assert!(!result.system_message.contains("[Long-Term Memory]"));
        assert!(!result.system_message.contains("[Working Memory]"));
        assert!(!result.system_message.contains("[Retrieved Knowledge]"));
    }

    #[test]
    fn metadata_totals_accurate() {
        let asm = ContextAssembler::with_default_budget();
        let id = test_identity();
        let wm = WorkingMemory::default();
        let conv = Conversation::new();
        let memories = vec![test_memory("A fact to remember")];

        let input = AssemblyInput {
            identity: &id,
            memories: &memories,
            working_memory: &wm,
            knowledge_chunks: &[],
            tool_definitions: &[],
            conversation: &conv,
            user_message: "Hello",
        };

        let result = asm.assemble(&input).unwrap();
        let sum: usize = result.metadata.per_layer.iter().map(|l| l.tokens).sum();
        assert_eq!(result.metadata.total_tokens, sum);
        assert!(result.metadata.utilization_pct > 0.0);
        assert!(result.metadata.utilization_pct <= 100.0);
        assert_eq!(result.metadata.budget, 4096);
    }

    #[test]
    fn per_layer_budget_caps_enforced() {
        let asm = ContextAssembler::new(TokenBudget {
            total: 4096,
            per_layer: PerLayerBudget {
                long_term_memory: Some(20), // Very tight cap
                ..Default::default()
            },
        });
        let id = test_identity();
        let wm = WorkingMemory::default();
        let conv = Conversation::new();

        // Create many memories that exceed 20 tokens
        let memories: Vec<MemoryEntry> = (0..10)
            .map(|i| test_memory(&format!("This is a somewhat long memory entry number {} that should exceed the tiny budget", i)))
            .collect();

        let input = AssemblyInput {
            identity: &id,
            memories: &memories,
            working_memory: &wm,
            knowledge_chunks: &[],
            tool_definitions: &[],
            conversation: &conv,
            user_message: "Hello",
        };

        let result = asm.assemble(&input).unwrap();
        let mem_layer = result
            .metadata
            .per_layer
            .iter()
            .find(|l| l.name == "long_term_memory")
            .unwrap();

        // Should not have included all 10 memories
        assert!(mem_layer.items_included < 10);
        // Should have a drop record
        assert!(
            result
                .metadata
                .drops
                .iter()
                .any(|d| d.layer == "long_term_memory")
        );
    }

    #[test]
    fn deterministic_assembly() {
        let asm = ContextAssembler::with_default_budget();
        let id = test_identity();
        let wm = WorkingMemory::default();
        let conv = Conversation::new();
        let memories = vec![test_memory("fact 1"), test_memory("fact 2")];
        let chunks = vec![test_chunk("chunk content", 0.9)];

        let input = AssemblyInput {
            identity: &id,
            memories: &memories,
            working_memory: &wm,
            knowledge_chunks: &chunks,
            tool_definitions: &[],
            conversation: &conv,
            user_message: "Test",
        };

        let result1 = asm.assemble(&input).unwrap();
        let result2 = asm.assemble(&input).unwrap();

        // Same system message
        assert_eq!(result1.system_message, result2.system_message);
        // Same token counts
        assert_eq!(result1.metadata.total_tokens, result2.metadata.total_tokens);
        // Same layer stats
        assert_eq!(
            result1.metadata.per_layer.len(),
            result2.metadata.per_layer.len()
        );
        for (a, b) in result1
            .metadata
            .per_layer
            .iter()
            .zip(result2.metadata.per_layer.iter())
        {
            assert_eq!(a.name, b.name);
            assert_eq!(a.tokens, b.tokens);
            assert_eq!(a.items_included, b.items_included);
        }
    }

    #[test]
    fn priority_order_respected() {
        // With a very tight budget, higher-priority layers should be filled first
        // and lower-priority layers should be starved.
        let id = test_identity();
        // System prompt is ~35 tokens. User message "Hi" is ~5 tokens.
        // Leave ~60 tokens for other layers.
        let sys_tokens = token::estimate_tokens(&id.system_prompt);
        let budget = sys_tokens + 5 + 60; // tight budget

        let asm = ContextAssembler::new(TokenBudget {
            total: budget,
            per_layer: PerLayerBudget::default(),
        });

        let mut wm = WorkingMemory::default();
        wm.add_thought("Important thought that should be included");

        let mut conv = Conversation::new();
        for i in 0..10 {
            conv.push(Message::user(format!("Old message {}", i)));
        }

        let memories = vec![test_memory("Critical fact")];
        let chunks = vec![test_chunk("Knowledge content here", 0.9)];

        let input = AssemblyInput {
            identity: &id,
            memories: &memories,
            working_memory: &wm,
            knowledge_chunks: &chunks,
            tool_definitions: &[],
            conversation: &conv,
            user_message: "Hi",
        };

        let result = asm.assemble(&input).unwrap();

        // Memory (Layer 2, higher priority) should be included
        let mem = result
            .metadata
            .per_layer
            .iter()
            .find(|l| l.name == "long_term_memory")
            .unwrap();
        assert!(
            mem.items_included > 0,
            "Memory should be included (high priority)"
        );

        // Conversation history (Layer 6, lowest priority) should be most starved
        let hist = result
            .metadata
            .per_layer
            .iter()
            .find(|l| l.name == "conversation_history")
            .unwrap();
        assert!(
            hist.items_included < hist.items_total,
            "History should be trimmed (low priority)"
        );
    }

    #[test]
    fn all_layers_populated() {
        let asm = ContextAssembler::new(TokenBudget {
            total: 8192, // generous
            per_layer: PerLayerBudget::default(),
        });
        let id = test_identity();
        let mut wm = WorkingMemory::default();
        wm.add_thought("thinking");
        let mut conv = Conversation::new();
        conv.push(Message::user("prev question"));
        conv.push(Message::assistant("prev answer"));

        let memories = vec![test_memory("a fact")];
        let chunks = vec![test_chunk("knowledge", 0.9)];
        let tools = vec![test_tool("calc")];

        let input = AssemblyInput {
            identity: &id,
            memories: &memories,
            working_memory: &wm,
            knowledge_chunks: &chunks,
            tool_definitions: &tools,
            conversation: &conv,
            user_message: "New question",
        };

        let result = asm.assemble(&input).unwrap();

        // All layers should have nonzero tokens
        assert!(result.system_message.contains("[Long-Term Memory]"));
        assert!(result.system_message.contains("[Working Memory]"));
        assert!(result.system_message.contains("[Retrieved Knowledge]"));
        assert_eq!(result.tool_definitions.len(), 1);

        // History messages + current user = 3 (prev user, prev assistant, new user)
        assert_eq!(result.messages.len(), 3);

        // No drops with generous budget
        assert!(result.metadata.drops.is_empty());
        assert!(result.metadata.utilization_pct.is_finite());
    }
}
