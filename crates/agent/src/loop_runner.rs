//! The agent reasoning loop implementation.

use std::sync::Arc;
use rustedclaw_core::event::{DomainEvent, EventBus};
use rustedclaw_core::identity::Identity;
use rustedclaw_core::memory::{MemoryBackend, MemoryEntry, MemoryQuery, SearchMode};
use rustedclaw_core::message::{Conversation, Message};
use rustedclaw_core::provider::{Provider, ProviderRequest};
use rustedclaw_core::tool::{ToolCall, ToolRegistry};
use chrono::Utc;
use tracing::{debug, info, warn};

/// The core agent loop that orchestrates LLM calls and tool execution.
pub struct AgentLoop {
    /// The LLM provider to use
    provider: Arc<dyn Provider>,

    /// The model to use
    model: String,

    /// Temperature setting
    temperature: f32,

    /// Default max tokens per response
    max_tokens: Option<u32>,

    /// Tool registry
    tools: Arc<ToolRegistry>,

    /// Agent identity
    identity: Identity,

    /// Maximum tool call iterations per turn
    max_iterations: u32,

    /// Event bus for domain events
    event_bus: Arc<EventBus>,

    /// Optional memory backend for recall and auto-save
    memory: Option<Arc<dyn MemoryBackend>>,

    /// Whether to auto-save conversation summaries to memory
    auto_save: bool,

    /// Maximum memories to recall per turn
    recall_limit: usize,
}

impl AgentLoop {
    /// Create a new agent loop.
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
            max_iterations: 25,
            event_bus,
            memory: None,
            auto_save: false,
            recall_limit: 5,
        }
    }

    /// Set the maximum number of tool call iterations.
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

    /// Recall relevant memories based on the user's last message.
    async fn recall_memories(&self, conversation: &Conversation) -> Vec<MemoryEntry> {
        let Some(memory) = &self.memory else {
            return vec![];
        };

        // Find the last user message
        let user_msg = conversation
            .messages
            .iter()
            .rev()
            .find(|m| m.role == rustedclaw_core::message::Role::User)
            .map(|m| m.content.clone());

        let Some(query_text) = user_msg else {
            return vec![];
        };

        match memory
            .search(MemoryQuery {
                text: query_text,
                limit: self.recall_limit,
                min_score: 0.0,
                tags: vec![],
                mode: SearchMode::Hybrid,
            })
            .await
        {
            Ok(entries) => {
                if !entries.is_empty() {
                    debug!(count = entries.len(), "Recalled memories for context");
                }
                entries
            }
            Err(e) => {
                warn!("Memory recall failed: {e}");
                vec![]
            }
        }
    }

    /// Format recalled memories into a context block for the system prompt.
    fn format_memory_context(memories: &[MemoryEntry]) -> String {
        if memories.is_empty() {
            return String::new();
        }

        let mut ctx = String::from("\n\n## Recalled Memories\n");
        for (i, mem) in memories.iter().enumerate() {
            ctx.push_str(&format!(
                "{}. [score={:.2}] {}\n",
                i + 1,
                mem.score,
                mem.content
            ));
        }
        ctx
    }

    /// Auto-save important conversation content to memory.
    async fn auto_save_to_memory(&self, conversation: &Conversation) {
        let Some(memory) = &self.memory else {
            return;
        };
        if !self.auto_save {
            return;
        }

        // Find the last user message and assistant response
        let messages: Vec<_> = conversation.messages.iter().rev().take(2).collect();
        let (user_msg, assistant_msg) = match messages.as_slice() {
            [assistant, user] if assistant.role == rustedclaw_core::message::Role::Assistant
                && user.role == rustedclaw_core::message::Role::User =>
            {
                (user.content.clone(), assistant.content.clone())
            }
            _ => return,
        };

        // Only save if there's meaningful content (not just short responses)
        if user_msg.len() < 10 || assistant_msg.len() < 10 {
            return;
        }

        let summary = format!("User asked: {}\nAssistant answered: {}", user_msg, assistant_msg);

        let entry = MemoryEntry {
            id: String::new(), // auto-generated
            content: summary,
            tags: vec!["conversation".into(), "auto-saved".into()],
            source: Some(conversation.id.to_string()),
            created_at: Utc::now(),
            last_accessed: Utc::now(),
            score: 0.0,
            embedding: None,
        };

        match memory.store(entry).await {
            Ok(id) => debug!(memory_id = %id, "Auto-saved conversation to memory"),
            Err(e) => warn!("Failed to auto-save to memory: {e}"),
        }
    }

    /// Process a user message and generate a response.
    ///
    /// This is the main entry point for the agent loop. It:
    /// 1. Builds the conversation context with system prompt
    /// 2. Calls the LLM
    /// 3. If tool calls are returned, executes them and loops
    /// 4. Returns the final text response
    pub async fn process(
        &self,
        conversation: &mut Conversation,
    ) -> Result<String, rustedclaw_core::Error> {
        info!(
            conversation_id = %conversation.id,
            messages = conversation.messages.len(),
            "Processing conversation"
        );

        // ── Memory recall ──
        let recalled = self.recall_memories(conversation).await;
        let memory_context = Self::format_memory_context(&recalled);

        // Build system prompt with recalled memories
        let system_prompt = if memory_context.is_empty() {
            self.identity.system_prompt.clone()
        } else {
            format!("{}{}", self.identity.system_prompt, memory_context)
        };

        // Ensure system prompt is the first message
        if conversation.messages.is_empty()
            || conversation.messages[0].role != rustedclaw_core::message::Role::System
        {
            conversation.messages.insert(0, Message::system(&system_prompt));
        } else {
            // Update existing system prompt with recalled memories
            conversation.messages[0] = Message::system(&system_prompt);
        }

        let tool_definitions = self.tools.definitions();
        let mut iteration = 0;

        loop {
            iteration += 1;

            if iteration > self.max_iterations {
                warn!(
                    conversation_id = %conversation.id,
                    iterations = iteration,
                    "Max tool iterations reached, forcing text response"
                );
                break;
            }

            debug!(
                conversation_id = %conversation.id,
                iteration = iteration,
                "Agent loop iteration"
            );

            // Build the provider request
            let request = ProviderRequest {
                model: self.model.clone(),
                messages: conversation.messages.clone(),
                temperature: self.temperature,
                max_tokens: self.max_tokens,
                tools: tool_definitions.clone(),
                stream: false,
                stop: vec![],
            };

            // Call the LLM
            let response = self.provider.complete(request).await?;

            // Track token usage
            if let Some(usage) = &response.usage {
                self.event_bus.publish(DomainEvent::ResponseGenerated {
                    conversation_id: conversation.id.to_string(),
                    model: response.model.clone(),
                    tokens_used: usage.total_tokens,
                    timestamp: chrono::Utc::now(),
                });
            }

            // Check if the LLM wants to call tools
            if response.message.tool_calls.is_empty() {
                // No tool calls — this is the final text response
                let response_text = response.message.content.clone();
                conversation.push(response.message);

                // ── Auto-save to memory ──
                self.auto_save_to_memory(conversation).await;

                return Ok(response_text);
            }

            // LLM wants to call tools — execute them
            debug!(
                tool_count = response.message.tool_calls.len(),
                "Executing tool calls"
            );

            // Add the assistant message (with tool calls) to conversation
            let tool_calls = response.message.tool_calls.clone();
            conversation.push(response.message);

            // Execute each tool call
            for tc in &tool_calls {
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
                        self.event_bus.publish(DomainEvent::ToolExecuted {
                            tool_name: tc.name.clone(),
                            success: tool_result.success,
                            duration_ms,
                            timestamp: chrono::Utc::now(),
                        });

                        // Add tool result to conversation
                        conversation.push(Message::tool_result(&tc.id, &tool_result.output));
                    }
                    Err(e) => {
                        warn!(tool = %tc.name, error = %e, "Tool execution failed");

                        self.event_bus.publish(DomainEvent::ToolExecuted {
                            tool_name: tc.name.clone(),
                            success: false,
                            duration_ms,
                            timestamp: chrono::Utc::now(),
                        });

                        // Report error to the LLM so it can recover
                        conversation.push(Message::tool_result(
                            &tc.id,
                            &format!("Error: {e}"),
                        ));
                    }
                }
            }

            // Loop back — the LLM will see the tool results and decide what to do next
        }

        // If we hit max iterations, return whatever we have
        self.auto_save_to_memory(conversation).await;
        Ok("I've reached the maximum number of tool call iterations. Please provide further guidance.".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustedclaw_core::error::ProviderError;
    use rustedclaw_core::provider::{ProviderResponse, Usage};

    /// A mock provider that returns a fixed response.
    struct MockProvider {
        response: String,
    }

    #[async_trait::async_trait]
    impl Provider for MockProvider {
        fn name(&self) -> &str { "mock" }

        async fn complete(
            &self,
            _request: ProviderRequest,
        ) -> Result<ProviderResponse, ProviderError> {
            Ok(ProviderResponse {
                message: Message::assistant(&self.response),
                usage: Some(Usage {
                    prompt_tokens: 10,
                    completion_tokens: 5,
                    total_tokens: 15,
                }),
                model: "mock-model".into(),
                metadata: serde_json::Map::new(),
            })
        }
    }

    #[tokio::test]
    async fn simple_text_response() {
        let provider = Arc::new(MockProvider {
            response: "Hello! How can I help?".into(),
        });
        let tools = Arc::new(ToolRegistry::new());
        let event_bus = Arc::new(EventBus::default());

        let agent = AgentLoop::new(
            provider,
            "mock-model",
            0.7,
            tools,
            Identity::default(),
            event_bus,
        );

        let mut conv = Conversation::new();
        conv.push(Message::user("Hello!"));

        let response = agent.process(&mut conv).await.unwrap();
        assert_eq!(response, "Hello! How can I help?");
        // System + User + Assistant = 3 messages
        assert_eq!(conv.messages.len(), 3);
    }

    #[tokio::test]
    async fn memory_recall_injects_context() {
        use rustedclaw_memory::InMemoryBackend;
        use rustedclaw_core::memory::MemoryBackend;

        let mem = Arc::new(InMemoryBackend::new());
        // Pre-store a memory
        mem.store(MemoryEntry {
            id: String::new(),
            content: "The user's favorite color is blue".into(),
            tags: vec![],
            source: None,
            created_at: Utc::now(),
            last_accessed: Utc::now(),
            score: 0.0,
            embedding: None,
        }).await.unwrap();

        let provider = Arc::new(MockProvider {
            response: "Your favorite color is blue!".into(),
        });
        let tools = Arc::new(ToolRegistry::new());
        let event_bus = Arc::new(EventBus::default());

        let agent = AgentLoop::new(
            provider,
            "mock-model",
            0.7,
            tools,
            Identity::default(),
            event_bus,
        )
        .with_memory(mem);

        let mut conv = Conversation::new();
        conv.push(Message::user("favorite color"));

        let response = agent.process(&mut conv).await.unwrap();
        assert_eq!(response, "Your favorite color is blue!");

        // System prompt should contain recalled memory
        let system_msg = &conv.messages[0].content;
        assert!(system_msg.contains("favorite color is blue"), "System prompt should contain recalled memory: {system_msg}");
    }

    #[tokio::test]
    async fn auto_save_stores_conversation() {
        use rustedclaw_memory::InMemoryBackend;
        use rustedclaw_core::memory::MemoryBackend;

        let mem = Arc::new(InMemoryBackend::new());

        let provider = Arc::new(MockProvider {
            response: "Rust is a systems programming language known for safety and performance.".into(),
        });
        let tools = Arc::new(ToolRegistry::new());
        let event_bus = Arc::new(EventBus::default());

        let agent = AgentLoop::new(
            provider,
            "mock-model",
            0.7,
            tools,
            Identity::default(),
            event_bus,
        )
        .with_memory(mem.clone())
        .with_auto_save(true);

        let mut conv = Conversation::new();
        conv.push(Message::user("Tell me about Rust programming language"));

        let _response = agent.process(&mut conv).await.unwrap();

        // Memory should now contain the auto-saved conversation
        let count = mem.count().await.unwrap();
        assert_eq!(count, 1, "Auto-save should have stored one memory");

        let results = mem.search(MemoryQuery {
            text: "Rust".into(),
            limit: 10,
            min_score: 0.0,
            tags: vec![],
            mode: SearchMode::Keyword,
        }).await.unwrap();

        assert_eq!(results.len(), 1);
        assert!(results[0].content.contains("Rust"));
        assert!(results[0].tags.contains(&"auto-saved".to_string()));
    }

    #[tokio::test]
    async fn no_auto_save_without_flag() {
        use rustedclaw_memory::InMemoryBackend;
        use rustedclaw_core::memory::MemoryBackend;

        let mem = Arc::new(InMemoryBackend::new());

        let provider = Arc::new(MockProvider {
            response: "Some response that is long enough to save.".into(),
        });
        let tools = Arc::new(ToolRegistry::new());
        let event_bus = Arc::new(EventBus::default());

        let agent = AgentLoop::new(
            provider,
            "mock-model",
            0.7,
            tools,
            Identity::default(),
            event_bus,
        )
        .with_memory(mem.clone()); // auto_save defaults to false

        let mut conv = Conversation::new();
        conv.push(Message::user("Tell me about Rust programming language"));

        let _response = agent.process(&mut conv).await.unwrap();

        // No auto-save when flag is false
        let count = mem.count().await.unwrap();
        assert_eq!(count, 0, "Should not auto-save when auto_save is false");
    }

    #[tokio::test]
    async fn format_memory_context_empty() {
        let ctx = AgentLoop::format_memory_context(&[]);
        assert!(ctx.is_empty());
    }

    #[tokio::test]
    async fn format_memory_context_with_entries() {
        let entries = vec![
            MemoryEntry {
                id: "1".into(),
                content: "User likes Rust".into(),
                tags: vec![],
                source: None,
                created_at: Utc::now(),
                last_accessed: Utc::now(),
                score: 0.95,
                embedding: None,
            },
        ];
        let ctx = AgentLoop::format_memory_context(&entries);
        assert!(ctx.contains("Recalled Memories"));
        assert!(ctx.contains("User likes Rust"));
        assert!(ctx.contains("0.95"));
    }
}
