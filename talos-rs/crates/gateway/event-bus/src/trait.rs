use async_trait::async_trait;
use talos_error::TalosResult;

use crate::{Event, EventFilter};

/// A subscriber that receives events matching a filter
pub struct EventSubscriber {
    pub receiver: tokio::sync::broadcast::Receiver<Event>,
}

impl EventSubscriber {
    /// Receive the next event
    pub async fn recv(&mut self) -> TalosResult<Event> {
        match self.receiver.recv().await {
            Ok(event) => Ok(event),
            Err(_) => Err(talos_error::TalosError::InternalError(
                "Event bus channel closed".to_string(),
            )),
        }
    }
}

/// EventBus trait defines the interface for event publishing and subscription
#[async_trait]
pub trait EventBus: Send + Sync {
    /// Emit an event to all matching subscribers
    async fn emit(&self, event: Event) -> TalosResult<()>;

    /// Subscribe to events matching the given filter
    async fn subscribe(&self, filter: EventFilter) -> TalosResult<EventSubscriber>;
}
