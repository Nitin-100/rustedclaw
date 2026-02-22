//! Agent configuration and state types.

use serde::{Deserialize, Serialize};

/// Configuration for the agent's behavior.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentConfig {
    /// Default LLM provider to use
    pub default_provider: String,

    /// Default model to use
    pub default_model: String,

    /// Default temperature
    #[serde(default = "default_temp")]
    pub default_temperature: f32,

    /// Maximum tool call iterations per turn (safety limit)
    #[serde(default = "default_max_iterations")]
    pub max_tool_iterations: u32,

    /// Autonomy level
    #[serde(default)]
    pub autonomy: AutonomyLevel,
}

fn default_temp() -> f32 {
    0.7
}
fn default_max_iterations() -> u32 {
    25
}

/// How much freedom the agent has to act.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutonomyLevel {
    /// Can only read, never execute
    ReadOnly,
    /// Must ask permission for destructive actions (default)
    #[default]
    Supervised,
    /// Full autonomy — execute everything
    Full,
}

/// Runtime state of the agent.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AgentState {
    /// Whether the agent is currently processing a request
    pub is_busy: bool,

    /// Number of requests processed since startup
    pub requests_processed: u64,

    /// Total tokens consumed since startup
    pub total_tokens: u64,

    /// Current active conversations count
    pub active_conversations: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn autonomy_defaults_to_supervised() {
        let level = AutonomyLevel::default();
        assert!(matches!(level, AutonomyLevel::Supervised));
    }

    #[test]
    fn agent_state_starts_idle() {
        let state = AgentState::default();
        assert!(!state.is_busy);
        assert_eq!(state.requests_processed, 0);
    }
}
