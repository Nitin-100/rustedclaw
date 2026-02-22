//! CLI channel — interactive terminal-based chat.
//!
//! This is the simplest channel: reads from stdin, writes to stdout.
//! Used for `rustedclaw agent` interactive mode.

use async_trait::async_trait;
use rustedclaw_core::channel::{Channel, ChannelId, ChannelMessage};
use rustedclaw_core::error::ChannelError;
use tokio::sync::mpsc;
use tokio::io::{self, AsyncBufReadExt, BufReader};

/// Interactive CLI channel for terminal-based chat.
pub struct CliChannel {
    id: ChannelId,
}

impl CliChannel {
    pub fn new() -> Self {
        Self {
            id: ChannelId("cli".into()),
        }
    }
}

impl Default for CliChannel {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Channel for CliChannel {
    fn name(&self) -> &str {
        "cli"
    }

    fn id(&self) -> &ChannelId {
        &self.id
    }

    async fn start(
        &self,
    ) -> Result<mpsc::Receiver<Result<ChannelMessage, ChannelError>>, ChannelError> {
        let (tx, rx) = mpsc::channel(32);
        let channel_id = self.id.clone();

        tokio::spawn(async move {
            let stdin = io::stdin();
            let reader = BufReader::new(stdin);
            let mut lines = reader.lines();

            loop {
                match lines.next_line().await {
                    Ok(Some(line)) => {
                        let line = line.trim().to_string();
                        if line.is_empty() {
                            continue;
                        }

                        // Check for exit commands
                        if matches!(
                            line.as_str(),
                            "exit" | "quit" | "/exit" | "/quit" | ":q"
                        ) {
                            break;
                        }

                        let msg = ChannelMessage {
                            channel_id: channel_id.clone(),
                            sender_id: "local_user".into(),
                            sender_name: Some("User".into()),
                            content: line,
                            chat_id: "cli_session".into(),
                            reply_to_message_id: None,
                            attachments: vec![],
                            metadata: serde_json::Map::new(),
                        };

                        if tx.send(Ok(msg)).await.is_err() {
                            break;
                        }
                    }
                    Ok(None) => break, // EOF (Ctrl+D)
                    Err(e) => {
                        let _ = tx.send(Err(ChannelError::ConnectionLost(e.to_string()))).await;
                        break;
                    }
                }
            }
        });

        Ok(rx)
    }

    async fn send(
        &self,
        _chat_id: &str,
        content: &str,
        _reply_to: Option<&str>,
    ) -> Result<(), ChannelError> {
        // When used directly (not through the agent command), just print the content
        println!("{content}");
        Ok(())
    }

    fn is_allowed(&self, _sender_id: &str) -> bool {
        true // CLI is always allowed (local user)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cli_channel_properties() {
        let ch = CliChannel::new();
        assert_eq!(ch.name(), "cli");
        assert_eq!(ch.id().0, "cli");
        assert!(ch.is_allowed("anyone"));
    }
}
