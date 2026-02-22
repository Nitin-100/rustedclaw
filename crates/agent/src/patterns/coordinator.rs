//! Multi-agent coordination pattern.
//!
//! A coordinator agent receives a complex task, decomposes it into
//! sub-tasks, delegates each to a specialist worker agent, and
//! aggregates the results into a final response.
//!
//! # Architecture
//!
//! ```text
//! User Question
//!       │
//!       ▼
//! ┌─────────────┐
//! │ Coordinator  │  ← Decomposes task, aggregates results
//! └──┬──────┬────┘
//!    │      │
//!    ▼      ▼
//! ┌──────┐ ┌──────┐
//! │ W-1  │ │ W-2  │  ← Specialist workers (each is a ReactAgent)
//! └──────┘ └──────┘
//! ```

use std::sync::Arc;
use rustedclaw_core::event::EventBus;
use rustedclaw_core::identity::Identity;
use rustedclaw_core::memory::MemoryEntry;
use rustedclaw_core::message::{Conversation, Message};
use rustedclaw_core::provider::{Provider, ProviderRequest};
use rustedclaw_core::tool::ToolRegistry;
use tracing::{debug, info};

use crate::context::working_memory::{TraceEntry, WorkingMemory};
use crate::patterns::react::ReactAgent;

/// Coordinator agent that delegates to workers.
pub struct CoordinatorAgent {
    /// LLM provider.
    provider: Arc<dyn Provider>,
    /// Model name.
    model: String,
    /// Temperature.
    temperature: f32,
    /// Available worker agents.
    workers: Vec<WorkerConfig>,
    /// Agent identity.
    identity: Identity,
    /// Tool registry (shared with workers).
    tools: Arc<ToolRegistry>,
    /// Event bus.
    event_bus: Arc<EventBus>,
}

/// Configuration for a worker agent.
pub struct WorkerConfig {
    /// Worker name (e.g., "researcher", "writer", "analyst").
    pub name: String,
    /// Description of what this worker specializes in.
    pub description: String,
    /// Custom identity for this worker (optional, falls back to default).
    pub identity: Option<Identity>,
}

/// Result of a coordinated multi-agent execution.
pub struct CoordinationResult {
    /// The final aggregated answer.
    pub answer: String,
    /// Results from each sub-task.
    pub sub_results: Vec<SubTaskResult>,
    /// Coordinator's working memory.
    pub working_memory: WorkingMemory,
    /// Total iterations across all agents.
    pub total_iterations: usize,
    /// Total tool calls across all agents.
    pub total_tool_calls: usize,
}

/// Result of a single sub-task executed by a worker.
pub struct SubTaskResult {
    /// Which worker handled this.
    pub worker_name: String,
    /// The sub-task description.
    pub task: String,
    /// The worker's answer.
    pub result: String,
    /// The worker's reasoning trace.
    pub trace: Vec<TraceEntry>,
    /// Iterations used by this worker.
    pub iterations: usize,
    /// Tool calls made by this worker.
    pub tool_calls: usize,
}

impl CoordinatorAgent {
    /// Create a new coordinator.
    pub fn new(
        provider: Arc<dyn Provider>,
        model: impl Into<String>,
        temperature: f32,
        tools: Arc<ToolRegistry>,
        identity: Identity,
        event_bus: Arc<EventBus>,
    ) -> Self {
        Self {
            provider,
            model: model.into(),
            temperature,
            workers: Vec::new(),
            identity,
            tools,
            event_bus,
        }
    }

    /// Add a worker agent.
    pub fn add_worker(mut self, name: impl Into<String>, description: impl Into<String>) -> Self {
        self.workers.push(WorkerConfig {
            name: name.into(),
            description: description.into(),
            identity: None,
        });
        self
    }

    /// Add a worker with custom identity.
    pub fn add_worker_with_identity(
        mut self,
        name: impl Into<String>,
        description: impl Into<String>,
        identity: Identity,
    ) -> Self {
        self.workers.push(WorkerConfig {
            name: name.into(),
            description: description.into(),
            identity: Some(identity),
        });
        self
    }

    /// Execute coordinated multi-agent task.
    ///
    /// 1. Decompose the task into sub-tasks (one per worker)
    /// 2. Execute each sub-task with the appropriate worker
    /// 3. Aggregate results into a final answer
    pub async fn run(
        &self,
        user_message: &str,
        memories: &[MemoryEntry],
    ) -> Result<CoordinationResult, rustedclaw_core::Error> {
        let mut coordinator_wm = WorkingMemory::new(10);
        let mut sub_results = Vec::new();
        let mut total_iterations = 0usize;
        let mut total_tool_calls = 0usize;

        info!(
            workers = self.workers.len(),
            "Coordinator: starting task decomposition"
        );

        // ── Step 1: Decompose task ──
        let sub_tasks = self.decompose_task(user_message).await?;

        coordinator_wm.set_plan(
            user_message,
            sub_tasks.iter().map(|t| t.task.clone()).collect(),
        );

        coordinator_wm.add_thought(&format!(
            "Decomposed into {} sub-tasks for {} workers",
            sub_tasks.len(),
            self.workers.len()
        ));

        debug!(sub_tasks = sub_tasks.len(), "Coordinator: tasks decomposed");

        // ── Step 2: Execute sub-tasks with workers ──
        for sub_task in &sub_tasks {
            let worker_name = &sub_task.worker;
            let task = &sub_task.task;

            coordinator_wm.add_action(&format!("Delegating to {}: {}", worker_name, task));

            // Create worker ReactAgent
            let worker_identity = self
                .workers
                .iter()
                .find(|w| w.name == *worker_name)
                .and_then(|w| w.identity.clone())
                .unwrap_or_else(|| {
                    let mut id = self.identity.clone();
                    id.name = worker_name.clone();
                    id.personality = format!("Specialist agent: {}", worker_name);
                    id
                });

            let worker = ReactAgent::new(
                self.provider.clone(),
                &self.model,
                self.temperature,
                self.tools.clone(),
                worker_identity,
                self.event_bus.clone(),
            )
            .with_max_iterations(5);

            let mut worker_conv = Conversation::new();
            let result = worker
                .run(task, &mut worker_conv, memories, &[])
                .await?;

            coordinator_wm.add_observation(&format!(
                "{} completed: {}",
                worker_name,
                &result.answer[..result.answer.len().min(100)]
            ));

            coordinator_wm.advance_plan(Some(result.answer.clone()));

            total_iterations += result.iterations;
            total_tool_calls += result.tool_calls_made;

            sub_results.push(SubTaskResult {
                worker_name: worker_name.clone(),
                task: task.clone(),
                result: result.answer,
                trace: result.trace,
                iterations: result.iterations,
                tool_calls: result.tool_calls_made,
            });
        }

        // ── Step 3: Aggregate results ──
        coordinator_wm.add_thought("Aggregating results from all workers");

        let answer = self.aggregate_results(user_message, &sub_results).await?;

        coordinator_wm.add_reflection(&format!(
            "Coordination complete: {} sub-tasks, {} total iterations, {} tool calls",
            sub_results.len(),
            total_iterations,
            total_tool_calls
        ));

        info!(
            sub_tasks = sub_results.len(),
            total_iterations,
            total_tool_calls,
            "Coordinator: complete"
        );

        Ok(CoordinationResult {
            answer,
            sub_results,
            working_memory: coordinator_wm,
            total_iterations,
            total_tool_calls,
        })
    }

    /// Decompose a complex task into sub-tasks assigned to workers.
    async fn decompose_task(
        &self,
        user_message: &str,
    ) -> Result<Vec<SubTask>, rustedclaw_core::Error> {
        if self.workers.is_empty() {
            return Ok(vec![SubTask {
                worker: "default".into(),
                task: user_message.to_string(),
            }]);
        }

        // Ask the LLM to decompose the task.
        let worker_list: String = self
            .workers
            .iter()
            .map(|w| format!("- {}: {}", w.name, w.description))
            .collect::<Vec<_>>()
            .join("\n");

        let decompose_prompt = format!(
            "You are a task coordinator. Decompose this task into sub-tasks for the available workers.\n\n\
            Available workers:\n{}\n\n\
            Task: {}\n\n\
            Respond with one line per sub-task in the format: WORKER_NAME: task description\n\
            Assign at least one task to each worker. Be concise.",
            worker_list, user_message
        );

        let request = ProviderRequest {
            model: self.model.clone(),
            messages: vec![Message::system(&decompose_prompt)],
            temperature: 0.3,
            max_tokens: Some(4096),
            tools: vec![],
            stream: false,
            stop: vec![],
        };

        let response = self.provider.complete(request).await?;
        let content = &response.message.content;

        // Parse response into sub-tasks.
        let sub_tasks: Vec<SubTask> = content
            .lines()
            .filter_map(|line| {
                let line = line.trim();
                if line.is_empty() {
                    return None;
                }
                // Try "WORKER: task" format
                if let Some((worker, task)) = line.split_once(':') {
                    let worker = worker.trim().to_lowercase();
                    let task = task.trim().to_string();
                    // Verify worker exists
                    if self.workers.iter().any(|w| w.name.to_lowercase() == worker) {
                        return Some(SubTask { worker, task });
                    }
                }
                None
            })
            .collect();

        // Fallback: if parsing failed, assign entire task to first worker.
        if sub_tasks.is_empty() {
            return Ok(vec![SubTask {
                worker: self.workers[0].name.clone(),
                task: user_message.to_string(),
            }]);
        }

        Ok(sub_tasks)
    }

    /// Aggregate sub-task results into a final answer.
    async fn aggregate_results(
        &self,
        original_question: &str,
        sub_results: &[SubTaskResult],
    ) -> Result<String, rustedclaw_core::Error> {
        let results_text: String = sub_results
            .iter()
            .map(|sr| format!("## {} ({})\n{}\n", sr.worker_name, sr.task, sr.result))
            .collect::<Vec<_>>()
            .join("\n");

        let aggregate_prompt = format!(
            "You are synthesizing results from multiple specialist agents.\n\n\
            Original question: {}\n\n\
            Worker results:\n{}\n\n\
            Provide a unified, coherent answer that combines all worker results.",
            original_question, results_text
        );

        let request = ProviderRequest {
            model: self.model.clone(),
            messages: vec![Message::system(&aggregate_prompt)],
            temperature: 0.3,
            max_tokens: Some(4096),
            tools: vec![],
            stream: false,
            stop: vec![],
        };

        let response = self.provider.complete(request).await?;
        Ok(response.message.content)
    }
}

/// Internal sub-task assignment.
struct SubTask {
    worker: String,
    task: String,
}

// ── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::patterns::test_helpers::*;

    #[tokio::test]
    async fn coordinator_with_workers() {
        // Mock provider responses:
        // 1. Decompose: "researcher: Find facts\nwriter: Write summary"
        // 2. Researcher worker: "Found key facts about Rust"
        // 3. Writer worker: "Summary written"
        // 4. Aggregate: "Combined result"
        let provider = Arc::new(SequentialMockProvider::new(vec![
            make_text_response("researcher: Research Rust performance\nwriter: Write a summary"),
            make_text_response("Rust has zero-cost abstractions and no GC."),
            make_text_response("Rust is a fast, safe systems language."),
            make_text_response("Rust combines performance with safety through its ownership system."),
        ]));

        let tools = Arc::new(rustedclaw_tools::default_registry());
        let event_bus = Arc::new(EventBus::default());

        let coordinator = CoordinatorAgent::new(
            provider,
            "mock-model",
            0.7,
            tools,
            Identity::default(),
            event_bus,
        )
        .add_worker("researcher", "Finds and analyzes information")
        .add_worker("writer", "Writes clear summaries");

        let result = coordinator
            .run("Write a report on Rust performance", &[])
            .await
            .unwrap();

        assert!(!result.answer.is_empty());
        assert_eq!(result.sub_results.len(), 2);
        assert_eq!(result.sub_results[0].worker_name, "researcher");
        assert_eq!(result.sub_results[1].worker_name, "writer");
    }

    #[tokio::test]
    async fn coordinator_working_memory() {
        let provider = Arc::new(SequentialMockProvider::new(vec![
            make_text_response("researcher: Do research"),
            make_text_response("Research done"),
            make_text_response("Final aggregated answer"),
        ]));

        let tools = Arc::new(rustedclaw_tools::default_registry());
        let event_bus = Arc::new(EventBus::default());

        let coordinator = CoordinatorAgent::new(
            provider,
            "mock-model",
            0.7,
            tools,
            Identity::default(),
            event_bus,
        )
        .add_worker("researcher", "Research specialist");

        let result = coordinator.run("Research topic X", &[]).await.unwrap();

        // Coordinator should have a plan
        assert!(result.working_memory.plan.is_some());
        // Should have traces
        assert!(!result.working_memory.trace.is_empty());
    }

    #[tokio::test]
    async fn coordinator_no_workers_fallback() {
        let provider = Arc::new(SequentialMockProvider::new(vec![
            make_text_response("Direct answer without workers"),
            make_text_response("Aggregated: Direct answer"),
        ]));

        let tools = Arc::new(rustedclaw_tools::default_registry());
        let event_bus = Arc::new(EventBus::default());

        let coordinator = CoordinatorAgent::new(
            provider,
            "mock-model",
            0.7,
            tools,
            Identity::default(),
            event_bus,
        );

        let result = coordinator
            .run("Simple question", &[])
            .await
            .unwrap();

        assert_eq!(result.sub_results.len(), 1);
        assert_eq!(result.sub_results[0].worker_name, "default");
    }

    #[tokio::test]
    async fn coordinator_tracks_totals() {
        let provider = Arc::new(SequentialMockProvider::new(vec![
            make_text_response("researcher: Research\nwriter: Write"),
            make_text_response("Research result"),
            make_text_response("Writing result"),
            make_text_response("Final combined answer"),
        ]));

        let tools = Arc::new(rustedclaw_tools::default_registry());
        let event_bus = Arc::new(EventBus::default());

        let coordinator = CoordinatorAgent::new(
            provider,
            "mock-model",
            0.7,
            tools,
            Identity::default(),
            event_bus,
        )
        .add_worker("researcher", "Research")
        .add_worker("writer", "Writing");

        let result = coordinator
            .run("Complex multi-step task", &[])
            .await
            .unwrap();

        // Each worker did 1 iteration (simple text response)
        assert_eq!(result.total_iterations, 2);
        assert_eq!(result.total_tool_calls, 0);
    }
}
