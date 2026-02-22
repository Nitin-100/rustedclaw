//! `rustedclaw routine` — Manage cron routines.

use rustedclaw_config::AppConfig;

pub async fn list() -> Result<(), Box<dyn std::error::Error>> {
    let config = AppConfig::load().map_err(|e| format!("Failed to load config: {e}"))?;

    if config.routines.is_empty() {
        println!("📋 No routines configured.");
        println!();
        println!("   Add one with:");
        println!("   rustedclaw routine add \"daily_check\" \"0 9 * * *\" \"Summarize my pending tasks\"");
        return Ok(());
    }

    println!("📋 Routines ({}):", config.routines.len());
    println!("{:-<72}", "");
    for (i, routine) in config.routines.iter().enumerate() {
        let status = if routine.enabled { "✅" } else { "⏸️ " };
        let action_type = match &routine.action {
            rustedclaw_config::RoutineAction::AgentTask { .. } => "agent",
            rustedclaw_config::RoutineAction::RunTool { tool, .. } => tool.as_str(),
            rustedclaw_config::RoutineAction::SendMessage { channel, .. } => channel.as_str(),
        };
        println!(
            "  {i:>2}. {status} {:<20} {:<20} → {action_type}",
            routine.name, routine.schedule
        );
        if let Some(ch) = &routine.target_channel {
            println!("      └─ channel: {ch}");
        }
    }

    Ok(())
}

pub async fn add(name: &str, schedule: &str, prompt: &str) -> Result<(), Box<dyn std::error::Error>> {
    let mut config = AppConfig::load().map_err(|e| format!("Failed to load config: {e}"))?;

    // Check for duplicate name
    if config.routines.iter().any(|r| r.name == name) {
        println!("❌ Routine '{name}' already exists. Remove it first or use a different name.");
        return Ok(());
    }

    // Validate cron expression (basic check)
    let fields: Vec<&str> = schedule.trim().split_whitespace().collect();
    if fields.len() != 5 {
        println!("❌ Invalid cron expression: expected 5 fields (minute hour dom month dow)");
        println!("   Example: \"*/30 * * * *\" = every 30 minutes");
        return Ok(());
    }

    config.routines.push(rustedclaw_config::RoutineConfig {
        name: name.to_string(),
        schedule: schedule.to_string(),
        action: rustedclaw_config::RoutineAction::AgentTask {
            prompt: prompt.to_string(),
            context: None,
        },
        target_channel: None,
        enabled: true,
    });

    save_config(&config)?;
    println!("✅ Routine '{name}' added with schedule: {schedule}");
    Ok(())
}

pub async fn remove(name: &str) -> Result<(), Box<dyn std::error::Error>> {
    let mut config = AppConfig::load().map_err(|e| format!("Failed to load config: {e}"))?;

    let before = config.routines.len();
    config.routines.retain(|r| r.name != name);

    if config.routines.len() == before {
        println!("❌ Routine '{name}' not found.");
    } else {
        save_config(&config)?;
        println!("🗑️  Routine '{name}' removed.");
    }

    Ok(())
}

pub async fn pause(name: &str) -> Result<(), Box<dyn std::error::Error>> {
    let mut config = AppConfig::load().map_err(|e| format!("Failed to load config: {e}"))?;

    if let Some(routine) = config.routines.iter_mut().find(|r| r.name == name) {
        routine.enabled = false;
        save_config(&config)?;
        println!("⏸️  Routine '{name}' paused.");
    } else {
        println!("❌ Routine '{name}' not found.");
    }

    Ok(())
}

pub async fn resume(name: &str) -> Result<(), Box<dyn std::error::Error>> {
    let mut config = AppConfig::load().map_err(|e| format!("Failed to load config: {e}"))?;

    if let Some(routine) = config.routines.iter_mut().find(|r| r.name == name) {
        routine.enabled = true;
        save_config(&config)?;
        println!("▶️  Routine '{name}' resumed.");
    } else {
        println!("❌ Routine '{name}' not found.");
    }

    Ok(())
}

/// Save config back to TOML file.
fn save_config(config: &AppConfig) -> Result<(), Box<dyn std::error::Error>> {
    let path = AppConfig::config_dir().join("config.toml");
    let toml_str = toml::to_string_pretty(config)?;
    std::fs::write(&path, toml_str)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn cron_field_validation() {
        let valid = "*/5 * * * *";
        let fields: Vec<&str> = valid.split_whitespace().collect();
        assert_eq!(fields.len(), 5);

        let invalid = "* *";
        let fields: Vec<&str> = invalid.split_whitespace().collect();
        assert_ne!(fields.len(), 5);
    }
}
