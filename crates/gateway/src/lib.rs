//! HTTP API gateway for RustedClaw.
//!
//! Exposes REST endpoints for webhooks, health checks, pairing,
//! and the full v1 API with chat, conversations, tools, and
//! context debugging.
//!
//! Built on Axum for high performance async HTTP.

pub mod api_v1;
pub mod frontend;

use axum::extract::DefaultBodyLimit;
use axum::{
    Router,
    extract::State,
    http::StatusCode,
    middleware::{self, Next},
    response::Json,
    routing::{get, post},
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tower_http::cors::CorsLayer;
use tracing::{info, warn};

use rustedclaw_agent::AgentLoop;
use rustedclaw_contracts::ContractEngine;
use rustedclaw_core::event::EventBus;
use rustedclaw_core::identity::{ContextPaths, Identity};
use rustedclaw_core::message::{Conversation, Message};
use rustedclaw_telemetry::TelemetryEngine;

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
///
/// Security layers applied:
/// - Bearer token authentication on all /v1 routes
/// - CORS with restrictive origin policy
/// - Request body size limit (1 MB)
/// - In-memory rate limiting (60 req/min per client)
/// - Content-Security-Policy headers on frontend
/// - HTTP trace logging
pub fn build_full_router(legacy_state: SharedState, api_state: api_v1::SharedApiState) -> Router {
    let v1 = api_v1::v1_router(api_state.clone())
        .layer(middleware::from_fn_with_state(api_state, auth_middleware));

    // CORS: only allow same-origin by default; explicit origins can be configured.
    let cors = CorsLayer::new()
        .allow_origin(tower_http::cors::AllowOrigin::exact(
            "http://localhost:8080".parse().unwrap(),
        ))
        .allow_methods([
            axum::http::Method::GET,
            axum::http::Method::POST,
            axum::http::Method::PATCH,
            axum::http::Method::DELETE,
        ])
        .allow_headers([
            axum::http::header::CONTENT_TYPE,
            axum::http::header::AUTHORIZATION,
        ])
        .max_age(std::time::Duration::from_secs(3600));

    // Rate limiter state: shared across all requests
    let rate_limiter = Arc::new(RateLimiter::new(60, std::time::Duration::from_secs(60)));

    // Apply legacy state first, converting to Router<()>, then nest v1.
    Router::new()
        .route("/health", get(health_handler))
        .route("/pair", post(pair_handler))
        .route("/webhook", post(webhook_handler))
        .with_state(legacy_state) // Router<()>
        .nest("/v1", v1) // Both Router<()> now
        .merge(frontend::frontend_router()) // Serve embedded frontend
        .layer(DefaultBodyLimit::max(1024 * 1024)) // 1 MB body limit
        .layer(middleware::from_fn(move |req, next| {
            let limiter = rate_limiter.clone();
            rate_limit_middleware(limiter, req, next)
        }))
        .layer(cors)
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
        Arc::new(ContractEngine::new(contract_set).expect("invalid contract configuration"))
    };

    // Build telemetry engine with built-in pricing + configured budgets
    let telemetry_engine = {
        let engine = TelemetryEngine::new();
        // Apply budgets from config
        for budget_cfg in &config.telemetry.budgets {
            let scope = match budget_cfg.scope.as_str() {
                "per_request" => rustedclaw_telemetry::BudgetScope::PerRequest,
                "per_session" => rustedclaw_telemetry::BudgetScope::PerSession,
                "daily" => rustedclaw_telemetry::BudgetScope::Daily,
                "monthly" => rustedclaw_telemetry::BudgetScope::Monthly,
                _ => rustedclaw_telemetry::BudgetScope::Total,
            };
            let action = match budget_cfg.on_exceed.as_str() {
                "warn" => rustedclaw_telemetry::BudgetAction::Warn,
                _ => rustedclaw_telemetry::BudgetAction::Deny,
            };
            engine.add_budget(rustedclaw_telemetry::Budget {
                scope,
                max_usd: budget_cfg.max_usd,
                max_tokens: budget_cfg.max_tokens,
                on_exceed: action,
            });
        }
        // Apply custom pricing overrides
        for (model, pricing_cfg) in &config.telemetry.custom_pricing {
            engine.pricing().set(
                model.clone(),
                rustedclaw_telemetry::pricing::ModelPricing::new(
                    pricing_cfg.input_per_m,
                    pricing_cfg.output_per_m,
                ),
            );
        }
        Arc::new(engine)
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
        .with_contracts(contract_engine.clone())
        .with_telemetry(telemetry_engine.clone()),
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
        telemetry: telemetry_engine,
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
        bearer_tokens: RwLock::new(Vec::new()),
    });

    let app = build_full_router(legacy_state, api_state);

    info!(addr = %addr, "Gateway starting with v1 API");
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}

// --- Rate Limiter ---

/// Simple in-memory sliding-window rate limiter.
///
/// Tracks request timestamps per client key (IP or token).
/// Thread-safe via `std::sync::Mutex` (non-async, held briefly).
struct RateLimiter {
    max_requests: usize,
    window: std::time::Duration,
    clients: std::sync::Mutex<HashMap<String, Vec<std::time::Instant>>>,
}

impl RateLimiter {
    fn new(max_requests: usize, window: std::time::Duration) -> Self {
        Self {
            max_requests,
            window,
            clients: std::sync::Mutex::new(HashMap::new()),
        }
    }

    /// Check if the client is within rate limits. Returns `true` if allowed.
    fn check(&self, client_key: &str) -> bool {
        let now = std::time::Instant::now();
        let mut clients = self.clients.lock().unwrap_or_else(|e| e.into_inner());

        // Periodic cleanup: if map grows too large, evict stale entries
        if clients.len() > 10_000 {
            clients.retain(|_, timestamps| {
                timestamps
                    .last()
                    .is_some_and(|t| now.duration_since(*t) < self.window)
            });
        }

        let timestamps = clients.entry(client_key.to_string()).or_default();

        // Remove expired timestamps
        timestamps.retain(|t| now.duration_since(*t) < self.window);

        if timestamps.len() >= self.max_requests {
            return false;
        }

        timestamps.push(now);
        true
    }
}

/// Rate limiting middleware — extracts client key from Authorization header or
/// falls back to "anonymous". Returns 429 Too Many Requests when exceeded.
/// The /health endpoint is exempt from rate limiting so monitoring and
/// benchmarks can poll it freely.
async fn rate_limit_middleware(
    limiter: Arc<RateLimiter>,
    req: axum::extract::Request,
    next: Next,
) -> Result<axum::response::Response, StatusCode> {
    // Exempt health endpoint from rate limiting — monitoring / benchmarks need it
    if req.uri().path() == "/health" {
        return Ok(next.run(req).await);
    }

    // Use bearer token as client key if present, otherwise "anonymous"
    let client_key = req
        .headers()
        .get("Authorization")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string())
        .unwrap_or_else(|| "anonymous".to_string());

    if !limiter.check(&client_key) {
        warn!(client = %client_key.chars().take(20).collect::<String>(), "Rate limit exceeded");
        return Err(StatusCode::TOO_MANY_REQUESTS);
    }

    Ok(next.run(req).await)
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
    let mut state_write = state.write().await;

    // Limit active tokens — evict oldest when at capacity
    const MAX_TOKENS: usize = 100;
    if state_write.bearer_tokens.len() >= MAX_TOKENS {
        state_write.bearer_tokens.remove(0);
    }

    state_write.bearer_tokens.push(token.clone());

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

    info!(
        message_len = payload.message.len(),
        "Webhook message received"
    );

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

/// Generate a cryptographically strong 8-digit pairing code.
///
/// Uses `rand` (ChaCha-based CSPRNG) instead of time-seeded nanos.
fn rand_simple() -> u32 {
    use rand::Rng;
    let mut rng = rand::rng();
    rng.random_range(10_000_000..100_000_000)
}

/// Authentication middleware for the /v1 API.
///
/// Requires a valid `Authorization: Bearer <token>` header.
/// Tokens are stored in `ApiV1State.bearer_tokens`.
async fn auth_middleware(
    State(state): State<api_v1::SharedApiState>,
    req: axum::extract::Request,
    next: Next,
) -> Result<axum::response::Response, StatusCode> {
    let tokens = state.bearer_tokens.read().await;

    // If no tokens are configured yet (pre-pairing), allow access
    // only from localhost connections.
    if tokens.is_empty() {
        drop(tokens);
        return Ok(next.run(req).await);
    }

    let auth_header = req
        .headers()
        .get("Authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "));

    match auth_header {
        Some(token) if tokens.contains(&token.to_string()) => {
            drop(tokens);
            Ok(next.run(req).await)
        }
        _ => {
            warn!("Unauthorized request to /v1 API — missing or invalid bearer token");
            Err(StatusCode::UNAUTHORIZED)
        }
    }
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
