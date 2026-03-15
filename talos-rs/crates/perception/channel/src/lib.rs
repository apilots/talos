//! Talos Channel Abstraction
//!
//! This module defines the abstract interface for all communication channels
//! in the Talos system. Channels are responsible for receiving and sending
//! messages from various sources like Telegram, WhatsApp, CLI, etc.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use talos_types::{AgentId, SessionId};
use tokio::sync::mpsc;

/// Input message type representing messages from different channels
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InputMessage {
    /// Unique message ID
    pub id: String,
    /// Channel name (e.g., "telegram", "whatsapp", "cli")
    pub channel: String,
    /// Session ID for this conversation
    pub session_id: SessionId,
    /// Sender ID (user ID, phone number, etc.)
    pub sender_id: String,
    /// Display name of the sender
    pub sender_name: String,
    /// Content of the message
    pub content: String,
    /// Message timestamp
    pub timestamp: u64,
    /// Additional metadata specific to the channel
    pub metadata: HashMap<String, String>,
}

/// Output message type representing responses to be sent back
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutputMessage {
    /// Corresponding input message ID (if replying to a specific message)
    pub input_id: Option<String>,
    /// Channel name (e.g., "telegram", "whatsapp", "cli")
    pub channel: String,
    /// Session ID for this conversation
    pub session_id: SessionId,
    /// Recipient ID (user ID, phone number, etc.)
    pub recipient_id: String,
    /// Content of the response
    pub content: String,
    /// Message timestamp
    pub timestamp: u64,
    /// Additional metadata specific to the channel
    pub metadata: HashMap<String, String>,
}

/// Channel error types
#[derive(Debug, thiserror::Error)]
pub enum ChannelError {
    #[error("Network error: {0}")]
    Network(String),
    #[error("Authentication error: {0}")]
    Authentication(String),
    #[error("Serialization error: {0}")]
    Serialization(String),
    #[error("Configuration error: {0}")]
    Configuration(String),
    #[error("Internal error: {0}")]
    Internal(String),
}

/// Result type for channel operations
pub type ChannelResult<T> = Result<T, ChannelError>;

/// Abstract trait for all communication channels
#[async_trait]
pub trait Channel: Send + Sync {
    /// Get the unique name of this channel
    fn name(&self) -> &str;

    /// Start the channel and begin receiving messages
    /// Returns a receiver for incoming messages
    async fn start(&self) -> ChannelResult<mpsc::Receiver<InputMessage>>;

    /// Send a message through this channel
    async fn send(&self, message: OutputMessage) -> ChannelResult<()>;

    /// Stop the channel gracefully
    async fn stop(&self) -> ChannelResult<()>;

    /// Get channel-specific configuration
    fn config(&self) -> &dyn std::any::Any;
}

/// Channel manager to handle multiple channels
pub struct ChannelManager {
    channels: HashMap<String, Box<dyn Channel>>,
}

impl ChannelManager {
    /// Create a new channel manager
    pub fn new() -> Self {
        Self {
            channels: HashMap::new(),
        }
    }

    /// Register a new channel
    pub fn register_channel(&mut self, channel: Box<dyn Channel>) {
        let name = channel.name().to_string();
        self.channels.insert(name, channel);
    }

    /// Get a reference to a registered channel
    pub fn get_channel(&self, name: &str) -> Option<&dyn Channel> {
        self.channels.get(name).map(|c| c.as_ref())
    }

    /// Get mutable reference to a registered channel
    pub fn get_channel_mut(&mut self, name: &str) -> Option<&mut Box<dyn Channel>> {
        self.channels.get_mut(name)
    }

    /// Get all registered channel names
    pub fn channel_names(&self) -> Vec<String> {
        self.channels.keys().cloned().collect()
    }

    /// Start all registered channels
    pub async fn start_all(&self) -> HashMap<String, mpsc::Receiver<InputMessage>> {
        let mut receivers = HashMap::new();

        for (name, channel) in &self.channels {
            match channel.start().await {
                Ok(receiver) => {
                    receivers.insert(name.clone(), receiver);
                }
                Err(e) => {
                    tracing::error!("Failed to start channel {}: {}", name, e);
                }
            }
        }

        receivers
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct MockChannel {
        name: String,
    }

    impl MockChannel {
        fn new(name: String) -> Self {
            Self { name }
        }
    }

    #[async_trait]
    impl Channel for MockChannel {
        fn name(&self) -> &str {
            &self.name
        }

        async fn start(&self) -> ChannelResult<mpsc::Receiver<InputMessage>> {
            let (_tx, rx) = mpsc::channel(100);
            Ok(rx)
        }

        async fn send(&self, _message: OutputMessage) -> ChannelResult<()> {
            Ok(())
        }

        async fn stop(&self) -> ChannelResult<()> {
            Ok(())
        }

        fn config(&self) -> &dyn std::any::Any {
            self
        }
    }

    #[test]
    fn test_channel_manager() {
        let mut manager = ChannelManager::new();

        let mock_channel = Box::new(MockChannel::new("mock".to_string()));
        manager.register_channel(mock_channel);

        assert_eq!(manager.channel_names(), vec!["mock"]);
        assert!(manager.get_channel("mock").is_some());
    }
}
