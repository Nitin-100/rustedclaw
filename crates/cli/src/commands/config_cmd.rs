//! `rustedclaw config` — Configuration management commands.

use rustedclaw_config::AppConfig;

pub async fn validate() -> Result<(), Box<dyn std::error::Error>> {
    println!("🔍 Validating configuration...");

    match AppConfig::load() {
        Ok(config) => {
            println!("   ✅ Config parsed successfully");

            // Additional validation checks
            let mut warnings = Vec::new();

            if config.api_key.is_none() {
                warnings.push("No API key set (set OPENAI_API_KEY or OPENROUTER_API_KEY env var)");
            }

            if config.default_temperature < 0.0 || config.default_temperature > 2.0 {
                warnings.push("Temperature out of range (0.0–2.0)");
            }

            if config.memory.vector_weight + config.memory.keyword_weight <= 0.0 {
                warnings.push("Memory search weights must sum to > 0");
            }

            if config.gateway.host == "0.0.0.0" && !config.gateway.allow_public_bind {
                warnings.push("Gateway bound to 0.0.0.0 without allow_public_bind = true");
            }

            if config
                .routines
                .iter()
                .any(|r| r.schedule.split_whitespace().count() != 5)
            {
                warnings.push("One or more routines have invalid cron expressions");
            }

            if warnings.is_empty() {
                println!("   ✅ All checks passed");
            } else {
                println!();
                for w in &warnings {
                    println!("   ⚠️  {w}");
                }
            }

            println!();
            println!("   Provider:  {}", config.default_provider);
            println!("   Model:     {}", config.default_model);
            println!(
                "   Gateway:   {}:{}",
                config.gateway.host, config.gateway.port
            );
            println!("   Memory:    {}", config.memory.backend);
            println!("   Autonomy:  {}", config.autonomy.level);
            println!("   Routines:  {}", config.routines.len());
        }
        Err(e) => {
            println!("   ❌ Config error: {e}");
            return Err(e.into());
        }
    }

    Ok(())
}

pub async fn show() -> Result<(), Box<dyn std::error::Error>> {
    let config = AppConfig::load().map_err(|e| format!("Failed to load config: {e}"))?;
    let toml_str = toml::to_string_pretty(&config)?;
    println!("{toml_str}");
    Ok(())
}

pub async fn path() -> Result<(), Box<dyn std::error::Error>> {
    let config_path = AppConfig::config_dir().join("config.toml");
    println!("{}", config_path.display());
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn config_path_is_valid() {
        let path = rustedclaw_config::AppConfig::config_dir().join("config.toml");
        assert!(path.to_str().unwrap().contains("config.toml"));
    }
}
