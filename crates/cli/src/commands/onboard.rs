//! `rustedclaw onboard` — First-time setup wizard.

use rustedclaw_config::AppConfig;

pub async fn run() -> Result<(), Box<dyn std::error::Error>> {
    let config_dir = AppConfig::config_dir();
    let config_path = config_dir.join("config.toml");
    let workspace_dir = AppConfig::workspace_dir();

    println!("🦀 RustedClaw — First-Time Setup");
    println!("================================\n");

    // Create directories
    if !config_dir.exists() {
        std::fs::create_dir_all(&config_dir)?;
        println!("✅ Created config directory: {}", config_dir.display());
    } else {
        println!("  Config directory exists: {}", config_dir.display());
    }

    if !workspace_dir.exists() {
        std::fs::create_dir_all(&workspace_dir)?;
        println!("✅ Created workspace directory: {}", workspace_dir.display());
    }

    // Create default identity files
    let identity_path = workspace_dir.join("IDENTITY.md");
    if !identity_path.exists() {
        std::fs::write(
            &identity_path,
            concat!(
                "# Identity\n\n",
                "You are RustedClaw, a helpful AI assistant.\n\n",
                "You have access to tools (shell, file_read, file_write) that let you\n",
                "interact with the user's system. Use them proactively when they would\n",
                "help accomplish the task.\n",
            ),
        )?;
        println!("✅ Created IDENTITY.md");
    }

    let soul_path = workspace_dir.join("SOUL.md");
    if !soul_path.exists() {
        std::fs::write(
            &soul_path,
            concat!(
                "# Personality & Tone\n\n",
                "- Be concise and direct\n",
                "- Show your reasoning when solving complex problems\n",
                "- Ask for clarification when the request is ambiguous\n",
                "- Be honest about limitations and uncertainties\n",
            ),
        )?;
        println!("✅ Created SOUL.md");
    }

    let user_path = workspace_dir.join("USER.md");
    if !user_path.exists() {
        std::fs::write(
            &user_path,
            concat!(
                "# User Context\n\n",
                "<!-- Add information about yourself that the agent should know -->\n",
                "<!-- Examples: preferred languages, coding style, project context -->\n\n",
                "- Operating System: Windows\n",
                "- Preferred Language: (edit this)\n",
            ),
        )?;
        println!("✅ Created USER.md");
    }

    // Create config file
    if config_path.exists() {
        println!("\n⚠️  Config already exists at: {}", config_path.display());
        println!("   Edit it manually or delete and re-run onboard.\n");
    } else {
        let default_toml = AppConfig::default_toml();
        std::fs::write(&config_path, &default_toml)?;
        println!("✅ Created config.toml at: {}", config_path.display());
        println!("\n📝 Next steps:");
        println!("   1. Edit {} and add your API key", config_path.display());
        println!("   2. Run: rustedclaw agent");
        println!("   3. Start chatting!\n");
    }

    println!("🎉 Setup complete! Run `rustedclaw agent` to start chatting.\n");

    Ok(())
}
