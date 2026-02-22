//! `rustedclaw agent` — Interactive or single-message chat mode.

use std::sync::Arc;
use rustedclaw_agent::AgentLoop;
use rustedclaw_channels::CliChannel;
use rustedclaw_config::AppConfig;
use rustedclaw_core::channel::Channel;
use rustedclaw_core::event::EventBus;
use rustedclaw_core::identity::{ContextPaths, Identity};
use rustedclaw_core::message::{Conversation, Message};

pub async fn run(message: Option<String>) -> Result<(), Box<dyn std::error::Error>> {
    let config = AppConfig::load().map_err(|e| format!("Failed to load config: {e}"))?;

    // Check for API key early — give a clear error
    if config.api_key.is_none() {
        eprintln!();
        eprintln!("  ERROR: No API key configured!");
        eprintln!();
        eprintln!("  Set one of these environment variables:");
        eprintln!("    $env:OPENROUTER_API_KEY = 'sk-or-v1-...'   (recommended)");
        eprintln!("    $env:OPENAI_API_KEY     = 'sk-...'         (for OpenAI direct)");
        eprintln!("    $env:RUSTEDCLAW_API_KEY   = 'sk-...'         (generic)");
        eprintln!();
        eprintln!("  Or add it to your config file:");
        eprintln!("    {}", AppConfig::config_dir().join("config.toml").display());
        eprintln!();
        eprintln!("  Get an OpenRouter key at: https://openrouter.ai/keys");
        eprintln!();
        return Err("No API key found. See above for setup instructions.".into());
    }

    // --- Context Loading ---
    // Build context paths from config + current working directory
    let cwd = std::env::current_dir().unwrap_or_default();
    let project_dir = if config.identity.load_project_context {
        let candidate = cwd.join(".rustedclaw");
        if candidate.is_dir() {
            Some(candidate)
        } else {
            None
        }
    } else {
        None
    };

    let context_paths = ContextPaths {
        global_dir: Some(AppConfig::workspace_dir()),
        project_dir,
        extra_files: config.identity.extra_context_files
            .iter()
            .map(std::path::PathBuf::from)
            .collect(),
        system_prompt_override: config.identity.system_prompt_override.clone(),
    };

    let identity = Identity::load(&context_paths);

    // Build provider from config
    let router = rustedclaw_providers::router::build_from_config(&config);
    let provider = router.default().ok_or("No default provider configured")?;

    // Build tools
    let tools = Arc::new(rustedclaw_tools::default_registry());

    // Build agent with loaded context
    let event_bus = Arc::new(EventBus::default());
    let context_files_count = identity.loaded_files.len();
    let context_tokens = identity.estimated_tokens();
    let agent_name = identity.name.clone();
    let agent = AgentLoop::new(
        provider,
        &config.default_model,
        config.default_temperature,
        tools,
        identity,
        event_bus,
    ).with_max_tokens(config.default_max_tokens);

    if let Some(msg) = message {
        // Single message mode
        let mut conv = Conversation::new();
        conv.push(Message::user(&msg));

        eprint!("  Thinking...");
        let response = agent.process(&mut conv).await?;
        eprint!("\r              \r");
        println!("{response}");
    } else {
        // Interactive mode
        println!();
        println!("  ╔══════════════════════════════════════════════╗");
        println!("  ║       RustedClaw Agent — Interactive Mode      ║");
        println!("  ╚══════════════════════════════════════════════╝");
        println!();
        println!("  Provider:  {}", config.default_provider);
        println!("  Model:     {}", config.default_model);
        println!("  Tools:     shell, file_read, file_write");
        println!("  Context:   {} files loaded (~{} tokens)", context_files_count, context_tokens);
        println!("  Agent:     {}", agent_name);
        println!();
        println!("  Type your message and press Enter.");
        println!("  Type 'exit' or Ctrl+C to quit.");
        println!();

        let channel = CliChannel::new();
        let mut rx = channel.start().await.map_err(|e| format!("Channel error: {e}"))?;
        let mut conv = Conversation::new();

        print!("  You > ");
        use std::io::Write;
        std::io::stdout().flush()?;

        while let Some(result) = rx.recv().await {
            match result {
                Ok(chan_msg) => {
                    conv.push(Message::user(&chan_msg.content));

                    eprint!("  ...");

                    match agent.process(&mut conv).await {
                        Ok(response) => {
                            eprint!("\r     \r");
                            println!();
                            // Print with a visible assistant prefix
                            for line in response.lines() {
                                println!("  Assistant > {line}");
                            }
                            println!();
                        }
                        Err(e) => {
                            eprint!("\r     \r");
                            eprintln!("  [Error] {e}");
                            println!();
                        }
                    }

                    print!("  You > ");
                    std::io::stdout().flush()?;
                }
                Err(e) => {
                    eprintln!("  [Channel Error] {e}");
                    break;
                }
            }
        }

        println!();
        println!("  Goodbye! 👋");
        println!();
    }

    Ok(())
}
