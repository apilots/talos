use bytes::Bytes;
use serde::{Deserialize, Serialize};
use talos_llm_provider::ChatMessage;
use tokio::sync::oneshot;

/// Normalized input for one Agent turn.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct UserInput {
    /// Text projection consumed by prompt, title, memory, and compaction paths.
    pub text: String,
    /// Ordered file references resolved only for the current model turn.
    #[serde(default)]
    pub file_ids: Vec<String>,
}

/// Ordered, authorized file content prepared for one Kernel model Turn.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedAttachment {
    pub file_id: String,
    pub filename: String,
    pub media_type: String,
    pub size_bytes: u64,
    pub bytes: Bytes,
}

/// A validated user input accepted by an active Agent turn.
///
/// The Harness resolves file authorization and model attachment support before
/// sending this object. The Kernel only owns safe-boundary ordering and context
/// application for already accepted inputs.
#[derive(Debug, Clone)]
pub struct ActiveTurnInput {
    pub input: UserInput,
    pub message: ChatMessage,
    pub attachment_tokens: usize,
}

/// Harness-to-Kernel delivery for one active-turn input.
///
/// The Kernel acknowledges the delivery only after taking it at a safe model
/// boundary. Callers must treat a dropped acknowledgement as a rejected input.
#[derive(Debug)]
pub struct ActiveTurnInputDelivery {
    input: ActiveTurnInput,
    accepted_tx: oneshot::Sender<()>,
}

impl ActiveTurnInputDelivery {
    /// Create a delivery whose acknowledgement is owned by the submitting caller.
    pub fn new(input: ActiveTurnInput, accepted_tx: oneshot::Sender<()>) -> Self {
        Self { input, accepted_tx }
    }

    /// Borrow the input before acknowledging its safe-boundary delivery.
    pub(crate) fn input(&self) -> &ActiveTurnInput {
        &self.input
    }

    /// Confirm that the Kernel took the input at a safe model boundary.
    pub fn acknowledge(self) -> ActiveTurnInput {
        let _ = self.accepted_tx.send(());
        self.input
    }
}

#[cfg(test)]
#[path = "input_tests.rs"]
mod tests;
