use std::sync::Arc;

use serde::{Deserialize, Serialize};
use talos_config::Config;
use talos_types::SessionId;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

#[derive(Debug, Clone)]
pub struct ChannelDefinition {
    pub id: &'static str,
    pub name: &'static str,
    pub capabilities: ChannelCapabilities,
}

#[derive(Debug, Clone, Default)]
pub struct ChannelCapabilities {
    pub supports_threads: bool,
    pub supports_edit: bool,
    pub supports_typing: bool,
    pub supports_media: Vec<MediaType>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaType {
    Photo,
    Document,
    Voice,
    Video,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ChannelAccountSnapshot {
    pub account_id: String,
    pub name: Option<String>,
    pub enabled: bool,
    pub configured: bool,
    pub running: bool,
    pub connected: bool,
    pub last_message_at: Option<i64>,
    pub last_error: Option<String>,
    pub last_start_at: Option<i64>,
    pub last_stop_at: Option<i64>,
    pub mode: Option<String>,
}

pub struct ChannelRuntimeContext {
    pub config: Arc<Config>,
    pub account_id: String,
    pub abort_signal: CancellationToken,
    pub event_tx: mpsc::Sender<ChannelEvent>,
    pub get_status: Arc<dyn Fn() -> ChannelAccountSnapshot + Send + Sync>,
    pub set_status: Arc<dyn Fn(ChannelAccountSnapshot) + Send + Sync>,
}

impl Clone for ChannelRuntimeContext {
    fn clone(&self) -> Self {
        Self {
            config: self.config.clone(),
            account_id: self.account_id.clone(),
            abort_signal: self.abort_signal.clone(),
            event_tx: self.event_tx.clone(),
            get_status: self.get_status.clone(),
            set_status: self.set_status.clone(),
        }
    }
}

#[derive(Debug, Clone)]
pub enum ChannelEvent {
    Message {
        channel_id: String,
        account_id: String,
        user_id: String,
        chat_id: String,
        thread_id: Option<String>,
        session_id: SessionId,
        content: MessageContent,
        reply_to_id: Option<String>,
    },
    Error {
        channel_id: String,
        account_id: String,
        error: String,
    },
}

#[derive(Debug, Clone)]
pub struct MessageTarget {
    pub account_id: String,
    pub chat_id: String,
    pub thread_id: Option<String>,
    pub reply_to: Option<String>,
}

#[derive(Debug, Clone)]
pub enum MessageContent {
    Text(String),
    Media { kind: MediaType, file_id: String },
}

#[derive(Debug, Clone)]
pub struct MediaContent {
    pub kind: MediaType,
    pub file_id: String,
    pub caption: Option<String>,
}

#[derive(Debug, Clone)]
pub struct OutboundContext {
    pub target: MessageTarget,
    pub text: String,
}

#[derive(Debug, Clone)]
pub struct DeliveryResult {
    pub message_id: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum DmPolicy {
    #[default]
    Open,
    Pairing,
    Allowlist,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum GroupPolicy {
    #[default]
    Open,
    Mention,
    Allowlist,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccessDecision {
    Allowed,
    Denied,
    NeedPairing,
}
