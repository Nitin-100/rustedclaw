//! RustedClaw CLI — the main entry point.
//!
//! Commands:
//! - `onboard`  — Initialize config & workspace
//! - `agent`    — Interactive chat or single-message mode
//! - `gateway`  — Start the HTTP webhook server
//! - `daemon`   — Start full autonomous runtime
//! - `status`   — Show system status
//! - `doctor`   — Diagnose system health

use clap::{Parser, Subcommand};

mod commands;

#[derive(Parser)]
#[command(
    name = "rustedclaw",
    about = "RustedClaw — AI Agent Runtime Infrastructure",
    version,
    author
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,

    /// Enable verbose logging
    #[arg(short, long, global = true)]
    verbose: bool,
}

#[derive(Subcommand)]
enum Commands {
    /// Initialize configuration and workspace
    Onboard,

    /// Chat with the AI agent
    Agent {
        /// Send a single message instead of entering interactive mode
        #[arg(short, long)]
        message: Option<String>,
    },

    /// Start the HTTP gateway server
    Gateway {
        /// Override the port
        #[arg(short, long)]
        port: Option<u16>,
    },

    /// Start the full daemon (gateway + channels + cron)
    Daemon,

    /// Show system status
    Status,

    /// Diagnose system health
    Doctor,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    // Initialize tracing
    let filter = if cli.verbose { "debug" } else { "info" };
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(filter)),
        )
        .with_target(false)
        .init();

    match cli.command {
        Commands::Onboard => commands::onboard::run().await?,
        Commands::Agent { message } => commands::agent::run(message).await?,
        Commands::Gateway { port } => commands::gateway::run(port).await?,
        Commands::Daemon => commands::daemon::run().await?,
        Commands::Status => commands::status::run().await?,
        Commands::Doctor => commands::doctor::run().await?,
    }

    Ok(())
}
