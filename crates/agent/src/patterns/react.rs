//! ReAct pattern — Thought → Action → Observation loop.
//!
//! The agent reasons step-by-step, choosing tools to gather information,
//! then synthesizes a final answer. All reasoning steps are recorded
//! in working memory and are fully inspectable.
//!
//! # Trace Format
//!
//! Each iteration records:
//! - **Thought**: The LLM's reasoning (from `content` field)
//! - **Action**: Which tool was called with what arguments
//! - **Observation**: The tool execution result
//!
//! The loop terminates when the LLM returns a response with no tool
//! calls, or when max iterations is reached.

use chrono::Utc;
use rustedclaw_core::event::{DomainEvent, EventBus};
use rustedclaw_core::identity::Identity;
use rustedclaw_core::memory::{MemoryBackend, MemoryEntry, MemoryQuery, SearchMode};
use rustedclaw_core::message::{Conversation, Message};
use rustedclaw_core::provider::{Provider, ProviderRequest};
use rustedclaw_core::tool::{ToolCall, ToolRegistry};
use rustedclaw_telemetry::TelemetryEngine;
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::{debug, info, warn};

use crate::context::assembler::{AssemblyMetadata, KnowledgeChunk};
use crate::context::working_memory::{TraceEntry, WorkingMemory};
use crate::context::{AssemblyInput, ContextAssembler, TokenBudget};

/// Configuration for the ReAct agent.
pub struct ReactAgent {
    /// LLM provider.
    provider: Arc<dyn Provider>,
    /// Model name.
    model: String,
    /// Temperature.
    temperature: f32,
    /// Default max tokens per response.
    max_tokens: Option<u32>,
    /// Tool registry.
    tools: Arc<ToolRegistry>,
    /// Agent identity.
    identity: Identity,
    /// Token budget for context assembly.
    budget: TokenBudget,
    /// Maximum reasoning iterations.
    max_iterations: u32,
    /// Event bus.
    event_bus: Arc<EventBus>,
    /// Optional memory backend for recall and auto-save.
    memory: Option<Arc<dyn MemoryBackend>>,
    /// Whether to auto-save conversation summaries to memory.
    auto_save: bool,
    /// Maximum memories to recall per turn.
    recall_limit: usize,
    /// Optional telemetry engine for execution tracing and cost tracking.
    telemetry: Option<Arc<TelemetryEngine>>,
}

/// The result of a ReAct execution.
pub struct ReactResult {
    /// The final answer text.
    pub answer: String,
    /// Complete reasoning trace.
    pub trace: Vec<TraceEntry>,
    /// Working memory snapshot at completion.
    pub working_memory: WorkingMemory,
    /// Number of iterations used.
    pub iterations: usize,
    /// Total tool calls made.
    pub tool_calls_made: usize,
    /// Context assembly metadata from the last iteration.
    pub last_context_metadata: Option<AssemblyMetadata>,
}

impl ReactAgent {
    /// Create a new ReAct agent.
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
            max_tokens: None,
            tools,
            identity,
            budget: TokenBudget::default(),
            max_iterations: 10,
            event_bus,
            memory: None,
            auto_save: false,
            recall_limit: 5,
            telemetry: None,
        }
    }

    /// Set the token budget.
    pub fn with_budget(mut self, budget: TokenBudget) -> Self {
        self.budget = budget;
        self
    }

    /// Set max iterations.
    pub fn with_max_iterations(mut self, max: u32) -> Self {
        self.max_iterations = max;
        self
    }

    /// Set the default max tokens per LLM response.
    pub fn with_max_tokens(mut self, max: u32) -> Self {
        self.max_tokens = Some(max);
        self
    }

    /// Attach a memory backend for automatic recall and save.
    pub fn with_memory(mut self, memory: Arc<dyn MemoryBackend>) -> Self {
        self.memory = Some(memory);
        self
    }

    /// Enable or disable auto-save of conversation content to memory.
    pub fn with_auto_save(mut self, enabled: bool) -> Self {
        self.auto_save = enabled;
        self
    }

    /// Set the maximum number of memories to recall per turn.
    pub fn with_recall_limit(mut self, limit: usize) -> Self {
        self.recall_limit = limit;
        self
    }

    /// Attach a telemetry engine for execution tracing and cost tracking.
    pub fn with_telemetry(mut self, engine: Arc<TelemetryEngine>) -> Self {
        self.telemetry = Some(engine);
        self
    }

    /// Recall relevant memories from the backend.
    async fn recall_memories(&self, user_message: &str) -> Vec<MemoryEntry> {
        let Some(memory) = &self.memory else {
            return vec![];
        };

        match memory
            .search(MemoryQuery {
                text: user_message.to_string(),
                limit: self.recall_limit,
                min_score: 0.0,
                tags: vec![],
                mode: SearchMode::Hybrid,
            })
            .await
        {
            Ok(entries) => {
                if !entries.is_empty() {
                    debug!(count = entries.len(), "ReactAgent recalled memories");
                }
                entries
            }
            Err(e) => {
                warn!("ReactAgent memory recall failed: {e}");
                vec![]
            }
        }
    }

    /// Auto-save a summary of the conversation to memory.
    async fn auto_save_to_memory(
        &self,
        user_message: &str,
        answer: &str,
        conversation: &Conversation,
    ) {
        let Some(memory) = &self.memory else {
            return;
        };
        if !self.auto_save {
            return;
        }

        // Only save meaningful exchanges
        if user_message.len() < 10 || answer.len() < 10 {
            return;
        }

        let summary = format!(
            "User asked: {}\nAssistant answered: {}",
            user_message, answer
        );

        let entry = MemoryEntry {
            id: String::new(),
            content: summary,
            tags: vec!["conversation".into(), "auto-saved".into(), "react".into()],
            source: Some(conversation.id.to_string()),
            created_at: Utc::now(),
            last_accessed: Utc::now(),
            score: 0.0,
            embedding: None,
        };

        match memory.store(entry).await {
            Ok(id) => debug!(memory_id = %id, "ReactAgent auto-saved to memory"),
            Err(e) => warn!("ReactAgent failed to auto-save to memory: {e}"),
        }
    }

    /// Execute the ReAct loop.
    ///
    /// Takes a user message, optional long-term memories, and optional
    /// knowledge chunks (for RAG integration). Returns the final answer
    /// along with the complete reasoning trace.
    pub async fn run(
        &self,
        user_message: &str,
        conversation: &mut Conversation,
        memories: &[MemoryEntry],
        knowledge_chunks: &[KnowledgeChunk],
    ) -> Result<ReactResult, rustedclaw_core::Error> {
        let mut wm = WorkingMemory::new(self.max_iterations as usize);
        let assembler = ContextAssembler::new(self.budget.clone());
        let tool_defs = self.tools.definitions();
        let mut total_tool_calls = 0usize;
        let mut last_metadata: Option<AssemblyMetadata> = None;

        info!(model = %self.model, max_iter = self.max_iterations, "ReAct loop starting");

        // ── Start telemetry trace ──
        let trace_id = self
            .telemetry
            .as_ref()
            .map(|t| t.start_trace(conversation.id.to_string()));

        // ── Auto-recall memories ──
        let recalled = self.recall_memories(user_message).await;
        let all_memories: Vec<MemoryEntry> = if recalled.is_empty() {
            memories.to_vec()
        } else {
            let mut combined = memories.to_vec();
            // Add recalled memories that aren't already in the provided list
            for r in recalled {
                if !combined.iter().any(|m| m.id == r.id) {
                    combined.push(r);
                }
            }
            combined
        };

        loop {
            if !wm.tick() {
                warn!("ReAct: max iterations reached ({})", self.max_iterations);
                break;
            }

            debug!(iteration = wm.iterations, "ReAct iteration");

            // ── Assemble context ──
            let input = AssemblyInput {
                identity: &self.identity,
                memories: &all_memories,
                working_memory: &wm,
                knowledge_chunks,
                tool_definitions: &tool_defs,
                conversation,
                user_message,
            };

            let assembled =
                assembler
                    .assemble(&input)
                    .map_err(|e| rustedclaw_core::Error::Config {
                        message: format!("Context assembly failed: {}", e),
                    })?;

            last_metadata = Some(assembled.metadata.clone());

            // ── Build LLM request ──
            let mut messages = vec![Message::system(&assembled.system_message)];
            messages.extend(assembled.messages);

            let request = ProviderRequest {
                model: self.model.clone(),
                messages,
                temperature: self.temperature,
                max_tokens: self.max_tokens,
                tools: assembled.tool_definitions,
                stream: false,
                stop: vec![],
            };

            // ── Call LLM ──
            let llm_start = std::time::Instant::now();
            let response = self.provider.complete(request).await?;
            let llm_duration_ms = llm_start.elapsed().as_millis() as u64;

            // Track usage
            if let Some(usage) = &response.usage {
                self.event_bus.publish(DomainEvent::ResponseGenerated {
                    conversation_id: conversation.id.to_string(),
                    model: response.model.clone(),
                    tokens_used: usage.total_tokens,
                    timestamp: chrono::Utc::now(),
                });

                // Record telemetry span for this LLM call
                if let (Some(telemetry), Some(tid)) = (&self.telemetry, &trace_id) {
                    let cost = telemetry.compute_cost(
                        &response.model,
                        usage.prompt_tokens,
                        usage.completion_tokens,
                    );
                    let mut span = rustedclaw_telemetry::Span::new(
                        rustedclaw_telemetry::SpanKind::LlmCall,
                        &response.model,
                    );
                    span.record_tokens(usage.prompt_tokens, usage.completion_tokens, cost);
                    span.duration_ms = Some(llm_duration_ms);
                    span.end(true);
                    telemetry.record_span(tid, span);
                }
            }

            // ── Record thought ──
            if !response.message.content.is_empty() {
                wm.add_thought(&response.message.content);
            }

            // ── Check for final answer ──
            if response.message.tool_calls.is_empty() {
                let answer = response.message.content.clone();
                conversation.push(response.message);

                // ── Auto-save to memory ──
                self.auto_save_to_memory(user_message, &answer, conversation)
                    .await;

                // ── End telemetry trace ──
                if let (Some(telemetry), Some(tid)) = (&self.telemetry, &trace_id) {
                    telemetry.end_trace(tid);
                }

                info!(
                    iterations = wm.iterations,
                    tool_calls = total_tool_calls,
                    "ReAct loop completed"
                );

                return Ok(ReactResult {
                    answer,
                    trace: wm.trace.clone(),
                    iterations: wm.iterations,
                    working_memory: wm,
                    tool_calls_made: total_tool_calls,
                    last_context_metadata: last_metadata,
                });
            }

            // ── Execute tool calls ──
            let tool_calls = response.message.tool_calls.clone();
            conversation.push(response.message);

            for tc in &tool_calls {
                total_tool_calls += 1;

                // Record Action
                wm.add_action(&format!("{}({})", tc.name, tc.arguments));

                let call = ToolCall {
                    id: tc.id.clone(),
                    name: tc.name.clone(),
                    arguments: serde_json::from_str(&tc.arguments).unwrap_or_default(),
                };

                let start = std::time::Instant::now();
                let result = self.tools.execute(&call).await;
                let duration_ms = start.elapsed().as_millis() as u64;

                match result {
                    Ok(tool_result) => {
                        // Record Observation
                        wm.add_observation(&tool_result.output);
                        wm.add_tool_result(
                            &tc.name,
                            &tc.arguments,
                            &tool_result.output,
                            tool_result.success,
                        );

                        self.event_bus.publish(DomainEvent::ToolExecuted {
                            tool_name: tc.name.clone(),
                            success: tool_result.success,
                            duration_ms,
                            timestamp: chrono::Utc::now(),
                        });

                        // Record tool span in telemetry
                        if let (Some(telemetry), Some(tid)) = (&self.telemetry, &trace_id) {
                            let mut span = rustedclaw_telemetry::Span::new(
                                rustedclaw_telemetry::SpanKind::ToolExecution,
                                &tc.name,
                            );
                            span.duration_ms = Some(duration_ms);
                            span.end(tool_result.success);
                            telemetry.record_span(tid, span);
                        }

                        conversation.push(Message::tool_result(&tc.id, &tool_result.output));
                    }
                    Err(e) => {
                        let error_msg = format!("Error: {}", e);
                        wm.add_observation(&error_msg);
                        wm.add_tool_result(&tc.name, &tc.arguments, &error_msg, false);

                        self.event_bus.publish(DomainEvent::ToolExecuted {
                            tool_name: tc.name.clone(),
                            success: false,
                            duration_ms,
                            timestamp: chrono::Utc::now(),
                        });

                        // Record failed tool span in telemetry
                        if let (Some(telemetry), Some(tid)) = (&self.telemetry, &trace_id) {
                            let mut span = rustedclaw_telemetry::Span::new(
                                rustedclaw_telemetry::SpanKind::ToolExecution,
                                &tc.name,
                            );
                            span.duration_ms = Some(duration_ms);
                            span.end(false);
                            telemetry.record_span(tid, span);
                        }

                        conversation.push(Message::tool_result(&tc.id, &error_msg));
                    }
                }
            }
        }

        // Max iterations exceeded — return partial result.
        // ── End telemetry trace ──
        if let (Some(telemetry), Some(tid)) = (&self.telemetry, &trace_id) {
            telemetry.end_trace(tid);
        }

        let answer =
            "I've reached the maximum number of reasoning iterations. Here's what I found so far."
                .to_string();

        Ok(ReactResult {
            answer,
            trace: wm.trace.clone(),
            working_memory: wm,
            iterations: self.max_iterations as usize,
            tool_calls_made: total_tool_calls,
            last_context_metadata: last_metadata,
        })
    }

    /// Streaming variant of [`run`].
    ///
    /// Returns an `mpsc::Receiver` that yields `AgentStreamEvent`s as the
    /// ReAct loop progresses.  The receiver is populated by a background
    /// task — the caller simply reads from it.
    pub async fn run_stream(
        &self,
        user_message: &str,
        conversation: &mut Conversation,
        memories: &[MemoryEntry],
        knowledge_chunks: &[KnowledgeChunk],
    ) -> Result<mpsc::Receiver<crate::stream_event::AgentStreamEvent>, rustedclaw_core::Error> {
        use crate::stream_event::AgentStreamEvent;

        let (tx, rx) = mpsc::channel::<AgentStreamEvent>(128);

        // ── Prepare everything we need to move into the spawned task ──
        let provider = self.provider.clone();
        let model = self.model.clone();
        let temperature = self.temperature;
        let max_tokens = self.max_tokens;
        let tools = self.tools.clone();
        let identity = self.identity.clone();
        let budget = self.budget.clone();
        let max_iterations = self.max_iterations;
        let event_bus = self.event_bus.clone();
        let memory = self.memory.clone();
        let auto_save = self.auto_save;
        let recall_limit = self.recall_limit;
        let telemetry = self.telemetry.clone();
        let user_msg = user_message.to_string();
        let mut conv = conversation.clone();
        let memories = memories.to_vec();
        let knowledge_chunks = knowledge_chunks.to_vec();

        tokio::spawn(async move {
            let mut wm = WorkingMemory::new(max_iterations as usize);
            let assembler = ContextAssembler::new(budget);
            let tool_defs = tools.definitions();
            let mut total_tool_calls = 0usize;
            let conv_id = conv.id.to_string();

            // ── Start telemetry trace ──
            let trace_id = telemetry.as_ref().map(|t| t.start_trace(&conv_id));

            // ── Auto-recall memories ──
            let recalled: Vec<MemoryEntry> = if let Some(mem) = &memory {
                mem.search(MemoryQuery {
                    text: user_msg.clone(),
                    limit: recall_limit,
                    min_score: 0.0,
                    tags: vec![],
                    mode: SearchMode::Hybrid,
                })
                .await
                .unwrap_or_default()
            } else {
                vec![]
            };

            let all_memories: Vec<MemoryEntry> = if recalled.is_empty() {
                memories.clone()
            } else {
                let mut combined = memories.clone();
                for r in recalled {
                    if !combined.iter().any(|m| m.id == r.id) {
                        combined.push(r);
                    }
                }
                combined
            };

            let mut last_usage = None;

            loop {
                if !wm.tick() {
                    warn!("ReAct stream: max iterations reached");
                    break;
                }

                // ── Assemble context ──
                let input = AssemblyInput {
                    identity: &identity,
                    memories: &all_memories,
                    working_memory: &wm,
                    knowledge_chunks: &knowledge_chunks,
                    tool_definitions: &tool_defs,
                    conversation: &conv,
                    user_message: &user_msg,
                };

                let assembled = match assembler.assemble(&input) {
                    Ok(a) => a,
                    Err(e) => {
                        let _ = tx
                            .send(AgentStreamEvent::Error {
                                message: format!("Context assembly failed: {}", e),
                            })
                            .await;
                        return;
                    }
                };

                let mut messages = vec![Message::system(&assembled.system_message)];
                messages.extend(assembled.messages);

                let request = ProviderRequest {
                    model: model.clone(),
                    messages,
                    temperature,
                    max_tokens,
                    tools: assembled.tool_definitions,
                    stream: true,
                    stop: vec![],
                };

                // ── Stream from provider ──
                let llm_start = std::time::Instant::now();
                let mut stream_rx = match provider.stream(request).await {
                    Ok(rx) => rx,
                    Err(e) => {
                        let _ = tx
                            .send(AgentStreamEvent::Error {
                                message: format!("Provider error: {}", e),
                            })
                            .await;
                        return;
                    }
                };

                // Accumulate the full response from streaming chunks
                let mut full_content = String::new();
                let mut accumulated_tool_calls: Vec<rustedclaw_core::message::MessageToolCall> =
                    Vec::new();

                while let Some(chunk_result) = stream_rx.recv().await {
                    match chunk_result {
                        Ok(chunk) => {
                            // Forward text chunks to client
                            if let Some(ref text) = chunk.content
                                && !text.is_empty()
                            {
                                full_content.push_str(text);
                                let _ = tx
                                    .send(AgentStreamEvent::Chunk {
                                        content: text.clone(),
                                    })
                                    .await;
                            }

                            // Accumulate tool calls
                            for tc in &chunk.tool_calls {
                                // Merge or add tool call deltas
                                if let Some(existing) =
                                    accumulated_tool_calls.iter_mut().find(|t| t.id == tc.id)
                                {
                                    existing.arguments.push_str(&tc.arguments);
                                } else {
                                    accumulated_tool_calls.push(tc.clone());
                                }
                            }

                            if let Some(usage) = chunk.usage {
                                last_usage = Some(usage);
                            }
                        }
                        Err(e) => {
                            let _ = tx
                                .send(AgentStreamEvent::Error {
                                    message: format!("Stream error: {}", e),
                                })
                                .await;
                            return;
                        }
                    }
                }

                let llm_duration_ms = llm_start.elapsed().as_millis() as u64;

                // Record telemetry span for this LLM call
                if let (Some(telem), Some(tid)) = (&telemetry, &trace_id)
                    && let Some(ref usage) = last_usage
                {
                    let cost =
                        telem.compute_cost(&model, usage.prompt_tokens, usage.completion_tokens);
                    let mut span = rustedclaw_telemetry::Span::new(
                        rustedclaw_telemetry::SpanKind::LlmCall,
                        &model,
                    );
                    span.record_tokens(usage.prompt_tokens, usage.completion_tokens, cost);
                    span.duration_ms = Some(llm_duration_ms);
                    span.end(true);
                    telem.record_span(tid, span);
                }

                // Record thought
                if !full_content.is_empty() {
                    wm.add_thought(&full_content);
                }

                // ── Check for final answer ──
                if accumulated_tool_calls.is_empty() {
                    let mut msg = Message::assistant(&full_content);
                    msg.tool_calls = vec![];
                    conv.push(msg);

                    // Auto-save
                    if auto_save
                        && let Some(mem) = &memory
                        && user_msg.len() >= 10
                        && full_content.len() >= 10
                    {
                        let summary = format!(
                            "User asked: {}\nAssistant answered: {}",
                            user_msg, full_content
                        );
                        let entry = MemoryEntry {
                            id: String::new(),
                            content: summary,
                            tags: vec![
                                "conversation".into(),
                                "auto-saved".into(),
                                "react-stream".into(),
                            ],
                            source: Some(conv_id.clone()),
                            created_at: Utc::now(),
                            last_accessed: Utc::now(),
                            score: 0.0,
                            embedding: None,
                        };
                        let _ = mem.store(entry).await;
                    }

                    // ── End telemetry trace ──
                    if let (Some(telem), Some(tid)) = (&telemetry, &trace_id) {
                        telem.end_trace(tid);
                    }

                    let _ = tx
                        .send(AgentStreamEvent::Done {
                            conversation_id: conv_id,
                            usage: last_usage,
                            iterations: wm.iterations,
                            tool_calls_made: total_tool_calls,
                        })
                        .await;
                    return;
                }

                // ── Execute tool calls ──
                let tool_calls_vec = accumulated_tool_calls.clone();
                let mut assistant_msg = Message::assistant(&full_content);
                assistant_msg.tool_calls = tool_calls_vec.clone();
                conv.push(assistant_msg);

                for tc in &tool_calls_vec {
                    total_tool_calls += 1;
                    wm.add_action(&format!("{}({})", tc.name, tc.arguments));

                    // Emit tool_call event
                    let _ = tx
                        .send(AgentStreamEvent::ToolCall {
                            id: tc.id.clone(),
                            name: tc.name.clone(),
                            input: serde_json::from_str(&tc.arguments).unwrap_or_default(),
                        })
                        .await;

                    let call = ToolCall {
                        id: tc.id.clone(),
                        name: tc.name.clone(),
                        arguments: serde_json::from_str(&tc.arguments).unwrap_or_default(),
                    };

                    let start = std::time::Instant::now();
                    let result = tools.execute(&call).await;
                    let duration_ms = start.elapsed().as_millis() as u64;

                    match result {
                        Ok(tool_result) => {
                            wm.add_observation(&tool_result.output);
                            wm.add_tool_result(
                                &tc.name,
                                &tc.arguments,
                                &tool_result.output,
                                tool_result.success,
                            );

                            event_bus.publish(DomainEvent::ToolExecuted {
                                tool_name: tc.name.clone(),
                                success: tool_result.success,
                                duration_ms,
                                timestamp: Utc::now(),
                            });

                            // Record tool span in telemetry
                            if let (Some(telem), Some(tid)) = (&telemetry, &trace_id) {
                                let mut span = rustedclaw_telemetry::Span::new(
                                    rustedclaw_telemetry::SpanKind::ToolExecution,
                                    &tc.name,
                                );
                                span.duration_ms = Some(duration_ms);
                                span.end(tool_result.success);
                                telem.record_span(tid, span);
                            }

                            let _ = tx
                                .send(AgentStreamEvent::ToolResult {
                                    id: tc.id.clone(),
                                    name: tc.name.clone(),
                                    output: tool_result.output.clone(),
                                    success: tool_result.success,
                                })
                                .await;

                            conv.push(Message::tool_result(&tc.id, &tool_result.output));
                        }
                        Err(e) => {
                            let error_msg = format!("Error: {}", e);
                            wm.add_observation(&error_msg);
                            wm.add_tool_result(&tc.name, &tc.arguments, &error_msg, false);

                            event_bus.publish(DomainEvent::ToolExecuted {
                                tool_name: tc.name.clone(),
                                success: false,
                                duration_ms,
                                timestamp: Utc::now(),
                            });

                            // Record failed tool span in telemetry
                            if let (Some(telem), Some(tid)) = (&telemetry, &trace_id) {
                                let mut span = rustedclaw_telemetry::Span::new(
                                    rustedclaw_telemetry::SpanKind::ToolExecution,
                                    &tc.name,
                                );
                                span.duration_ms = Some(duration_ms);
                                span.end(false);
                                telem.record_span(tid, span);
                            }

                            let _ = tx
                                .send(AgentStreamEvent::ToolResult {
                                    id: tc.id.clone(),
                                    name: tc.name.clone(),
                                    output: error_msg.clone(),
                                    success: false,
                                })
                                .await;

                            conv.push(Message::tool_result(&tc.id, &error_msg));
                        }
                    }
                }
            }

            // Max iterations exceeded
            // ── End telemetry trace ──
            if let (Some(telem), Some(tid)) = (&telemetry, &trace_id) {
                telem.end_trace(tid);
            }

            let _ = tx
                .send(AgentStreamEvent::Done {
                    conversation_id: conv_id,
                    usage: last_usage,
                    iterations: max_iterations as usize,
                    tool_calls_made: total_tool_calls,
                })
                .await;
        });

        Ok(rx)
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::working_memory::TraceKind;
    use crate::patterns::test_helpers::*;

    fn setup_react() -> (ReactAgent, Conversation) {
        let tools = Arc::new(rustedclaw_tools::default_registry());
        let event_bus = Arc::new(EventBus::default());
        let agent = ReactAgent::new(
            Arc::new(SequentialMockProvider::single_text("Final answer")),
            "mock-model",
            0.7,
            tools,
            Identity::default(),
            event_bus,
        );
        let conv = Conversation::new();
        (agent, conv)
    }

    #[tokio::test]
    async fn simple_text_response() {
        let (agent, mut conv) = setup_react();

        let result = agent.run("Hello", &mut conv, &[], &[]).await.unwrap();
        assert_eq!(result.answer, "Final answer");
        assert_eq!(result.iterations, 1);
        assert_eq!(result.tool_calls_made, 0);
    }

    #[tokio::test]
    async fn thought_action_observation_trace() {
        let tool_calls = vec![make_tool_call(
            "calculator",
            serde_json::json!({"expression": "2 + 3"}),
        )];

        let provider = Arc::new(SequentialMockProvider::tool_then_answer(
            tool_calls,
            "I need to calculate 2 + 3",
            "The result is 5",
        ));

        let tools = Arc::new(rustedclaw_tools::default_registry());
        let event_bus = Arc::new(EventBus::default());
        let agent = ReactAgent::new(
            provider,
            "mock-model",
            0.7,
            tools,
            Identity::default(),
            event_bus,
        );

        let mut conv = Conversation::new();
        let result = agent
            .run("What is 2+3?", &mut conv, &[], &[])
            .await
            .unwrap();

        assert_eq!(result.answer, "The result is 5");
        assert_eq!(result.tool_calls_made, 1);

        // Verify trace: Thought → Action → Observation → Thought (final)
        assert!(result.trace.len() >= 3);
        assert_eq!(result.trace[0].kind, TraceKind::Thought);
        assert!(result.trace[0].content.contains("calculate"));
        assert_eq!(result.trace[1].kind, TraceKind::Action);
        assert!(result.trace[1].content.contains("calculator"));
        assert_eq!(result.trace[2].kind, TraceKind::Observation);
        assert!(result.trace[2].content.contains("5"));
    }

    #[tokio::test]
    async fn multiple_tool_calls() {
        let tool_calls = vec![
            make_tool_call("calculator", serde_json::json!({"expression": "10 * 5"})),
            make_tool_call("weather_lookup", serde_json::json!({"location": "Tokyo"})),
        ];

        let provider = Arc::new(SequentialMockProvider::tool_then_answer(
            tool_calls,
            "I need to calculate and check weather",
            "Done: 50, and Tokyo weather checked.",
        ));

        let tools = Arc::new(rustedclaw_tools::default_registry());
        let event_bus = Arc::new(EventBus::default());
        let agent = ReactAgent::new(
            provider,
            "mock-model",
            0.7,
            tools,
            Identity::default(),
            event_bus,
        );

        let mut conv = Conversation::new();
        let result = agent
            .run(
                "Calculate 10*5 and check Tokyo weather",
                &mut conv,
                &[],
                &[],
            )
            .await
            .unwrap();

        assert_eq!(result.tool_calls_made, 2);
        assert_eq!(result.answer, "Done: 50, and Tokyo weather checked.");

        // Should have tool results in working memory
        assert_eq!(result.working_memory.tool_results.len(), 2);
        assert!(result.working_memory.tool_results[0].success);
        assert!(result.working_memory.tool_results[1].success);
    }

    #[tokio::test]
    async fn max_iterations_respected() {
        // Provider always returns tool calls, never gives final answer.
        let responses: Vec<_> = (0..5)
            .map(|_| {
                make_tool_call_response(
                    vec![make_tool_call(
                        "calculator",
                        serde_json::json!({"expression": "1+1"}),
                    )],
                    "Thinking...",
                )
            })
            .collect();

        let provider = Arc::new(SequentialMockProvider::new(responses));
        let tools = Arc::new(rustedclaw_tools::default_registry());
        let event_bus = Arc::new(EventBus::default());
        let agent = ReactAgent::new(
            provider,
            "mock-model",
            0.7,
            tools,
            Identity::default(),
            event_bus,
        )
        .with_max_iterations(3);

        let mut conv = Conversation::new();
        let result = agent
            .run("Infinite loop", &mut conv, &[], &[])
            .await
            .unwrap();

        assert!(result.answer.contains("maximum"));
        assert_eq!(result.iterations, 3);
    }

    #[tokio::test]
    async fn working_memory_populated() {
        let (agent, mut conv) = setup_react();

        let result = agent.run("Test", &mut conv, &[], &[]).await.unwrap();
        assert!(result.last_context_metadata.is_some());

        let meta = result.last_context_metadata.unwrap();
        assert!(meta.total_tokens > 0);
        assert!(meta.utilization_pct > 0.0);
    }

    #[tokio::test]
    async fn context_assembly_used() {
        let (agent, mut conv) = setup_react();

        let memories = vec![MemoryEntry {
            id: "m1".into(),
            content: "User prefers Celsius".into(),
            tags: vec![],
            source: None,
            created_at: chrono::Utc::now(),
            last_accessed: chrono::Utc::now(),
            score: 0.0,
            embedding: None,
        }];

        let result = agent.run("Test", &mut conv, &memories, &[]).await.unwrap();

        // Context assembly should have included the memory
        let meta = result.last_context_metadata.unwrap();
        let mem_layer = meta
            .per_layer
            .iter()
            .find(|l| l.name == "long_term_memory")
            .unwrap();
        assert_eq!(mem_layer.items_included, 1);
    }

    // ── Streaming tests ──

    #[tokio::test]
    async fn stream_simple_text() {
        use crate::stream_event::AgentStreamEvent;

        let (agent, mut conv) = setup_react();
        let mut rx = agent
            .run_stream("Hello", &mut conv, &[], &[])
            .await
            .unwrap();

        let mut events = vec![];
        while let Some(event) = rx.recv().await {
            events.push(event);
        }

        // Should have at least a Chunk and a Done event
        assert!(
            events.len() >= 2,
            "Expected >=2 events, got {}",
            events.len()
        );

        // Last event should be Done
        match events.last().unwrap() {
            AgentStreamEvent::Done {
                iterations,
                tool_calls_made,
                ..
            } => {
                assert_eq!(*iterations, 1);
                assert_eq!(*tool_calls_made, 0);
            }
            other => panic!("Expected Done, got {:?}", other),
        }

        // Should have at least one Chunk with "Final answer"
        let chunks: Vec<_> = events
            .iter()
            .filter_map(|e| match e {
                AgentStreamEvent::Chunk { content } => Some(content.as_str()),
                _ => None,
            })
            .collect();
        let full_text: String = chunks.concat();
        assert!(
            full_text.contains("Final answer"),
            "Expected 'Final answer', got '{}'",
            full_text
        );
    }

    #[tokio::test]
    async fn stream_with_tool_calls() {
        use crate::stream_event::AgentStreamEvent;

        let tool_calls = vec![make_tool_call(
            "calculator",
            serde_json::json!({"expression": "2 + 3"}),
        )];

        let provider = Arc::new(SequentialMockProvider::tool_then_answer(
            tool_calls,
            "I need to calculate",
            "The result is 5",
        ));

        let tools = Arc::new(rustedclaw_tools::default_registry());
        let event_bus = Arc::new(EventBus::default());
        let agent = ReactAgent::new(
            provider,
            "mock-model",
            0.7,
            tools,
            Identity::default(),
            event_bus,
        );

        let mut conv = Conversation::new();
        let mut rx = agent
            .run_stream("What is 2+3?", &mut conv, &[], &[])
            .await
            .unwrap();

        let mut events = vec![];
        while let Some(event) = rx.recv().await {
            events.push(event);
        }

        // Should contain ToolCall and ToolResult events
        let has_tool_call = events
            .iter()
            .any(|e| matches!(e, AgentStreamEvent::ToolCall { name, .. } if name == "calculator"));
        let has_tool_result = events.iter().any(|e| matches!(e, AgentStreamEvent::ToolResult { name, success, .. } if name == "calculator" && *success));
        let has_done = events.iter().any(|e| matches!(e, AgentStreamEvent::Done { tool_calls_made, .. } if *tool_calls_made == 1));

        assert!(has_tool_call, "Missing ToolCall event");
        assert!(has_tool_result, "Missing ToolResult event");
        assert!(has_done, "Missing Done event");
    }
}
