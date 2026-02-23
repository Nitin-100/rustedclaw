//! Contract data model — the types that define behavior specifications.

use serde::{Deserialize, Serialize};

/// A set of contracts loaded from configuration.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ContractSet {
    /// All active contracts.
    #[serde(default)]
    pub contracts: Vec<Contract>,
}

impl ContractSet {
    pub fn new() -> Self {
        Self::default()
    }

    /// Load contracts from a TOML string.
    pub fn from_toml(toml_str: &str) -> Result<Self, crate::ContractError> {
        let set: ContractSet = toml::from_str(toml_str)?;
        set.validate()?;
        Ok(set)
    }

    /// Add a contract to the set.
    pub fn add(&mut self, contract: Contract) {
        self.contracts.push(contract);
    }

    /// Remove a contract by name.  Returns `true` if found.
    pub fn remove(&mut self, name: &str) -> bool {
        let before = self.contracts.len();
        self.contracts.retain(|c| c.name != name);
        self.contracts.len() < before
    }

    /// Validate all contracts in the set.
    pub fn validate(&self) -> Result<(), crate::ContractError> {
        for contract in &self.contracts {
            contract.validate()?;
        }
        Ok(())
    }

    /// Get contracts that match a given trigger.
    pub fn matching(&self, trigger: &Trigger) -> Vec<&Contract> {
        self.contracts
            .iter()
            .filter(|c| c.enabled && c.trigger.matches(trigger))
            .collect()
    }

    /// Number of active (enabled) contracts.
    pub fn active_count(&self) -> usize {
        self.contracts.iter().filter(|c| c.enabled).count()
    }
}

/// A single behavior contract.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Contract {
    /// Unique name for this contract.
    pub name: String,

    /// Human-readable description of what this contract enforces.
    #[serde(default)]
    pub description: String,

    /// What triggers this contract (e.g. "tool:shell", "tool:*", "response").
    pub trigger: Trigger,

    /// The condition expression to evaluate.
    /// If omitted or empty, the contract fires on every matching trigger.
    #[serde(default)]
    pub condition: String,

    /// What to do when the condition matches.
    #[serde(default)]
    pub action: Action,

    /// Message to display when the contract fires.
    #[serde(default)]
    pub message: String,

    /// Whether this contract is enabled.
    #[serde(default = "default_true")]
    pub enabled: bool,

    /// Priority (higher = evaluated first).  Ties broken by insertion order.
    #[serde(default)]
    pub priority: i32,
}

fn default_true() -> bool {
    true
}

impl Contract {
    /// Validate that the contract is well-formed.
    pub fn validate(&self) -> Result<(), crate::ContractError> {
        if self.name.is_empty() {
            return Err(crate::ContractError::InvalidContract {
                name: "(empty)".into(),
                reason: "contract name cannot be empty".into(),
            });
        }
        // Validate the trigger.
        self.trigger.validate(&self.name)?;
        // Validate the condition expression if non-empty.
        if !self.condition.is_empty() {
            crate::parse_condition(&self.condition).map_err(|e| {
                crate::ContractError::ConditionParseError {
                    name: self.name.clone(),
                    detail: e,
                }
            })?;
        }
        Ok(())
    }
}

/// What triggers a contract evaluation.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(from = "String", into = "String")]
pub enum Trigger {
    /// Matches a specific tool by name (e.g. `tool:shell`).
    Tool(String),
    /// Matches any tool invocation (`tool:*`).
    AnyTool,
    /// Matches the agent's text response before it is returned.
    Response,
    /// Matches any event.
    Any,
}

impl Trigger {
    /// Does this trigger match the given concrete trigger?
    pub fn matches(&self, other: &Trigger) -> bool {
        match (self, other) {
            (Trigger::Any, _) => true,
            (Trigger::AnyTool, Trigger::Tool(_) | Trigger::AnyTool) => true,
            (Trigger::Tool(a), Trigger::Tool(b)) => a == b,
            (Trigger::Response, Trigger::Response) => true,
            _ => false,
        }
    }

    fn validate(&self, contract_name: &str) -> Result<(), crate::ContractError> {
        if let Trigger::Tool(name) = self {
            if name.is_empty() {
                return Err(crate::ContractError::InvalidContract {
                    name: contract_name.into(),
                    reason: "tool trigger name cannot be empty".into(),
                });
            }
        }
        Ok(())
    }
}

impl From<String> for Trigger {
    fn from(s: String) -> Self {
        match s.as_str() {
            "*" | "any" => Trigger::Any,
            "tool:*" => Trigger::AnyTool,
            "response" => Trigger::Response,
            other => {
                if let Some(name) = other.strip_prefix("tool:") {
                    Trigger::Tool(name.to_string())
                } else {
                    // Bare name treated as tool trigger.
                    Trigger::Tool(other.to_string())
                }
            }
        }
    }
}

impl From<Trigger> for String {
    fn from(t: Trigger) -> Self {
        match t {
            Trigger::Tool(name) => format!("tool:{name}"),
            Trigger::AnyTool => "tool:*".into(),
            Trigger::Response => "response".into(),
            Trigger::Any => "*".into(),
        }
    }
}

/// What happens when a contract's condition is satisfied.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    /// Allow the action (useful as an explicit pass in priority chains).
    Allow,
    /// Deny the action — the tool call is blocked.
    #[default]
    Deny,
    /// Pause and ask the user for confirmation.
    Confirm,
    /// Log a warning but allow the action to proceed.
    Warn,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trigger_from_string() {
        assert!(matches!(Trigger::from(String::from("tool:shell")), Trigger::Tool(n) if n == "shell"));
        assert!(matches!(Trigger::from(String::from("tool:*")), Trigger::AnyTool));
        assert!(matches!(Trigger::from(String::from("response")), Trigger::Response));
        assert!(matches!(Trigger::from(String::from("*")), Trigger::Any));
        assert!(matches!(Trigger::from(String::from("any")), Trigger::Any));
        assert!(matches!(Trigger::from(String::from("shell")), Trigger::Tool(n) if n == "shell"));
    }

    #[test]
    fn trigger_matching() {
        let shell = Trigger::Tool("shell".into());
        let file = Trigger::Tool("file_write".into());

        assert!(Trigger::Any.matches(&shell));
        assert!(Trigger::Any.matches(&Trigger::Response));
        assert!(Trigger::AnyTool.matches(&shell));
        assert!(Trigger::AnyTool.matches(&file));
        assert!(!Trigger::AnyTool.matches(&Trigger::Response));
        assert!(shell.matches(&Trigger::Tool("shell".into())));
        assert!(!shell.matches(&file));
    }

    #[test]
    fn action_default_is_deny() {
        assert_eq!(Action::default(), Action::Deny);
    }

    #[test]
    fn contract_set_from_toml() {
        let toml = r#"
[[contracts]]
name = "no-rm-rf"
description = "Block rm -rf commands"
trigger = "tool:shell"
condition = 'args.command CONTAINS "rm -rf"'
action = "deny"
message = "Blocked: rm -rf is forbidden"

[[contracts]]
name = "warn-http"
trigger = "tool:http_request"
action = "warn"
message = "HTTP request detected"
"#;
        let set = ContractSet::from_toml(toml).unwrap();
        assert_eq!(set.contracts.len(), 2);
        assert_eq!(set.active_count(), 2);
        assert_eq!(set.contracts[0].name, "no-rm-rf");
        assert_eq!(set.contracts[0].action, Action::Deny);
        assert_eq!(set.contracts[1].action, Action::Warn);
    }

    #[test]
    fn contract_set_add_remove() {
        let mut set = ContractSet::new();
        set.add(Contract {
            name: "test".into(),
            description: "a test".into(),
            trigger: Trigger::Any,
            condition: String::new(),
            action: Action::Allow,
            message: String::new(),
            enabled: true,
            priority: 0,
        });
        assert_eq!(set.active_count(), 1);
        assert!(set.remove("test"));
        assert_eq!(set.active_count(), 0);
        assert!(!set.remove("nonexistent"));
    }

    #[test]
    fn matching_filters_by_trigger_and_enabled() {
        let mut set = ContractSet::new();
        set.add(Contract {
            name: "a".into(),
            description: String::new(),
            trigger: Trigger::Tool("shell".into()),
            condition: String::new(),
            action: Action::Deny,
            message: String::new(),
            enabled: true,
            priority: 0,
        });
        set.add(Contract {
            name: "b".into(),
            description: String::new(),
            trigger: Trigger::Tool("file_write".into()),
            condition: String::new(),
            action: Action::Deny,
            message: String::new(),
            enabled: true,
            priority: 0,
        });
        set.add(Contract {
            name: "c-disabled".into(),
            description: String::new(),
            trigger: Trigger::Tool("shell".into()),
            condition: String::new(),
            action: Action::Deny,
            message: String::new(),
            enabled: false,
            priority: 0,
        });

        let shell_trigger = Trigger::Tool("shell".into());
        let matches = set.matching(&shell_trigger);
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].name, "a");
    }

    #[test]
    fn empty_name_rejects() {
        let c = Contract {
            name: String::new(),
            description: String::new(),
            trigger: Trigger::Any,
            condition: String::new(),
            action: Action::Deny,
            message: String::new(),
            enabled: true,
            priority: 0,
        };
        assert!(c.validate().is_err());
    }
}
