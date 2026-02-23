//! Execution tracing, cost tracking, and budget enforcement for RustedClaw.
//!
//! Provides span-based tracing of every agent action (LLM calls, tool
//! executions, memory operations), real-time cost estimation with built-in
//! model pricing, and budget limits that can halt runaway API spend.

pub mod engine;
pub mod model;
pub mod pricing;

pub use engine::TelemetryEngine;
pub use model::{
    Budget, BudgetAction, BudgetScope, CostSummary, Span, SpanKind, Trace, UsageSnapshot,
};
pub use pricing::PricingTable;

/// Errors from the telemetry subsystem.
#[derive(Debug, thiserror::Error)]
pub enum TelemetryError {
    #[error("budget exceeded: {0}")]
    BudgetExceeded(String),

    #[error("unknown span id: {0}")]
    UnknownSpan(String),

    #[error("serialization error: {0}")]
    SerdeError(#[from] serde_json::Error),
}
