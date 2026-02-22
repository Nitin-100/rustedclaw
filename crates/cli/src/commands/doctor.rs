//! `rustedclaw doctor` — Diagnose system health.

use rustedclaw_config::AppConfig;

pub async fn run() -> Result<(), Box<dyn std::error::Error>> {
    println!("🩺 RustedClaw Doctor — System Diagnostics");
    println!("========================================\n");

    let mut issues = 0;

    // Check Rust version
    println!("  ✅ Rust binary running");

    // Check config
    let config_path = AppConfig::config_dir().join("config.toml");
    if config_path.exists() {
        match AppConfig::load() {
            Ok(config) => {
                println!("  ✅ Config file valid");

                // Check API key
                if config.api_key.is_some() || !config.providers.is_empty() {
                    println!("  ✅ API key configured");
                } else {
                    println!("  ⚠️  No API key configured — add api_key to config.toml");
                    issues += 1;
                }
            }
            Err(e) => {
                println!("  ❌ Config file invalid: {e}");
                issues += 1;
            }
        }
    } else {
        println!("  ❌ No config file — run `rustedclaw onboard`");
        issues += 1;
    }

    // Check workspace
    let workspace_dir = AppConfig::workspace_dir();
    if workspace_dir.exists() {
        println!("  ✅ Workspace directory exists");
    } else {
        println!("  ⚠️  No workspace directory — run `rustedclaw onboard`");
        issues += 1;
    }

    // Summary
    println!();
    if issues == 0 {
        println!("  🎉 All checks passed!");
    } else {
        println!("  ⚠️  {issues} issue(s) found. See above for details.");
    }

    Ok(())
}
