//! `rustedclaw gateway` — Start the HTTP API server.

use rustedclaw_config::AppConfig;

pub async fn run(port_override: Option<u16>) -> Result<(), Box<dyn std::error::Error>> {
    let mut config = AppConfig::load().map_err(|e| format!("Failed to load config: {e}"))?;

    if let Some(port) = port_override {
        config.gateway.port = port;
    }

    println!("🦀 RustedClaw Gateway");
    println!("   Listening: {}:{}", config.gateway.host, config.gateway.port);
    println!("   Pairing required: {}", config.gateway.require_pairing);

    rustedclaw_gateway::start(config).await?;

    Ok(())
}
