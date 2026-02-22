//! RAG pattern — Retrieval-Augmented Generation.
//!
//! The agent retrieves relevant knowledge chunks, injects them into
//! context Layer 4 (Knowledge/RAG), and generates a response grounded
//! in the retrieved content. Sources are tracked for citation.
//!
//! # Flow
//!
//! 1. Receive user question
//! 2. Call `knowledge_base_query` tool to retrieve relevant chunks
//! 3. Assemble context with chunks in the Knowledge layer
//! 4. Generate response grounded in retrieved knowledge
//! 5. Return answer with source attributions

use std::sync::Arc;
use rustedclaw_core::event::EventBus;
use rustedclaw_core::identity::Identity;
use rustedclaw_core::memory::MemoryEntry;
use rustedclaw_core::message::{Conversation, Message};
use rustedclaw_core::provider::{Provider, ProviderRequest};
use rustedclaw_core::tool::ToolRegistry;
use tracing::{debug, info};

use crate::context::assembler::{AssemblyMetadata, KnowledgeChunk};
use crate::context::working_memory::WorkingMemory;
use crate::context::{AssemblyInput, ContextAssembler, TokenBudget};

/// RAG agent configuration.
pub struct RagAgent {
    /// LLM provider.
    provider: Arc<dyn Provider>,
    /// Model name.
    model: String,
    /// Temperature.
    temperature: f32,
    /// Tool registry (used to call knowledge_base_query).
    tools: Arc<ToolRegistry>,
    /// Agent identity.
    identity: Identity,
    /// Token budget.
    budget: TokenBudget,
    /// Event bus.
    event_bus: Arc<EventBus>,
}

/// Result of a RAG execution.
pub struct RagResult {
    /// The generated answer.
    pub answer: String,
    /// Knowledge chunks that were retrieved.
    pub retrieved_chunks: Vec<KnowledgeChunk>,
    /// The query used for retrieval.
    pub retrieval_query: String,
    /// Working memory snapshot.
    pub working_memory: WorkingMemory,
    /// Context assembly metadata.
    pub context_metadata: Option<AssemblyMetadata>,
}

impl RagAgent {
    /// Create a new RAG agent.
    pub fn new(
        provider: Arc<dyn Provider>,
        model: impl Into<String>,
        temperature: f32,
        tools: Arc<ToolRegistry>,
        identity: Identity,
        event_bus: Arc<EventBus>,
    ) -> Self {
        Self {
            provider,
            model: model.into(),
            temperature,
            tools,
            identity,
            budget: TokenBudget::default(),
            event_bus,
        }
    }

    /// Set the token budget.
    pub fn with_budget(mut self, budget: TokenBudget) -> Self {
        self.budget = budget;
        self
    }

    /// Execute the RAG pattern.
    ///
    /// 1. Calls knowledge_base_query to retrieve relevant chunks
    /// 2. Assembles context with chunks in Layer 4
    /// 3. Generates a grounded response
    pub async fn run(
        &self,
        user_message: &str,
        conversation: &mut Conversation,
        memories: &[MemoryEntry],
    ) -> Result<RagResult, rustedclaw_core::Error> {
        let mut wm = WorkingMemory::new(5);

        info!(model = %self.model, "RAG: starting retrieval");

        // ── Step 1: Retrieve knowledge chunks ──
        wm.add_thought(&format!("Retrieving knowledge for: {}", user_message));

        let chunks = self.retrieve_chunks(user_message).await?;
        let retrieval_query = user_message.to_string();

        wm.add_observation(&format!("Retrieved {} knowledge chunks", chunks.len()));

        for chunk in &chunks {
            wm.add_tool_result(
                "knowledge_base_query",
                user_message,
                &format!("[{}] {:.80}...", chunk.source, chunk.content),
                true,
            );
        }

        debug!(chunks = chunks.len(), "RAG: chunks retrieved");

        // ── Step 2: Assemble context with knowledge layer ──
        let assembler = ContextAssembler::new(self.budget.clone());
        let tool_defs = self.tools.definitions();

        let input = AssemblyInput {
            identity: &self.identity,
            memories,
            working_memory: &wm,
            knowledge_chunks: &chunks,
            tool_definitions: &tool_defs,
            conversation,
            user_message,
        };

        let assembled = assembler.assemble(&input).map_err(|e| {
            rustedclaw_core::Error::Config { message: format!("Context assembly failed: {}", e) }
        })?;

        let metadata = assembled.metadata.clone();

        // ── Step 3: Generate grounded response ──
        let mut messages = vec![Message::system(&assembled.system_message)];
        messages.extend(assembled.messages);

        let request = ProviderRequest {
            model: self.model.clone(),
            messages,
            temperature: self.temperature,
            max_tokens: Some(4096),
            tools: vec![], // No tools during generation
            stream: false,
            stop: vec![],
        };

        let response = self.provider.complete(request).await?;
        let answer = response.message.content.clone();
        conversation.push(response.message);

        wm.add_reflection(&format!(
            "Generated answer using {} sources",
            chunks.len()
        ));

        info!(
            chunks = chunks.len(),
            answer_len = answer.len(),
            "RAG: response generated"
        );

        Ok(RagResult {
            answer,
            retrieved_chunks: chunks,
            retrieval_query,
            working_memory: wm,
            context_metadata: Some(metadata),
        })
    }

    /// Retrieve knowledge chunks using the knowledge_base_query tool.
    async fn retrieve_chunks(
        &self,
        query: &str,
    ) -> Result<Vec<KnowledgeChunk>, rustedclaw_core::Error> {
        let call = rustedclaw_core::tool::ToolCall {
            id: "rag_retrieval".into(),
            name: "knowledge_base_query".into(),
            arguments: serde_json::json!({"query": query, "top_k": 5}),
        };

        let result = self.tools.execute(&call).await.map_err(|e| {
            rustedclaw_core::Error::Tool(rustedclaw_core::error::ToolError::ExecutionFailed {
                tool_name: "knowledge_base_query".into(),
                reason: format!("{}", e),
            })
        })?;

        // Parse the tool output into KnowledgeChunks.
        let raw: Vec<serde_json::Value> =
            serde_json::from_str(&result.output).unwrap_or_default();

        let chunks: Vec<KnowledgeChunk> = raw
            .into_iter()
            .enumerate()
            .map(|(i, v)| KnowledgeChunk {
                document_id: v["document_id"]
                    .as_str()
                    .unwrap_or("unknown")
                    .to_string(),
                chunk_index: v["chunk_index"].as_u64().unwrap_or(i as u64) as usize,
                content: v["content"]
                    .as_str()
                    .unwrap_or("")
                    .to_string(),
                source: v["source"]
                    .as_str()
                    .unwrap_or("unknown")
                    .to_string(),
                similarity: v["similarity"].as_f64().unwrap_or(0.0) as f32,
            })
            .collect();

        Ok(chunks)
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::patterns::test_helpers::*;

    fn setup_rag() -> RagAgent {
        let provider = Arc::new(SequentialMockProvider::single_text(
            "Based on the retrieved knowledge, Rust is a systems programming language.",
        ));
        let tools = Arc::new(rustedclaw_tools::default_registry());
        let event_bus = Arc::new(EventBus::default());

        RagAgent::new(
            provider,
            "mock-model",
            0.3,
            tools,
            Identity::default(),
            event_bus,
        )
    }

    #[tokio::test]
    async fn rag_retrieves_and_generates() {
        let agent = setup_rag();
        let mut conv = Conversation::new();

        let result = agent
            .run("Tell me about Rust", &mut conv, &[])
            .await
            .unwrap();

        assert!(result.answer.contains("Rust"));
        assert!(!result.retrieved_chunks.is_empty());
        assert!(result.retrieved_chunks[0].similarity > 0.0);
    }

    #[tokio::test]
    async fn rag_populates_knowledge_layer() {
        let agent = setup_rag();
        let mut conv = Conversation::new();

        let result = agent
            .run("Tell me about Rust", &mut conv, &[])
            .await
            .unwrap();

        let meta = result.context_metadata.unwrap();
        let knowledge = meta
            .per_layer
            .iter()
            .find(|l| l.name == "knowledge")
            .unwrap();
        assert!(knowledge.items_included > 0);
        assert!(knowledge.tokens > 0);
    }

    #[tokio::test]
    async fn rag_records_working_memory() {
        let agent = setup_rag();
        let mut conv = Conversation::new();

        let result = agent
            .run("Tell me about agents", &mut conv, &[])
            .await
            .unwrap();

        // Should have thought, observation, reflection traces
        assert!(!result.working_memory.trace.is_empty());
        assert!(!result.working_memory.tool_results.is_empty());
    }

    #[tokio::test]
    async fn rag_with_memories() {
        let agent = setup_rag();
        let mut conv = Conversation::new();

        let memories = vec![MemoryEntry {
            id: "m1".into(),
            content: "User is learning Rust".into(),
            tags: vec![],
            source: None,
            created_at: chrono::Utc::now(),
            last_accessed: chrono::Utc::now(),
            score: 0.0,
            embedding: None,
        }];

        let result = agent
            .run("Tell me about Rust", &mut conv, &memories)
            .await
            .unwrap();

        let meta = result.context_metadata.unwrap();
        let mem_layer = meta
            .per_layer
            .iter()
            .find(|l| l.name == "long_term_memory")
            .unwrap();
        assert_eq!(mem_layer.items_included, 1);
    }

    #[tokio::test]
    async fn rag_sources_tracked() {
        let agent = setup_rag();
        let mut conv = Conversation::new();

        let result = agent
            .run("Tell me about WASM", &mut conv, &[])
            .await
            .unwrap();

        // Should have retrieved chunks with source info
        for chunk in &result.retrieved_chunks {
            assert!(!chunk.source.is_empty());
            assert!(!chunk.document_id.is_empty());
        }
    }
}
