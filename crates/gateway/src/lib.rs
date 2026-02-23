//! HTTP API gateway for RustedClaw.
//!
//! Exposes REST endpoints for webhooks, health checks, pairing,
//! and the full v1 API with chat, conversations, tools, and
//! context debugging.
//!
//! Built on Axum for high performance async HTTP.

pub mod api_v1;
pub mod frontend;

use axum::{
    Router,
    extract::State,
    http::StatusCode,
    response::Json,
    routing::{get, post},
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::info;

use rustedclaw_agent::AgentLoop;
use rustedclaw_contracts::ContractEngine;
use rustedclaw_core::event::EventBus;
use rustedclaw_core::identity::{ContextPaths, Identity};
use rustedclaw_core::message::{Conversation, Message};

/// Shared application state for the gateway.
pub struct GatewayState {
    pub config: rustedclaw_config::AppConfig,
    pub pairing_code: Option<String>,
    pub bearer_tokens: Vec<String>,
    pub agent: Arc<AgentLoop>,
}

type SharedState = Arc<RwLock<GatewayState>>;

/// Build the Axum router with all gateway routes.
pub fn build_router(state: SharedState) -> Router {
    Router::new()
        .route("/health", get(health_handler))
        .route("/pair", post(pair_handler))
        .route("/webhook", post(webhook_handler))
        .layer(tower_http::trace::TraceLayer::new_for_http())
        .with_state(state)
}

/// Build the full router including v1 API.
pub fn build_full_router(legacy_state: SharedState, api_state: api_v1::SharedApiState) -> Router {
    let v1 = api_v1::v1_router(api_state); // Router<()> (state already applied)

    // Apply legacy state first, converting to Router<()>, then nest v1.
    Router::new()
        .route("/health", get(health_handler))
        .route("/pair", post(pair_handler))
        .route("/webhook", post(webhook_handler))
        .with_state(legacy_state) // Router<()>
        .nest("/v1", v1) // Both Router<()> now
        .merge(frontend::frontend_router()) // Serve embedded frontend
        .layer(tower_http::trace::TraceLayer::new_for_http())
}

/// Start the gateway HTTP server.
///
/// Memory-optimized: builds provider, tools, identity, and event bus
/// only ONCE and shares them via Arc between legacy and v1 state.
pub async fn start(config: rustedclaw_config::AppConfig) -> Result<(), Box<dyn std::error::Error>> {
    let host = config.gateway.host.clone();
    let port = config.gateway.port;
    let addr = format!("{host}:{port}");

    // Generate pairing code if required
    let pairing_code = if config.gateway.require_pairing {
        let code = format!("{:06}", rand_simple());
        info!(code = %code, "Pairing code generated — use POST /pair with X-Pairing-Code header");
        Some(code)
    } else {
        None
    };

    // === Build shared subsystems ONCE (no duplication) ===
    let router = rustedclaw_providers::router::build_from_config(&config);
    let provider = router
        .default()
        .expect("No default provider configured — set an API key");

    let context_paths = ContextPaths {
        global_dir: Some(rustedclaw_config::AppConfig::workspace_dir()),
        project_dir: None,
        extra_files: config
            .identity
            .extra_context_files
            .iter()
            .map(std::path::PathBuf::from)
            .collect(),
        system_prompt_override: config.identity.system_prompt_override.clone(),
    };
    let identity = Identity::load(&context_paths);
    let tools = Arc::new(rustedclaw_tools::default_registry());
    let event_bus = Arc::new(EventBus::default());

    // Build contract engine from config
    let contract_engine = {
        let mut contract_set = rustedclaw_contracts::ContractSet::new();
        for cc in &config.contracts {
            contract_set.add(rustedclaw_contracts::Contract {
                name: cc.name.clone(),
                description: cc.description.clone(),
                trigger: cc.trigger.clone().into(),
                condition: cc.condition.clone(),
                action: match cc.action.as_str() {
                    "allow" => rustedclaw_contracts::Action::Allow,
                    "confirm" => rustedclaw_contracts::Action::Confirm,
                    "warn" => rustedclaw_contracts::Action::Warn,
                    _ => rustedclaw_contracts::Action::Deny,
                },
                message: cc.message.clone(),
                enabled: cc.enabled,
                priority: cc.priority,
            });
        }
        Arc::new(
            ContractEngine::new(contract_set)
                .expect("invalid contract configuration"),
        )
    };

    // Shared agent for legacy routes (reuses same provider/tools/identity)
    let agent = Arc::new(
        AgentLoop::new(
            provider.clone(),
            &config.default_model,
            config.default_temperature,
            tools.clone(),
            identity.clone(),
            event_bus.clone(),
        )
        .with_max_tokens(config.default_max_tokens)
        .with_contracts(contract_engine.clone()),
    );

    // Build shared state for legacy routes.
    let legacy_state = Arc::new(RwLock::new(GatewayState {
        pairing_code,
        bearer_tokens: Vec::new(),
        config: config.clone(),
        agent,
    }));

    // Build v1 API state (reuses same provider/tools/identity/event_bus).
    let api_state = Arc::new(api_v1::ApiV1State {
        provider,
        model: config.default_model.clone(),
        temperature: config.default_temperature,
        tools,
        identity,
        event_bus,
        contracts: contract_engine,
        conversations: RwLock::new(HashMap::new()),
        workflow: Some(Arc::new(rustedclaw_workflow::WorkflowEngine::new(
            config.heartbeat.enabled,
            config.heartbeat.interval_minutes,
        ))),
        config: RwLock::new(config.clone()),
        start_time: chrono::Utc::now(),
        memories: RwLock::new(Vec::new()),
        documents: RwLock::new(Vec::new()),
        jobs: RwLock::new(Vec::new()),
    });

    let app = build_full_router(legacy_state, api_state);

    info!(addr = %addr, "Gateway starting with v1 API");
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}

// --- Handlers ---

#[derive(Serialize)]
struct HealthResponse {
    status: &'static str,
    version: &'static str,
}

async fn health_handler() -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok",
        version: env!("CARGO_PKG_VERSION"),
    })
}

#[derive(Deserialize)]
#[allow(dead_code)]
struct PairRequest {
    // Pairing code sent in X-Pairing-Code header
}

#[derive(Serialize)]
struct PairResponse {
    token: String,
}

async fn pair_handler(
    State(state): State<SharedState>,
    headers: axum::http::HeaderMap,
) -> Result<Json<PairResponse>, StatusCode> {
    let state_read = state.read().await;

    let expected_code = state_read.pairing_code.as_deref();

    if let Some(expected) = expected_code {
        let provided = headers.get("X-Pairing-Code").and_then(|v| v.to_str().ok());

        if provided != Some(expected) {
            return Err(StatusCode::UNAUTHORIZED);
        }
    }

    // Generate a bearer token
    let token = uuid::Uuid::new_v4().to_string();

    drop(state_read);
    state.write().await.bearer_tokens.push(token.clone());

    Ok(Json(PairResponse { token }))
}

#[derive(Deserialize)]
struct WebhookRequest {
    message: String,
}

#[derive(Serialize)]
struct WebhookResponse {
    response: String,
}

async fn webhook_handler(
    State(state): State<SharedState>,
    headers: axum::http::HeaderMap,
    Json(payload): Json<WebhookRequest>,
) -> Result<Json<WebhookResponse>, StatusCode> {
    let state_read = state.read().await;

    // Check bearer token
    if !state_read.bearer_tokens.is_empty() {
        let auth_header = headers
            .get("Authorization")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "));

        match auth_header {
            Some(token) if state_read.bearer_tokens.contains(&token.to_string()) => {}
            _ => return Err(StatusCode::UNAUTHORIZED),
        }
    }

    info!(message = %payload.message, "Webhook message received");

    // Route to agent loop
    let agent = state_read.agent.clone();
    drop(state_read); // Release the lock before async work

    let mut conv = Conversation::new();
    conv.push(Message::user(&payload.message));

    match agent.process(&mut conv).await {
        Ok(response) => Ok(Json(WebhookResponse { response })),
        Err(e) => {
            tracing::error!(error = %e, "Agent processing failed");
            Ok(Json(WebhookResponse {
                response: format!("Error processing message: {e}"),
            }))
        }
    }
}

/// Simple deterministic "random" number for pairing codes (no extra dependency).
fn rand_simple() -> u32 {
    use std::time::{SystemTime, UNIX_EPOCH};
    let seed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .subsec_nanos();
    seed % 1_000_000
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request;
    use tower::ServiceExt;

    fn test_state() -> SharedState {
        let config = rustedclaw_config::AppConfig::default();
        let router = rustedclaw_providers::router::build_from_config(&config);
        let provider = router.default().expect("No default provider configured");
        let context_paths = ContextPaths {
            global_dir: Some(rustedclaw_config::AppConfig::workspace_dir()),
            project_dir: None,
            extra_files: vec![],
            system_prompt_override: None,
        };
        let identity = Identity::load(&context_paths);
        let tools = Arc::new(rustedclaw_tools::default_registry());
        let event_bus = Arc::new(EventBus::default());
        let agent = Arc::new(
            AgentLoop::new(
                provider,
                &config.default_model,
                config.default_temperature,
                tools,
                identity,
                event_bus,
            )
            .with_max_tokens(config.default_max_tokens),
        );
        Arc::new(RwLock::new(GatewayState {
            config,
            pairing_code: None,
            bearer_tokens: Vec::new(),
            agent,
        }))
    }

    #[tokio::test]
    async fn health_endpoint() {
        let app = build_router(test_state());

        let req = Request::builder()
            .uri("/health")
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(req).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }
}
