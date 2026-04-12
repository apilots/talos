use serde::{Deserialize, Serialize};
use std::fmt;
use uuid::Uuid;

/// Agent ID.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct AgentId(pub String);

impl fmt::Display for AgentId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl AgentId {
    pub fn new(id: &str) -> Self {
        Self(id.to_string())
    }

    pub fn new_random() -> Self {
        Self(Uuid::new_v4().to_string())
    }
}

/// Session ID.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct SessionId(pub String);

impl SessionId {
    pub fn new() -> Self {
        Self(Uuid::new_v4().to_string())
    }
}

impl Default for SessionId {
    fn default() -> Self {
        Self::new()
    }
}

/// User ID.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct UserId(pub String);

impl UserId {
    pub fn new(id: &str) -> Self {
        Self(id.to_string())
    }
}

/// Channel ID.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct ChannelId(pub String);

impl ChannelId {
    pub fn new(id: &str) -> Self {
        Self(id.to_string())
    }
}

/// Message ID.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct MessageId(pub String);

impl MessageId {
    pub fn new() -> Self {
        Self(Uuid::new_v4().to_string())
    }
}

impl Default for MessageId {
    fn default() -> Self {
        Self::new()
    }
}
