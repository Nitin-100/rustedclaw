//! Configuration loading, validation, and management for RustedClaw.
//!
//! Loads configuration from `~/.rustedclaw/config.toml` with environment
//! variable overrides. Validates all settings at startup.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// The root configuration structure.
///
/// Maps directly to `~/.rustedclaw/config.toml`.
#[derive(Clone, Serialize, Deserialize)]
pub struct AppConfig {
    /// API key (can be overridden per-provider)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,

    /// Default LLM provider
    #[serde(default = "default_provider")]
    pub default_provider: String,

    /// Default model
    #[serde(default = "default_model")]
    pub default_model: String,

    /// Default temperature
    #[serde(default = "default_temperature")]
    pub default_temperature: f32,

    /// Default max tokens per LLM response
    #[serde(default = "default_max_tokens")]
    pub default_max_tokens: u32,

    /// Memory configuration
    #[serde(default)]
    pub memory: MemoryConfig,

    /// Gateway configuration
    #[serde(default)]
    pub gateway: GatewayConfig,

    /// Autonomy and security settings
    #[serde(default)]
    pub autonomy: AutonomyConfig,

    /// Runtime configuration
    #[serde(default)]
    pub runtime: RuntimeConfig,

    /// Provider-specific configurations
    #[serde(default)]
    pub providers: HashMap<String, ProviderConfig>,

    /// Channel configurations
    #[serde(default)]
    pub channels_config: HashMap<String, ChannelConfig>,

    /// Identity configuration
    #[serde(default)]
    pub identity: IdentityConfig,

    /// Heartbeat configuration
    #[serde(default)]
    pub heartbeat: HeartbeatConfig,

    /// Tunnel configuration
    #[serde(default)]
    pub tunnel: TunnelConfig,

    /// Secrets configuration
    #[serde(default)]
    pub secrets: SecretsConfig,

    /// Routine (scheduled task) configurations
    #[serde(default)]
    pub routines: Vec<RoutineConfig>,

    /// Agent contract configurations (behavior guardrails)
    #[serde(default)]
    pub contracts: Vec<ContractConfig>,

    /// Telemetry, cost tracking, and budget configuration
    #[serde(default)]
    pub telemetry: TelemetryConfig,
}

fn default_provider() -> String {
    "openrouter".into()
}
fn default_model() -> String {
    "anthropic/claude-sonnet-4".into()
}
fn default_temperature() -> f32 {
    0.7
}
fn default_max_tokens() -> u32 {
    4096
}

/// Redact a secret string for Debug output: show first 4 chars + "***".
fn redact(s: &Option<String>) -> &'static str {
    match s {
        Some(_) => "[REDACTED]",
        None => "None",
    }
}

impl std::fmt::Debug for AppConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AppConfig")
            .field("api_key", &redact(&self.api_key))
            .field("default_provider", &self.default_provider)
            .field("default_model", &self.default_model)
            .field("default_temperature", &self.default_temperature)
            .field("default_max_tokens", &self.default_max_tokens)
            .field("memory", &self.memory)
            .field("gateway", &self.gateway)
            .field("autonomy", &self.autonomy)
            .field("runtime", &self.runtime)
            .field("providers", &self.providers)
            .field("channels_config", &self.channels_config)
            .field("identity", &self.identity)
            .field("heartbeat", &self.heartbeat)
            .field("tunnel", &self.tunnel)
            .field("secrets", &self.secrets)
            .field("routines", &self.routines)
            .field("contracts", &self.contracts)
            .field("telemetry", &self.telemetry)
            .finish()
    }
}

impl std::fmt::Debug for ProviderConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProviderConfig")
            .field("api_key", &redact(&self.api_key))
            .field("api_url", &self.api_url)
            .field("default_model", &self.default_model)
            .finish()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryConfig {
    #[serde(default = "default_memory_backend")]
    pub backend: String,

    #[serde(default = "default_true")]
    pub auto_save: bool,

    #[serde(default = "default_embedding_provider")]
    pub embedding_provider: String,

    #[serde(default = "default_vector_weight")]
    pub vector_weight: f32,

    #[serde(default = "default_keyword_weight")]
    pub keyword_weight: f32,
}

fn default_memory_backend() -> String {
    "sqlite".into()
}
fn default_embedding_provider() -> String {
    "none".into()
}
fn default_vector_weight() -> f32 {
    0.7
}
fn default_keyword_weight() -> f32 {
    0.3
}
fn default_true() -> bool {
    true
}

impl Default for MemoryConfig {
    fn default() -> Self {
        Self {
            backend: default_memory_backend(),
            auto_save: true,
            embedding_provider: default_embedding_provider(),
            vector_weight: default_vector_weight(),
            keyword_weight: default_keyword_weight(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GatewayConfig {
    #[serde(default = "default_port")]
    pub port: u16,

    #[serde(default = "default_host")]
    pub host: String,

    #[serde(default = "default_true")]
    pub require_pairing: bool,

    #[serde(default)]
    pub allow_public_bind: bool,
}

fn default_port() -> u16 {
    42617
}
fn default_host() -> String {
    "127.0.0.1".into()
}

impl Default for GatewayConfig {
    fn default() -> Self {
        Self {
            port: default_port(),
            host: default_host(),
            require_pairing: true,
            allow_public_bind: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutonomyConfig {
    #[serde(default = "default_autonomy_level")]
    pub level: String,

    #[serde(default = "default_true")]
    pub workspace_only: bool,

    #[serde(default)]
    pub allowed_commands: Vec<String>,

    #[serde(default)]
    pub forbidden_paths: Vec<String>,

    #[serde(default)]
    pub allowed_roots: Vec<String>,
}

fn default_autonomy_level() -> String {
    "supervised".into()
}

impl Default for AutonomyConfig {
    fn default() -> Self {
        Self {
            level: default_autonomy_level(),
            workspace_only: true,
            allowed_commands: vec![
                "git".into(),
                "npm".into(),
                "cargo".into(),
                "ls".into(),
                "cat".into(),
                "grep".into(),
            ],
            forbidden_paths: vec![
                "/etc".into(),
                "/root".into(),
                "/proc".into(),
                "/sys".into(),
                "~/.ssh".into(),
                "~/.gnupg".into(),
                "~/.aws".into(),
            ],
            allowed_roots: vec![],
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeConfig {
    #[serde(default = "default_runtime_kind")]
    pub kind: String,
}

fn default_runtime_kind() -> String {
    "native".into()
}

impl Default for RuntimeConfig {
    fn default() -> Self {
        Self {
            kind: default_runtime_kind(),
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct ProviderConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_url: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_model: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelConfig {
    #[serde(default)]
    pub enabled: bool,

    /// Allowlist of sender IDs. Empty = deny all. ["*"] = allow all.
    #[serde(default)]
    pub allowed_users: Vec<String>,

    /// Channel-specific settings (varies by platform)
    #[serde(flatten)]
    pub settings: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdentityConfig {
    #[serde(default = "default_identity_format")]
    pub format: String,

    /// Override the system prompt entirely (skips file loading)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub system_prompt_override: Option<String>,

    /// Additional context files to load (absolute paths)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub extra_context_files: Vec<String>,

    /// Whether to load project-local .rustedclaw/ context (default: true)
    #[serde(default = "default_true")]
    pub load_project_context: bool,
}

fn default_identity_format() -> String {
    "rustedclaw".into()
}

impl Default for IdentityConfig {
    fn default() -> Self {
        Self {
            format: default_identity_format(),
            system_prompt_override: None,
            extra_context_files: vec![],
            load_project_context: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HeartbeatConfig {
    #[serde(default)]
    pub enabled: bool,

    #[serde(default = "default_heartbeat_interval")]
    pub interval_minutes: u32,
}

fn default_heartbeat_interval() -> u32 {
    30
}

impl Default for HeartbeatConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            interval_minutes: default_heartbeat_interval(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TunnelConfig {
    #[serde(default = "default_tunnel_provider")]
    pub provider: String,
}

fn default_tunnel_provider() -> String {
    "none".into()
}

impl Default for TunnelConfig {
    fn default() -> Self {
        Self {
            provider: default_tunnel_provider(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecretsConfig {
    #[serde(default = "default_true")]
    pub encrypt: bool,
}

impl Default for SecretsConfig {
    fn default() -> Self {
        Self { encrypt: true }
    }
}

/// Configuration for a scheduled routine (cron task).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoutineConfig {
    /// Unique name for this routine
    pub name: String,

    /// Cron expression (5-field: minute hour dom month dow)
    pub schedule: String,

    /// The action to perform when triggered
    pub action: RoutineAction,

    /// Which channel to send results to (optional)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_channel: Option<String>,

    /// Whether this routine is enabled (default: true)
    #[serde(default = "default_true")]
    pub enabled: bool,
}

/// The action a routine performs when triggered.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RoutineAction {
    /// Run a prompt through the agent
    AgentTask {
        prompt: String,
        #[serde(default)]
        context: Option<String>,
    },
    /// Run a specific tool
    RunTool {
        tool: String,
        #[serde(default)]
        input: serde_json::Value,
    },
    /// Send a message to a channel
    SendMessage {
        channel: String,
        #[serde(default)]
        recipient: Option<String>,
        template: String,
    },
}

/// Configuration for an agent behavior contract (guardrail).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContractConfig {
    /// Unique name for this contract
    pub name: String,

    /// Human-readable description
    #[serde(default)]
    pub description: String,

    /// What triggers this contract (e.g. "tool:shell", "tool:*", "response")
    pub trigger: String,

    /// Condition expression (e.g. `args.command CONTAINS "rm -rf"`)
    #[serde(default)]
    pub condition: String,

    /// Action to take: "deny", "confirm", "warn", "allow"
    #[serde(default = "default_deny")]
    pub action: String,

    /// Message to display when the contract fires
    #[serde(default)]
    pub message: String,

    /// Whether this contract is enabled
    #[serde(default = "default_true")]
    pub enabled: bool,

    /// Priority (higher = evaluated first)
    #[serde(default)]
    pub priority: i32,
}

fn default_deny() -> String {
    "deny".into()
}

/// Telemetry, cost tracking, and budget configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TelemetryConfig {
    /// Whether telemetry is enabled
    #[serde(default = "default_true")]
    pub enabled: bool,

    /// Spending budgets
    #[serde(default)]
    pub budgets: Vec<BudgetConfig>,

    /// Custom model pricing overrides (model name → pricing)
    #[serde(default)]
    pub custom_pricing: HashMap<String, PricingOverrideConfig>,
}

impl Default for TelemetryConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            budgets: vec![],
            custom_pricing: HashMap::new(),
        }
    }
}

/// A spending budget limit.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BudgetConfig {
    /// Scope: "per_request", "per_session", "daily", "monthly", "total"
    pub scope: String,

    /// Maximum spend in USD
    pub max_usd: f64,

    /// Maximum tokens (0 = unlimited)
    #[serde(default)]
    pub max_tokens: u64,

    /// Action when exceeded: "deny" or "warn"
    #[serde(default = "default_deny")]
    pub on_exceed: String,
}

/// Custom per-million-token pricing for a model.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PricingOverrideConfig {
    /// Price per 1M input tokens in USD
    pub input_per_m: f64,
    /// Price per 1M output tokens in USD
    pub output_per_m: f64,
}

impl AppConfig {
    /// Load configuration from the default path (~/.rustedclaw/config.toml).
    ///
    /// Also checks environment variables for API keys:
    /// - `RUSTEDCLAW_API_KEY` (highest priority)
    /// - `OPENROUTER_API_KEY`
    /// - `OPENAI_API_KEY`
    pub fn load() -> Result<Self, ConfigError> {
        let config_dir = Self::config_dir();
        let config_path = config_dir.join("config.toml");
        let mut config = Self::load_from(&config_path)?;

        // Environment variable overrides (highest priority)
        if config.api_key.is_none() {
            config.api_key = std::env::var("RUSTEDCLAW_API_KEY")
                .ok()
                .or_else(|| std::env::var("OPENROUTER_API_KEY").ok())
                .or_else(|| std::env::var("OPENAI_API_KEY").ok());
        }

        // Allow env var to override default provider
        if let Ok(provider) = std::env::var("RUSTEDCLAW_PROVIDER") {
            config.default_provider = provider;
        }

        // Allow env var to override default model
        if let Ok(model) = std::env::var("RUSTEDCLAW_MODEL") {
            config.default_model = model;
        }

        Ok(config)
    }

    /// Load configuration from a specific file path.
    pub fn load_from(path: &Path) -> Result<Self, ConfigError> {
        if !path.exists() {
            tracing::info!("No config file found at {}, using defaults", path.display());
            return Ok(Self::default());
        }

        let content = std::fs::read_to_string(path).map_err(|e| ConfigError::ReadError {
            path: path.to_path_buf(),
            reason: e.to_string(),
        })?;

        let config: Self = toml::from_str(&content).map_err(|e| ConfigError::ParseError {
            path: path.to_path_buf(),
            reason: e.to_string(),
        })?;

        config.validate()?;
        Ok(config)
    }

    /// Get the configuration directory path.
    pub fn config_dir() -> PathBuf {
        dirs_home().join(".rustedclaw")
    }

    /// Get the workspace directory path.
    pub fn workspace_dir() -> PathBuf {
        Self::config_dir().join("workspace")
    }

    /// Validate the configuration.
    fn validate(&self) -> Result<(), ConfigError> {
        if self.default_temperature < 0.0 || self.default_temperature > 2.0 {
            return Err(ConfigError::ValidationError(
                "default_temperature must be between 0.0 and 2.0".into(),
            ));
        }

        if self.memory.vector_weight + self.memory.keyword_weight <= 0.0 {
            return Err(ConfigError::ValidationError(
                "vector_weight + keyword_weight must be > 0".into(),
            ));
        }

        Ok(())
    }

    /// Check if an API key is available (from config or environment).
    pub fn has_api_key(&self) -> bool {
        self.api_key.is_some()
    }

    /// Generate a default config TOML string (for `onboard` command).
    pub fn default_toml() -> String {
        let config = Self::default();
        toml::to_string_pretty(&config).unwrap_or_default()
    }
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            api_key: None,
            default_provider: default_provider(),
            default_model: default_model(),
            default_temperature: default_temperature(),
            default_max_tokens: default_max_tokens(),
            memory: MemoryConfig::default(),
            gateway: GatewayConfig::default(),
            autonomy: AutonomyConfig::default(),
            runtime: RuntimeConfig::default(),
            providers: HashMap::new(),
            channels_config: HashMap::new(),
            identity: IdentityConfig::default(),
            heartbeat: HeartbeatConfig::default(),
            tunnel: TunnelConfig::default(),
            secrets: SecretsConfig::default(),
            routines: vec![],
            contracts: vec![],
            telemetry: TelemetryConfig::default(),
        }
    }
}

/// Get the user's home directory.
fn dirs_home() -> PathBuf {
    #[cfg(target_os = "windows")]
    {
        std::env::var("USERPROFILE")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("C:\\Users\\Default"))
    }
    #[cfg(not(target_os = "windows"))]
    {
        std::env::var("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("/tmp"))
    }
}

/// Configuration errors.
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("Failed to read config file at {path}: {reason}")]
    ReadError { path: PathBuf, reason: String },

    #[error("Failed to parse config file at {path}: {reason}")]
    ParseError { path: PathBuf, reason: String },

    #[error("Configuration validation failed: {0}")]
    ValidationError(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_is_valid() {
        let config = AppConfig::default();
        assert_eq!(config.default_provider, "openrouter");
        assert_eq!(config.gateway.port, 42617);
        assert!(config.autonomy.workspace_only);
    }

    #[test]
    fn config_roundtrip_toml() {
        let config = AppConfig::default();
        let toml_str = toml::to_string_pretty(&config).unwrap();
        let parsed: AppConfig = toml::from_str(&toml_str).unwrap();
        assert_eq!(parsed.default_provider, config.default_provider);
        assert_eq!(parsed.gateway.port, config.gateway.port);
    }

    #[test]
    fn invalid_temperature_rejected() {
        let config = AppConfig {
            default_temperature: 5.0,
            ..AppConfig::default()
        };
        assert!(config.validate().is_err());
    }

    #[test]
    fn missing_config_file_returns_defaults() {
        let result = AppConfig::load_from(Path::new("/nonexistent/config.toml"));
        assert!(result.is_ok());
        let config = result.unwrap();
        assert_eq!(config.default_provider, "openrouter");
    }

    #[test]
    fn default_toml_generation() {
        let toml_str = AppConfig::default_toml();
        assert!(toml_str.contains("openrouter"));
        assert!(toml_str.contains("42617"));
    }

    #[test]
    fn routine_config_parsing() {
        let toml_str = r#"
[[routines]]
name = "daily_summary"
schedule = "0 9 * * *"
target_channel = "telegram"
[routines.action]
type = "agent_task"
prompt = "Summarize my day"

[[routines]]
name = "health_check"
schedule = "*/5 * * * *"
[routines.action]
type = "run_tool"
tool = "http_request"
[routines.action.input]
url = "https://myapp.com/health"
"#;
        let config: AppConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.routines.len(), 2);
        assert_eq!(config.routines[0].name, "daily_summary");
        assert_eq!(config.routines[0].schedule, "0 9 * * *");
        assert_eq!(config.routines[0].target_channel, Some("telegram".into()));
        assert!(matches!(
            config.routines[0].action,
            RoutineAction::AgentTask { .. }
        ));

        assert_eq!(config.routines[1].name, "health_check");
        assert!(matches!(
            config.routines[1].action,
            RoutineAction::RunTool { .. }
        ));
    }

    #[test]
    fn routine_action_agent_task() {
        let action = RoutineAction::AgentTask {
            prompt: "Hello".into(),
            context: Some("extra".into()),
        };
        let json = serde_json::to_string(&action).unwrap();
        assert!(json.contains("agent_task"));
        assert!(json.contains("Hello"));
    }

    #[test]
    fn routine_action_send_message() {
        let action = RoutineAction::SendMessage {
            channel: "telegram".into(),
            recipient: Some("user123".into()),
            template: "Good morning!".into(),
        };
        let json = serde_json::to_string(&action).unwrap();
        assert!(json.contains("send_message"));
        assert!(json.contains("telegram"));
    }

    #[test]
    fn default_config_has_no_routines() {
        let config = AppConfig::default();
        assert!(config.routines.is_empty());
    }
}
