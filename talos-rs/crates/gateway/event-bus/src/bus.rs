use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use tokio::sync::{broadcast, RwLock};
use uuid::Uuid;

use crate::{Event, EventBus, EventFilter, EventSubscriber};
use talos_error::TalosResult;

/// Implementation of the EventBus trait using a broadcast channel pattern
pub struct EventBusImpl {
    /// Channel sender for broadcasting events
    sender: broadcast::Sender<Event>,

    /// Store for active subscriptions with their filters
    subscriptions: Arc<RwLock<HashMap<String, EventFilter>>>,
}

impl EventBusImpl {
    /// Create a new EventBusImpl with the specified capacity for the broadcast channel
    pub fn new(capacity: usize) -> Self {
        let (sender, _) = broadcast::channel(capacity);

        Self {
            sender,
            subscriptions: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Create a new EventBusImpl with default capacity
    pub fn default() -> Self {
        Self::new(100)
    }

    /// Check if an event matches any of the active filters
    async fn matches_any_filter(&self, event: &Event) -> bool {
        let subscriptions = self.subscriptions.read().await;

        for filter in subscriptions.values() {
            if filter.matches(event) {
                return true;
            }
        }

        false
    }
}

#[async_trait]
impl EventBus for EventBusImpl {
    async fn emit(&self, event: Event) -> TalosResult<()> {
        // Check if the event matches any active filter before sending
        // This avoids unnecessary work if no one is listening for this event
        if self.matches_any_filter(&event).await {
            // Send the event to all active subscribers
            let _ = self.sender.send(event);
        }

        Ok(())
    }

    async fn subscribe(&self, filter: EventFilter) -> TalosResult<EventSubscriber> {
        // Create a new receiver for this subscription
        let receiver = self.sender.subscribe();

        // Generate a unique ID for this subscription
        let subscription_id = Uuid::new_v4().to_string();

        // Store the filter associated with this subscription
        {
            let mut subscriptions = self.subscriptions.write().await;
            subscriptions.insert(subscription_id, filter);
        }

        Ok(EventSubscriber { receiver })
    }
}

impl Default for EventBusImpl {
    fn default() -> Self {
        Self::new(100)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::time::{timeout, Duration};

    #[tokio::test]
    async fn test_basic_emit_and_subscribe() {
        let event_bus = EventBusImpl::default();

        // Subscribe to all events
        let mut subscriber = event_bus.subscribe(EventFilter::new()).await.unwrap();

        // Emit an event
        let event = Event::SessionCreated {
            session_id: "test_session".to_string(),
            user_id: "test_user".to_string(),
        };
        event_bus.emit(event.clone()).await.unwrap();

        // Receive the event
        let received_event = timeout(Duration::from_millis(100), subscriber.recv())
            .await
            .unwrap()
            .unwrap();

        assert_eq!(received_event, event);
    }

    #[tokio::test]
    async fn test_filtered_subscription() {
        let event_bus = EventBusImpl::default();

        // Subscribe only to session created events
        let filter = EventFilter::new().with_event_types(vec!["session_created".to_string()]);
        let mut subscriber = event_bus.subscribe(filter).await.unwrap();

        // Emit a session created event
        let session_event = Event::SessionCreated {
            session_id: "test_session".to_string(),
            user_id: "test_user".to_string(),
        };
        event_bus.emit(session_event.clone()).await.unwrap();

        // Emit a message received event (should not be received)
        let message_event = Event::MessageReceived {
            session_id: "test_session".to_string(),
            content: "hello".to_string(),
        };
        event_bus.emit(message_event.clone()).await.unwrap();

        // Receive the session created event
        let received_event = timeout(Duration::from_millis(100), subscriber.recv())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(received_event, session_event);

        // Verify that the message event is not received immediately
        let result = timeout(Duration::from_millis(100), subscriber.recv()).await;
        assert!(result.is_err()); // Should timeout because no matching event was sent
    }

    #[tokio::test]
    async fn test_multiple_subscribers() {
        let event_bus = EventBusImpl::default();

        // Subscribe two subscribers to all events
        let mut subscriber1 = event_bus.subscribe(EventFilter::new()).await.unwrap();
        let mut subscriber2 = event_bus.subscribe(EventFilter::new()).await.unwrap();

        // Emit an event
        let event = Event::SessionCreated {
            session_id: "test_session".to_string(),
            user_id: "test_user".to_string(),
        };
        event_bus.emit(event.clone()).await.unwrap();

        // Both subscribers should receive the event
        let received_event1 = timeout(Duration::from_millis(100), subscriber1.recv())
            .await
            .unwrap()
            .unwrap();
        let received_event2 = timeout(Duration::from_millis(100), subscriber2.recv())
            .await
            .unwrap()
            .unwrap();

        assert_eq!(received_event1, event);
        assert_eq!(received_event2, event);
    }

    #[tokio::test]
    async fn test_session_id_filter() {
        let event_bus = EventBusImpl::default();

        // Subscribe to events for a specific session
        let filter = EventFilter::new().with_session_ids(vec!["session1".to_string()]);
        let mut subscriber = event_bus.subscribe(filter).await.unwrap();

        // Emit an event for session1 (should be received)
        let event1 = Event::SessionCreated {
            session_id: "session1".to_string(),
            user_id: "user1".to_string(),
        };
        event_bus.emit(event1.clone()).await.unwrap();

        // Emit an event for session2 (should not be received)
        let event2 = Event::SessionCreated {
            session_id: "session2".to_string(),
            user_id: "user2".to_string(),
        };
        event_bus.emit(event2).await.unwrap();

        // Receive the first event
        let received_event = timeout(Duration::from_millis(100), subscriber.recv())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(received_event, event1);

        // Verify that the second event is not received
        let result = timeout(Duration::from_millis(100), subscriber.recv()).await;
        assert!(result.is_err()); // Should timeout
    }

    #[tokio::test]
    async fn test_user_id_filter() {
        let event_bus = EventBusImpl::default();

        // Subscribe to events for a specific user
        let filter = EventFilter::new().with_user_ids(vec!["user1".to_string()]);
        let mut subscriber = event_bus.subscribe(filter).await.unwrap();

        // Emit an event for user1 (should be received)
        let event1 = Event::SessionCreated {
            session_id: "session1".to_string(),
            user_id: "user1".to_string(),
        };
        event_bus.emit(event1.clone()).await.unwrap();

        // Emit an event for user2 (should not be received)
        let event2 = Event::SessionCreated {
            session_id: "session2".to_string(),
            user_id: "user2".to_string(),
        };
        event_bus.emit(event2).await.unwrap();

        // Receive the first event
        let received_event = timeout(Duration::from_millis(100), subscriber.recv())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(received_event, event1);

        // Verify that the second event is not received
        let result = timeout(Duration::from_millis(100), subscriber.recv()).await;
        assert!(result.is_err()); // Should timeout
    }
}
