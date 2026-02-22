//! `rustedclaw status` — Show system status.

use rustedclaw_config::AppConfig;

pub async fn run() -> Result<(), Box<dyn std::error::Error>> {
    let config = AppConfig::load().map_err(|e| format!("Failed to load config: {e}"))?;

    println!("🦀 RustedClaw Status");
    println!("==================");
    println!("  Config dir:   {}", AppConfig::config_dir().display());
    println!("  Workspace:    {}", AppConfig::workspace_dir().display());
    println!("  Provider:     {}", config.default_provider);
    println!("  Model:        {}", config.default_model);
    println!("  Temperature:  {}", config.default_temperature);
    println!("  Memory:       {}", config.memory.backend);
    println!("  Gateway:      {}:{}", config.gateway.host, config.gateway.port);
    println!("  Autonomy:     {}", config.autonomy.level);
    println!("  Runtime:      {}", config.runtime.kind);
    println!("  Heartbeat:    {}", if config.heartbeat.enabled { "enabled" } else { "disabled" });
    println!("  Tunnel:       {}", config.tunnel.provider);
    println!("  Secrets:      {}", if config.secrets.encrypt { "encrypted" } else { "plaintext" });

    // Check config file existence
    let config_path = AppConfig::config_dir().join("config.toml");
    if config_path.exists() {
        println!("\n  ✅ Config file found");
    } else {
        println!("\n  ⚠️  No config file — run `rustedclaw onboard` first");
    }

    Ok(())
}
