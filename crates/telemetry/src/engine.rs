//! Thread-safe telemetry engine — collects spans, tracks costs,
//! enforces budgets, and serves usage reports.

use crate::TelemetryError;
use crate::model::*;
use crate::pricing::PricingTable;
use chrono::{Datelike, Utc};
use std::sync::RwLock;

/// The core telemetry engine.
///
/// Thread-safe via `RwLock`. Tracks execution spans, computes costs
/// using the built-in pricing table, and enforces spending budgets.
pub struct TelemetryEngine {
    /// Pricing table for cost computation.
    pricing: PricingTable,
    /// All recorded traces (most recent last).
    traces: RwLock<Vec<Trace>>,
    /// Configured budgets.
    budgets: RwLock<Vec<Budget>>,
    /// Running totals.
    totals: RwLock<RunningTotals>,
}

/// Internal running totals for fast budget checks.
#[derive(Debug, Default)]
struct RunningTotals {
    /// Total cost since engine creation.
    total_cost: f64,
    /// Total input tokens.
    total_input_tokens: u64,
    /// Total output tokens.
    total_output_tokens: u64,
    /// Total LLM calls.
    total_llm_calls: u64,
    /// Total tool executions.
    total_tool_execs: u64,
    /// Day of year for daily tracking.
    current_day: u32,
    /// Daily cost accumulator.
    daily_cost: f64,
    /// Daily tokens accumulator.
    daily_tokens: u64,
    /// Month for monthly tracking.
    current_month: u32,
    /// Monthly cost accumulator.
    monthly_cost: f64,
    /// Monthly tokens accumulator.
    monthly_tokens: u64,
}

impl TelemetryEngine {
    /// Create a new telemetry engine with default pricing.
    pub fn new() -> Self {
        let now = Utc::now();
        Self {
            pricing: PricingTable::with_defaults(),
            traces: RwLock::new(Vec::new()),
            budgets: RwLock::new(Vec::new()),
            totals: RwLock::new(RunningTotals {
                current_day: now.ordinal(),
                current_month: now.month(),
                ..Default::default()
            }),
        }
    }

    /// Create a telemetry engine with custom pricing.
    pub fn with_pricing(pricing: PricingTable) -> Self {
        let now = Utc::now();
        Self {
            pricing,
            traces: RwLock::new(Vec::new()),
            budgets: RwLock::new(Vec::new()),
            totals: RwLock::new(RunningTotals {
                current_day: now.ordinal(),
                current_month: now.month(),
                ..Default::default()
            }),
        }
    }

    /// Get a reference to the pricing table.
    pub fn pricing(&self) -> &PricingTable {
        &self.pricing
    }

    // ── Budget management ─────────────────────────────────────────────

    /// Add a budget limit.
    pub fn add_budget(&self, budget: Budget) {
        let mut budgets = self.budgets.write().unwrap();
        // Replace existing budget with same scope
        budgets.retain(|b| b.scope != budget.scope);
        budgets.push(budget);
    }

    /// Remove a budget by scope.
    pub fn remove_budget(&self, scope: &BudgetScope) -> bool {
        let mut budgets = self.budgets.write().unwrap();
        let before = budgets.len();
        budgets.retain(|b| &b.scope != scope);
        budgets.len() < before
    }

    /// List all configured budgets.
    pub fn list_budgets(&self) -> Vec<Budget> {
        self.budgets.read().unwrap().clone()
    }

    /// Check if a budget would be exceeded by an estimated cost.
    /// Returns Ok(()) if within budget, Err with details if exceeded.
    pub fn check_budget(&self, estimated_cost: f64) -> Result<(), TelemetryError> {
        let budgets = self.budgets.read().unwrap();
        let totals = self.totals.read().unwrap();

        for budget in budgets.iter() {
            let current_cost = match budget.scope {
                BudgetScope::PerRequest => 0.0, // per-request checks against the request cost alone
                BudgetScope::PerSession | BudgetScope::Total => totals.total_cost,
                BudgetScope::Daily => totals.daily_cost,
                BudgetScope::Monthly => totals.monthly_cost,
            };

            let projected = if budget.scope == BudgetScope::PerRequest {
                estimated_cost
            } else {
                current_cost + estimated_cost
            };

            if budget.max_usd > 0.0 && projected > budget.max_usd {
                if budget.on_exceed == BudgetAction::Deny {
                    return Err(TelemetryError::BudgetExceeded(format!(
                        "{} budget: projected ${:.4} exceeds limit ${:.4} (current: ${:.4})",
                        budget.scope, projected, budget.max_usd, current_cost
                    )));
                }
                tracing::warn!(
                    scope = %budget.scope,
                    projected = projected,
                    limit = budget.max_usd,
                    "Budget warning: approaching limit"
                );
            }
        }

        Ok(())
    }

    // ── Trace management ──────────────────────────────────────────────

    /// Start a new trace for a conversation turn.
    pub fn start_trace(&self, conversation_id: impl Into<String>) -> String {
        let trace = Trace::new(conversation_id);
        let id = trace.id.clone();
        let mut traces = self.traces.write().unwrap();

        // Auto-prune completed traces if too many accumulate
        const MAX_TRACES: usize = 5_000;
        if traces.len() >= MAX_TRACES {
            // Remove oldest completed traces first
            let drain_count = MAX_TRACES / 10;
            let mut removed = 0;
            traces.retain(|t| {
                if removed >= drain_count {
                    return true;
                }
                if t.ended_at.is_some() {
                    removed += 1;
                    return false;
                }
                true
            });
        }

        traces.push(trace);
        id
    }

    /// End a trace.
    pub fn end_trace(&self, trace_id: &str) {
        let mut traces = self.traces.write().unwrap();
        if let Some(trace) = traces.iter_mut().find(|t| t.id == trace_id) {
            trace.end();
        }
    }

    /// Record a completed span in a trace and update running totals.
    pub fn record_span(&self, trace_id: &str, span: Span) {
        // Update running totals
        {
            let mut totals = self.totals.write().unwrap();
            let now = Utc::now();

            // Roll over daily counter
            if now.ordinal() != totals.current_day {
                totals.current_day = now.ordinal();
                totals.daily_cost = 0.0;
                totals.daily_tokens = 0;
            }

            // Roll over monthly counter
            if now.month() != totals.current_month {
                totals.current_month = now.month();
                totals.monthly_cost = 0.0;
                totals.monthly_tokens = 0;
            }

            if let Some(cost) = span.cost_usd {
                totals.total_cost += cost;
                totals.daily_cost += cost;
                totals.monthly_cost += cost;
            }

            let tokens = span.total_tokens() as u64;
            totals.total_input_tokens += span.input_tokens.unwrap_or(0) as u64;
            totals.total_output_tokens += span.output_tokens.unwrap_or(0) as u64;
            totals.daily_tokens += tokens;
            totals.monthly_tokens += tokens;

            match span.kind {
                SpanKind::LlmCall => totals.total_llm_calls += 1,
                SpanKind::ToolExecution => totals.total_tool_execs += 1,
                _ => {}
            }
        }

        // Add span to trace
        let mut traces = self.traces.write().unwrap();
        if let Some(trace) = traces.iter_mut().find(|t| t.id == trace_id) {
            trace.add_span(span);
        }
    }

    /// Compute cost for an LLM call using the pricing table.
    pub fn compute_cost(&self, model: &str, input_tokens: u32, output_tokens: u32) -> f64 {
        self.pricing
            .compute_cost(model, input_tokens, output_tokens)
    }

    // ── Queries ───────────────────────────────────────────────────────

    /// Get a specific trace by ID.
    pub fn get_trace(&self, trace_id: &str) -> Option<Trace> {
        let traces = self.traces.read().unwrap();
        traces.iter().find(|t| t.id == trace_id).cloned()
    }

    /// List recent traces (most recent first).
    pub fn recent_traces(&self, limit: usize) -> Vec<Trace> {
        let traces = self.traces.read().unwrap();
        traces.iter().rev().take(limit).cloned().collect()
    }

    /// Get traces for a specific conversation.
    pub fn traces_for_conversation(&self, conversation_id: &str) -> Vec<Trace> {
        let traces = self.traces.read().unwrap();
        traces
            .iter()
            .filter(|t| t.conversation_id == conversation_id)
            .cloned()
            .collect()
    }

    /// Total number of traces recorded.
    pub fn trace_count(&self) -> usize {
        self.traces.read().unwrap().len()
    }

    /// Get a real-time usage snapshot.
    pub fn usage_snapshot(&self) -> UsageSnapshot {
        let totals = self.totals.read().unwrap();
        let budgets_cfg = self.budgets.read().unwrap();

        let budget_statuses: Vec<BudgetStatus> = budgets_cfg
            .iter()
            .map(|b| {
                let (spent, used_tokens) = match b.scope {
                    BudgetScope::Daily => (totals.daily_cost, totals.daily_tokens),
                    BudgetScope::Monthly => (totals.monthly_cost, totals.monthly_tokens),
                    BudgetScope::PerRequest => (0.0, 0),
                    BudgetScope::PerSession | BudgetScope::Total => (
                        totals.total_cost,
                        totals.total_input_tokens + totals.total_output_tokens,
                    ),
                };

                BudgetStatus {
                    scope: b.scope.clone(),
                    max_usd: b.max_usd,
                    spent_usd: spent,
                    remaining_usd: (b.max_usd - spent).max(0.0),
                    max_tokens: b.max_tokens,
                    used_tokens,
                    exceeded: b.max_usd > 0.0 && spent > b.max_usd,
                }
            })
            .collect();

        UsageSnapshot {
            session_cost_usd: totals.total_cost,
            daily_cost_usd: totals.daily_cost,
            monthly_cost_usd: totals.monthly_cost,
            total_cost_usd: totals.total_cost,
            session_tokens: totals.total_input_tokens + totals.total_output_tokens,
            budgets: budget_statuses,
            trace_count: self.traces.read().unwrap().len() as u64,
        }
    }

    /// Generate a cost summary for a time window.
    pub fn cost_summary(
        &self,
        from: chrono::DateTime<Utc>,
        to: chrono::DateTime<Utc>,
    ) -> CostSummary {
        let traces = self.traces.read().unwrap();

        let mut total_cost = 0.0f64;
        let mut total_input = 0u64;
        let mut total_output = 0u64;
        let mut llm_calls = 0u64;
        let mut tool_execs = 0u64;
        let mut trace_count = 0u64;
        let mut by_model: std::collections::HashMap<String, (f64, u64, u64, u64)> =
            std::collections::HashMap::new();

        for trace in traces.iter() {
            if trace.started_at < from || trace.started_at > to {
                continue;
            }
            trace_count += 1;

            for span in &trace.spans {
                if let Some(cost) = span.cost_usd {
                    total_cost += cost;
                }
                let input = span.input_tokens.unwrap_or(0) as u64;
                let output = span.output_tokens.unwrap_or(0) as u64;
                total_input += input;
                total_output += output;

                match span.kind {
                    SpanKind::LlmCall => {
                        llm_calls += 1;
                        let entry = by_model.entry(span.label.clone()).or_insert((0.0, 0, 0, 0));
                        entry.0 += span.cost_usd.unwrap_or(0.0);
                        entry.1 += input;
                        entry.2 += output;
                        entry.3 += 1;
                    }
                    SpanKind::ToolExecution => tool_execs += 1,
                    _ => {}
                }
            }
        }

        let mut model_costs: Vec<ModelCost> = by_model
            .into_iter()
            .map(|(model, (cost, inp, out, calls))| ModelCost {
                model,
                cost_usd: cost,
                input_tokens: inp,
                output_tokens: out,
                calls,
            })
            .collect();
        model_costs.sort_by(|a, b| b.cost_usd.partial_cmp(&a.cost_usd).unwrap());

        CostSummary {
            total_cost_usd: total_cost,
            total_input_tokens: total_input,
            total_output_tokens: total_output,
            llm_calls,
            tool_executions: tool_execs,
            trace_count,
            by_model: model_costs,
            from,
            to,
        }
    }

    /// Prune traces older than a given age.
    pub fn prune_before(&self, cutoff: chrono::DateTime<Utc>) -> usize {
        let mut traces = self.traces.write().unwrap();
        let before = traces.len();
        traces.retain(|t| t.started_at >= cutoff);
        before - traces.len()
    }
}

impl Default for TelemetryEngine {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::SpanKind;
    use chrono::Duration;

    fn make_engine() -> TelemetryEngine {
        TelemetryEngine::new()
    }

    #[test]
    fn start_and_end_trace() {
        let engine = make_engine();
        let trace_id = engine.start_trace("conv-1");
        assert_eq!(engine.trace_count(), 1);

        engine.end_trace(&trace_id);
        let trace = engine.get_trace(&trace_id).unwrap();
        assert!(trace.ended_at.is_some());
    }

    #[test]
    fn record_span_updates_totals() {
        let engine = make_engine();
        let trace_id = engine.start_trace("conv-1");

        let mut span = Span::new(SpanKind::LlmCall, "anthropic/claude-sonnet-4");
        span.record_tokens(1000, 500, 0.0105);
        span.end(true);
        engine.record_span(&trace_id, span);

        let snapshot = engine.usage_snapshot();
        assert!((snapshot.session_cost_usd - 0.0105).abs() < 1e-10);
        assert_eq!(snapshot.session_tokens, 1500);
    }

    #[test]
    fn compute_cost_from_pricing() {
        let engine = make_engine();
        let cost = engine.compute_cost("anthropic/claude-sonnet-4", 1000, 500);
        assert!((cost - 0.0105).abs() < 1e-10);
    }

    #[test]
    fn budget_deny() {
        let engine = make_engine();
        engine.add_budget(Budget {
            scope: BudgetScope::Daily,
            max_usd: 0.05,
            max_tokens: 0,
            on_exceed: BudgetAction::Deny,
        });

        // First check should pass
        assert!(engine.check_budget(0.01).is_ok());

        // Record some spending
        let trace_id = engine.start_trace("conv-1");
        let mut span = Span::new(SpanKind::LlmCall, "openai/gpt-4o");
        span.record_tokens(5000, 2000, 0.04);
        span.end(true);
        engine.record_span(&trace_id, span);

        // Now a $0.02 call should be denied (0.04 + 0.02 = 0.06 > 0.05)
        let result = engine.check_budget(0.02);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("budget"));
    }

    #[test]
    fn budget_warn_allows() {
        let engine = make_engine();
        engine.add_budget(Budget {
            scope: BudgetScope::Daily,
            max_usd: 0.01,
            max_tokens: 0,
            on_exceed: BudgetAction::Warn,
        });

        // Record exceeding amount
        let trace_id = engine.start_trace("conv-1");
        let mut span = Span::new(SpanKind::LlmCall, "model");
        span.record_tokens(1000, 500, 0.02);
        span.end(true);
        engine.record_span(&trace_id, span);

        // Warn action should not deny
        assert!(engine.check_budget(0.01).is_ok());
    }

    #[test]
    fn per_request_budget() {
        let engine = make_engine();
        engine.add_budget(Budget {
            scope: BudgetScope::PerRequest,
            max_usd: 0.05,
            max_tokens: 0,
            on_exceed: BudgetAction::Deny,
        });

        // Even with prior spending, per-request only checks the single request
        let trace_id = engine.start_trace("conv-1");
        let mut span = Span::new(SpanKind::LlmCall, "model");
        span.record_tokens(1000, 500, 0.10);
        span.end(true);
        engine.record_span(&trace_id, span);

        // $0.01 should pass (per-request only sees $0.01)
        assert!(engine.check_budget(0.01).is_ok());
        // $0.10 should fail (exceeds $0.05 per-request limit)
        assert!(engine.check_budget(0.10).is_err());
    }

    #[test]
    fn remove_budget() {
        let engine = make_engine();
        engine.add_budget(Budget {
            scope: BudgetScope::Daily,
            max_usd: 1.0,
            max_tokens: 0,
            on_exceed: BudgetAction::Deny,
        });
        assert_eq!(engine.list_budgets().len(), 1);

        assert!(engine.remove_budget(&BudgetScope::Daily));
        assert_eq!(engine.list_budgets().len(), 0);

        assert!(!engine.remove_budget(&BudgetScope::Daily));
    }

    #[test]
    fn recent_traces() {
        let engine = make_engine();
        for i in 0..5 {
            engine.start_trace(format!("conv-{i}"));
        }

        let recent = engine.recent_traces(3);
        assert_eq!(recent.len(), 3);
        assert_eq!(recent[0].conversation_id, "conv-4");
        assert_eq!(recent[2].conversation_id, "conv-2");
    }

    #[test]
    fn traces_for_conversation() {
        let engine = make_engine();
        engine.start_trace("conv-a");
        engine.start_trace("conv-b");
        engine.start_trace("conv-a");

        let traces = engine.traces_for_conversation("conv-a");
        assert_eq!(traces.len(), 2);
    }

    #[test]
    fn cost_summary_filters_by_time() {
        let engine = make_engine();

        let trace_id = engine.start_trace("conv-1");
        let mut span = Span::new(SpanKind::LlmCall, "openai/gpt-4o");
        span.record_tokens(1000, 500, 0.01);
        span.end(true);
        engine.record_span(&trace_id, span);

        // Summary for a future window should be empty
        let future = Utc::now() + Duration::hours(1);
        let summary = engine.cost_summary(future, future + Duration::hours(1));
        assert_eq!(summary.trace_count, 0);
        assert!((summary.total_cost_usd - 0.0).abs() < 1e-10);

        // Summary including now should have the data
        let past = Utc::now() - Duration::hours(1);
        let summary = engine.cost_summary(past, future);
        assert_eq!(summary.trace_count, 1);
        assert!((summary.total_cost_usd - 0.01).abs() < 1e-10);
        assert_eq!(summary.llm_calls, 1);
        assert_eq!(summary.by_model.len(), 1);
        assert_eq!(summary.by_model[0].model, "openai/gpt-4o");
    }

    #[test]
    fn prune_old_traces() {
        let engine = make_engine();
        engine.start_trace("conv-1");
        engine.start_trace("conv-2");
        assert_eq!(engine.trace_count(), 2);

        // Prune with future cutoff should remove all
        let future = Utc::now() + Duration::hours(1);
        let pruned = engine.prune_before(future);
        assert_eq!(pruned, 2);
        assert_eq!(engine.trace_count(), 0);
    }

    #[test]
    fn usage_snapshot_with_budgets() {
        let engine = make_engine();
        engine.add_budget(Budget {
            scope: BudgetScope::Daily,
            max_usd: 1.0,
            max_tokens: 10000,
            on_exceed: BudgetAction::Deny,
        });

        let trace_id = engine.start_trace("conv-1");
        let mut span = Span::new(SpanKind::LlmCall, "model");
        span.record_tokens(100, 50, 0.002);
        span.end(true);
        engine.record_span(&trace_id, span);

        let snapshot = engine.usage_snapshot();
        assert_eq!(snapshot.budgets.len(), 1);
        let b = &snapshot.budgets[0];
        assert_eq!(b.scope, BudgetScope::Daily);
        assert!((b.spent_usd - 0.002).abs() < 1e-10);
        assert!((b.remaining_usd - 0.998).abs() < 1e-10);
        assert!(!b.exceeded);
    }

    #[test]
    fn multiple_spans_accumulate() {
        let engine = make_engine();
        let trace_id = engine.start_trace("conv-1");

        for i in 0..5 {
            let mut span = Span::new(SpanKind::LlmCall, format!("model-{i}"));
            span.record_tokens(100, 50, 0.001);
            span.end(true);
            engine.record_span(&trace_id, span);
        }

        let mut tool_span = Span::new(SpanKind::ToolExecution, "calculator");
        tool_span.end(true);
        engine.record_span(&trace_id, tool_span);

        let snapshot = engine.usage_snapshot();
        assert!((snapshot.session_cost_usd - 0.005).abs() < 1e-10);
        assert_eq!(snapshot.session_tokens, 750); // 5 * 150

        let trace = engine.get_trace(&trace_id).unwrap();
        assert_eq!(trace.llm_call_count(), 5);
        assert_eq!(trace.tool_execution_count(), 1);
    }

    #[test]
    fn budget_replace_same_scope() {
        let engine = make_engine();
        engine.add_budget(Budget {
            scope: BudgetScope::Daily,
            max_usd: 1.0,
            max_tokens: 0,
            on_exceed: BudgetAction::Deny,
        });
        engine.add_budget(Budget {
            scope: BudgetScope::Daily,
            max_usd: 5.0,
            max_tokens: 0,
            on_exceed: BudgetAction::Warn,
        });

        let budgets = engine.list_budgets();
        assert_eq!(budgets.len(), 1);
        assert!((budgets[0].max_usd - 5.0).abs() < 1e-10);
    }

    #[test]
    fn default_engine() {
        let engine = TelemetryEngine::default();
        assert_eq!(engine.trace_count(), 0);
        assert!(engine.pricing().len() >= 20);
    }
}
