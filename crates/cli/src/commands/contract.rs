//! CLI commands for managing agent contracts (behavior guardrails).

use rustedclaw_config::AppConfig;
use rustedclaw_contracts::{Contract, ContractEngine, ContractSet};

/// List all configured contracts.
pub async fn list() -> Result<(), Box<dyn std::error::Error>> {
    let config = AppConfig::load()?;
    let contracts = build_contracts(&config);

    if contracts.is_empty() {
        println!("No contracts configured.");
        println!("\nAdd contracts in config.toml:");
        println!("  [[contracts]]");
        println!("  name = \"no-rm-rf\"");
        println!("  trigger = \"tool:shell\"");
        println!("  condition = 'args.command CONTAINS \"rm -rf\"'");
        println!("  action = \"deny\"");
        println!("  message = \"Blocked: rm -rf is forbidden\"");
        return Ok(());
    }

    println!("Agent Contracts ({} active):\n", contracts.len());
    for (i, c) in contracts.iter().enumerate() {
        let status = if c.enabled { "ON " } else { "OFF" };
        let trigger_str: String = c.trigger.clone().into();
        let action = format!("{:?}", c.action).to_lowercase();
        println!(
            "  {}. [{}] {} (trigger: {}, action: {}, priority: {})",
            i + 1,
            status,
            c.name,
            trigger_str,
            action,
            c.priority
        );
        if !c.description.is_empty() {
            println!("     {}", c.description);
        }
        if !c.condition.is_empty() {
            println!("     condition: {}", c.condition);
        }
        if !c.message.is_empty() {
            println!("     message: {}", c.message);
        }
    }
    Ok(())
}

/// Validate all contracts in the configuration.
pub async fn validate() -> Result<(), Box<dyn std::error::Error>> {
    let config = AppConfig::load()?;
    let contracts = build_contracts(&config);

    if contracts.is_empty() {
        println!("No contracts configured.");
        return Ok(());
    }

    let mut contract_set = ContractSet::new();
    for c in &contracts {
        contract_set.add(c.clone());
    }

    match ContractEngine::new(contract_set) {
        Ok(engine) => {
            println!(
                "All {} contracts are valid. {} active.",
                contracts.len(),
                engine.active_count()
            );
        }
        Err(e) => {
            eprintln!("Contract validation failed: {e}");
            std::process::exit(1);
        }
    }
    Ok(())
}

/// Test a contract against a simulated tool call.
pub async fn test(tool: &str, args_json: &str) -> Result<(), Box<dyn std::error::Error>> {
    let config = AppConfig::load()?;
    let contracts = build_contracts(&config);

    if contracts.is_empty() {
        println!("No contracts configured.");
        return Ok(());
    }

    let mut contract_set = ContractSet::new();
    for c in &contracts {
        contract_set.add(c.clone());
    }
    let engine = ContractEngine::new(contract_set)?;

    let args: serde_json::Value = serde_json::from_str(args_json)
        .unwrap_or_else(|_| serde_json::json!({ "command": args_json }));

    let verdict = engine.check_tool_call(tool, &args);

    let action = format!("{:?}", verdict.action).to_lowercase();
    if verdict.allowed {
        println!("ALLOWED (action: {action})");
    } else {
        println!("BLOCKED (action: {action})");
    }
    if let Some(name) = &verdict.contract_name {
        println!("  contract: {name}");
    }
    if !verdict.message.is_empty() {
        println!("  message: {}", verdict.message);
    }
    Ok(())
}

fn build_contracts(config: &AppConfig) -> Vec<Contract> {
    config
        .contracts
        .iter()
        .map(|cc| Contract {
            name: cc.name.clone(),
            description: cc.description.clone(),
            trigger: cc.trigger.clone().into(),
            condition: cc.condition.clone(),
            action: match cc.action.as_str() {
                "allow" => rustedclaw_contracts::Action::Allow,
                "confirm" => rustedclaw_contracts::Action::Confirm,
                "warn" => rustedclaw_contracts::Action::Warn,
                _ => rustedclaw_contracts::Action::Deny,
            },
            message: cc.message.clone(),
            enabled: cc.enabled,
            priority: cc.priority,
        })
        .collect()
}
