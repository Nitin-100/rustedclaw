//! RustedClaw CLI — the main entry point.
//!
//! Commands:
//! - `onboard`      — Initialize config & workspace
//! - `agent`        — Interactive chat or single-message mode
//! - `gateway`      — Start the HTTP webhook server
//! - `daemon`       — Start full autonomous runtime
//! - `status`       — Show system status
//! - `doctor`       — Diagnose system health
//! - `completions`  — Generate shell completions
//! - `estop`        — Emergency stop all running tasks
//! - `migrate`      — Import data from other runtimes
//! - `routine`      — Manage cron routines
//! - `memory`       — Memory management commands
//! - `config`       — Configuration management
//! - `providers`    — List supported providers
//! - `version`      — Show detailed version info

use clap::{CommandFactory, Parser, Subcommand};
use clap_complete::{Shell, generate};

mod commands;

#[derive(Parser)]
#[command(
    name = "rustedclaw",
    about = "RustedClaw — AI Agent Runtime Infrastructure. No account required. No lock-in. Bring your own API key.",
    version,
    author = "RustedClaw Contributors",
    long_about = "RustedClaw is a lightweight, self-hosted AI agent runtime.\n\nNo account required. No vendor lock-in. Bring your own API key from any provider.\nSingle static binary, <7 MB RAM, deploys on $5 hardware."
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

        /// Override the host (e.g. 0.0.0.0 for Docker)
        #[arg(long)]
        host: Option<String>,
    },

    /// Start the full daemon (gateway + channels + cron)
    Daemon,

    /// Show system status
    Status,

    /// Diagnose system health
    Doctor,

    /// Generate shell completion scripts
    Completions {
        /// Shell to generate completions for
        #[arg(value_enum)]
        shell: Shell,
    },

    /// Emergency stop — halt all background tasks
    Estop {
        /// Resume from emergency stop
        #[arg(long)]
        resume: bool,
    },

    /// Import data from another runtime
    Migrate {
        /// Source runtime to migrate from
        #[arg(value_enum)]
        source: MigrateSource,

        /// Preview changes without applying them
        #[arg(long)]
        dry_run: bool,

        /// Path to the source workspace (default: auto-detect)
        #[arg(long)]
        path: Option<String>,
    },

    /// Manage cron routines
    Routine {
        #[command(subcommand)]
        action: RoutineAction,
    },

    /// Memory management commands
    Memory {
        #[command(subcommand)]
        action: MemoryAction,
    },

    /// Configuration management
    Config {
        #[command(subcommand)]
        action: ConfigAction,
    },

    /// Manage agent behavior contracts (guardrails)
    Contract {
        #[command(subcommand)]
        action: ContractAction,
    },

    /// Usage, cost tracking, and budget management
    Usage {
        #[command(subcommand)]
        action: UsageAction,
    },

    /// List supported LLM providers and aliases
    Providers,

    /// Show detailed version and build info
    Version,
}

#[derive(Subcommand)]
enum RoutineAction {
    /// List all configured routines
    List,
    /// Add a new routine
    Add {
        /// Routine name
        name: String,
        /// Cron schedule (e.g. "*/30 * * * *")
        schedule: String,
        /// Prompt/instruction to run
        prompt: String,
    },
    /// Remove a routine by name
    Remove { name: String },
    /// Pause a routine
    Pause { name: String },
    /// Resume a paused routine
    Resume { name: String },
}

#[derive(Subcommand)]
enum MemoryAction {
    /// Show memory statistics
    Stats,
    /// Search memories
    Search {
        /// Search query
        query: String,
        /// Max results
        #[arg(short = 'n', long, default_value = "5")]
        limit: usize,
    },
    /// Export all memories to JSON
    Export {
        /// Output file path
        #[arg(short, long, default_value = "memories.json")]
        output: String,
    },
    /// Clear all memories (requires --confirm)
    Clear {
        #[arg(long)]
        confirm: bool,
    },
}

#[derive(Subcommand)]
enum ConfigAction {
    /// Validate the current configuration
    Validate,
    /// Show the resolved configuration
    Show,
    /// Show the config file path
    Path,
}

#[derive(Subcommand)]
enum ContractAction {
    /// List all configured contracts
    List,
    /// Validate contract definitions
    Validate,
    /// Test a contract against a simulated tool call
    Test {
        /// Tool name to simulate (e.g. "shell")
        tool: String,
        /// Arguments JSON or simple string (e.g. '{"command": "rm -rf /"}')
        args: String,
    },
}

#[derive(Subcommand)]
enum UsageAction {
    /// Show current usage snapshot (costs, tokens, budgets)
    Show,
    /// List available model pricing
    Pricing,
    /// Show configured budgets
    Budgets,
    /// Estimate cost for a model and token count
    Estimate {
        /// Model name (e.g. "anthropic/claude-sonnet-4")
        model: String,
        /// Input tokens
        #[arg(short, long, default_value = "1000")]
        input_tokens: u32,
        /// Output tokens
        #[arg(short, long, default_value = "500")]
        output_tokens: u32,
    },
}

#[derive(Clone, clap::ValueEnum)]
enum MigrateSource {
    /// Migrate from OpenClaw
    Openclaw,
}

#[tokio::main(flavor = "multi_thread", worker_threads = 2)]
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
        Commands::Gateway { port, host } => commands::gateway::run(port, host).await?,
        Commands::Daemon => commands::daemon::run().await?,
        Commands::Status => commands::status::run().await?,
        Commands::Doctor => commands::doctor::run().await?,

        Commands::Completions { shell } => {
            let mut cmd = Cli::command();
            generate(shell, &mut cmd, "rustedclaw", &mut std::io::stdout());
        }

        Commands::Estop { resume } => commands::estop::run(resume).await?,

        Commands::Migrate {
            source,
            dry_run,
            path,
        } => match source {
            MigrateSource::Openclaw => commands::migrate::run_openclaw(dry_run, path).await?,
        },

        Commands::Routine { action } => match action {
            RoutineAction::List => commands::routine::list().await?,
            RoutineAction::Add {
                name,
                schedule,
                prompt,
            } => commands::routine::add(&name, &schedule, &prompt).await?,
            RoutineAction::Remove { name } => commands::routine::remove(&name).await?,
            RoutineAction::Pause { name } => commands::routine::pause(&name).await?,
            RoutineAction::Resume { name } => commands::routine::resume(&name).await?,
        },

        Commands::Memory { action } => match action {
            MemoryAction::Stats => commands::memory::stats().await?,
            MemoryAction::Search { query, limit } => {
                commands::memory::search(&query, limit).await?
            }
            MemoryAction::Export { output } => commands::memory::export(&output).await?,
            MemoryAction::Clear { confirm } => commands::memory::clear(confirm).await?,
        },

        Commands::Config { action } => match action {
            ConfigAction::Validate => commands::config_cmd::validate().await?,
            ConfigAction::Show => commands::config_cmd::show().await?,
            ConfigAction::Path => commands::config_cmd::path().await?,
        },

        Commands::Contract { action } => match action {
            ContractAction::List => commands::contract::list().await?,
            ContractAction::Validate => commands::contract::validate().await?,
            ContractAction::Test { tool, args } => commands::contract::test(&tool, &args).await?,
        },

        Commands::Usage { action } => match action {
            UsageAction::Show => commands::usage::usage().await?,
            UsageAction::Pricing => commands::usage::pricing().await?,
            UsageAction::Budgets => commands::usage::budgets().await?,
            UsageAction::Estimate {
                model,
                input_tokens,
                output_tokens,
            } => commands::usage::estimate(&model, input_tokens, output_tokens).await?,
        },

        Commands::Providers => commands::providers::run().await?,

        Commands::Version => {
            println!("🦞 RustedClaw v{}", env!("CARGO_PKG_VERSION"));
            println!("   Arch:    {}", std::env::consts::ARCH);
            println!("   OS:      {}", std::env::consts::OS);
            println!("   Rust:    compiled with edition 2024");
            println!("   License: MIT OR Apache-2.0");
            println!("   Repo:    https://github.com/Nitin-100/rustedclaw");
            println!("\n   No account required. No lock-in. Bring your own API key.");
        }
    }

    Ok(())
}
