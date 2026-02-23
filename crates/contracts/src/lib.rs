//! Agent Contracts — declarative behavior specifications for AI agents.
//!
//! Contracts let users define guardrails for agent behavior via TOML config.
//! They act as a static-analysis layer that intercepts tool calls and LLM
//! responses *before* execution, enforcing rules like:
//!
//! - "Never delete files outside the workspace"
//! - "Block HTTP requests to internal IPs"
//! - "Require confirmation for purchases over $50"
//! - "Deny shell commands matching `rm -rf`"
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────┐    ┌──────────────┐    ┌─────────────┐
//! │  Agent Loop  │───▶│  Contract    │───▶│    Tool      │
//! │  (tool call) │    │  Engine      │    │  Registry    │
//! └─────────────┘    └──────────────┘    └─────────────┘
//!                          │
//!                    ┌─────┴─────┐
//!                    │  Verdict  │
//!                    │ Allow     │
//!                    │ Deny      │
//!                    │ Confirm   │
//!                    └───────────┘
//! ```
//!
//! # Example Contract
//!
//! ```toml
//! [[contracts]]
//! name = "no-delete-outside-workspace"
//! description = "Never delete files outside the workspace directory"
//! trigger = "tool:shell"
//! condition = 'args.command MATCHES "rm" AND args.command NOT MATCHES "^rm .*workspace"'
//! action = "deny"
//! message = "Blocked: cannot delete files outside workspace"
//! ```

mod engine;
mod model;
mod parser;

pub use engine::{ContractEngine, Verdict};
pub use model::{Action, Contract, ContractSet, Trigger};
pub use parser::parse_condition;

/// Re-export for convenience.
pub type ContractResult<T> = std::result::Result<T, ContractError>;

/// Errors from the contract subsystem.
#[derive(Debug, thiserror::Error)]
pub enum ContractError {
    #[error("invalid contract '{name}': {reason}")]
    InvalidContract { name: String, reason: String },

    #[error("condition parse error in contract '{name}': {detail}")]
    ConditionParseError { name: String, detail: String },

    #[error("contract file error: {0}")]
    IoError(#[from] std::io::Error),

    #[error("TOML parse error: {0}")]
    TomlError(#[from] toml::de::Error),
}
