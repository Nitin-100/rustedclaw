//! Chat channel implementations for RustedClaw.
//!
//! Each channel connects to a chat platform and relays messages to/from
//! the agent. Channels are trait-based and platform-agnostic.
//!
//! Available channels:
//! - **CLI** — Interactive terminal chat (stdin/stdout)
//! - **Telegram** — Telegram Bot API (stub, needs teloxide in production)
//! - **Discord** — Discord Bot API (stub, needs serenity in production)
//! - **Slack** — Slack Socket Mode (stub, needs WebSocket in production)
//! - **Web** — HTTP/WebSocket web gateway
//! - **Webhook** — Generic inbound HTTP webhooks
//! - **Registry** — Central channel manager and message router

pub mod cli;
pub mod discord;
pub mod registry;
pub mod slack;
pub mod telegram;
pub mod web;
pub mod webhook;

pub use cli::CliChannel;
pub use discord::{DiscordChannel, DiscordConfig};
pub use registry::ChannelRegistry;
pub use slack::{SlackChannel, SlackConfig};
pub use telegram::{TelegramChannel, TelegramConfig};
pub use web::{WebChannel, WebConfig};
pub use webhook::{WebhookChannel, WebhookConfig};
