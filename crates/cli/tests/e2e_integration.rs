//! End-to-end integration tests for the RustedClaw AI agent runtime.
//!
//! These tests exercise the full pipeline from user input to agent output,
//! including context assembly, tool execution, and pattern-based reasoning.

use std::sync::Arc;

use rustedclaw_agent::{
    AssemblyInput, ContextAssembler, CoordinatorAgent, KnowledgeChunk, RagAgent, ReactAgent,
    TokenBudget, WorkingMemory,
};
use rustedclaw_core::error::ProviderError;
use rustedclaw_core::event::EventBus;
use rustedclaw_core::identity::Identity;
use rustedclaw_core::memory::MemoryEntry;
use rustedclaw_core::message::{Conversation, Message, MessageToolCall};
use rustedclaw_core::provider::{Provider, ProviderRequest, ProviderResponse, Usage};
use rustedclaw_tools::default_registry;

// ── Mock Provider ────────────────────────────────────────────────────────

/// A mock provider that returns scripted responses in sequence.
struct ScriptedProvider {
    responses: std::sync::Mutex<Vec<ProviderResponse>>,
    call_count: std::sync::Mutex<usize>,
}

impl ScriptedProvider {
    fn new(responses: Vec<ProviderResponse>) -> Self {
        Self {
            responses: std::sync::Mutex::new(responses),
            call_count: std::sync::Mutex::new(0),
        }
    }

    fn text(response: &str) -> Self {
        Self::new(vec![text_response(response)])
    }

    fn tool_then_text(tool_calls: Vec<MessageToolCall>, thought: &str, answer: &str) -> Self {
        Self::new(vec![
            tool_response(tool_calls, thought),
            text_response(answer),
        ])
    }

    fn calls(&self) -> usize {
        *self.call_count.lock().unwrap()
    }
}

#[async_trait::async_trait]
impl Provider for ScriptedProvider {
    fn name(&self) -> &str {
        "e2e_mock"
    }

    async fn complete(&self, _request: ProviderRequest) -> Result<ProviderResponse, ProviderError> {
        let mut count = self.call_count.lock().unwrap();
        let responses = self.responses.lock().unwrap();
        if *count >= responses.len() {
            panic!(
                "ScriptedProvider exhausted: call #{}, have {}",
                *count,
                responses.len()
            );
        }
        let resp = responses[*count].clone();
        *count += 1;
        Ok(resp)
    }
}

fn text_response(text: &str) -> ProviderResponse {
    ProviderResponse {
        message: Message::assistant(text),
        usage: Some(Usage {
            prompt_tokens: 10,
            completion_tokens: 5,
            total_tokens: 15,
        }),
        model: "mock".into(),
        metadata: serde_json::Map::new(),
    }
}

fn tool_response(tool_calls: Vec<MessageToolCall>, thought: &str) -> ProviderResponse {
    let mut msg = Message::assistant(thought);
    msg.tool_calls = tool_calls;
    ProviderResponse {
        message: msg,
        usage: Some(Usage {
            prompt_tokens: 10,
            completion_tokens: 5,
            total_tokens: 15,
        }),
        model: "mock".into(),
        metadata: serde_json::Map::new(),
    }
}

fn make_tool_call(name: &str, args: serde_json::Value) -> MessageToolCall {
    MessageToolCall {
        id: format!("call_{name}"),
        name: name.to_string(),
        arguments: serde_json::to_string(&args).unwrap(),
    }
}

// ── E2E: Full ReAct Pipeline ─────────────────────────────────────────────

#[tokio::test]
async fn e2e_react_calculator_tool_invocation() {
    // Scenario: User asks "what is 2+2?", agent decides to use calculator,
    // then provides final answer.
    let provider = Arc::new(ScriptedProvider::tool_then_text(
        vec![make_tool_call(
            "calculator",
            serde_json::json!({"expression": "2 + 2"}),
        )],
        "I need to calculate 2+2",
        "The answer is 4.",
    ));

    let tools = Arc::new(default_registry());
    let identity = Identity::default();
    let event_bus = Arc::new(EventBus::default());
    let mut conv = Conversation::new();

    let agent = ReactAgent::new(provider.clone(), "mock", 0.7, tools, identity, event_bus);

    let result = agent
        .run("what is 2+2?", &mut conv, &[], &[])
        .await
        .expect("Agent should succeed");

    assert_eq!(result.answer, "The answer is 4.");
    assert_eq!(result.tool_calls_made, 1);
    assert!(result.iterations <= 5);

    // Verify trace has thought → action → observation → final answer.
    assert!(result.trace.len() >= 3);
    assert!(provider.calls() == 2); // tool call + final answer

    // Conversation should have messages.
    assert!(!conv.messages.is_empty());
}

#[tokio::test]
async fn e2e_react_web_search_then_answer() {
    let provider = Arc::new(ScriptedProvider::tool_then_text(
        vec![make_tool_call(
            "web_search",
            serde_json::json!({"query": "rust programming language", "num_results": 3}),
        )],
        "Let me search for information about Rust",
        "Rust is a systems programming language focused on safety and performance.",
    ));

    let tools = Arc::new(default_registry());
    let identity = Identity::default();
    let event_bus = Arc::new(EventBus::default());
    let mut conv = Conversation::new();

    let agent = ReactAgent::new(provider.clone(), "mock", 0.7, tools, identity, event_bus);

    let result = agent
        .run("Tell me about Rust", &mut conv, &[], &[])
        .await
        .expect("Agent should succeed");

    assert!(result.answer.contains("Rust"));
    assert_eq!(result.tool_calls_made, 1);
}

#[tokio::test]
async fn e2e_react_with_long_term_memories() {
    let provider = Arc::new(ScriptedProvider::text(
        "Based on your preference for dark themes, I recommend using a dark color scheme.",
    ));

    let tools = Arc::new(default_registry());
    let identity = Identity::default();
    let event_bus = Arc::new(EventBus::default());
    let mut conv = Conversation::new();

    let memories = vec![MemoryEntry {
        id: "mem_1".into(),
        content: "User prefers dark themes".into(),
        tags: vec!["preference".into()],
        source: None,
        created_at: chrono::Utc::now(),
        last_accessed: chrono::Utc::now(),
        score: 0.9,
        embedding: None,
    }];

    let agent = ReactAgent::new(provider.clone(), "mock", 0.7, tools, identity, event_bus);

    let result = agent
        .run("What theme should I use?", &mut conv, &memories, &[])
        .await
        .expect("Agent should succeed");

    assert!(result.answer.contains("dark"));

    // Verify context assembly included the memory.
    assert!(result.last_context_metadata.is_some());
    let meta = result.last_context_metadata.unwrap();
    assert!(meta.total_tokens > 0);
}

#[tokio::test]
async fn e2e_react_direct_answer_no_tools() {
    let provider = Arc::new(ScriptedProvider::text("Hello! How can I help you today?"));

    let tools = Arc::new(default_registry());
    let identity = Identity::default();
    let event_bus = Arc::new(EventBus::default());
    let mut conv = Conversation::new();

    let agent = ReactAgent::new(provider.clone(), "mock", 0.7, tools, identity, event_bus);

    let result = agent
        .run("Hi there!", &mut conv, &[], &[])
        .await
        .expect("Agent should succeed");

    assert_eq!(result.answer, "Hello! How can I help you today?");
    assert_eq!(result.tool_calls_made, 0);
    assert_eq!(result.iterations, 1);
    assert_eq!(provider.calls(), 1);
}

// ── E2E: Full RAG Pipeline ──────────────────────────────────────────────

#[tokio::test]
async fn e2e_rag_knowledge_grounded_response() {
    // RAG agent: retrieves knowledge via knowledge_base_query tool, then generates.
    let provider = Arc::new(ScriptedProvider::new(vec![
        // First call: the RAG agent makes the knowledge_base_query tool call internally
        // by calling the tool directly, then assembles context and generates.
        text_response(
            "Based on the retrieved knowledge, Rust's ownership system ensures memory safety without garbage collection.",
        ),
    ]));

    let tools = Arc::new(default_registry());
    let identity = Identity::default();
    let event_bus = Arc::new(EventBus::default());
    let mut conv = Conversation::new();

    let agent = RagAgent::new(provider.clone(), "mock", 0.7, tools, identity, event_bus);

    let result = agent
        .run("How does Rust handle memory?", &mut conv, &[])
        .await
        .expect("RAG agent should succeed");

    assert!(!result.answer.is_empty());
    assert!(!result.retrieved_chunks.is_empty());
    assert_eq!(result.retrieval_query, "How does Rust handle memory?");

    // Working memory should record the retrieval.
    assert!(!result.working_memory.trace.is_empty());
}

#[tokio::test]
async fn e2e_rag_with_existing_memories() {
    let provider = Arc::new(ScriptedProvider::text(
        "Considering your background in Python and the retrieved Rust documentation, here's a comparison...",
    ));

    let tools = Arc::new(default_registry());
    let identity = Identity::default();
    let event_bus = Arc::new(EventBus::default());
    let mut conv = Conversation::new();

    let memories = vec![MemoryEntry {
        id: "mem_bg".into(),
        content: "User has experience with Python".into(),
        tags: vec!["background".into()],
        source: None,
        created_at: chrono::Utc::now(),
        last_accessed: chrono::Utc::now(),
        score: 0.8,
        embedding: None,
    }];

    let agent = RagAgent::new(provider.clone(), "mock", 0.7, tools, identity, event_bus);

    let result = agent
        .run("Compare Rust and Python", &mut conv, &memories)
        .await
        .expect("RAG agent should succeed");

    assert!(result.answer.contains("Python"));
    // Context metadata should show knowledge layer was populated.
    assert!(result.context_metadata.is_some());
}

// ── E2E: Multi-Agent Coordinator ────────────────────────────────────────

#[tokio::test]
async fn e2e_coordinator_decomposes_and_delegates() {
    // Coordinator with 2 workers: "researcher" and "writer".
    // Coordinator decomposes, delegates, aggregates.
    let provider = Arc::new(ScriptedProvider::new(vec![
        // Coordinator decomposition response (must use "WORKER: task" format per parser).
        text_response("researcher: Research Rust advantages\nwriter: Write summary"),
        // Worker 1 (researcher) response.
        text_response(
            "Rust advantages: memory safety, zero-cost abstractions, fearless concurrency.",
        ),
        // Worker 2 (writer) response.
        text_response(
            "Rust is a modern systems programming language offering memory safety without garbage collection.",
        ),
        // Coordinator aggregation response.
        text_response(
            "Rust offers memory safety, zero-cost abstractions, and fearless concurrency. It is a modern systems programming language that achieves memory safety without garbage collection.",
        ),
    ]));

    let tools = Arc::new(default_registry());
    let identity = Identity::default();
    let event_bus = Arc::new(EventBus::default());
    let _conv = Conversation::new();

    let agent = CoordinatorAgent::new(provider.clone(), "mock", 0.7, tools, identity, event_bus)
        .add_worker("researcher", "Researches topics")
        .add_worker("writer", "Writes summaries");

    let result = agent
        .run("Write a summary of Rust's advantages", &[])
        .await
        .expect("Coordinator should succeed");

    assert!(!result.answer.is_empty());
    assert_eq!(result.sub_results.len(), 2);
    assert!(result.total_iterations >= 2);

    // Working memory should track the coordination.
    assert!(!result.working_memory.trace.is_empty());
}

// ── E2E: Context Assembly Pipeline ──────────────────────────────────────

#[tokio::test]
async fn e2e_context_assembly_full_layers() {
    // Test that all 6 layers assemble correctly.
    let budget = TokenBudget {
        total: 8192,
        ..TokenBudget::default()
    };

    let assembler = ContextAssembler::new(budget);

    let identity = Identity {
        system_prompt: "You are a helpful AI assistant.".into(),
        ..Identity::default()
    };

    let memories = vec![MemoryEntry {
        id: "m1".into(),
        content: "User is a Rust developer".into(),
        tags: vec!["profile".into()],
        source: None,
        created_at: chrono::Utc::now(),
        last_accessed: chrono::Utc::now(),
        score: 0.9,
        embedding: None,
    }];

    let mut wm = WorkingMemory::default();
    wm.add_thought("The user wants to know about error handling");
    wm.set_plan(
        "Explain Rust error handling",
        vec!["overview".to_string(), "examples".to_string()],
    );

    let chunks = vec![KnowledgeChunk {
        document_id: "doc1".into(),
        chunk_index: 0,
        content: "Rust uses Result<T, E> for error handling.".into(),
        source: "rust-book.md".into(),
        similarity: 0.92,
    }];

    let tools = default_registry();
    let tool_defs = tools.definitions();

    let mut conv = Conversation::new();
    conv.push(Message::user("How does error handling work in Rust?"));

    let input = AssemblyInput {
        identity: &identity,
        memories: &memories,
        working_memory: &wm,
        knowledge_chunks: &chunks,
        tool_definitions: &tool_defs,
        conversation: &conv,
        user_message: "How does error handling work in Rust?",
    };

    let result = assembler.assemble(&input).expect("Assembly should succeed");

    // Verify system message contains all injected sections.
    assert!(result.system_message.contains("helpful AI assistant"));
    assert!(result.system_message.contains("Rust developer"));
    assert!(result.system_message.contains("error handling"));
    assert!(result.system_message.contains("Result<T, E>"));

    // Verify metadata.
    assert!(result.metadata.total_tokens > 0);
    assert!(result.metadata.total_tokens <= 8192);
    assert!(result.metadata.utilization_pct > 0.0);
    assert!(result.metadata.per_layer.len() == 7); // system, memory, wm, knowledge, tools, history, user_message

    // Verify tool definitions are included.
    assert!(!result.tool_definitions.is_empty());

    // Verify conversation messages are included.
    assert!(!result.messages.is_empty());
}

#[tokio::test]
async fn e2e_context_assembly_budget_pressure() {
    // Test that with a very tight budget, layers are properly trimmed.
    let budget = TokenBudget {
        total: 200, // Very tight budget
        ..TokenBudget::default()
    };

    let assembler = ContextAssembler::new(budget);
    let identity = Identity::default();

    let memories: Vec<MemoryEntry> = (0..20)
        .map(|i| MemoryEntry {
            id: format!("m{i}"),
            content: format!("Memory entry number {i} with some extra content to take up tokens"),
            tags: vec![],
            source: None,
            created_at: chrono::Utc::now(),
            last_accessed: chrono::Utc::now(),
            score: 0.5,
            embedding: None,
        })
        .collect();

    let wm = WorkingMemory::default();
    let tools = default_registry();
    let tool_defs = tools.definitions();
    let conv = Conversation::new();

    let input = AssemblyInput {
        identity: &identity,
        memories: &memories,
        working_memory: &wm,
        knowledge_chunks: &[],
        tool_definitions: &tool_defs,
        conversation: &conv,
        user_message: "test",
    };

    let result = assembler.assemble(&input).expect("Assembly should succeed");

    // With tight budget, some layers should have drops.
    assert!(result.metadata.total_tokens <= 200);

    // At least system prompt should be included.
    assert!(!result.system_message.is_empty());
}

// ── E2E: Working Memory Lifecycle ───────────────────────────────────────

#[tokio::test]
async fn e2e_working_memory_full_lifecycle() {
    let mut wm = WorkingMemory::default();

    // Phase 1: Set a plan.
    wm.set_plan(
        "Build a web server",
        vec![
            "Choose framework".to_string(),
            "Implement routes".to_string(),
            "Add middleware".to_string(),
            "Deploy".to_string(),
        ],
    );
    assert!(!wm.is_plan_complete());

    // Phase 2: Record thoughts and actions.
    wm.add_thought("I should use Axum for the web framework");
    wm.add_action("web_search: best Rust web frameworks 2024");
    wm.add_observation("Results: Axum, Actix-Web, Warp");

    // Phase 3: Advance through the plan.
    wm.advance_plan(Some("Done".into())); // Complete "Choose framework"
    wm.tick();
    assert_eq!(wm.iterations, 1);

    wm.add_thought("Now implementing routes");
    wm.advance_plan(Some("Done".into())); // Complete "Implement routes"
    wm.tick();

    // Phase 4: Record a tool result.
    wm.add_tool_result("web_search", "rust web frameworks", "Found 3 results", true);

    // Phase 5: Fail a step and recover.
    wm.fail_plan_step("Middleware integration error");
    wm.add_reflection("Need to fix middleware order");
    wm.advance_plan(Some("Fixed".into())); // Retry and complete "Add middleware"
    wm.tick();

    wm.advance_plan(Some("Deployed".into())); // Complete "Deploy"
    assert!(wm.is_plan_complete());

    // Phase 6: Verify render output.
    let rendered = wm.render();
    assert!(rendered.contains("Build a web server"));
    assert!(rendered.contains("Axum"));
    assert!(rendered.contains("middleware"));

    // Phase 7: Verify summary.
    let summary = wm.summarize();
    assert!(summary.contains("iterations"));

    // Phase 8: Serialization roundtrip.
    let json = serde_json::to_string(&wm).unwrap();
    let deserialized: WorkingMemory = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized.iterations, wm.iterations);
    assert_eq!(deserialized.trace.len(), wm.trace.len());
}

// ── E2E: Tool Registry Full Coverage ────────────────────────────────────

#[tokio::test]
async fn e2e_all_tools_executable() {
    let registry = default_registry();

    // Verify all 7 tools are registered.
    let names = registry.names();
    assert!(names.len() >= 7);

    let expected_tools = [
        "shell",
        "file_read",
        "file_write",
        "web_search",
        "calculator",
        "weather_lookup",
        "knowledge_base_query",
    ];

    for tool_name in &expected_tools {
        assert!(
            registry.get(tool_name).is_some(),
            "Tool '{tool_name}' should be registered"
        );
    }

    // Test calculator.
    let calc_result = registry
        .execute(&rustedclaw_core::tool::ToolCall {
            id: "tc1".into(),
            name: "calculator".into(),
            arguments: serde_json::json!({"expression": "3 * 4 + 5"}),
        })
        .await
        .expect("Calculator should work");
    assert!(calc_result.success);
    assert!(calc_result.output.contains("17"));

    // Test web_search.
    let search_result = registry
        .execute(&rustedclaw_core::tool::ToolCall {
            id: "tc2".into(),
            name: "web_search".into(),
            arguments: serde_json::json!({"query": "rust", "num_results": 2}),
        })
        .await
        .expect("Web search should work");
    assert!(search_result.success);

    // Test weather.
    let weather_result = registry
        .execute(&rustedclaw_core::tool::ToolCall {
            id: "tc3".into(),
            name: "weather_lookup".into(),
            arguments: serde_json::json!({"location": "New York"}),
        })
        .await
        .expect("Weather should work");
    assert!(weather_result.success);
    assert!(weather_result.output.contains("New York"));

    // Test knowledge_base_query.
    let kb_result = registry
        .execute(&rustedclaw_core::tool::ToolCall {
            id: "tc4".into(),
            name: "knowledge_base_query".into(),
            arguments: serde_json::json!({"query": "rust ownership", "top_k": 3}),
        })
        .await
        .expect("KB query should work");
    assert!(kb_result.success);
}

// ── E2E: Memory Backends ────────────────────────────────────────────────

#[tokio::test]
async fn e2e_in_memory_backend_lifecycle() {
    use rustedclaw_core::memory::{MemoryBackend, MemoryQuery, SearchMode};
    use rustedclaw_memory::InMemoryBackend;

    let backend = InMemoryBackend::new();

    // Store entries.
    let id1 = backend
        .store(MemoryEntry {
            id: String::new(),
            content: "Rust is a systems programming language".into(),
            tags: vec!["programming".into(), "rust".into()],
            source: Some("conversation_1".into()),
            created_at: chrono::Utc::now(),
            last_accessed: chrono::Utc::now(),
            score: 0.0,
            embedding: None,
        })
        .await
        .expect("Store should work");

    let id2 = backend
        .store(MemoryEntry {
            id: String::new(),
            content: "Python is great for data science".into(),
            tags: vec!["programming".into(), "python".into()],
            source: Some("conversation_2".into()),
            created_at: chrono::Utc::now(),
            last_accessed: chrono::Utc::now(),
            score: 0.0,
            embedding: None,
        })
        .await
        .expect("Store should work");

    // Count.
    assert_eq!(backend.count().await.unwrap(), 2);

    // Search.
    let results = backend
        .search(MemoryQuery {
            text: "Rust".into(),
            limit: 10,
            min_score: 0.0,
            tags: vec![],
            mode: SearchMode::Keyword,
        })
        .await
        .expect("Search should work");
    assert_eq!(results.len(), 1);
    assert!(results[0].content.contains("Rust"));

    // Get by ID.
    let entry = backend.get(&id1).await.expect("Get should work");
    assert!(entry.is_some());

    // Delete.
    let deleted = backend.delete(&id2).await.expect("Delete should work");
    assert!(deleted);
    assert_eq!(backend.count().await.unwrap(), 1);

    // Clear.
    backend.clear().await.expect("Clear should work");
    assert_eq!(backend.count().await.unwrap(), 0);
}

#[tokio::test]
async fn e2e_file_backend_persistence() {
    use rustedclaw_core::memory::MemoryBackend;
    use rustedclaw_memory::FileBackend;

    let dir = std::env::temp_dir().join("rustedclaw_e2e_test");
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join("e2e_memories.jsonl");
    let _ = std::fs::remove_file(&path); // Clean slate.

    // Write.
    {
        let backend = FileBackend::new(path.clone());
        backend
            .store(MemoryEntry {
                id: "persist_1".into(),
                content: "Persistent memory test".into(),
                tags: vec!["e2e".into()],
                source: None,
                created_at: chrono::Utc::now(),
                last_accessed: chrono::Utc::now(),
                score: 0.0,
                embedding: None,
            })
            .await
            .expect("Store should work");
    }

    // Read back with a new instance.
    {
        let backend = FileBackend::new(path.clone());
        let entry = backend.get("persist_1").await.expect("Get should work");
        assert!(entry.is_some());
        assert_eq!(entry.unwrap().content, "Persistent memory test");
    }

    // Cleanup.
    let _ = std::fs::remove_file(&path);
    let _ = std::fs::remove_dir(&dir);
}

// ── E2E: Gateway API (router only, no server) ──────────────────────────

#[tokio::test]
async fn e2e_gateway_health_and_tools() {
    use axum::body::Body;
    use axum::http::Request;
    use tower::ServiceExt;

    // Build gateway router.
    let config = rustedclaw_config::AppConfig::default();
    let agent = {
        let router = rustedclaw_providers::router::build_from_config(&config);
        let provider = router.default().expect("Need a provider");
        let tools = Arc::new(default_registry());
        let identity = Identity::default();
        let event_bus = Arc::new(EventBus::default());
        Arc::new(rustedclaw_agent::AgentLoop::new(
            provider,
            &config.default_model,
            config.default_temperature,
            tools,
            identity,
            event_bus,
        ))
    };

    let state = Arc::new(tokio::sync::RwLock::new(rustedclaw_gateway::GatewayState {
        config,
        pairing_code: None,
        bearer_tokens: Vec::new(),
        agent,
    }));

    let app = rustedclaw_gateway::build_router(state);

    // Test health endpoint.
    let req = Request::builder()
        .uri("/health")
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), 200);
}

// ── E2E: Configuration System ───────────────────────────────────────────

#[tokio::test]
async fn e2e_config_defaults_and_validation() {
    let config = rustedclaw_config::AppConfig::default();

    // Verify sensible defaults.
    assert!(!config.default_model.is_empty());
    assert!(config.default_temperature >= 0.0);
    assert!(config.default_temperature <= 2.0);
    assert!(config.gateway.port > 0);
    assert!(!config.gateway.host.is_empty());

    // Verify TOML roundtrip.
    let toml_str = toml::to_string_pretty(&config).expect("Config should serialize");
    let reparsed: rustedclaw_config::AppConfig =
        toml::from_str(&toml_str).expect("Config should parse back");

    assert_eq!(reparsed.default_model, config.default_model);
    assert_eq!(reparsed.gateway.port, config.gateway.port);
}

// ── E2E: Event System ──────────────────────────────────────────────────

#[tokio::test]
async fn e2e_event_bus_pubsub() {
    use rustedclaw_core::event::DomainEvent;

    let bus = EventBus::default();
    let mut rx = bus.subscribe();

    // Publish events.
    bus.publish(DomainEvent::ToolExecuted {
        tool_name: "calculator".into(),
        success: true,
        duration_ms: 42,
        timestamp: chrono::Utc::now(),
    });
    bus.publish(DomainEvent::ErrorOccurred {
        context: "test".into(),
        error_message: "something went wrong".into(),
        timestamp: chrono::Utc::now(),
    });

    // Receive events.
    let event1 = tokio::time::timeout(std::time::Duration::from_millis(100), rx.recv())
        .await
        .expect("Should receive event")
        .expect("Channel should be open");

    match event1.as_ref() {
        DomainEvent::ToolExecuted {
            tool_name, success, ..
        } => {
            assert_eq!(tool_name, "calculator");
            assert!(*success);
        }
        other => panic!("Expected ToolExecuted, got {:?}", other),
    }
}

// ── E2E: Identity System ───────────────────────────────────────────────

#[tokio::test]
async fn e2e_identity_with_custom_system_prompt() {
    let identity = Identity {
        name: "TestAgent".into(),
        system_prompt: "You are a test agent for E2E testing.".into(),
        personality: "Thorough and precise".into(),
        ..Identity::default()
    };

    assert_eq!(identity.name, "TestAgent");
    assert!(identity.system_prompt.contains("E2E testing"));
    assert!(identity.estimated_tokens() > 0);
}
