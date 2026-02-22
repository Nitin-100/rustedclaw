//! Workflow engine — cron scheduling, heartbeat, and event-driven tasks.
//!
//! Manages background tasks that run on schedules or in response to events.
//! Includes a zero-dependency cron expression parser supporting standard 5-field
//! expressions: `minute hour day-of-month month day-of-week`.

use chrono::{DateTime, Datelike, Timelike, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{RwLock, mpsc};
use tracing::{debug, info, warn};

/// A scheduled task.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CronTask {
    /// Unique task ID
    pub id: String,

    /// Human-readable name
    pub name: String,

    /// The prompt/instruction to send to the agent
    pub instruction: String,

    /// Cron expression (e.g., "*/30 * * * *" = every 30 minutes)
    pub schedule: String,

    /// Whether this task is active
    pub enabled: bool,

    /// Target channel to send results to
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_channel: Option<String>,

    /// The action to perform (defaults to AgentTask with instruction as prompt)
    #[serde(default)]
    pub action: TaskAction,

    /// When this task last ran
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_run: Option<DateTime<Utc>>,

    /// When this task should next run
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_run: Option<DateTime<Utc>>,
}

/// The action to perform when a cron task fires.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TaskAction {
    /// Run a prompt through the agent (default)
    AgentTask {
        prompt: String,
        #[serde(default)]
        context: Option<String>,
    },
    /// Execute a specific tool
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

impl Default for TaskAction {
    fn default() -> Self {
        TaskAction::AgentTask {
            prompt: String::new(),
            context: None,
        }
    }
}

// ── Cron expression parser ──────────────────────────────────────────────────

/// A parsed 5-field cron expression: minute hour dom month dow.
#[derive(Debug, Clone)]
struct CronExpr {
    minutes: Vec<u32>,
    hours: Vec<u32>,
    days_of_month: Vec<u32>,
    months: Vec<u32>,
    days_of_week: Vec<u32>, // 0=Sun, 6=Sat
}

impl CronExpr {
    /// Parse a standard 5-field cron expression.
    ///
    /// Supports: `*`, `*/N` (step), `N` (literal), `N-M` (range), `N,M` (list).
    fn parse(expr: &str) -> Result<Self, String> {
        let fields: Vec<&str> = expr.trim().split_whitespace().collect();
        if fields.len() != 5 {
            return Err(format!(
                "Expected 5 fields (minute hour dom month dow), got {}",
                fields.len()
            ));
        }

        Ok(CronExpr {
            minutes: Self::parse_field(fields[0], 0, 59)?,
            hours: Self::parse_field(fields[1], 0, 23)?,
            days_of_month: Self::parse_field(fields[2], 1, 31)?,
            months: Self::parse_field(fields[3], 1, 12)?,
            days_of_week: Self::parse_field(fields[4], 0, 6)?,
        })
    }

    fn parse_field(field: &str, min: u32, max: u32) -> Result<Vec<u32>, String> {
        let mut values = Vec::new();

        for part in field.split(',') {
            let part = part.trim();

            if part.contains('/') {
                // Step: */N or M-N/S
                let pieces: Vec<&str> = part.splitn(2, '/').collect();
                let step: u32 = pieces[1]
                    .parse()
                    .map_err(|_| format!("Invalid step: {}", pieces[1]))?;
                if step == 0 {
                    return Err("Step cannot be zero".into());
                }
                let (start, end) = if pieces[0] == "*" {
                    (min, max)
                } else if pieces[0].contains('-') {
                    Self::parse_range(pieces[0], min, max)?
                } else {
                    let s: u32 = pieces[0]
                        .parse()
                        .map_err(|_| format!("Invalid number: {}", pieces[0]))?;
                    (s, max)
                };
                let mut v = start;
                while v <= end {
                    values.push(v);
                    v += step;
                }
            } else if part.contains('-') {
                // Range: M-N
                let (start, end) = Self::parse_range(part, min, max)?;
                for v in start..=end {
                    values.push(v);
                }
            } else if part == "*" {
                for v in min..=max {
                    values.push(v);
                }
            } else {
                // Literal
                let v: u32 = part
                    .parse()
                    .map_err(|_| format!("Invalid number: {part}"))?;
                if v < min || v > max {
                    return Err(format!("{v} out of range {min}-{max}"));
                }
                values.push(v);
            }
        }

        values.sort();
        values.dedup();
        if values.is_empty() {
            return Err("Field produced no values".into());
        }
        Ok(values)
    }

    fn parse_range(s: &str, min: u32, max: u32) -> Result<(u32, u32), String> {
        let pieces: Vec<&str> = s.splitn(2, '-').collect();
        let start: u32 = pieces[0]
            .parse()
            .map_err(|_| format!("Invalid range start: {}", pieces[0]))?;
        let end: u32 = pieces[1]
            .parse()
            .map_err(|_| format!("Invalid range end: {}", pieces[1]))?;
        if start < min || end > max || start > end {
            return Err(format!("Range {start}-{end} invalid for {min}-{max}"));
        }
        Ok((start, end))
    }

    /// Check if the given datetime matches this cron expression.
    fn matches(&self, dt: &DateTime<Utc>) -> bool {
        let minute = dt.minute();
        let hour = dt.hour();
        let dom = dt.day();
        let month = dt.month();
        let dow = dt.weekday().num_days_from_sunday(); // 0=Sun

        self.minutes.contains(&minute)
            && self.hours.contains(&hour)
            && self.days_of_month.contains(&dom)
            && self.months.contains(&month)
            && self.days_of_week.contains(&dow)
    }
}

// ── Triggered task output ───────────────────────────────────────────────────

/// A triggered task ready for execution by the agent.
#[derive(Debug, Clone)]
pub struct TriggeredTask {
    pub task_id: String,
    pub instruction: String,
    pub target_channel: Option<String>,
    pub action: TaskAction,
}

/// The workflow engine manages cron tasks and heartbeat.
pub struct WorkflowEngine {
    tasks: Arc<RwLock<HashMap<String, CronTask>>>,
    heartbeat_interval_minutes: u32,
    heartbeat_enabled: bool,
}

impl WorkflowEngine {
    pub fn new(heartbeat_enabled: bool, heartbeat_interval_minutes: u32) -> Self {
        Self {
            tasks: Arc::new(RwLock::new(HashMap::new())),
            heartbeat_interval_minutes,
            heartbeat_enabled,
        }
    }

    /// Add a cron task.
    pub async fn add_task(&self, task: CronTask) -> Result<(), String> {
        // Validate cron expression at registration time
        CronExpr::parse(&task.schedule)?;
        info!(task_id = %task.id, name = %task.name, schedule = %task.schedule, "Adding cron task");
        self.tasks.write().await.insert(task.id.clone(), task);
        Ok(())
    }

    /// Remove a cron task.
    pub async fn remove_task(&self, id: &str) -> bool {
        self.tasks.write().await.remove(id).is_some()
    }

    /// List all tasks.
    pub async fn list_tasks(&self) -> Vec<CronTask> {
        self.tasks.read().await.values().cloned().collect()
    }

    /// Pause a task.
    pub async fn pause_task(&self, id: &str) -> bool {
        if let Some(task) = self.tasks.write().await.get_mut(id) {
            task.enabled = false;
            true
        } else {
            false
        }
    }

    /// Resume a task.
    pub async fn resume_task(&self, id: &str) -> bool {
        if let Some(task) = self.tasks.write().await.get_mut(id) {
            task.enabled = true;
            true
        } else {
            false
        }
    }

    /// Start the workflow engine background loop.
    ///
    /// Returns a channel receiver that emits triggered tasks (the caller is
    /// responsible for feeding them into the agent loop) and a join handle.
    pub fn start(&self) -> (mpsc::Receiver<TriggeredTask>, tokio::task::JoinHandle<()>) {
        let tasks = self.tasks.clone();
        let heartbeat_enabled = self.heartbeat_enabled;
        let heartbeat_interval = self.heartbeat_interval_minutes;
        let (tx, rx) = mpsc::channel::<TriggeredTask>(64);

        let handle = tokio::spawn(async move {
            let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(60));

            loop {
                interval.tick().await;
                let now = Utc::now();

                // Check cron tasks
                let task_ids: Vec<String> = {
                    let map = tasks.read().await;
                    map.keys().cloned().collect()
                };

                for task_id in task_ids {
                    let should_fire = {
                        let map = tasks.read().await;
                        let Some(task) = map.get(&task_id) else {
                            continue;
                        };
                        if !task.enabled {
                            continue;
                        }

                        // Parse the cron expression
                        let expr = match CronExpr::parse(&task.schedule) {
                            Ok(e) => e,
                            Err(e) => {
                                warn!(task_id = %task_id, error = %e, "Invalid cron expression, skipping");
                                continue;
                            }
                        };

                        // Check if current time matches AND we haven't fired this minute
                        if expr.matches(&now) {
                            match &task.last_run {
                                Some(last) => {
                                    // Don't fire twice in the same minute
                                    last.minute() != now.minute()
                                        || last.hour() != now.hour()
                                        || last.day() != now.day()
                                }
                                None => true,
                            }
                        } else {
                            false
                        }
                    };

                    if should_fire {
                        let (instruction, target_channel, action) = {
                            let mut map = tasks.write().await;
                            if let Some(task) = map.get_mut(&task_id) {
                                task.last_run = Some(now);
                                info!(task_id = %task_id, name = %task.name, "Cron task triggered");
                                (task.instruction.clone(), task.target_channel.clone(), task.action.clone())
                            } else {
                                continue;
                            }
                        };

                        let triggered = TriggeredTask {
                            task_id: task_id.clone(),
                            instruction,
                            target_channel,
                            action,
                        };
                        if tx.send(triggered).await.is_err() {
                            debug!("Triggered task receiver dropped, stopping cron loop");
                            return;
                        }
                    }
                }

                // Heartbeat
                if heartbeat_enabled {
                    debug!(interval_minutes = heartbeat_interval, "Heartbeat tick");
                }
            }
        });

        (rx, handle)
    }
}

impl Default for WorkflowEngine {
    fn default() -> Self {
        Self::new(false, 30)
    }
}

impl CronTask {
    /// Create a CronTask from a config routine.
    pub fn from_routine_config(config: &rustedclaw_config::RoutineConfig) -> Self {
        let action = match &config.action {
            rustedclaw_config::RoutineAction::AgentTask { prompt, context } => TaskAction::AgentTask {
                prompt: prompt.clone(),
                context: context.clone(),
            },
            rustedclaw_config::RoutineAction::RunTool { tool, input } => TaskAction::RunTool {
                tool: tool.clone(),
                input: input.clone(),
            },
            rustedclaw_config::RoutineAction::SendMessage {
                channel,
                recipient,
                template,
            } => TaskAction::SendMessage {
                channel: channel.clone(),
                recipient: recipient.clone(),
                template: template.clone(),
            },
        };

        let instruction = match &config.action {
            rustedclaw_config::RoutineAction::AgentTask { prompt, .. } => prompt.clone(),
            rustedclaw_config::RoutineAction::RunTool { tool, .. } => {
                format!("Run tool: {tool}")
            }
            rustedclaw_config::RoutineAction::SendMessage { template, .. } => template.clone(),
        };

        CronTask {
            id: config.name.clone(),
            name: config.name.clone(),
            instruction,
            schedule: config.schedule.clone(),
            enabled: config.enabled,
            target_channel: config.target_channel.clone(),
            action,
            last_run: None,
            next_run: None,
        }
    }
}

impl WorkflowEngine {
    /// Load routines from the application config.
    pub async fn load_routines(&self, routines: &[rustedclaw_config::RoutineConfig]) -> Vec<String> {
        let mut errors = Vec::new();
        for routine in routines {
            let task = CronTask::from_routine_config(routine);
            if let Err(e) = self.add_task(task).await {
                errors.push(format!("Routine '{}': {e}", routine.name));
            }
        }
        errors
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn add_and_list_tasks() {
        let engine = WorkflowEngine::default();

        engine.add_task(CronTask {
            id: "task_1".into(),
            name: "Check email".into(),
            instruction: "Check for new emails and summarize them".into(),
            schedule: "*/30 * * * *".into(),
            enabled: true,
            target_channel: None,
            action: TaskAction::default(),
            last_run: None,
            next_run: None,
        }).await.unwrap();

        let tasks = engine.list_tasks().await;
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].name, "Check email");
    }

    #[tokio::test]
    async fn pause_and_resume() {
        let engine = WorkflowEngine::default();

        engine.add_task(CronTask {
            id: "t1".into(),
            name: "Test".into(),
            instruction: "Do something".into(),
            schedule: "* * * * *".into(),
            enabled: true,
            target_channel: None,
            action: TaskAction::default(),
            last_run: None,
            next_run: None,
        }).await.unwrap();

        assert!(engine.pause_task("t1").await);
        let tasks = engine.list_tasks().await;
        assert!(!tasks[0].enabled);

        assert!(engine.resume_task("t1").await);
        let tasks = engine.list_tasks().await;
        assert!(tasks[0].enabled);
    }

    #[tokio::test]
    async fn remove_task() {
        let engine = WorkflowEngine::default();

        engine.add_task(CronTask {
            id: "t1".into(),
            name: "Test".into(),
            instruction: "Test".into(),
            schedule: "* * * * *".into(),
            enabled: true,
            target_channel: None,
            action: TaskAction::default(),
            last_run: None,
            next_run: None,
        }).await.unwrap();

        assert!(engine.remove_task("t1").await);
        assert!(!engine.remove_task("t1").await); // Already removed
        assert_eq!(engine.list_tasks().await.len(), 0);
    }

    #[tokio::test]
    async fn invalid_cron_rejected() {
        let engine = WorkflowEngine::default();

        let result = engine.add_task(CronTask {
            id: "bad".into(),
            name: "Bad".into(),
            instruction: "Won't work".into(),
            schedule: "not a cron".into(),
            enabled: true,
            target_channel: None,
            action: TaskAction::default(),
            last_run: None,
            next_run: None,
        }).await;

        assert!(result.is_err());
    }

    #[test]
    fn cron_expr_every_minute() {
        let expr = CronExpr::parse("* * * * *").unwrap();
        assert_eq!(expr.minutes.len(), 60);
        assert_eq!(expr.hours.len(), 24);
    }

    #[test]
    fn cron_expr_specific_time() {
        let expr = CronExpr::parse("30 9 * * 1-5").unwrap();
        assert_eq!(expr.minutes, vec![30]);
        assert_eq!(expr.hours, vec![9]);
        assert_eq!(expr.days_of_week, vec![1, 2, 3, 4, 5]);
    }

    #[test]
    fn cron_expr_step() {
        let expr = CronExpr::parse("*/15 * * * *").unwrap();
        assert_eq!(expr.minutes, vec![0, 15, 30, 45]);
    }

    #[test]
    fn cron_expr_list() {
        let expr = CronExpr::parse("0,30 * * * *").unwrap();
        assert_eq!(expr.minutes, vec![0, 30]);
    }

    #[test]
    fn cron_matches_datetime() {
        // "At 09:30 on weekdays"
        let expr = CronExpr::parse("30 9 * * 1-5").unwrap();

        // 2026-02-23 is a Monday (dow=1)
        let monday_930 = chrono::NaiveDate::from_ymd_opt(2026, 2, 23)
            .unwrap()
            .and_hms_opt(9, 30, 0)
            .unwrap()
            .and_utc();
        assert!(expr.matches(&monday_930));

        // 2026-02-22 is a Sunday (dow=0) — should NOT match
        let sunday_930 = chrono::NaiveDate::from_ymd_opt(2026, 2, 22)
            .unwrap()
            .and_hms_opt(9, 30, 0)
            .unwrap()
            .and_utc();
        assert!(!expr.matches(&sunday_930));

        // Monday but wrong time — should NOT match
        let monday_1000 = chrono::NaiveDate::from_ymd_opt(2026, 2, 23)
            .unwrap()
            .and_hms_opt(10, 0, 0)
            .unwrap()
            .and_utc();
        assert!(!expr.matches(&monday_1000));
    }

    #[test]
    fn cron_invalid_field_count() {
        assert!(CronExpr::parse("* * *").is_err());
    }

    #[test]
    fn cron_invalid_range() {
        assert!(CronExpr::parse("70 * * * *").is_err());
    }

    #[tokio::test]
    async fn load_routines_from_config() {
        let engine = WorkflowEngine::default();
        let routines = vec![
            rustedclaw_config::RoutineConfig {
                name: "daily_summary".into(),
                schedule: "0 9 * * *".into(),
                action: rustedclaw_config::RoutineAction::AgentTask {
                    prompt: "Summarize my day".into(),
                    context: None,
                },
                target_channel: Some("telegram".into()),
                enabled: true,
            },
            rustedclaw_config::RoutineConfig {
                name: "health_check".into(),
                schedule: "*/5 * * * *".into(),
                action: rustedclaw_config::RoutineAction::RunTool {
                    tool: "http_request".into(),
                    input: serde_json::json!({"url": "https://myapp.com/health"}),
                },
                target_channel: None,
                enabled: true,
            },
        ];

        let errors = engine.load_routines(&routines).await;
        assert!(errors.is_empty(), "Unexpected errors: {:?}", errors);

        let tasks = engine.list_tasks().await;
        assert_eq!(tasks.len(), 2);
    }

    #[tokio::test]
    async fn load_routines_reports_invalid_cron() {
        let engine = WorkflowEngine::default();
        let routines = vec![rustedclaw_config::RoutineConfig {
            name: "bad_routine".into(),
            schedule: "bad cron".into(),
            action: rustedclaw_config::RoutineAction::AgentTask {
                prompt: "fail".into(),
                context: None,
            },
            target_channel: None,
            enabled: true,
        }];

        let errors = engine.load_routines(&routines).await;
        assert_eq!(errors.len(), 1);
        assert!(errors[0].contains("bad_routine"));
    }

    #[test]
    fn cron_task_from_routine_config() {
        let config = rustedclaw_config::RoutineConfig {
            name: "test_routine".into(),
            schedule: "30 9 * * 1-5".into(),
            action: rustedclaw_config::RoutineAction::AgentTask {
                prompt: "Do work".into(),
                context: Some("with context".into()),
            },
            target_channel: Some("slack".into()),
            enabled: true,
        };

        let task = CronTask::from_routine_config(&config);
        assert_eq!(task.id, "test_routine");
        assert_eq!(task.name, "test_routine");
        assert_eq!(task.instruction, "Do work");
        assert_eq!(task.schedule, "30 9 * * 1-5");
        assert_eq!(task.target_channel, Some("slack".into()));
        assert!(task.enabled);
        assert!(matches!(task.action, TaskAction::AgentTask { .. }));
    }

    #[test]
    fn task_action_serialization() {
        let action = TaskAction::RunTool {
            tool: "http_request".into(),
            input: serde_json::json!({"url": "https://example.com"}),
        };
        let json = serde_json::to_string(&action).unwrap();
        assert!(json.contains("run_tool"));
        assert!(json.contains("http_request"));

        let action2 = TaskAction::SendMessage {
            channel: "telegram".into(),
            recipient: Some("user123".into()),
            template: "Hello!".into(),
        };
        let json2 = serde_json::to_string(&action2).unwrap();
        assert!(json2.contains("send_message"));
        assert!(json2.contains("telegram"));
    }

    #[test]
    fn task_with_target_channel() {
        let task = CronTask {
            id: "t1".into(),
            name: "Test".into(),
            instruction: "Summarize".into(),
            schedule: "0 9 * * *".into(),
            enabled: true,
            target_channel: Some("telegram".into()),
            action: TaskAction::AgentTask {
                prompt: "Summarize".into(),
                context: None,
            },
            last_run: None,
            next_run: None,
        };
        assert_eq!(task.target_channel, Some("telegram".into()));

        let json = serde_json::to_string(&task).unwrap();
        assert!(json.contains("telegram"));
    }
}
