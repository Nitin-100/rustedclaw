//! Contract enforcement engine.
//!
//! The engine evaluates contracts against pending tool calls or agent
//! responses and returns a [`Verdict`] that the agent loop must obey.

use crate::model::{Action, Contract, ContractSet, Trigger};
use crate::parser::{Condition, EvalContext};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::sync::RwLock;
use tracing::{debug, info, warn};

/// The outcome of evaluating contracts against an action.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Verdict {
    /// Whether the action is allowed.
    pub allowed: bool,
    /// The action to take.
    pub action: Action,
    /// Which contract fired (if any).
    pub contract_name: Option<String>,
    /// Human-readable message explaining the verdict.
    pub message: String,
    /// When the verdict was rendered.
    pub timestamp: DateTime<Utc>,
}

impl Verdict {
    /// Create an "allow" verdict (no contract fired).
    pub fn allow() -> Self {
        Self {
            allowed: true,
            action: Action::Allow,
            contract_name: None,
            message: String::new(),
            timestamp: Utc::now(),
        }
    }

    /// Create a verdict from a contract match.
    fn from_contract(contract: &Contract) -> Self {
        let allowed = matches!(contract.action, Action::Allow | Action::Warn);
        Self {
            allowed,
            action: contract.action.clone(),
            contract_name: Some(contract.name.clone()),
            message: if contract.message.is_empty() {
                format!("Contract '{}' triggered", contract.name)
            } else {
                contract.message.clone()
            },
            timestamp: Utc::now(),
        }
    }
}

/// An entry in the contract evaluation log.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContractLogEntry {
    pub contract_name: String,
    pub trigger: String,
    pub verdict: Verdict,
    pub tool_name: Option<String>,
    pub timestamp: DateTime<Utc>,
}

/// Maximum contract log entries kept in memory.
const MAX_CONTRACT_LOG: usize = 5_000;

/// The contract enforcement engine.
///
/// Thread-safe.  Holds and evaluates a [`ContractSet`] against incoming
/// tool calls and responses.
pub struct ContractEngine {
    contracts: RwLock<ContractSet>,
    /// Parsed condition cache — indexed by contract name.
    conditions: RwLock<Vec<(String, Condition)>>,
    /// Evaluation log (bounded to MAX_CONTRACT_LOG entries).
    log: RwLock<Vec<ContractLogEntry>>,
}

impl ContractEngine {
    /// Create a new engine with the given contract set.
    pub fn new(contract_set: ContractSet) -> Result<Self, crate::ContractError> {
        let conditions = Self::compile_conditions(&contract_set)?;
        Ok(Self {
            contracts: RwLock::new(contract_set),
            conditions: RwLock::new(conditions),
            log: RwLock::new(Vec::new()),
        })
    }

    /// Create an empty engine.
    pub fn empty() -> Self {
        Self {
            contracts: RwLock::new(ContractSet::new()),
            conditions: RwLock::new(Vec::new()),
            log: RwLock::new(Vec::new()),
        }
    }

    /// Reload contracts from a new set.
    pub fn reload(&self, contract_set: ContractSet) -> Result<(), crate::ContractError> {
        let conditions = Self::compile_conditions(&contract_set)?;
        *self.contracts.write().unwrap() = contract_set;
        *self.conditions.write().unwrap() = conditions;
        info!("Contracts reloaded");
        Ok(())
    }

    /// Evaluate contracts for a tool call.
    ///
    /// Returns the verdict for the highest-priority matching contract,
    /// or `Verdict::allow()` if no contracts match.
    pub fn check_tool_call(&self, tool_name: &str, arguments: &serde_json::Value) -> Verdict {
        let trigger = Trigger::Tool(tool_name.to_string());
        let ctx = EvalContext {
            args: Some(arguments),
            content: None,
            tool_name: Some(tool_name),
        };
        self.evaluate(&trigger, &ctx, Some(tool_name))
    }

    /// Evaluate contracts for an agent response.
    pub fn check_response(&self, content: &str) -> Verdict {
        let trigger = Trigger::Response;
        let ctx = EvalContext {
            args: None,
            content: Some(content),
            tool_name: None,
        };
        self.evaluate(&trigger, &ctx, None)
    }

    /// Get the evaluation log.
    pub fn log(&self) -> Vec<ContractLogEntry> {
        self.log.read().unwrap().clone()
    }

    /// Get the number of active contracts.
    pub fn active_count(&self) -> usize {
        self.contracts.read().unwrap().active_count()
    }

    /// List all contracts.
    pub fn list_contracts(&self) -> Vec<Contract> {
        self.contracts.read().unwrap().contracts.clone()
    }

    /// Add a contract at runtime.
    pub fn add_contract(&self, contract: Contract) -> Result<(), crate::ContractError> {
        contract.validate()?;
        let condition = if contract.condition.is_empty() {
            Condition::Always
        } else {
            crate::parse_condition(&contract.condition).map_err(|e| {
                crate::ContractError::ConditionParseError {
                    name: contract.name.clone(),
                    detail: e,
                }
            })?
        };
        self.conditions
            .write()
            .unwrap()
            .push((contract.name.clone(), condition));
        self.contracts.write().unwrap().add(contract);
        Ok(())
    }

    /// Remove a contract by name at runtime.
    pub fn remove_contract(&self, name: &str) -> bool {
        let removed = self.contracts.write().unwrap().remove(name);
        if removed {
            self.conditions.write().unwrap().retain(|(n, _)| n != name);
        }
        removed
    }

    // ── Internal ───────────────────────────────────────────────────

    fn compile_conditions(
        set: &ContractSet,
    ) -> Result<Vec<(String, Condition)>, crate::ContractError> {
        let mut compiled = Vec::with_capacity(set.contracts.len());
        for contract in &set.contracts {
            let cond = if contract.condition.is_empty() {
                Condition::Always
            } else {
                crate::parse_condition(&contract.condition).map_err(|e| {
                    crate::ContractError::ConditionParseError {
                        name: contract.name.clone(),
                        detail: e,
                    }
                })?
            };
            compiled.push((contract.name.clone(), cond));
        }
        Ok(compiled)
    }

    fn evaluate(
        &self,
        trigger: &Trigger,
        ctx: &EvalContext<'_>,
        tool_name: Option<&str>,
    ) -> Verdict {
        let contracts = self.contracts.read().unwrap();
        let conditions = self.conditions.read().unwrap();

        // Get matching contracts, sorted by priority (highest first).
        let mut matching: Vec<(&Contract, &Condition)> = contracts
            .contracts
            .iter()
            .filter(|c| c.enabled && c.trigger.matches(trigger))
            .filter_map(|c| {
                conditions
                    .iter()
                    .find(|(name, _)| name == &c.name)
                    .map(|(_, cond)| (c, cond))
            })
            .collect();

        matching.sort_by(|a, b| b.0.priority.cmp(&a.0.priority));

        for (contract, condition) in &matching {
            if condition.evaluate(ctx) {
                let verdict = Verdict::from_contract(contract);

                match &verdict.action {
                    Action::Deny => {
                        warn!(
                            contract = %contract.name,
                            tool = ?tool_name,
                            "Contract DENIED action: {}",
                            verdict.message
                        );
                    }
                    Action::Confirm => {
                        info!(
                            contract = %contract.name,
                            tool = ?tool_name,
                            "Contract requires CONFIRMATION: {}",
                            verdict.message
                        );
                    }
                    Action::Warn => {
                        warn!(
                            contract = %contract.name,
                            tool = ?tool_name,
                            "Contract WARNING: {}",
                            verdict.message
                        );
                    }
                    Action::Allow => {
                        debug!(
                            contract = %contract.name,
                            tool = ?tool_name,
                            "Contract explicitly ALLOWED"
                        );
                    }
                }

                // Log the evaluation (bounded).
                let entry = ContractLogEntry {
                    contract_name: contract.name.clone(),
                    trigger: String::from(contract.trigger.clone()),
                    verdict: verdict.clone(),
                    tool_name: tool_name.map(String::from),
                    timestamp: Utc::now(),
                };
                {
                    let mut log = self.log.write().unwrap();
                    if log.len() >= MAX_CONTRACT_LOG {
                        log.drain(..MAX_CONTRACT_LOG / 10);
                    }
                    log.push(entry);
                }

                return verdict;
            }
        }

        Verdict::allow()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::*;

    fn test_contracts() -> ContractSet {
        ContractSet::from_toml(
            r#"
[[contracts]]
name = "no-rm-rf"
description = "Block destructive rm -rf commands"
trigger = "tool:shell"
condition = 'args.command CONTAINS "rm -rf"'
action = "deny"
message = "Blocked: rm -rf is too dangerous"
priority = 100

[[contracts]]
name = "block-internal-ips"
description = "Block HTTP requests to internal networks"
trigger = "tool:http_request"
condition = 'args.url MATCHES "^https?://(10\\.|172\\.(1[6-9]|2[0-9]|3[01])\\.|192\\.168\\.)"'
action = "deny"
message = "Blocked: cannot access internal network addresses"
priority = 90

[[contracts]]
name = "warn-file-write"
description = "Warn on any file write"
trigger = "tool:file_write"
action = "warn"
message = "File write operation detected"
priority = 50

[[contracts]]
name = "confirm-large-purchase"
description = "Require confirmation for purchases over $50"
trigger = "tool:http_request"
condition = "args.amount > 50"
action = "confirm"
message = "Purchase exceeds $50 — please confirm"
priority = 80

[[contracts]]
name = "no-password-leak"
description = "Block responses containing passwords"
trigger = "response"
condition = 'content CONTAINS "password" AND content CONTAINS "sk-"'
action = "deny"
message = "Blocked: response appears to contain credentials"
priority = 100
"#,
        )
        .unwrap()
    }

    #[test]
    fn deny_rm_rf() {
        let engine = ContractEngine::new(test_contracts()).unwrap();
        let verdict = engine.check_tool_call("shell", &serde_json::json!({"command": "rm -rf /"}));
        assert!(!verdict.allowed);
        assert_eq!(verdict.action, Action::Deny);
        assert_eq!(verdict.contract_name.as_deref(), Some("no-rm-rf"));
    }

    #[test]
    fn allow_safe_shell() {
        let engine = ContractEngine::new(test_contracts()).unwrap();
        let verdict = engine.check_tool_call("shell", &serde_json::json!({"command": "ls -la"}));
        assert!(verdict.allowed);
        assert_eq!(verdict.action, Action::Allow);
        assert!(verdict.contract_name.is_none());
    }

    #[test]
    fn deny_internal_ip() {
        let engine = ContractEngine::new(test_contracts()).unwrap();
        let verdict = engine.check_tool_call(
            "http_request",
            &serde_json::json!({"url": "http://10.0.0.1/admin", "method": "GET"}),
        );
        assert!(!verdict.allowed);
        assert_eq!(verdict.contract_name.as_deref(), Some("block-internal-ips"));
    }

    #[test]
    fn allow_external_url() {
        let engine = ContractEngine::new(test_contracts()).unwrap();
        let verdict = engine.check_tool_call(
            "http_request",
            &serde_json::json!({"url": "https://api.example.com/data", "method": "GET"}),
        );
        assert!(verdict.allowed);
    }

    #[test]
    fn warn_file_write() {
        let engine = ContractEngine::new(test_contracts()).unwrap();
        let verdict = engine.check_tool_call(
            "file_write",
            &serde_json::json!({"path": "/tmp/test.txt", "content": "hello"}),
        );
        assert!(verdict.allowed); // Warn allows the action.
        assert_eq!(verdict.action, Action::Warn);
        assert_eq!(verdict.contract_name.as_deref(), Some("warn-file-write"));
    }

    #[test]
    fn confirm_large_purchase() {
        let engine = ContractEngine::new(test_contracts()).unwrap();
        let verdict = engine.check_tool_call(
            "http_request",
            &serde_json::json!({"url": "https://shop.com/buy", "amount": 100}),
        );
        assert!(!verdict.allowed); // Confirm = not auto-allowed.
        assert_eq!(verdict.action, Action::Confirm);
    }

    #[test]
    fn deny_password_in_response() {
        let engine = ContractEngine::new(test_contracts()).unwrap();
        let verdict = engine.check_response("Here is your password and API key: sk-abc123");
        assert!(!verdict.allowed);
        assert_eq!(verdict.contract_name.as_deref(), Some("no-password-leak"));
    }

    #[test]
    fn allow_clean_response() {
        let engine = ContractEngine::new(test_contracts()).unwrap();
        let verdict = engine.check_response("The weather today is sunny.");
        assert!(verdict.allowed);
    }

    #[test]
    fn engine_logs_evaluations() {
        let engine = ContractEngine::new(test_contracts()).unwrap();
        engine.check_tool_call("shell", &serde_json::json!({"command": "rm -rf /"}));
        engine.check_tool_call("file_write", &serde_json::json!({"path": "/tmp/x"}));

        let log = engine.log();
        assert_eq!(log.len(), 2);
        assert_eq!(log[0].contract_name, "no-rm-rf");
        assert_eq!(log[1].contract_name, "warn-file-write");
    }

    #[test]
    fn empty_engine_allows_all() {
        let engine = ContractEngine::empty();
        let verdict = engine.check_tool_call("shell", &serde_json::json!({"command": "rm -rf /"}));
        assert!(verdict.allowed);
        assert_eq!(engine.active_count(), 0);
    }

    #[test]
    fn add_remove_at_runtime() {
        let engine = ContractEngine::empty();
        assert_eq!(engine.active_count(), 0);

        engine
            .add_contract(Contract {
                name: "dynamic".into(),
                description: "Added at runtime".into(),
                trigger: Trigger::AnyTool,
                condition: String::new(),
                action: Action::Deny,
                message: "Denied by dynamic contract".into(),
                enabled: true,
                priority: 0,
            })
            .unwrap();

        assert_eq!(engine.active_count(), 1);
        let verdict = engine.check_tool_call("anything", &serde_json::json!({}));
        assert!(!verdict.allowed);

        assert!(engine.remove_contract("dynamic"));
        assert_eq!(engine.active_count(), 0);

        let verdict = engine.check_tool_call("anything", &serde_json::json!({}));
        assert!(verdict.allowed);
    }

    #[test]
    fn priority_ordering() {
        let toml = r#"
[[contracts]]
name = "low-priority-allow"
trigger = "tool:shell"
action = "allow"
priority = 10

[[contracts]]
name = "high-priority-deny"
trigger = "tool:shell"
condition = 'args.command CONTAINS "dangerous"'
action = "deny"
message = "Denied by high-priority contract"
priority = 100
"#;
        let set = ContractSet::from_toml(toml).unwrap();
        let engine = ContractEngine::new(set).unwrap();

        // "dangerous" matches the high-priority deny contract.
        let verdict =
            engine.check_tool_call("shell", &serde_json::json!({"command": "dangerous stuff"}));
        assert!(!verdict.allowed);
        assert_eq!(verdict.contract_name.as_deref(), Some("high-priority-deny"));
    }

    #[test]
    fn disabled_contracts_are_skipped() {
        let toml = r#"
[[contracts]]
name = "disabled"
trigger = "tool:*"
action = "deny"
enabled = false
"#;
        let set = ContractSet::from_toml(toml).unwrap();
        let engine = ContractEngine::new(set).unwrap();
        let verdict = engine.check_tool_call("shell", &serde_json::json!({}));
        assert!(verdict.allowed);
        assert_eq!(engine.active_count(), 0);
    }

    #[test]
    fn list_contracts_returns_all() {
        let engine = ContractEngine::new(test_contracts()).unwrap();
        let contracts = engine.list_contracts();
        assert_eq!(contracts.len(), 5);
    }
}
