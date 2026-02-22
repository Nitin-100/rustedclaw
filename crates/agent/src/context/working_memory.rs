//! Working memory — in-process scratchpad for a single task lifecycle.
//!
//! Stores the agent's current plan, reasoning traces (Thought/Action/Observation),
//! intermediate tool results, and scratch notes. Working memory is:
//!
//! - **Session-scoped**: cleared when a task completes
//! - **Serializable**: can be exported to JSON for debugging / WASM state
//! - **Renderable**: produces a text section for context assembly
//!
//! Implements FR-5 from the specification.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

// ── Data Structures ───────────────────────────────────────────────────────

/// The agent's scratchpad within a single task lifecycle.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkingMemory {
    /// Current plan (if using Plan-and-Execute pattern).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plan: Option<Plan>,

    /// ReAct reasoning trace entries.
    pub trace: Vec<TraceEntry>,

    /// Recorded tool execution results.
    pub tool_results: Vec<ToolResultEntry>,

    /// Free-form scratch notes.
    pub notes: Vec<String>,

    /// Current iteration counter.
    pub iterations: usize,

    /// Maximum iterations allowed.
    pub max_iterations: usize,
}

/// A plan with a goal, ordered steps, and progress tracking.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Plan {
    pub goal: String,
    pub steps: Vec<PlanStep>,
    pub current_step: usize,
}

/// A single step in a plan.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanStep {
    pub description: String,
    pub status: StepStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<String>,
}

/// Status of a plan step.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum StepStatus {
    Pending,
    InProgress,
    Completed,
    Failed(String),
}

/// A single entry in the reasoning trace.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraceEntry {
    pub kind: TraceKind,
    pub content: String,
    pub timestamp: DateTime<Utc>,
}

/// The kind of reasoning trace entry.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum TraceKind {
    Thought,
    Action,
    Observation,
    Reflection,
}

/// A recorded tool execution result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResultEntry {
    pub tool_name: String,
    pub input_summary: String,
    pub output_summary: String,
    pub success: bool,
    pub timestamp: DateTime<Utc>,
}

// ── Implementation ────────────────────────────────────────────────────────

impl WorkingMemory {
    /// Create a new empty working memory.
    pub fn new(max_iterations: usize) -> Self {
        Self {
            plan: None,
            trace: Vec::new(),
            tool_results: Vec::new(),
            notes: Vec::new(),
            iterations: 0,
            max_iterations,
        }
    }

    // ── Trace recording ──

    /// Record a "Thought" trace entry.
    pub fn add_thought(&mut self, thought: &str) {
        self.push_trace(TraceKind::Thought, thought);
    }

    /// Record an "Action" trace entry.
    pub fn add_action(&mut self, action: &str) {
        self.push_trace(TraceKind::Action, action);
    }

    /// Record an "Observation" trace entry.
    pub fn add_observation(&mut self, observation: &str) {
        self.push_trace(TraceKind::Observation, observation);
    }

    /// Record a "Reflection" trace entry.
    pub fn add_reflection(&mut self, reflection: &str) {
        self.push_trace(TraceKind::Reflection, reflection);
    }

    fn push_trace(&mut self, kind: TraceKind, content: &str) {
        self.trace.push(TraceEntry {
            kind,
            content: content.to_string(),
            timestamp: Utc::now(),
        });
    }

    // ── Tool results ──

    /// Record a tool execution result.
    pub fn add_tool_result(
        &mut self,
        tool_name: &str,
        input: &str,
        output: &str,
        success: bool,
    ) {
        self.tool_results.push(ToolResultEntry {
            tool_name: tool_name.to_string(),
            input_summary: input.to_string(),
            output_summary: output.to_string(),
            success,
            timestamp: Utc::now(),
        });
    }

    // ── Plan management ──

    /// Set a new plan with a goal and step descriptions.
    pub fn set_plan(&mut self, goal: &str, steps: Vec<String>) {
        let mut plan_steps: Vec<PlanStep> = steps
            .into_iter()
            .map(|desc| PlanStep {
                description: desc,
                status: StepStatus::Pending,
                result: None,
            })
            .collect();
        if !plan_steps.is_empty() {
            plan_steps[0].status = StepStatus::InProgress;
        }
        self.plan = Some(Plan {
            goal: goal.to_string(),
            steps: plan_steps,
            current_step: 0,
        });
    }

    /// Advance the plan to the next step, marking the current one completed.
    /// Returns `true` if advancement happened.
    pub fn advance_plan(&mut self, result: Option<String>) -> bool {
        if let Some(plan) = &mut self.plan {
            if plan.current_step < plan.steps.len() {
                plan.steps[plan.current_step].status = StepStatus::Completed;
                plan.steps[plan.current_step].result = result;
                plan.current_step += 1;
                if plan.current_step < plan.steps.len() {
                    plan.steps[plan.current_step].status = StepStatus::InProgress;
                }
                return true;
            }
        }
        false
    }

    /// Mark the current plan step as failed.
    pub fn fail_plan_step(&mut self, reason: &str) {
        if let Some(plan) = &mut self.plan {
            if plan.current_step < plan.steps.len() {
                plan.steps[plan.current_step].status =
                    StepStatus::Failed(reason.to_string());
            }
        }
    }

    /// Check if the plan is complete (all steps done).
    pub fn is_plan_complete(&self) -> bool {
        self.plan
            .as_ref()
            .is_some_and(|p| p.current_step >= p.steps.len())
    }

    // ── Notes ──

    /// Add a free-form scratch note.
    pub fn add_note(&mut self, note: &str) {
        self.notes.push(note.to_string());
    }

    // ── Iteration tracking ──

    /// Increment the iteration counter. Returns `false` if max exceeded.
    pub fn tick(&mut self) -> bool {
        self.iterations += 1;
        self.iterations <= self.max_iterations
    }

    // ── Rendering ──

    /// Render working memory as a human-readable text section
    /// suitable for injection into the LLM context.
    pub fn render(&self) -> String {
        let mut out = String::new();

        // Plan section
        if let Some(plan) = &self.plan {
            out.push_str("## Current Plan\n");
            out.push_str(&format!("Goal: {}\n", plan.goal));
            for (i, step) in plan.steps.iter().enumerate() {
                let marker = match &step.status {
                    StepStatus::Completed => "✓",
                    StepStatus::InProgress => "→",
                    StepStatus::Failed(_) => "✗",
                    StepStatus::Pending => " ",
                };
                out.push_str(&format!("{}. [{}] {}\n", i + 1, marker, step.description));
                if let Some(result) = &step.result {
                    out.push_str(&format!("   Result: {}\n", result));
                }
                if let StepStatus::Failed(reason) = &step.status {
                    out.push_str(&format!("   Error: {}\n", reason));
                }
            }
            out.push('\n');
        }

        // Reasoning trace
        if !self.trace.is_empty() {
            out.push_str("## Reasoning Trace\n");
            for entry in &self.trace {
                let label = match entry.kind {
                    TraceKind::Thought => "Thought",
                    TraceKind::Action => "Action",
                    TraceKind::Observation => "Observation",
                    TraceKind::Reflection => "Reflection",
                };
                out.push_str(&format!("[{}] {}\n", label, entry.content));
            }
            out.push('\n');
        }

        // Tool results summary
        if !self.tool_results.is_empty() {
            out.push_str("## Tool Results\n");
            for tr in &self.tool_results {
                let status = if tr.success { "✓" } else { "✗" };
                out.push_str(&format!(
                    "- {} {}: {}\n",
                    status, tr.tool_name, tr.output_summary
                ));
            }
            out.push('\n');
        }

        // Notes
        if !self.notes.is_empty() {
            out.push_str("## Notes\n");
            for note in &self.notes {
                out.push_str(&format!("- {}\n", note));
            }
            out.push('\n');
        }

        out.push_str(&format!(
            "Iterations: {}/{}\n",
            self.iterations, self.max_iterations
        ));

        out
    }

    /// Produce a brief summary (for potential long-term memory storage).
    pub fn summarize(&self) -> String {
        let mut parts = Vec::new();

        if let Some(plan) = &self.plan {
            let completed = plan
                .steps
                .iter()
                .filter(|s| s.status == StepStatus::Completed)
                .count();
            parts.push(format!(
                "Plan '{}': {}/{} steps completed",
                plan.goal,
                completed,
                plan.steps.len()
            ));
        }

        if !self.tool_results.is_empty() {
            let success = self.tool_results.iter().filter(|t| t.success).count();
            parts.push(format!(
                "{} tool calls ({} successful)",
                self.tool_results.len(),
                success
            ));
        }

        parts.push(format!("{} iterations used", self.iterations));

        parts.join(". ")
    }

    /// Clear all working memory (call when a task completes).
    pub fn clear(&mut self) {
        self.plan = None;
        self.trace.clear();
        self.tool_results.clear();
        self.notes.clear();
        self.iterations = 0;
    }

    /// Return the total number of items across all sections.
    pub fn item_count(&self) -> usize {
        let plan_items = if self.plan.is_some() { 1 } else { 0 };
        plan_items + self.trace.len() + self.tool_results.len() + self.notes.len()
    }

    /// Check if working memory is empty (no plan, no traces, no notes).
    pub fn is_empty(&self) -> bool {
        self.plan.is_none()
            && self.trace.is_empty()
            && self.tool_results.is_empty()
            && self.notes.is_empty()
    }
}

impl Default for WorkingMemory {
    fn default() -> Self {
        Self::new(20)
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_working_memory_is_empty() {
        let wm = WorkingMemory::new(10);
        assert!(wm.is_empty());
        assert_eq!(wm.iterations, 0);
        assert_eq!(wm.max_iterations, 10);
        assert!(wm.plan.is_none());
    }

    #[test]
    fn trace_recording() {
        let mut wm = WorkingMemory::default();
        wm.add_thought("I need to check the weather");
        wm.add_action("weather_lookup(location=\"Tokyo\")");
        wm.add_observation("Tokyo: 18°C, cloudy, 70% rain");

        assert_eq!(wm.trace.len(), 3);
        assert_eq!(wm.trace[0].kind, TraceKind::Thought);
        assert_eq!(wm.trace[1].kind, TraceKind::Action);
        assert_eq!(wm.trace[2].kind, TraceKind::Observation);
    }

    #[test]
    fn plan_creation_and_advancement() {
        let mut wm = WorkingMemory::default();
        wm.set_plan("Build a report", vec![
            "Research data".into(),
            "Analyze findings".into(),
            "Write draft".into(),
        ]);

        let plan = wm.plan.as_ref().unwrap();
        assert_eq!(plan.steps.len(), 3);
        assert_eq!(plan.steps[0].status, StepStatus::InProgress);
        assert_eq!(plan.steps[1].status, StepStatus::Pending);

        assert!(wm.advance_plan(Some("Found 5 papers".into())));
        let plan = wm.plan.as_ref().unwrap();
        assert_eq!(plan.steps[0].status, StepStatus::Completed);
        assert_eq!(plan.steps[0].result.as_deref(), Some("Found 5 papers"));
        assert_eq!(plan.steps[1].status, StepStatus::InProgress);
        assert_eq!(plan.current_step, 1);

        assert!(!wm.is_plan_complete());
        wm.advance_plan(None);
        wm.advance_plan(None);
        assert!(wm.is_plan_complete());
    }

    #[test]
    fn plan_step_failure() {
        let mut wm = WorkingMemory::default();
        wm.set_plan("Test", vec!["Step 1".into()]);
        wm.fail_plan_step("Connection timeout");

        let step = &wm.plan.as_ref().unwrap().steps[0];
        assert_eq!(step.status, StepStatus::Failed("Connection timeout".into()));
    }

    #[test]
    fn tool_result_recording() {
        let mut wm = WorkingMemory::default();
        wm.add_tool_result("calculator", "2+2", "4", true);
        wm.add_tool_result("web_search", "rust wasm", "Error: timeout", false);

        assert_eq!(wm.tool_results.len(), 2);
        assert!(wm.tool_results[0].success);
        assert!(!wm.tool_results[1].success);
    }

    #[test]
    fn iteration_tracking() {
        let mut wm = WorkingMemory::new(3);
        assert!(wm.tick()); // 1
        assert!(wm.tick()); // 2
        assert!(wm.tick()); // 3
        assert!(!wm.tick()); // 4 > max
    }

    #[test]
    fn render_produces_readable_output() {
        let mut wm = WorkingMemory::default();
        wm.set_plan("Check weather", vec![
            "Look up Tokyo".into(),
            "Give advice".into(),
        ]);
        wm.add_thought("Need to check weather");
        wm.add_action("weather_lookup(Tokyo)");
        wm.add_observation("18°C, rain likely");
        wm.add_tool_result("weather_lookup", "Tokyo", "18°C, rain", true);
        wm.add_note("User prefers metric units");

        let rendered = wm.render();
        assert!(rendered.contains("## Current Plan"));
        assert!(rendered.contains("Check weather"));
        assert!(rendered.contains("[Thought]"));
        assert!(rendered.contains("[Action]"));
        assert!(rendered.contains("[Observation]"));
        assert!(rendered.contains("## Tool Results"));
        assert!(rendered.contains("## Notes"));
        assert!(rendered.contains("metric units"));
    }

    #[test]
    fn serialization_roundtrip() {
        let mut wm = WorkingMemory::default();
        wm.add_thought("test thought");
        wm.set_plan("test goal", vec!["step 1".into()]);
        wm.add_note("a note");

        let json = serde_json::to_string(&wm).unwrap();
        let deserialized: WorkingMemory = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.trace.len(), 1);
        assert!(deserialized.plan.is_some());
        assert_eq!(deserialized.notes.len(), 1);
    }

    #[test]
    fn summarize_output() {
        let mut wm = WorkingMemory::default();
        wm.set_plan("Build report", vec!["Research".into(), "Write".into()]);
        wm.advance_plan(Some("done".into()));
        wm.add_tool_result("search", "query", "results", true);
        wm.iterations = 3;

        let summary = wm.summarize();
        assert!(summary.contains("1/2 steps completed"));
        assert!(summary.contains("1 tool calls"));
        assert!(summary.contains("3 iterations"));
    }

    #[test]
    fn clear_resets_everything() {
        let mut wm = WorkingMemory::default();
        wm.add_thought("thought");
        wm.set_plan("goal", vec!["step".into()]);
        wm.add_note("note");
        wm.add_tool_result("tool", "in", "out", true);
        wm.iterations = 5;

        wm.clear();
        assert!(wm.is_empty());
        assert_eq!(wm.iterations, 0);
    }

    #[test]
    fn item_count() {
        let mut wm = WorkingMemory::default();
        assert_eq!(wm.item_count(), 0);

        wm.add_thought("t");
        assert_eq!(wm.item_count(), 1);

        wm.set_plan("g", vec!["s".into()]);
        assert_eq!(wm.item_count(), 2); // 1 trace + 1 plan

        wm.add_note("n");
        wm.add_tool_result("t", "i", "o", true);
        assert_eq!(wm.item_count(), 4);
    }
}
