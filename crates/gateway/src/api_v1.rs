//! HTTP API v1 — full REST API for the agent runtime.
//!
//! Endpoints:
//!
//! - `POST /v1/chat`               — Send a message, get a response
//! - `POST /v1/chat/stream`        — Send a message, get SSE stream
//! - `GET  /v1/ws`                 — WebSocket for bidirectional streaming
//! - `GET  /v1/logs`               — SSE log stream
//! - `GET  /v1/conversations`      — List conversations
//! - `POST /v1/conversations`      — Create a conversation
//! - `GET  /v1/conversations/:id`  — Get a specific conversation
//! - `GET  /v1/tools`              — List available tools
//! - `POST /v1/context/debug`      — Context assembly debug view

use axum::{
    Router,
    extract::ws::{Message as WsMessage, WebSocket, WebSocketUpgrade},
    extract::{Path, State},
    http::StatusCode,
    response::sse::{Event as SseEvent, Sse},
    response::{IntoResponse, Json},
    routing::{get, post},
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::convert::Infallible;
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio_stream::StreamExt;
use tokio_stream::wrappers::ReceiverStream;
use tracing::info;

use rustedclaw_agent::{
    AgentStreamEvent, AssemblyInput, ContextAssembler, KnowledgeChunk, ReactAgent, TokenBudget,
    WorkingMemory,
};
use rustedclaw_contracts::ContractEngine;
use rustedclaw_core::event::EventBus;
use rustedclaw_core::identity::Identity;
use rustedclaw_core::memory::MemoryEntry;
use rustedclaw_core::message::{Conversation, ConversationId, Message};
use rustedclaw_core::provider::Provider;
use rustedclaw_core::tool::ToolRegistry;
use rustedclaw_telemetry::TelemetryEngine;

// ── State ─────────────────────────────────────────────────────────────────

/// Maximum number of in-memory conversations before oldest are evicted.
const MAX_CONVERSATIONS: usize = 1_000;
/// Maximum number of in-memory memory entries.
const MAX_MEMORIES: usize = 10_000;
/// Maximum number of in-memory document entries.
const MAX_DOCUMENTS: usize = 5_000;
/// Maximum number of in-memory job entries.
const MAX_JOBS: usize = 1_000;
/// Maximum number of active bearer tokens.
const MAX_BEARER_TOKENS: usize = 100;

/// Shared state for the v1 API.
pub struct ApiV1State {
    pub provider: Arc<dyn Provider>,
    pub model: String,
    pub temperature: f32,
    pub tools: Arc<ToolRegistry>,
    pub identity: Identity,
    pub event_bus: Arc<EventBus>,
    pub contracts: Arc<ContractEngine>,
    pub telemetry: Arc<TelemetryEngine>,
    pub conversations: RwLock<HashMap<String, Conversation>>,
    pub workflow: Option<Arc<rustedclaw_workflow::WorkflowEngine>>,
    pub config: RwLock<rustedclaw_config::AppConfig>,
    pub start_time: chrono::DateTime<chrono::Utc>,
    pub memories: RwLock<Vec<MemoryEntry>>,
    pub documents: RwLock<Vec<DocumentEntry>>,
    pub jobs: RwLock<Vec<JobEntry>>,
    /// Bearer tokens for API authentication.
    pub bearer_tokens: RwLock<Vec<String>>,
}

pub type SharedApiState = Arc<ApiV1State>;

// ── Router ────────────────────────────────────────────────────────────────

/// Build the v1 API router. Nest this under "/v1" in the main router.
pub fn v1_router(state: SharedApiState) -> Router {
    Router::new()
        .route("/chat", post(chat_handler))
        .route("/chat/stream", post(chat_stream_handler))
        .route("/ws", get(ws_handler))
        .route("/logs", get(log_stream_handler))
        .route("/conversations", get(list_conversations_handler))
        .route("/conversations", post(create_conversation_handler))
        .route("/conversations/{id}", get(get_conversation_handler))
        .route("/tools", get(list_tools_handler))
        .route("/tools/install", post(install_tool_handler))
        .route("/context/debug", post(context_debug_handler))
        .route("/routines", get(list_routines_handler))
        .route("/routines", post(create_routine_handler))
        .route("/routines/{id}", axum::routing::put(update_routine_handler))
        .route(
            "/routines/{id}",
            axum::routing::delete(delete_routine_handler),
        )
        .route("/documents", post(ingest_document_handler))
        .route("/memory", get(search_memory_handler))
        .route("/memory", post(create_memory_handler))
        .route("/memory/agent/{agent_id}", get(list_agent_memory_handler))
        .route("/memory/{id}", axum::routing::delete(delete_memory_handler))
        .route("/jobs", get(list_jobs_handler))
        .route("/jobs/{id}", get(get_job_handler))
        .route("/channels", get(list_channels_handler))
        .route("/channels/{name}/test", post(test_channel_handler))
        .route("/config", get(get_config_handler))
        .route("/config", axum::routing::patch(update_config_handler))
        .route("/contracts", get(list_contracts_handler))
        .route("/contracts", post(add_contract_handler))
        .route(
            "/contracts/{name}",
            axum::routing::delete(delete_contract_handler),
        )
        .route("/usage", get(usage_handler))
        .route("/traces", get(list_traces_handler))
        .route("/traces/{id}", get(get_trace_handler))
        .route("/budgets", get(list_budgets_handler))
        .route("/budgets", post(add_budget_handler))
        .route(
            "/budgets/{scope}",
            axum::routing::delete(delete_budget_handler),
        )
        .route("/status", get(status_handler))
        .with_state(state)
}

// ── Request / Response types ──────────────────────────────────────────────

#[derive(Deserialize)]
struct ChatRequest {
    /// Existing conversation ID (omit to create new).
    #[serde(default)]
    conversation_id: Option<String>,
    /// The user's message.
    message: String,
    /// Which agent pattern to use: "react" (default), "rag", "direct".
    #[serde(default = "default_pattern")]
    pattern: String,
}

fn default_pattern() -> String {
    "react".into()
}

#[derive(Serialize)]
struct ChatResponse {
    conversation_id: String,
    response: String,
    pattern: String,
    iterations: usize,
    tool_calls: usize,
    trace: Vec<TraceEntryDto>,
    context_metadata: Option<ContextMetadataDto>,
}

#[derive(Serialize)]
struct TraceEntryDto {
    kind: String,
    content: String,
}

#[derive(Serialize, Deserialize)]
struct ContextMetadataDto {
    total_tokens: usize,
    budget: usize,
    utilization_pct: f32,
    layers: Vec<LayerStatsDto>,
    drops: Vec<DropInfoDto>,
}

#[derive(Serialize, Deserialize)]
struct LayerStatsDto {
    name: String,
    tokens: usize,
    items_included: usize,
    items_total: usize,
}

#[derive(Serialize, Deserialize)]
struct DropInfoDto {
    layer: String,
    items_dropped: usize,
    reason: String,
}

#[derive(Serialize, Deserialize)]
struct ConversationListResponse {
    conversations: Vec<ConversationSummaryDto>,
}

#[derive(Serialize, Deserialize)]
struct ConversationSummaryDto {
    id: String,
    message_count: usize,
    created_at: String,
    updated_at: String,
    title: Option<String>,
}

#[derive(Serialize)]
struct ConversationDetailResponse {
    id: String,
    messages: Vec<MessageDto>,
    created_at: String,
    updated_at: String,
    title: Option<String>,
}

#[derive(Serialize, Deserialize)]
struct MessageDto {
    id: String,
    role: String,
    content: String,
    timestamp: String,
}

#[derive(Serialize, Deserialize)]
struct CreateConversationResponse {
    id: String,
    created_at: String,
}

#[derive(Serialize, Deserialize)]
struct ToolListResponse {
    tools: Vec<ToolDto>,
    count: usize,
}

#[derive(Serialize, Deserialize)]
struct ToolDto {
    name: String,
    description: String,
    parameters: serde_json::Value,
}

#[derive(Deserialize)]
struct ContextDebugRequest {
    message: String,
    #[serde(default)]
    memories: Vec<MemoryDto>,
    #[serde(default)]
    knowledge_chunks: Vec<KnowledgeChunkDto>,
    #[serde(default = "default_budget")]
    budget: usize,
}

fn default_budget() -> usize {
    4096
}

#[derive(Deserialize)]
struct MemoryDto {
    content: String,
    #[serde(default)]
    tags: Vec<String>,
}

#[derive(Deserialize)]
struct KnowledgeChunkDto {
    content: String,
    source: String,
    #[serde(default)]
    similarity: f32,
}

#[derive(Serialize, Deserialize)]
struct ContextDebugResponse {
    system_message: String,
    messages: Vec<MessageDto>,
    tool_definitions: Vec<String>,
    metadata: ContextMetadataDto,
}

#[derive(Serialize)]
struct ErrorResponse {
    error: String,
}

// ── Handlers ──────────────────────────────────────────────────────────────

async fn chat_handler(
    State(state): State<SharedApiState>,
    Json(payload): Json<ChatRequest>,
) -> Result<Json<ChatResponse>, (StatusCode, Json<ErrorResponse>)> {
    info!(pattern = %payload.pattern, "v1/chat request");

    // Get or create conversation.
    let conv_id = payload
        .conversation_id
        .unwrap_or_else(|| ConversationId::new().to_string());

    let mut conversations = state.conversations.write().await;

    // Evict oldest conversation if at capacity
    if conversations.len() >= MAX_CONVERSATIONS && !conversations.contains_key(&conv_id) {
        if let Some(oldest_key) = conversations
            .iter()
            .min_by_key(|(_, c)| c.created_at)
            .map(|(k, _)| k.clone())
        {
            conversations.remove(&oldest_key);
        }
    }

    let conv = conversations
        .entry(conv_id.clone())
        .or_insert_with(Conversation::new);

    conv.push(Message::user(&payload.message));

    // Execute with the requested pattern.
    match payload.pattern.as_str() {
        "react" | "direct" => {
            let agent = ReactAgent::new(
                state.provider.clone(),
                &state.model,
                state.temperature,
                state.tools.clone(),
                state.identity.clone(),
                state.event_bus.clone(),
            )
            .with_telemetry(state.telemetry.clone());

            // Release the lock before the async LLM call.
            // We need to clone the conversation for the agent.
            let mut conv_clone = conv.clone();
            drop(conversations);

            let result = agent
                .run(&payload.message, &mut conv_clone, &[], &[])
                .await
                .map_err(|e| {
                    (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(ErrorResponse {
                            error: format!("Agent error: {}", e),
                        }),
                    )
                })?;

            // Store updated conversation back.
            let mut conversations = state.conversations.write().await;
            conversations.insert(conv_id.clone(), conv_clone);

            let trace: Vec<TraceEntryDto> = result
                .trace
                .iter()
                .map(|t| TraceEntryDto {
                    kind: format!("{:?}", t.kind),
                    content: t.content.clone(),
                })
                .collect();

            let context_metadata = result.last_context_metadata.map(|m| ContextMetadataDto {
                total_tokens: m.total_tokens,
                budget: m.budget,
                utilization_pct: m.utilization_pct,
                layers: m
                    .per_layer
                    .iter()
                    .map(|l| LayerStatsDto {
                        name: l.name.clone(),
                        tokens: l.tokens,
                        items_included: l.items_included,
                        items_total: l.items_total,
                    })
                    .collect(),
                drops: m
                    .drops
                    .iter()
                    .map(|d| DropInfoDto {
                        layer: d.layer.clone(),
                        items_dropped: d.items_dropped,
                        reason: d.reason.clone(),
                    })
                    .collect(),
            });

            Ok(Json(ChatResponse {
                conversation_id: conv_id,
                response: result.answer,
                pattern: payload.pattern,
                iterations: result.iterations,
                tool_calls: result.tool_calls_made,
                trace,
                context_metadata,
            }))
        }
        "rag" => {
            let agent = rustedclaw_agent::RagAgent::new(
                state.provider.clone(),
                &state.model,
                state.temperature,
                state.tools.clone(),
                state.identity.clone(),
                state.event_bus.clone(),
            );

            let mut conv_clone = conv.clone();
            drop(conversations);

            let result = agent
                .run(&payload.message, &mut conv_clone, &[])
                .await
                .map_err(|e| {
                    (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(ErrorResponse {
                            error: format!("RAG agent error: {}", e),
                        }),
                    )
                })?;

            let mut conversations = state.conversations.write().await;
            conversations.insert(conv_id.clone(), conv_clone);

            let trace: Vec<TraceEntryDto> = result
                .working_memory
                .trace
                .iter()
                .map(|t| TraceEntryDto {
                    kind: format!("{:?}", t.kind),
                    content: t.content.clone(),
                })
                .collect();

            let context_metadata = result.context_metadata.map(|m| ContextMetadataDto {
                total_tokens: m.total_tokens,
                budget: m.budget,
                utilization_pct: m.utilization_pct,
                layers: m
                    .per_layer
                    .iter()
                    .map(|l| LayerStatsDto {
                        name: l.name.clone(),
                        tokens: l.tokens,
                        items_included: l.items_included,
                        items_total: l.items_total,
                    })
                    .collect(),
                drops: m
                    .drops
                    .iter()
                    .map(|d| DropInfoDto {
                        layer: d.layer.clone(),
                        items_dropped: d.items_dropped,
                        reason: d.reason.clone(),
                    })
                    .collect(),
            });

            Ok(Json(ChatResponse {
                conversation_id: conv_id,
                response: result.answer,
                pattern: "rag".into(),
                iterations: 1,
                tool_calls: result.retrieved_chunks.len(),
                trace,
                context_metadata,
            }))
        }
        other => Err((
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: format!(
                    "Unknown pattern: '{}'. Use 'react', 'rag', or 'direct'.",
                    other
                ),
            }),
        )),
    }
}

// ── SSE Streaming ─────────────────────────────────────────────────────────

/// `POST /v1/chat/stream` — Send a message, receive an SSE stream of events.
async fn chat_stream_handler(
    State(state): State<SharedApiState>,
    Json(payload): Json<ChatRequest>,
) -> Result<
    Sse<impl futures::Stream<Item = Result<SseEvent, Infallible>>>,
    (StatusCode, Json<ErrorResponse>),
> {
    info!(pattern = %payload.pattern, "v1/chat/stream SSE request");

    let conv_id = payload
        .conversation_id
        .unwrap_or_else(|| ConversationId::new().to_string());

    let mut conversations = state.conversations.write().await;

    // Evict oldest conversation if at capacity
    if conversations.len() >= MAX_CONVERSATIONS && !conversations.contains_key(&conv_id) {
        if let Some(oldest_key) = conversations
            .iter()
            .min_by_key(|(_, c)| c.created_at)
            .map(|(k, _)| k.clone())
        {
            conversations.remove(&oldest_key);
        }
    }

    let conv = conversations
        .entry(conv_id.clone())
        .or_insert_with(Conversation::new);
    conv.push(Message::user(&payload.message));

    let agent = ReactAgent::new(
        state.provider.clone(),
        &state.model,
        state.temperature,
        state.tools.clone(),
        state.identity.clone(),
        state.event_bus.clone(),
    )
    .with_telemetry(state.telemetry.clone());

    let mut conv_clone = conv.clone();
    drop(conversations);

    let rx = agent
        .run_stream(&payload.message, &mut conv_clone, &[], &[])
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: format!("Agent stream error: {}", e),
                }),
            )
        })?;

    let stream = ReceiverStream::new(rx).map(|event| {
        let event_type = event.event_type().to_string();
        let data = serde_json::to_string(&event).unwrap_or_default();
        Ok(SseEvent::default().event(event_type).data(data))
    });

    Ok(Sse::new(stream))
}

// ── WebSocket ─────────────────────────────────────────────────────────────

/// `GET /v1/ws` — Full bidirectional WebSocket connection.
///
/// Protocol (per PRD):
/// - Client → Server: `{ "type": "message", "content": "..." }`
/// - Server → Client: `AgentStreamEvent` JSON frames (chunk, tool_call, tool_result, done)
async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<SharedApiState>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_ws_connection(socket, state))
}

/// WebSocket message from the client.
#[derive(Deserialize)]
struct WsClientMessage {
    #[serde(rename = "type")]
    msg_type: String,
    content: String,
    #[serde(default)]
    conversation_id: Option<String>,
}

async fn handle_ws_connection(mut socket: WebSocket, state: SharedApiState) {
    info!("WebSocket connection established");

    while let Some(msg) = socket.recv().await {
        let msg = match msg {
            Ok(WsMessage::Text(text)) => text,
            Ok(WsMessage::Close(_)) => break,
            Ok(_) => continue, // ignore binary, ping, pong
            Err(_) => break,
        };

        let client_msg: WsClientMessage = match serde_json::from_str(&msg) {
            Ok(m) => m,
            Err(e) => {
                let err = AgentStreamEvent::Error {
                    message: format!("Invalid message: {}", e),
                };
                let _ = socket
                    .send(WsMessage::Text(
                        serde_json::to_string(&err).unwrap_or_default().into(),
                    ))
                    .await;
                continue;
            }
        };

        if client_msg.msg_type != "message" {
            let err = AgentStreamEvent::Error {
                message: format!("Unknown message type: '{}'", client_msg.msg_type),
            };
            let _ = socket
                .send(WsMessage::Text(
                    serde_json::to_string(&err).unwrap_or_default().into(),
                ))
                .await;
            continue;
        }

        let conv_id = client_msg
            .conversation_id
            .unwrap_or_else(|| ConversationId::new().to_string());

        let mut conversations = state.conversations.write().await;
        let conv = conversations
            .entry(conv_id.clone())
            .or_insert_with(Conversation::new);
        conv.push(Message::user(&client_msg.content));

        let agent = ReactAgent::new(
            state.provider.clone(),
            &state.model,
            state.temperature,
            state.tools.clone(),
            state.identity.clone(),
            state.event_bus.clone(),
        )
        .with_telemetry(state.telemetry.clone());

        let mut conv_clone = conv.clone();
        drop(conversations);

        match agent
            .run_stream(&client_msg.content, &mut conv_clone, &[], &[])
            .await
        {
            Ok(mut rx) => {
                while let Some(event) = rx.recv().await {
                    let json = serde_json::to_string(&event).unwrap_or_default();
                    if socket.send(WsMessage::Text(json.into())).await.is_err() {
                        return; // client disconnected
                    }
                }
            }
            Err(e) => {
                let err = AgentStreamEvent::Error {
                    message: format!("Agent error: {}", e),
                };
                let _ = socket
                    .send(WsMessage::Text(
                        serde_json::to_string(&err).unwrap_or_default().into(),
                    ))
                    .await;
            }
        }
    }

    info!("WebSocket connection closed");
}

// ── SSE Log Stream ────────────────────────────────────────────────────────

/// `GET /v1/logs` — SSE stream of domain events (agent activity, tool calls, etc.).
async fn log_stream_handler(
    State(state): State<SharedApiState>,
) -> Sse<impl futures::Stream<Item = Result<SseEvent, Infallible>>> {
    let event_bus = state.event_bus.clone();
    let rx = event_bus.subscribe();
    let stream = tokio_stream::wrappers::BroadcastStream::new(rx)
        .filter_map(|result| result.ok())
        .map(|event| {
            let data = serde_json::to_string(event.as_ref()).unwrap_or_default();
            let event_name = match event.as_ref() {
                rustedclaw_core::event::DomainEvent::ResponseGenerated { .. } => {
                    "response_generated"
                }
                rustedclaw_core::event::DomainEvent::ToolExecuted { .. } => "tool_executed",
                rustedclaw_core::event::DomainEvent::MemoryAccessed { .. } => "memory_accessed",
                rustedclaw_core::event::DomainEvent::MessageReceived { .. } => "message_received",
                rustedclaw_core::event::DomainEvent::ErrorOccurred { .. } => "error_occurred",
                rustedclaw_core::event::DomainEvent::AgentStateChanged { .. } => {
                    "agent_state_changed"
                }
                rustedclaw_core::event::DomainEvent::ContractViolation { .. } => {
                    "contract_violation"
                }
                rustedclaw_core::event::DomainEvent::BudgetExceeded { .. } => "budget_exceeded",
            };
            Ok(SseEvent::default().event(event_name).data(data))
        });

    Sse::new(stream)
}

async fn list_conversations_handler(
    State(state): State<SharedApiState>,
) -> Json<ConversationListResponse> {
    let conversations = state.conversations.read().await;

    let mut summaries: Vec<ConversationSummaryDto> = conversations
        .values()
        .map(|c| ConversationSummaryDto {
            id: c.id.to_string(),
            message_count: c.messages.len(),
            created_at: c.created_at.to_rfc3339(),
            updated_at: c.updated_at.to_rfc3339(),
            title: c.title.clone(),
        })
        .collect();

    summaries.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));

    Json(ConversationListResponse {
        conversations: summaries,
    })
}

async fn create_conversation_handler(
    State(state): State<SharedApiState>,
) -> (StatusCode, Json<CreateConversationResponse>) {
    let conv = Conversation::new();
    let id = conv.id.to_string();
    let created = conv.created_at.to_rfc3339();

    let mut conversations = state.conversations.write().await;

    // Evict oldest if at capacity
    if conversations.len() >= MAX_CONVERSATIONS {
        if let Some(oldest_key) = conversations
            .iter()
            .min_by_key(|(_, c)| c.created_at)
            .map(|(k, _)| k.clone())
        {
            conversations.remove(&oldest_key);
        }
    }

    conversations.insert(id.clone(), conv);

    (
        StatusCode::CREATED,
        Json(CreateConversationResponse {
            id,
            created_at: created,
        }),
    )
}

async fn get_conversation_handler(
    State(state): State<SharedApiState>,
    Path(id): Path<String>,
) -> Result<Json<ConversationDetailResponse>, StatusCode> {
    let conversations = state.conversations.read().await;

    let conv = conversations.get(&id).ok_or(StatusCode::NOT_FOUND)?;

    Ok(Json(ConversationDetailResponse {
        id: conv.id.to_string(),
        messages: conv
            .messages
            .iter()
            .map(|m| MessageDto {
                id: m.id.clone(),
                role: format!("{:?}", m.role),
                content: m.content.clone(),
                timestamp: m.timestamp.to_rfc3339(),
            })
            .collect(),
        created_at: conv.created_at.to_rfc3339(),
        updated_at: conv.updated_at.to_rfc3339(),
        title: conv.title.clone(),
    }))
}

async fn list_tools_handler(State(state): State<SharedApiState>) -> Json<ToolListResponse> {
    let defs = state.tools.definitions();
    let count = defs.len();

    Json(ToolListResponse {
        tools: defs
            .into_iter()
            .map(|d| ToolDto {
                name: d.name,
                description: d.description,
                parameters: d.parameters,
            })
            .collect(),
        count,
    })
}

async fn context_debug_handler(
    State(state): State<SharedApiState>,
    Json(payload): Json<ContextDebugRequest>,
) -> Result<Json<ContextDebugResponse>, (StatusCode, Json<ErrorResponse>)> {
    let budget = TokenBudget {
        total: payload.budget,
        ..TokenBudget::default()
    };

    let assembler = ContextAssembler::new(budget);
    let wm = WorkingMemory::default();
    let conv = Conversation::new();
    let tool_defs = state.tools.definitions();

    let memories: Vec<MemoryEntry> = payload
        .memories
        .iter()
        .map(|m| MemoryEntry {
            id: uuid::Uuid::new_v4().to_string(),
            content: m.content.clone(),
            tags: m.tags.clone(),
            source: None,
            created_at: chrono::Utc::now(),
            last_accessed: chrono::Utc::now(),
            score: 0.0,
            embedding: None,
        })
        .collect();

    let chunks: Vec<KnowledgeChunk> = payload
        .knowledge_chunks
        .iter()
        .enumerate()
        .map(|(i, c)| KnowledgeChunk {
            document_id: format!("doc_{}", i),
            chunk_index: i,
            content: c.content.clone(),
            source: c.source.clone(),
            similarity: c.similarity,
        })
        .collect();

    let input = AssemblyInput {
        identity: &state.identity,
        memories: &memories,
        working_memory: &wm,
        knowledge_chunks: &chunks,
        tool_definitions: &tool_defs,
        conversation: &conv,
        user_message: &payload.message,
    };

    let assembled = assembler.assemble(&input).map_err(|e| {
        (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: format!("Assembly failed: {}", e),
            }),
        )
    })?;

    Ok(Json(ContextDebugResponse {
        system_message: assembled.system_message,
        messages: assembled
            .messages
            .iter()
            .map(|m| MessageDto {
                id: m.id.clone(),
                role: format!("{:?}", m.role),
                content: m.content.clone(),
                timestamp: m.timestamp.to_rfc3339(),
            })
            .collect(),
        tool_definitions: assembled
            .tool_definitions
            .iter()
            .map(|t| t.name.clone())
            .collect(),
        metadata: ContextMetadataDto {
            total_tokens: assembled.metadata.total_tokens,
            budget: assembled.metadata.budget,
            utilization_pct: assembled.metadata.utilization_pct,
            layers: assembled
                .metadata
                .per_layer
                .iter()
                .map(|l| LayerStatsDto {
                    name: l.name.clone(),
                    tokens: l.tokens,
                    items_included: l.items_included,
                    items_total: l.items_total,
                })
                .collect(),
            drops: assembled
                .metadata
                .drops
                .iter()
                .map(|d| DropInfoDto {
                    layer: d.layer.clone(),
                    items_dropped: d.items_dropped,
                    reason: d.reason.clone(),
                })
                .collect(),
        },
    }))
}

// ── Routine types ─────────────────────────────────────────────────────────

#[derive(Serialize, Deserialize)]
struct RoutineListResponse {
    routines: Vec<RoutineDto>,
    count: usize,
}

#[derive(Serialize, Deserialize, Clone)]
struct RoutineDto {
    id: String,
    name: String,
    schedule: String,
    instruction: String,
    enabled: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    target_channel: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    last_run: Option<String>,
}

#[derive(Deserialize)]
struct CreateRoutineRequest {
    name: String,
    schedule: String,
    instruction: String,
    #[serde(default = "default_true_fn")]
    enabled: bool,
    #[serde(default)]
    target_channel: Option<String>,
}

fn default_true_fn() -> bool {
    true
}

#[derive(Deserialize)]
struct UpdateRoutineRequest {
    #[serde(default)]
    enabled: Option<bool>,
    #[serde(default)]
    schedule: Option<String>,
    #[serde(default)]
    instruction: Option<String>,
    #[serde(default)]
    target_channel: Option<Option<String>>,
}

#[derive(Serialize, Deserialize)]
struct RoutineActionResponse {
    success: bool,
    message: String,
}

// ── Routine handlers ──────────────────────────────────────────────────────

async fn list_routines_handler(
    State(state): State<SharedApiState>,
) -> Result<Json<RoutineListResponse>, StatusCode> {
    let engine = state
        .workflow
        .as_ref()
        .ok_or(StatusCode::SERVICE_UNAVAILABLE)?;
    let tasks = engine.list_tasks().await;

    let routines: Vec<RoutineDto> = tasks
        .into_iter()
        .map(|t| RoutineDto {
            id: t.id.clone(),
            name: t.name,
            schedule: t.schedule,
            instruction: t.instruction,
            enabled: t.enabled,
            target_channel: t.target_channel,
            last_run: t.last_run.map(|dt| dt.to_rfc3339()),
        })
        .collect();

    let count = routines.len();
    Ok(Json(RoutineListResponse { routines, count }))
}

async fn create_routine_handler(
    State(state): State<SharedApiState>,
    Json(req): Json<CreateRoutineRequest>,
) -> Result<(StatusCode, Json<RoutineDto>), (StatusCode, Json<RoutineActionResponse>)> {
    let engine = state.workflow.as_ref().ok_or((
        StatusCode::SERVICE_UNAVAILABLE,
        Json(RoutineActionResponse {
            success: false,
            message: "Workflow engine not available".into(),
        }),
    ))?;

    let task = rustedclaw_workflow::CronTask {
        id: req.name.clone(),
        name: req.name.clone(),
        instruction: req.instruction.clone(),
        schedule: req.schedule.clone(),
        enabled: req.enabled,
        target_channel: req.target_channel.clone(),
        action: rustedclaw_workflow::TaskAction::AgentTask {
            prompt: req.instruction.clone(),
            context: None,
        },
        last_run: None,
        next_run: None,
    };

    engine.add_task(task).await.map_err(|e| {
        (
            StatusCode::BAD_REQUEST,
            Json(RoutineActionResponse {
                success: false,
                message: format!("Invalid routine: {e}"),
            }),
        )
    })?;

    let dto = RoutineDto {
        id: req.name.clone(),
        name: req.name,
        schedule: req.schedule,
        instruction: req.instruction,
        enabled: req.enabled,
        target_channel: req.target_channel,
        last_run: None,
    };

    Ok((StatusCode::CREATED, Json(dto)))
}

async fn update_routine_handler(
    State(state): State<SharedApiState>,
    Path(id): Path<String>,
    Json(req): Json<UpdateRoutineRequest>,
) -> Result<Json<RoutineActionResponse>, (StatusCode, Json<RoutineActionResponse>)> {
    let engine = state.workflow.as_ref().ok_or((
        StatusCode::SERVICE_UNAVAILABLE,
        Json(RoutineActionResponse {
            success: false,
            message: "Workflow engine not available".into(),
        }),
    ))?;

    // Handle enable/disable
    if let Some(enabled) = req.enabled {
        if enabled {
            engine.resume_task(&id).await;
        } else {
            engine.pause_task(&id).await;
        }
    }

    // For schedule/instruction changes, we'd need to remove and re-add
    // For now, we handle the common case of enabling/disabling
    if req.schedule.is_some() || req.instruction.is_some() {
        // Get current task, remove it, modify, re-add
        let tasks = engine.list_tasks().await;
        if let Some(existing) = tasks.into_iter().find(|t| t.id == id) {
            engine.remove_task(&id).await;
            let updated = rustedclaw_workflow::CronTask {
                schedule: req.schedule.unwrap_or(existing.schedule),
                instruction: req
                    .instruction
                    .clone()
                    .unwrap_or(existing.instruction.clone()),
                target_channel: req.target_channel.unwrap_or(existing.target_channel),
                action: if let Some(ref instr) = req.instruction {
                    rustedclaw_workflow::TaskAction::AgentTask {
                        prompt: instr.clone(),
                        context: None,
                    }
                } else {
                    existing.action
                },
                ..existing
            };
            engine.add_task(updated).await.map_err(|e| {
                (
                    StatusCode::BAD_REQUEST,
                    Json(RoutineActionResponse {
                        success: false,
                        message: format!("Invalid update: {e}"),
                    }),
                )
            })?;
        } else {
            return Err((
                StatusCode::NOT_FOUND,
                Json(RoutineActionResponse {
                    success: false,
                    message: format!("Routine '{id}' not found"),
                }),
            ));
        }
    }

    Ok(Json(RoutineActionResponse {
        success: true,
        message: format!("Routine '{id}' updated"),
    }))
}

async fn delete_routine_handler(
    State(state): State<SharedApiState>,
    Path(id): Path<String>,
) -> Result<Json<RoutineActionResponse>, (StatusCode, Json<RoutineActionResponse>)> {
    let engine = state.workflow.as_ref().ok_or((
        StatusCode::SERVICE_UNAVAILABLE,
        Json(RoutineActionResponse {
            success: false,
            message: "Workflow engine not available".into(),
        }),
    ))?;

    if engine.remove_task(&id).await {
        Ok(Json(RoutineActionResponse {
            success: true,
            message: format!("Routine '{id}' deleted"),
        }))
    } else {
        Err((
            StatusCode::NOT_FOUND,
            Json(RoutineActionResponse {
                success: false,
                message: format!("Routine '{id}' not found"),
            }),
        ))
    }
}

// ── Document + Memory + Job types ─────────────────────────────────────────

/// A document chunk stored in the knowledge base.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocumentEntry {
    pub id: String,
    pub content: String,
    pub source: String,
    pub metadata: Option<serde_json::Value>,
    pub chunk_index: usize,
    pub document_id: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

/// A job (routine execution) entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobEntry {
    pub id: String,
    pub routine_id: String,
    pub status: JobStatus,
    pub started_at: chrono::DateTime<chrono::Utc>,
    pub completed_at: Option<chrono::DateTime<chrono::Utc>>,
    pub result: Option<String>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum JobStatus {
    Running,
    Completed,
    Failed,
}

// ── Document ingest ───────────────────────────────────────────────────────

#[derive(Deserialize)]
struct IngestDocumentRequest {
    content: String,
    source: String,
    #[serde(default)]
    metadata: Option<serde_json::Value>,
    #[serde(default)]
    chunk_index: Option<usize>,
    #[serde(default)]
    document_id: Option<String>,
}

#[derive(Serialize, Deserialize)]
struct IngestDocumentResponse {
    id: String,
    document_id: String,
    chunk_index: usize,
    source: String,
}

async fn ingest_document_handler(
    State(state): State<SharedApiState>,
    Json(req): Json<IngestDocumentRequest>,
) -> (StatusCode, Json<IngestDocumentResponse>) {
    let id = uuid::Uuid::new_v4().to_string();
    let document_id = req
        .document_id
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    let chunk_index = req.chunk_index.unwrap_or(0);

    let entry = DocumentEntry {
        id: id.clone(),
        content: req.content,
        source: req.source.clone(),
        metadata: req.metadata,
        chunk_index,
        document_id: document_id.clone(),
        created_at: chrono::Utc::now(),
    };

    let mut documents = state.documents.write().await;

    // Evict oldest 10% if at capacity
    if documents.len() >= MAX_DOCUMENTS {
        let drain_count = MAX_DOCUMENTS / 10;
        // Sort by created_at to remove oldest
        documents.sort_by_key(|d| d.created_at);
        documents.drain(..drain_count);
    }

    documents.push(entry);

    (
        StatusCode::CREATED,
        Json(IngestDocumentResponse {
            id,
            document_id,
            chunk_index,
            source: req.source,
        }),
    )
}

// ── Memory CRUD ───────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct CreateMemoryRequest {
    content: String,
    #[serde(default)]
    agent_id: Option<String>,
    #[serde(default)]
    category: Option<String>,
    #[serde(default)]
    tags: Vec<String>,
    #[serde(default = "default_confidence")]
    confidence: f32,
}

fn default_confidence() -> f32 {
    1.0
}

#[derive(Serialize, Deserialize)]
struct CreateMemoryResponse {
    id: String,
    content: String,
    created_at: String,
}

#[derive(Serialize, Deserialize)]
struct MemoryListResponse {
    memories: Vec<MemoryItemDto>,
    count: usize,
}

#[derive(Serialize, Deserialize)]
struct MemoryItemDto {
    id: String,
    content: String,
    tags: Vec<String>,
    score: f32,
    created_at: String,
    last_accessed: String,
}

#[derive(Serialize, Deserialize)]
struct MemoryDeleteResponse {
    success: bool,
    message: String,
}

async fn create_memory_handler(
    State(state): State<SharedApiState>,
    Json(req): Json<CreateMemoryRequest>,
) -> (StatusCode, Json<CreateMemoryResponse>) {
    let id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now();

    let mut tags = req.tags;
    if let Some(ref cat) = req.category {
        tags.push(format!("category:{cat}"));
    }
    if let Some(ref agent_id) = req.agent_id {
        tags.push(format!("agent:{agent_id}"));
    }

    let entry = MemoryEntry {
        id: id.clone(),
        content: req.content.clone(),
        tags,
        source: req.agent_id,
        created_at: now,
        last_accessed: now,
        score: req.confidence,
        embedding: None,
    };

    let mut memories = state.memories.write().await;

    // Evict oldest 10% if at capacity
    if memories.len() >= MAX_MEMORIES {
        let drain_count = MAX_MEMORIES / 10;
        memories.sort_by_key(|m| m.created_at);
        memories.drain(..drain_count);
    }

    memories.push(entry);

    (
        StatusCode::CREATED,
        Json(CreateMemoryResponse {
            id,
            content: req.content,
            created_at: now.to_rfc3339(),
        }),
    )
}

async fn search_memory_handler(State(state): State<SharedApiState>) -> Json<MemoryListResponse> {
    let memories = state.memories.read().await;
    let items: Vec<MemoryItemDto> = memories
        .iter()
        .map(|m| MemoryItemDto {
            id: m.id.clone(),
            content: m.content.clone(),
            tags: m.tags.clone(),
            score: m.score,
            created_at: m.created_at.to_rfc3339(),
            last_accessed: m.last_accessed.to_rfc3339(),
        })
        .collect();
    let count = items.len();
    Json(MemoryListResponse {
        memories: items,
        count,
    })
}

async fn list_agent_memory_handler(
    State(state): State<SharedApiState>,
    Path(agent_id): Path<String>,
) -> Json<MemoryListResponse> {
    let memories = state.memories.read().await;
    let tag_filter = format!("agent:{agent_id}");
    let items: Vec<MemoryItemDto> = memories
        .iter()
        .filter(|m| {
            m.tags.iter().any(|t| t == &tag_filter) || m.source.as_deref() == Some(&agent_id)
        })
        .map(|m| MemoryItemDto {
            id: m.id.clone(),
            content: m.content.clone(),
            tags: m.tags.clone(),
            score: m.score,
            created_at: m.created_at.to_rfc3339(),
            last_accessed: m.last_accessed.to_rfc3339(),
        })
        .collect();
    let count = items.len();
    Json(MemoryListResponse {
        memories: items,
        count,
    })
}

async fn delete_memory_handler(
    State(state): State<SharedApiState>,
    Path(id): Path<String>,
) -> Result<Json<MemoryDeleteResponse>, (StatusCode, Json<MemoryDeleteResponse>)> {
    let mut memories = state.memories.write().await;
    let before = memories.len();
    memories.retain(|m| m.id != id);
    if memories.len() < before {
        Ok(Json(MemoryDeleteResponse {
            success: true,
            message: format!("Memory '{id}' deleted"),
        }))
    } else {
        Err((
            StatusCode::NOT_FOUND,
            Json(MemoryDeleteResponse {
                success: false,
                message: format!("Memory '{id}' not found"),
            }),
        ))
    }
}

// ── Job endpoints ─────────────────────────────────────────────────────────

#[derive(Serialize, Deserialize)]
struct JobListResponse {
    jobs: Vec<JobEntry>,
    count: usize,
}

async fn list_jobs_handler(State(state): State<SharedApiState>) -> Json<JobListResponse> {
    let jobs = state.jobs.read().await;
    let count = jobs.len();
    Json(JobListResponse {
        jobs: jobs.clone(),
        count,
    })
}

async fn get_job_handler(
    State(state): State<SharedApiState>,
    Path(id): Path<String>,
) -> Result<Json<JobEntry>, StatusCode> {
    let jobs = state.jobs.read().await;
    jobs.iter()
        .find(|j| j.id == id)
        .cloned()
        .map(Json)
        .ok_or(StatusCode::NOT_FOUND)
}

// ── Tool install ──────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct InstallToolRequest {
    name: String,
    #[serde(default)]
    #[allow(dead_code)]
    description: Option<String>,
    /// Base64-encoded WASM binary
    #[serde(default)]
    #[allow(dead_code)]
    wasm_base64: Option<String>,
}

#[derive(Serialize, Deserialize)]
struct InstallToolResponse {
    success: bool,
    name: String,
    message: String,
}

async fn install_tool_handler(
    Json(req): Json<InstallToolRequest>,
) -> Result<(StatusCode, Json<InstallToolResponse>), (StatusCode, Json<ErrorResponse>)> {
    // Stub: accept the request, validate the name, return success
    if req.name.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "Tool name is required".into(),
            }),
        ));
    }

    Ok((
        StatusCode::CREATED,
        Json(InstallToolResponse {
            success: true,
            name: req.name.clone(),
            message: format!(
                "Tool '{}' registered (stub — WASM validation not yet wired)",
                req.name
            ),
        }),
    ))
}

// ── Channel endpoints ─────────────────────────────────────────────────────

#[derive(Serialize, Deserialize)]
struct ChannelListResponse {
    channels: Vec<ChannelStatusDto>,
}

#[derive(Serialize, Deserialize)]
struct ChannelStatusDto {
    name: String,
    enabled: bool,
    connected: bool,
    health: String,
}

#[derive(Serialize, Deserialize)]
struct ChannelTestResponse {
    channel: String,
    success: bool,
    message: String,
}

async fn list_channels_handler(State(state): State<SharedApiState>) -> Json<ChannelListResponse> {
    let config = state.config.read().await;
    let mut channels = Vec::new();

    // Report channels from config
    for (name, ch) in &config.channels_config {
        channels.push(ChannelStatusDto {
            name: name.clone(),
            enabled: ch.enabled,
            connected: ch.enabled, // Stub: assume connected if enabled
            health: if ch.enabled {
                "healthy".into()
            } else {
                "disabled".into()
            },
        });
    }

    // Always include web gateway
    channels.push(ChannelStatusDto {
        name: "web".into(),
        enabled: true,
        connected: true,
        health: "healthy".into(),
    });

    Json(ChannelListResponse { channels })
}

async fn test_channel_handler(
    State(state): State<SharedApiState>,
    Path(name): Path<String>,
) -> Result<Json<ChannelTestResponse>, (StatusCode, Json<ChannelTestResponse>)> {
    let config = state.config.read().await;

    if name == "web" {
        return Ok(Json(ChannelTestResponse {
            channel: "web".into(),
            success: true,
            message: "Web gateway is running".into(),
        }));
    }

    match config.channels_config.get(&name) {
        Some(ch) if ch.enabled => Ok(Json(ChannelTestResponse {
            channel: name,
            success: true,
            message: "Channel connection test passed (stub)".into(),
        })),
        Some(_) => Err((
            StatusCode::BAD_REQUEST,
            Json(ChannelTestResponse {
                channel: name,
                success: false,
                message: "Channel is disabled".into(),
            }),
        )),
        None => Err((
            StatusCode::NOT_FOUND,
            Json(ChannelTestResponse {
                channel: name,
                success: false,
                message: "Channel not found".into(),
            }),
        )),
    }
}

// ── Config endpoints ──────────────────────────────────────────────────────

#[derive(Serialize, Deserialize)]
struct ConfigResponse {
    config: serde_json::Value,
}

#[derive(Serialize, Deserialize)]
struct ConfigUpdateResponse {
    success: bool,
    message: String,
}

async fn get_config_handler(State(state): State<SharedApiState>) -> Json<ConfigResponse> {
    let config = state.config.read().await;
    let mut value = serde_json::to_value(&*config).unwrap_or(serde_json::json!({}));

    // Redact secrets
    redact_secrets(&mut value);

    Json(ConfigResponse { config: value })
}

fn redact_secrets(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::Object(map) => {
            for (key, val) in map.iter_mut() {
                let key_lower = key.to_lowercase();
                if key_lower.contains("key")
                    || key_lower.contains("secret")
                    || key_lower.contains("token")
                    || key_lower.contains("password")
                {
                    if val.is_string() && !val.as_str().unwrap_or("").is_empty() {
                        *val = serde_json::json!("***REDACTED***");
                    }
                } else {
                    redact_secrets(val);
                }
            }
        }
        serde_json::Value::Array(arr) => {
            for item in arr.iter_mut() {
                redact_secrets(item);
            }
        }
        _ => {}
    }
}

async fn update_config_handler(
    State(state): State<SharedApiState>,
    Json(payload): Json<serde_json::Value>,
) -> Result<Json<ConfigUpdateResponse>, (StatusCode, Json<ErrorResponse>)> {
    if !payload.is_object() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "Request body must be a JSON object".into(),
            }),
        ));
    }

    // Merge partial update into current config
    let mut config = state.config.write().await;
    let mut current = serde_json::to_value(&*config).unwrap_or(serde_json::json!({}));
    merge_json(&mut current, &payload);

    match serde_json::from_value::<rustedclaw_config::AppConfig>(current) {
        Ok(updated) => {
            *config = updated;
            Ok(Json(ConfigUpdateResponse {
                success: true,
                message: "Configuration updated".into(),
            }))
        }
        Err(e) => Err((
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: format!("Invalid config update: {e}"),
            }),
        )),
    }
}

fn merge_json(base: &mut serde_json::Value, patch: &serde_json::Value) {
    if let (serde_json::Value::Object(base_map), serde_json::Value::Object(patch_map)) =
        (base, patch)
    {
        for (key, value) in patch_map {
            if value.is_object() && base_map.get(key).is_some_and(|v| v.is_object()) {
                merge_json(base_map.get_mut(key).unwrap(), value);
            } else {
                base_map.insert(key.clone(), value.clone());
            }
        }
    }
}

// ── Status endpoint ───────────────────────────────────────────────────────

// ── Contract endpoints ────────────────────────────────────────────────────

#[derive(Serialize)]
struct ContractDto {
    name: String,
    description: String,
    trigger: String,
    condition: String,
    action: String,
    message: String,
    enabled: bool,
    priority: i32,
}

async fn list_contracts_handler(State(state): State<SharedApiState>) -> Json<Vec<ContractDto>> {
    let contracts = state.contracts.list_contracts();
    Json(
        contracts
            .into_iter()
            .map(|c| ContractDto {
                name: c.name,
                description: c.description,
                trigger: c.trigger.into(),
                action: format!("{:?}", c.action).to_lowercase(),
                condition: c.condition,
                message: c.message,
                enabled: c.enabled,
                priority: c.priority,
            })
            .collect(),
    )
}

#[derive(Deserialize)]
struct AddContractRequest {
    name: String,
    #[serde(default)]
    description: String,
    trigger: String,
    #[serde(default)]
    condition: String,
    #[serde(default = "default_deny_action")]
    action: String,
    #[serde(default)]
    message: String,
    #[serde(default = "default_contract_enabled")]
    enabled: bool,
    #[serde(default)]
    priority: i32,
}

fn default_deny_action() -> String {
    "deny".into()
}

fn default_contract_enabled() -> bool {
    true
}

async fn add_contract_handler(
    State(state): State<SharedApiState>,
    Json(req): Json<AddContractRequest>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let action = match req.action.as_str() {
        "allow" => rustedclaw_contracts::Action::Allow,
        "confirm" => rustedclaw_contracts::Action::Confirm,
        "warn" => rustedclaw_contracts::Action::Warn,
        "deny" => rustedclaw_contracts::Action::Deny,
        _ => return Err(StatusCode::BAD_REQUEST),
    };
    let contract = rustedclaw_contracts::Contract {
        name: req.name.clone(),
        description: req.description,
        trigger: req.trigger.into(),
        condition: req.condition,
        action,
        message: req.message,
        enabled: req.enabled,
        priority: req.priority,
    };
    state
        .contracts
        .add_contract(contract)
        .map_err(|_| StatusCode::BAD_REQUEST)?;
    Ok(Json(serde_json::json!({ "added": req.name })))
}

async fn delete_contract_handler(
    State(state): State<SharedApiState>,
    Path(name): Path<String>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    if state.contracts.remove_contract(&name) {
        Ok(Json(serde_json::json!({ "removed": name })))
    } else {
        Err(StatusCode::NOT_FOUND)
    }
}

// ── Telemetry / Usage / Budgets ───────────────────────────────────────────

async fn usage_handler(
    State(state): State<SharedApiState>,
) -> Json<rustedclaw_telemetry::UsageSnapshot> {
    Json(state.telemetry.usage_snapshot())
}

#[derive(Serialize, Deserialize)]
struct TraceListResponse {
    count: usize,
    traces: Vec<TraceSummaryDto>,
}

#[derive(Serialize, Deserialize)]
struct TraceSummaryDto {
    id: String,
    conversation_id: String,
    spans: usize,
    total_cost_usd: f64,
    total_tokens: u32,
    started_at: String,
    ended: bool,
}

async fn list_traces_handler(State(state): State<SharedApiState>) -> Json<TraceListResponse> {
    let traces = state.telemetry.recent_traces(50);
    let dtos: Vec<TraceSummaryDto> = traces
        .iter()
        .map(|t| TraceSummaryDto {
            id: t.id.clone(),
            conversation_id: t.conversation_id.clone(),
            spans: t.spans.len(),
            total_cost_usd: t.total_cost(),
            total_tokens: t.total_tokens(),
            started_at: t.started_at.to_rfc3339(),
            ended: t.ended_at.is_some(),
        })
        .collect();
    Json(TraceListResponse {
        count: dtos.len(),
        traces: dtos,
    })
}

async fn get_trace_handler(
    State(state): State<SharedApiState>,
    Path(id): Path<String>,
) -> Result<Json<rustedclaw_telemetry::Trace>, StatusCode> {
    state
        .telemetry
        .get_trace(&id)
        .map(Json)
        .ok_or(StatusCode::NOT_FOUND)
}

#[derive(Serialize, Deserialize)]
struct BudgetListResponse {
    count: usize,
    budgets: Vec<BudgetDto>,
}

#[derive(Serialize, Deserialize)]
struct BudgetDto {
    scope: String,
    max_usd: f64,
    max_tokens: u64,
    on_exceed: String,
}

async fn list_budgets_handler(State(state): State<SharedApiState>) -> Json<BudgetListResponse> {
    let budgets = state.telemetry.list_budgets();
    let dtos: Vec<BudgetDto> = budgets
        .iter()
        .map(|b| BudgetDto {
            scope: b.scope.to_string(),
            max_usd: b.max_usd,
            max_tokens: b.max_tokens,
            on_exceed: format!("{:?}", b.on_exceed),
        })
        .collect();
    Json(BudgetListResponse {
        count: dtos.len(),
        budgets: dtos,
    })
}

#[derive(Deserialize)]
struct AddBudgetRequest {
    scope: String,
    max_usd: f64,
    #[serde(default)]
    max_tokens: u64,
    #[serde(default = "default_on_exceed")]
    on_exceed: String,
}

fn default_on_exceed() -> String {
    "deny".into()
}

async fn add_budget_handler(
    State(state): State<SharedApiState>,
    Json(req): Json<AddBudgetRequest>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let scope = match req.scope.as_str() {
        "per_request" => rustedclaw_telemetry::BudgetScope::PerRequest,
        "per_session" => rustedclaw_telemetry::BudgetScope::PerSession,
        "daily" => rustedclaw_telemetry::BudgetScope::Daily,
        "monthly" => rustedclaw_telemetry::BudgetScope::Monthly,
        "total" => rustedclaw_telemetry::BudgetScope::Total,
        _ => return Err(StatusCode::BAD_REQUEST),
    };
    let action = match req.on_exceed.as_str() {
        "warn" => rustedclaw_telemetry::BudgetAction::Warn,
        _ => rustedclaw_telemetry::BudgetAction::Deny,
    };
    state.telemetry.add_budget(rustedclaw_telemetry::Budget {
        scope,
        max_usd: req.max_usd,
        max_tokens: req.max_tokens,
        on_exceed: action,
    });
    Ok(Json(serde_json::json!({ "added": req.scope })))
}

async fn delete_budget_handler(
    State(state): State<SharedApiState>,
    Path(scope): Path<String>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let scope_enum = match scope.as_str() {
        "per_request" => rustedclaw_telemetry::BudgetScope::PerRequest,
        "per_session" => rustedclaw_telemetry::BudgetScope::PerSession,
        "daily" => rustedclaw_telemetry::BudgetScope::Daily,
        "monthly" => rustedclaw_telemetry::BudgetScope::Monthly,
        "total" => rustedclaw_telemetry::BudgetScope::Total,
        _ => return Err(StatusCode::BAD_REQUEST),
    };
    if state.telemetry.remove_budget(&scope_enum) {
        Ok(Json(serde_json::json!({ "removed": scope })))
    } else {
        Err(StatusCode::NOT_FOUND)
    }
}

// ── Status ────────────────────────────────────────────────────────────────

#[derive(Serialize, Deserialize)]
struct StatusResponse {
    status: String,
    version: String,
    uptime_secs: u64,
    active_conversations: usize,
    memory_entries: usize,
    document_entries: usize,
    tools_count: usize,
    contracts_count: usize,
    session_cost_usd: f64,
    trace_count: usize,
    provider: String,
    workflow_engine: bool,
}

async fn status_handler(State(state): State<SharedApiState>) -> Json<StatusResponse> {
    let conversations = state.conversations.read().await;
    let memories = state.memories.read().await;
    let documents = state.documents.read().await;

    let uptime = chrono::Utc::now()
        .signed_duration_since(state.start_time)
        .num_seconds() as u64;

    Json(StatusResponse {
        status: "healthy".into(),
        version: env!("CARGO_PKG_VERSION").into(),
        uptime_secs: uptime,
        active_conversations: conversations.len(),
        memory_entries: memories.len(),
        document_entries: documents.len(),
        tools_count: state.tools.definitions().len(),
        contracts_count: state.contracts.active_count(),
        session_cost_usd: state.telemetry.usage_snapshot().session_cost_usd,
        trace_count: state.telemetry.trace_count(),
        provider: state.provider.name().into(),
        workflow_engine: state.workflow.is_some(),
    })
}

// ── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request;
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    use rustedclaw_core::error::ProviderError;
    use rustedclaw_core::provider::{ProviderRequest, ProviderResponse, Usage};

    /// Lightweight mock provider for gateway tests.
    struct MockProvider {
        response_text: String,
    }

    impl MockProvider {
        fn new(text: &str) -> Self {
            Self {
                response_text: text.to_string(),
            }
        }
    }

    #[async_trait::async_trait]
    impl Provider for MockProvider {
        fn name(&self) -> &str {
            "gateway_mock"
        }

        async fn complete(
            &self,
            _request: ProviderRequest,
        ) -> Result<ProviderResponse, ProviderError> {
            Ok(ProviderResponse {
                message: rustedclaw_core::message::Message::assistant(&self.response_text),
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

    fn test_api_state() -> SharedApiState {
        let provider: Arc<dyn Provider> = Arc::new(MockProvider::new("Mock response from agent"));
        let tools = Arc::new(rustedclaw_tools::default_registry());
        let identity = Identity::default();
        let event_bus = Arc::new(EventBus::default());

        Arc::new(ApiV1State {
            provider,
            model: "mock-model".into(),
            temperature: 0.7,
            tools,
            identity,
            event_bus,
            contracts: Arc::new(rustedclaw_contracts::ContractEngine::empty()),
            telemetry: Arc::new(rustedclaw_telemetry::TelemetryEngine::new()),
            conversations: RwLock::new(HashMap::new()),
            workflow: Some(Arc::new(rustedclaw_workflow::WorkflowEngine::default())),
            config: RwLock::new(rustedclaw_config::AppConfig::default()),
            start_time: chrono::Utc::now(),
            memories: RwLock::new(Vec::new()),
            documents: RwLock::new(Vec::new()),
            jobs: RwLock::new(Vec::new()),
            bearer_tokens: RwLock::new(Vec::new()),
        })
    }

    #[tokio::test]
    async fn list_tools() {
        let app = v1_router(test_api_state());

        let req = Request::builder()
            .uri("/tools")
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(req).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        let body = response.into_body().collect().await.unwrap().to_bytes();
        let json: ToolListResponse = serde_json::from_slice(&body).unwrap();
        assert!(json.count >= 7); // 7 built-in tools
        assert!(json.tools.iter().any(|t| t.name == "calculator"));
        assert!(json.tools.iter().any(|t| t.name == "web_search"));
        assert!(json.tools.iter().any(|t| t.name == "weather_lookup"));
        assert!(json.tools.iter().any(|t| t.name == "knowledge_base_query"));
    }

    #[tokio::test]
    async fn create_and_list_conversations() {
        let state = test_api_state();
        let app = v1_router(state.clone());

        // Create a conversation
        let req = Request::builder()
            .method("POST")
            .uri("/conversations")
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(req).await.unwrap();
        assert_eq!(response.status(), StatusCode::CREATED);

        let body = response.into_body().collect().await.unwrap().to_bytes();
        let created: CreateConversationResponse = serde_json::from_slice(&body).unwrap();
        assert!(!created.id.is_empty());

        // List conversations
        let app = v1_router(state.clone());
        let req = Request::builder()
            .uri("/conversations")
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(req).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        let body = response.into_body().collect().await.unwrap().to_bytes();
        let list: ConversationListResponse = serde_json::from_slice(&body).unwrap();
        assert_eq!(list.conversations.len(), 1);
    }

    #[tokio::test]
    async fn get_conversation_not_found() {
        let app = v1_router(test_api_state());

        let req = Request::builder()
            .uri("/conversations/nonexistent")
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(req).await.unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn context_debug_endpoint() {
        let app = v1_router(test_api_state());

        let body = serde_json::json!({
            "message": "What is Rust?",
            "memories": [{"content": "User likes Rust"}],
            "knowledge_chunks": [{"content": "Rust is a systems language", "source": "doc.md", "similarity": 0.9}]
        });

        let req = Request::builder()
            .method("POST")
            .uri("/context/debug")
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_string(&body).unwrap()))
            .unwrap();

        let response = app.oneshot(req).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        let body = response.into_body().collect().await.unwrap().to_bytes();
        let debug: ContextDebugResponse = serde_json::from_slice(&body).unwrap();

        assert!(debug.system_message.contains("[Long-Term Memory]"));
        assert!(debug.system_message.contains("[Retrieved Knowledge]"));
        assert!(debug.metadata.total_tokens > 0);
        assert!(!debug.tool_definitions.is_empty());
    }

    #[tokio::test]
    async fn chat_unknown_pattern() {
        let app = v1_router(test_api_state());

        let body = serde_json::json!({
            "message": "Hello",
            "pattern": "unknown_pattern"
        });

        let req = Request::builder()
            .method("POST")
            .uri("/chat")
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_string(&body).unwrap()))
            .unwrap();

        let response = app.oneshot(req).await.unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn list_routines_empty() {
        let app = v1_router(test_api_state());

        let req = Request::builder()
            .uri("/routines")
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(req).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        let body = response.into_body().collect().await.unwrap().to_bytes();
        let list: RoutineListResponse = serde_json::from_slice(&body).unwrap();
        assert_eq!(list.count, 0);
        assert!(list.routines.is_empty());
    }

    #[tokio::test]
    async fn create_and_list_routines() {
        let state = test_api_state();

        // Create a routine
        let body = serde_json::json!({
            "name": "daily_summary",
            "schedule": "0 9 * * *",
            "instruction": "Summarize my day",
            "target_channel": "telegram"
        });

        let app = v1_router(state.clone());
        let req = Request::builder()
            .method("POST")
            .uri("/routines")
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_string(&body).unwrap()))
            .unwrap();

        let response = app.oneshot(req).await.unwrap();
        assert_eq!(response.status(), StatusCode::CREATED);

        let body = response.into_body().collect().await.unwrap().to_bytes();
        let created: RoutineDto = serde_json::from_slice(&body).unwrap();
        assert_eq!(created.name, "daily_summary");
        assert_eq!(created.schedule, "0 9 * * *");

        // List routines
        let app = v1_router(state.clone());
        let req = Request::builder()
            .uri("/routines")
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(req).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        let body = response.into_body().collect().await.unwrap().to_bytes();
        let list: RoutineListResponse = serde_json::from_slice(&body).unwrap();
        assert_eq!(list.count, 1);
    }

    #[tokio::test]
    async fn create_routine_invalid_cron() {
        let app = v1_router(test_api_state());

        let body = serde_json::json!({
            "name": "bad",
            "schedule": "not valid",
            "instruction": "fail"
        });

        let req = Request::builder()
            .method("POST")
            .uri("/routines")
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_string(&body).unwrap()))
            .unwrap();

        let response = app.oneshot(req).await.unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn delete_routine() {
        let state = test_api_state();

        // Create first
        let body = serde_json::json!({
            "name": "to_delete",
            "schedule": "* * * * *",
            "instruction": "test"
        });

        let app = v1_router(state.clone());
        let req = Request::builder()
            .method("POST")
            .uri("/routines")
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_string(&body).unwrap()))
            .unwrap();
        app.oneshot(req).await.unwrap();

        // Delete
        let app = v1_router(state.clone());
        let req = Request::builder()
            .method("DELETE")
            .uri("/routines/to_delete")
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(req).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        // Verify deleted
        let app = v1_router(state.clone());
        let req = Request::builder()
            .uri("/routines")
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(req).await.unwrap();
        let body = response.into_body().collect().await.unwrap().to_bytes();
        let list: RoutineListResponse = serde_json::from_slice(&body).unwrap();
        assert_eq!(list.count, 0);
    }

    #[tokio::test]
    async fn delete_nonexistent_routine() {
        let app = v1_router(test_api_state());

        let req = Request::builder()
            .method("DELETE")
            .uri("/routines/nonexistent")
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(req).await.unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    // ── Memory endpoint tests ──────────────────────────────────────────

    #[tokio::test]
    async fn create_and_search_memory() {
        let state = test_api_state();

        // Create a memory
        let body = serde_json::json!({
            "content": "User prefers dark mode",
            "tags": ["preference"],
            "agent_id": "agent1"
        });

        let app = v1_router(state.clone());
        let req = Request::builder()
            .method("POST")
            .uri("/memory")
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_string(&body).unwrap()))
            .unwrap();

        let response = app.oneshot(req).await.unwrap();
        assert_eq!(response.status(), StatusCode::CREATED);

        // Search all memories
        let app = v1_router(state.clone());
        let req = Request::builder()
            .uri("/memory")
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(req).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        let body = response.into_body().collect().await.unwrap().to_bytes();
        let list: MemoryListResponse = serde_json::from_slice(&body).unwrap();
        assert_eq!(list.count, 1);
        assert!(list.memories[0].content.contains("dark mode"));
    }

    #[tokio::test]
    async fn list_agent_memory_filters() {
        let state = test_api_state();

        // Create memories for different agents
        for (content, agent) in [("Fact A", "agent1"), ("Fact B", "agent2")] {
            let body = serde_json::json!({ "content": content, "agent_id": agent });
            let app = v1_router(state.clone());
            let req = Request::builder()
                .method("POST")
                .uri("/memory")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_string(&body).unwrap()))
                .unwrap();
            app.oneshot(req).await.unwrap();
        }

        // List only agent1's memories
        let app = v1_router(state.clone());
        let req = Request::builder()
            .uri("/memory/agent/agent1")
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(req).await.unwrap();
        let body = response.into_body().collect().await.unwrap().to_bytes();
        let list: MemoryListResponse = serde_json::from_slice(&body).unwrap();
        assert_eq!(list.count, 1);
        assert!(list.memories[0].content.contains("Fact A"));
    }

    #[tokio::test]
    async fn delete_memory() {
        let state = test_api_state();

        // Create
        let body = serde_json::json!({ "content": "to delete" });
        let app = v1_router(state.clone());
        let req = Request::builder()
            .method("POST")
            .uri("/memory")
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_string(&body).unwrap()))
            .unwrap();

        let response = app.oneshot(req).await.unwrap();
        let body = response.into_body().collect().await.unwrap().to_bytes();
        let created: CreateMemoryResponse = serde_json::from_slice(&body).unwrap();

        // Delete
        let app = v1_router(state.clone());
        let req = Request::builder()
            .method("DELETE")
            .uri(format!("/memory/{}", created.id))
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(req).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        // Verify empty
        let app = v1_router(state.clone());
        let req = Request::builder()
            .uri("/memory")
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(req).await.unwrap();
        let body = response.into_body().collect().await.unwrap().to_bytes();
        let list: MemoryListResponse = serde_json::from_slice(&body).unwrap();
        assert_eq!(list.count, 0);
    }

    #[tokio::test]
    async fn delete_nonexistent_memory() {
        let app = v1_router(test_api_state());

        let req = Request::builder()
            .method("DELETE")
            .uri("/memory/nonexistent")
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(req).await.unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    // ── Document endpoint tests ────────────────────────────────────────

    #[tokio::test]
    async fn ingest_document() {
        let state = test_api_state();

        let body = serde_json::json!({
            "content": "Rust is a systems programming language",
            "source": "docs/overview.md",
            "chunk_index": 0
        });

        let app = v1_router(state.clone());
        let req = Request::builder()
            .method("POST")
            .uri("/documents")
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_string(&body).unwrap()))
            .unwrap();

        let response = app.oneshot(req).await.unwrap();
        assert_eq!(response.status(), StatusCode::CREATED);

        let body = response.into_body().collect().await.unwrap().to_bytes();
        let resp: IngestDocumentResponse = serde_json::from_slice(&body).unwrap();
        assert_eq!(resp.source, "docs/overview.md");
        assert_eq!(resp.chunk_index, 0);
    }

    // ── Job endpoint tests ─────────────────────────────────────────────

    #[tokio::test]
    async fn list_jobs_empty() {
        let app = v1_router(test_api_state());

        let req = Request::builder().uri("/jobs").body(Body::empty()).unwrap();

        let response = app.oneshot(req).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        let body = response.into_body().collect().await.unwrap().to_bytes();
        let list: JobListResponse = serde_json::from_slice(&body).unwrap();
        assert_eq!(list.count, 0);
    }

    #[tokio::test]
    async fn get_job_not_found() {
        let app = v1_router(test_api_state());

        let req = Request::builder()
            .uri("/jobs/nonexistent")
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(req).await.unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    // ── Channel endpoint tests ─────────────────────────────────────────

    #[tokio::test]
    async fn list_channels() {
        let app = v1_router(test_api_state());

        let req = Request::builder()
            .uri("/channels")
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(req).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        let body = response.into_body().collect().await.unwrap().to_bytes();
        let list: ChannelListResponse = serde_json::from_slice(&body).unwrap();
        // Should at least have "web" channel
        assert!(list.channels.iter().any(|c| c.name == "web"));
    }

    #[tokio::test]
    async fn test_web_channel() {
        let app = v1_router(test_api_state());

        let req = Request::builder()
            .method("POST")
            .uri("/channels/web/test")
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(req).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        let body = response.into_body().collect().await.unwrap().to_bytes();
        let resp: ChannelTestResponse = serde_json::from_slice(&body).unwrap();
        assert!(resp.success);
    }

    #[tokio::test]
    async fn test_unknown_channel() {
        let app = v1_router(test_api_state());

        let req = Request::builder()
            .method("POST")
            .uri("/channels/nonexistent/test")
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(req).await.unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    // ── Config endpoint tests ──────────────────────────────────────────

    #[tokio::test]
    async fn get_config_redacted() {
        let app = v1_router(test_api_state());

        let req = Request::builder()
            .uri("/config")
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(req).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        let body = response.into_body().collect().await.unwrap().to_bytes();
        let resp: ConfigResponse = serde_json::from_slice(&body).unwrap();
        assert!(resp.config.is_object());
    }

    // ── Status endpoint tests ──────────────────────────────────────────

    #[tokio::test]
    async fn status_endpoint() {
        let app = v1_router(test_api_state());

        let req = Request::builder()
            .uri("/status")
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(req).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        let body = response.into_body().collect().await.unwrap().to_bytes();
        let resp: StatusResponse = serde_json::from_slice(&body).unwrap();
        assert_eq!(resp.status, "healthy");
        assert!(resp.tools_count >= 7);
        assert!(resp.workflow_engine);
    }

    // ── Tool install endpoint tests ────────────────────────────────────

    #[tokio::test]
    async fn install_tool_stub() {
        let app = v1_router(test_api_state());

        let body = serde_json::json!({
            "name": "my_custom_tool",
            "description": "A custom tool"
        });

        let req = Request::builder()
            .method("POST")
            .uri("/tools/install")
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_string(&body).unwrap()))
            .unwrap();

        let response = app.oneshot(req).await.unwrap();
        assert_eq!(response.status(), StatusCode::CREATED);
    }

    #[tokio::test]
    async fn install_tool_empty_name() {
        let app = v1_router(test_api_state());

        let body = serde_json::json!({ "name": "" });

        let req = Request::builder()
            .method("POST")
            .uri("/tools/install")
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_string(&body).unwrap()))
            .unwrap();

        let response = app.oneshot(req).await.unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    // ── Streaming tests ──

    #[tokio::test]
    async fn chat_stream_returns_sse() {
        let app = v1_router(test_api_state());

        let body = serde_json::json!({
            "message": "Hello, streaming!",
            "pattern": "react"
        });

        let req = Request::builder()
            .method("POST")
            .uri("/chat/stream")
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_string(&body).unwrap()))
            .unwrap();

        let response = app.oneshot(req).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        // SSE response should have text/event-stream content type
        let content_type = response
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        assert!(
            content_type.contains("text/event-stream"),
            "Expected text/event-stream, got '{}'",
            content_type
        );

        // Read the body — should contain SSE events
        let body = response.into_body().collect().await.unwrap().to_bytes();
        let text = String::from_utf8_lossy(&body);
        assert!(
            text.contains("event: chunk") || text.contains("event: done"),
            "SSE body should contain chunk or done events, got: {}",
            text
        );
    }

    #[tokio::test]
    async fn chat_stream_contains_done_event() {
        let app = v1_router(test_api_state());

        let body = serde_json::json!({
            "message": "Test done event",
            "pattern": "react"
        });

        let req = Request::builder()
            .method("POST")
            .uri("/chat/stream")
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_string(&body).unwrap()))
            .unwrap();

        let response = app.oneshot(req).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        let body = response.into_body().collect().await.unwrap().to_bytes();
        let text = String::from_utf8_lossy(&body);

        // Must always end with a done event
        assert!(
            text.contains("event: done"),
            "Missing done event in SSE stream: {}",
            text
        );
    }

    #[tokio::test]
    async fn ws_upgrade_accepted() {
        // We can't do a full WS handshake with oneshot, but we can verify
        // the route exists and returns the right response for a non-upgrade request
        let app = v1_router(test_api_state());

        let req = Request::builder().uri("/ws").body(Body::empty()).unwrap();

        let response = app.oneshot(req).await.unwrap();
        // Without proper upgrade headers, axum returns 405 or similar
        // Just verify the route exists (not 404)
        assert_ne!(response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn log_stream_returns_sse() {
        let app = v1_router(test_api_state());

        let req = Request::builder().uri("/logs").body(Body::empty()).unwrap();

        let response = app.oneshot(req).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        let content_type = response
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        assert!(
            content_type.contains("text/event-stream"),
            "Expected text/event-stream, got '{}'",
            content_type
        );
    }
}
