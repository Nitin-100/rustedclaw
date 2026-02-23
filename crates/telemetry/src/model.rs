//! Data model for execution traces, spans, budgets, and cost summaries.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// ── Span ──────────────────────────────────────────────────────────────────

/// The kind of work a span represents.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SpanKind {
    /// An LLM completion call.
    LlmCall,
    /// A tool execution.
    ToolExecution,
    /// A memory read or write.
    MemoryOp,
    /// A contract evaluation.
    ContractCheck,
    /// Top-level turn (user message → final response).
    Turn,
}

impl std::fmt::Display for SpanKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::LlmCall => write!(f, "llm_call"),
            Self::ToolExecution => write!(f, "tool_execution"),
            Self::MemoryOp => write!(f, "memory_op"),
            Self::ContractCheck => write!(f, "contract_check"),
            Self::Turn => write!(f, "turn"),
        }
    }
}

/// A single traced execution unit.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Span {
    /// Unique identifier.
    pub id: String,
    /// Parent span id (None for root spans).
    pub parent_id: Option<String>,
    /// What kind of work this represents.
    pub kind: SpanKind,
    /// Human-readable label (e.g. tool name, model name).
    pub label: String,
    /// When the span started.
    pub started_at: DateTime<Utc>,
    /// When the span ended (None if still running).
    pub ended_at: Option<DateTime<Utc>>,
    /// Duration in milliseconds (computed on end).
    pub duration_ms: Option<u64>,
    /// Input tokens consumed (for LLM calls).
    pub input_tokens: Option<u32>,
    /// Output tokens produced (for LLM calls).
    pub output_tokens: Option<u32>,
    /// Estimated cost in USD (micro-dollars for precision).
    pub cost_usd: Option<f64>,
    /// Whether the operation succeeded.
    pub success: Option<bool>,
    /// Arbitrary metadata.
    pub metadata: serde_json::Map<String, serde_json::Value>,
}

impl Span {
    /// Create a new span with the given kind and label.
    pub fn new(kind: SpanKind, label: impl Into<String>) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            parent_id: None,
            kind,
            label: label.into(),
            started_at: Utc::now(),
            ended_at: None,
            duration_ms: None,
            input_tokens: None,
            output_tokens: None,
            cost_usd: None,
            success: None,
            metadata: serde_json::Map::new(),
        }
    }

    /// Set the parent span.
    pub fn with_parent(mut self, parent_id: impl Into<String>) -> Self {
        self.parent_id = Some(parent_id.into());
        self
    }

    /// Mark the span as ended with the given success status.
    pub fn end(&mut self, success: bool) {
        let now = Utc::now();
        self.ended_at = Some(now);
        self.duration_ms = Some(
            now.signed_duration_since(self.started_at)
                .num_milliseconds()
                .max(0) as u64,
        );
        self.success = Some(success);
    }

    /// Record token usage and compute cost.
    pub fn record_tokens(&mut self, input: u32, output: u32, cost: f64) {
        self.input_tokens = Some(input);
        self.output_tokens = Some(output);
        self.cost_usd = Some(cost);
    }

    /// Total tokens (input + output), or 0 if not recorded.
    pub fn total_tokens(&self) -> u32 {
        self.input_tokens.unwrap_or(0) + self.output_tokens.unwrap_or(0)
    }
}

// ── Trace ─────────────────────────────────────────────────────────────────

/// A collection of spans representing one conversation turn.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Trace {
    /// Unique trace id.
    pub id: String,
    /// Conversation id this trace belongs to.
    pub conversation_id: String,
    /// All spans in this trace.
    pub spans: Vec<Span>,
    /// When the trace started.
    pub started_at: DateTime<Utc>,
    /// When the trace ended.
    pub ended_at: Option<DateTime<Utc>>,
}

impl Trace {
    /// Create a new trace for a conversation.
    pub fn new(conversation_id: impl Into<String>) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            conversation_id: conversation_id.into(),
            spans: Vec::new(),
            started_at: Utc::now(),
            ended_at: None,
        }
    }

    /// Add a span to this trace.
    pub fn add_span(&mut self, span: Span) {
        self.spans.push(span);
    }

    /// Mark the trace as complete.
    pub fn end(&mut self) {
        self.ended_at = Some(Utc::now());
    }

    /// Total cost across all spans in USD.
    pub fn total_cost(&self) -> f64 {
        self.spans.iter().filter_map(|s| s.cost_usd).sum()
    }

    /// Total tokens across all spans.
    pub fn total_tokens(&self) -> u32 {
        self.spans.iter().map(|s| s.total_tokens()).sum()
    }

    /// Total duration in milliseconds.
    pub fn total_duration_ms(&self) -> u64 {
        self.spans.iter().filter_map(|s| s.duration_ms).sum()
    }

    /// Number of LLM calls in this trace.
    pub fn llm_call_count(&self) -> usize {
        self.spans
            .iter()
            .filter(|s| s.kind == SpanKind::LlmCall)
            .count()
    }

    /// Number of tool executions in this trace.
    pub fn tool_execution_count(&self) -> usize {
        self.spans
            .iter()
            .filter(|s| s.kind == SpanKind::ToolExecution)
            .count()
    }
}

// ── Budget ────────────────────────────────────────────────────────────────

/// The scope a budget applies to.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BudgetScope {
    /// Per individual request / turn.
    PerRequest,
    /// Per conversation session.
    PerSession,
    /// Rolling daily limit.
    Daily,
    /// Rolling monthly limit.
    Monthly,
    /// Total lifetime spend.
    Total,
}

impl std::fmt::Display for BudgetScope {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::PerRequest => write!(f, "per_request"),
            Self::PerSession => write!(f, "per_session"),
            Self::Daily => write!(f, "daily"),
            Self::Monthly => write!(f, "monthly"),
            Self::Total => write!(f, "total"),
        }
    }
}

/// A spending limit.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Budget {
    /// What scope this limit applies to.
    pub scope: BudgetScope,
    /// Maximum spend in USD.
    pub max_usd: f64,
    /// Maximum tokens (0 = unlimited).
    pub max_tokens: u64,
    /// Action when exceeded: "deny", "warn".
    pub on_exceed: BudgetAction,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum BudgetAction {
    /// Block the request.
    #[default]
    Deny,
    /// Log a warning but allow.
    Warn,
}

// ── Aggregated views ──────────────────────────────────────────────────────

/// Aggregated cost summary.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CostSummary {
    /// Total cost in USD.
    pub total_cost_usd: f64,
    /// Total input tokens.
    pub total_input_tokens: u64,
    /// Total output tokens.
    pub total_output_tokens: u64,
    /// Total number of LLM calls.
    pub llm_calls: u64,
    /// Total number of tool executions.
    pub tool_executions: u64,
    /// Total number of traces.
    pub trace_count: u64,
    /// Cost breakdown by model.
    pub by_model: Vec<ModelCost>,
    /// Time window start.
    pub from: DateTime<Utc>,
    /// Time window end.
    pub to: DateTime<Utc>,
}

/// Cost breakdown for a single model.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelCost {
    /// Model name.
    pub model: String,
    /// Total cost for this model.
    pub cost_usd: f64,
    /// Total input tokens.
    pub input_tokens: u64,
    /// Total output tokens.
    pub output_tokens: u64,
    /// Number of calls.
    pub calls: u64,
}

/// A point-in-time usage snapshot (for the /v1/usage endpoint).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageSnapshot {
    /// Current session cost.
    pub session_cost_usd: f64,
    /// Daily cost so far.
    pub daily_cost_usd: f64,
    /// Monthly cost so far.
    pub monthly_cost_usd: f64,
    /// Total lifetime cost.
    pub total_cost_usd: f64,
    /// Total tokens used this session.
    pub session_tokens: u64,
    /// Active budgets and their remaining amounts.
    pub budgets: Vec<BudgetStatus>,
    /// Number of traces recorded.
    pub trace_count: u64,
}

/// Status of a single budget.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BudgetStatus {
    /// Budget scope.
    pub scope: BudgetScope,
    /// Maximum allowed USD.
    pub max_usd: f64,
    /// Amount spent so far.
    pub spent_usd: f64,
    /// Remaining USD.
    pub remaining_usd: f64,
    /// Maximum tokens.
    pub max_tokens: u64,
    /// Tokens used.
    pub used_tokens: u64,
    /// Whether this budget is currently exceeded.
    pub exceeded: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn span_lifecycle() {
        let mut span = Span::new(SpanKind::LlmCall, "claude-sonnet-4");
        assert!(span.ended_at.is_none());
        assert_eq!(span.total_tokens(), 0);

        span.record_tokens(100, 50, 0.003);
        assert_eq!(span.total_tokens(), 150);
        assert!((span.cost_usd.unwrap() - 0.003).abs() < 1e-10);

        span.end(true);
        assert!(span.ended_at.is_some());
        assert!(span.success.unwrap());
        assert!(span.duration_ms.is_some());
    }

    #[test]
    fn span_with_parent() {
        let parent = Span::new(SpanKind::Turn, "user-turn");
        let child = Span::new(SpanKind::LlmCall, "gpt-4o").with_parent(&parent.id);
        assert_eq!(child.parent_id, Some(parent.id));
    }

    #[test]
    fn trace_aggregation() {
        let mut trace = Trace::new("conv-1");

        let mut s1 = Span::new(SpanKind::LlmCall, "claude-sonnet-4");
        s1.record_tokens(100, 50, 0.003);
        s1.end(true);
        trace.add_span(s1);

        let mut s2 = Span::new(SpanKind::ToolExecution, "shell");
        s2.end(true);
        trace.add_span(s2);

        let mut s3 = Span::new(SpanKind::LlmCall, "claude-sonnet-4");
        s3.record_tokens(200, 100, 0.006);
        s3.end(true);
        trace.add_span(s3);

        trace.end();

        assert_eq!(trace.total_tokens(), 450);
        assert!((trace.total_cost() - 0.009).abs() < 1e-10);
        assert_eq!(trace.llm_call_count(), 2);
        assert_eq!(trace.tool_execution_count(), 1);
        assert!(trace.ended_at.is_some());
    }

    #[test]
    fn span_kind_display() {
        assert_eq!(SpanKind::LlmCall.to_string(), "llm_call");
        assert_eq!(SpanKind::ToolExecution.to_string(), "tool_execution");
        assert_eq!(SpanKind::MemoryOp.to_string(), "memory_op");
        assert_eq!(SpanKind::ContractCheck.to_string(), "contract_check");
        assert_eq!(SpanKind::Turn.to_string(), "turn");
    }

    #[test]
    fn budget_scope_display() {
        assert_eq!(BudgetScope::PerRequest.to_string(), "per_request");
        assert_eq!(BudgetScope::Daily.to_string(), "daily");
        assert_eq!(BudgetScope::Monthly.to_string(), "monthly");
        assert_eq!(BudgetScope::Total.to_string(), "total");
    }

    #[test]
    fn span_serialization_roundtrip() {
        let mut span = Span::new(SpanKind::LlmCall, "gpt-4o");
        span.record_tokens(500, 200, 0.01);
        span.end(true);

        let json = serde_json::to_string(&span).unwrap();
        let roundtrip: Span = serde_json::from_str(&json).unwrap();

        assert_eq!(roundtrip.kind, span.kind);
        assert_eq!(roundtrip.label, "gpt-4o");
        assert_eq!(roundtrip.input_tokens, Some(500));
        assert_eq!(roundtrip.output_tokens, Some(200));
    }

    #[test]
    fn trace_serialization_roundtrip() {
        let mut trace = Trace::new("conv-42");
        let mut s = Span::new(SpanKind::ToolExecution, "calculator");
        s.end(true);
        trace.add_span(s);
        trace.end();

        let json = serde_json::to_string(&trace).unwrap();
        let roundtrip: Trace = serde_json::from_str(&json).unwrap();

        assert_eq!(roundtrip.conversation_id, "conv-42");
        assert_eq!(roundtrip.spans.len(), 1);
    }
}
