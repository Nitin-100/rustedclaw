//! `rustedclaw gateway` — Start the HTTP API server.

use rustedclaw_config::AppConfig;

pub async fn run(
    port_override: Option<u16>,
    host_override: Option<String>,
    local: bool,
    model_override: Option<String>,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut config = AppConfig::load().map_err(|e| format!("Failed to load config: {e}"))?;

    if let Some(port) = port_override {
        config.gateway.port = port;
    }
    if let Some(host) = host_override {
        config.gateway.host = host;
    }

    // If --local is set, override provider to local inference
    if local {
        let model = model_override.unwrap_or_else(|| "tinyllama".to_string());

        config.default_provider = "local".to_string();
        config.default_model = model.clone();

        config.providers.insert(
            "local".to_string(),
            rustedclaw_config::ProviderConfig {
                api_key: None,
                api_url: Some("local://candle".to_string()),
                default_model: Some(model),
            },
        );
    }

    println!("🦀 RustedClaw Gateway");
    println!(
        "   Listening: {}:{}",
        config.gateway.host, config.gateway.port
    );
    if local {
        println!("   Provider:  local (Candle — Rust-native ML)");
        println!("   Model:     {}", config.default_model);
        println!("   Network:   OFFLINE — zero API calls");
    }
    println!("   Pairing required: {}", config.gateway.require_pairing);

    rustedclaw_gateway::start(config).await?;

    Ok(())
}
