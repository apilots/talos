use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use talos_types::talos::TalosJsonValue;

/// Response from coordinator after handling a message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoordinatorResponse {
    /// The result of message processing
    pub result: String,

    /// Optional data payload
    pub data: Option<HashMap<String, TalosJsonValue>>,

    /// Status indicating success or failure
    pub status: CoordinatorStatus,
}

/// Status of coordinator operation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CoordinatorStatus {
    Success,
    Error,
    Pending,
    Forwarded,
}

impl Default for CoordinatorResponse {
    fn default() -> Self {
        Self {
            result: String::new(),
            data: None,
            status: CoordinatorStatus::Success,
        }
    }
}
