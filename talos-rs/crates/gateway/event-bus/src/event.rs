use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

/// Event types that can be emitted and subscribed to
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum Event {
    /// A new session has been created
    SessionCreated { session_id: String, user_id: String },

    /// A message has been received in a session
    MessageReceived { session_id: String, content: String },

    /// A tool call has been made
    ToolCall { tool_name: String, input: Value },

    /// A tool call result has been received
    ToolCallResult { tool_name: String, result: Value },

    /// A session has been closed
    SessionClosed { session_id: String, user_id: String },

    /// A user has connected to the system
    UserConnected { user_id: String, timestamp: u64 },

    /// A user has disconnected from the system
    UserDisconnected { user_id: String, timestamp: u64 },

    /// Custom event with arbitrary data
    Custom {
        event_type: String,
        data: HashMap<String, Value>,
    },
}

impl Event {
    /// Get the type of the event as a string
    pub fn event_type(&self) -> &'static str {
        match self {
            Event::SessionCreated { .. } => "session_created",
            Event::MessageReceived { .. } => "message_received",
            Event::ToolCall { .. } => "tool_call",
            Event::ToolCallResult { .. } => "tool_call_result",
            Event::SessionClosed { .. } => "session_closed",
            Event::UserConnected { .. } => "user_connected",
            Event::UserDisconnected { .. } => "user_disconnected",
            Event::Custom { .. } => "custom",
        }
    }
}

/// Filter for subscribing to specific events
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventFilter {
    /// Optional event types to filter by
    pub event_types: Option<Vec<String>>,

    /// Optional session IDs to filter by
    pub session_ids: Option<Vec<String>>,

    /// Optional user IDs to filter by
    pub user_ids: Option<Vec<String>>,

    /// Additional custom filters
    pub custom_filters: HashMap<String, Value>,
}

impl EventFilter {
    /// Create a new filter that matches all events
    pub fn new() -> Self {
        Self {
            event_types: None,
            session_ids: None,
            user_ids: None,
            custom_filters: HashMap::new(),
        }
    }

    /// Create a filter for specific event types
    pub fn with_event_types<I>(mut self, types: I) -> Self
    where
        I: IntoIterator<Item = String>,
    {
        self.event_types = Some(types.into_iter().collect());
        self
    }

    /// Create a filter for specific session IDs
    pub fn with_session_ids<I>(mut self, session_ids: I) -> Self
    where
        I: IntoIterator<Item = String>,
    {
        self.session_ids = Some(session_ids.into_iter().collect());
        self
    }

    /// Create a filter for specific user IDs
    pub fn with_user_ids<I>(mut self, user_ids: I) -> Self
    where
        I: IntoIterator<Item = String>,
    {
        self.user_ids = Some(user_ids.into_iter().collect());
        self
    }

    /// Check if an event matches this filter
    pub fn matches(&self, event: &Event) -> bool {
        // Check event types
        if let Some(ref event_types) = self.event_types {
            if !event_types.contains(&event.event_type().to_string()) {
                return false;
            }
        }

        // Check session IDs based on event type
        if let Some(ref session_ids) = self.session_ids {
            match event {
                Event::SessionCreated { session_id, .. }
                | Event::MessageReceived { session_id, .. }
                | Event::SessionClosed { session_id, .. } => {
                    if !session_ids.contains(session_id) {
                        return false;
                    }
                }
                _ => {} // Other event types don't have session IDs
            }
        }

        // Check user IDs based on event type
        if let Some(ref user_ids) = self.user_ids {
            match event {
                Event::SessionCreated { user_id, .. }
                | Event::SessionClosed { user_id, .. }
                | Event::UserConnected { user_id, .. }
                | Event::UserDisconnected { user_id, .. } => {
                    if !user_ids.contains(user_id) {
                        return false;
                    }
                }
                _ => {} // Other event types don't have user IDs
            }
        }

        // For now, we don't implement custom filter logic
        // This could be extended based on custom_filters

        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_event_type() {
        let event = Event::SessionCreated {
            session_id: "test_session".to_string(),
            user_id: "test_user".to_string(),
        };
        assert_eq!(event.event_type(), "session_created");
    }

    #[test]
    fn test_filter_matches_all() {
        let filter = EventFilter::new();
        let event = Event::SessionCreated {
            session_id: "test_session".to_string(),
            user_id: "test_user".to_string(),
        };
        assert!(filter.matches(&event));
    }

    #[test]
    fn test_filter_by_event_type() {
        let filter = EventFilter::new().with_event_types(vec!["session_created".to_string()]);

        let event = Event::SessionCreated {
            session_id: "test_session".to_string(),
            user_id: "test_user".to_string(),
        };
        assert!(filter.matches(&event));

        let event2 = Event::MessageReceived {
            session_id: "test_session".to_string(),
            content: "hello".to_string(),
        };
        assert!(!filter.matches(&event2));
    }

    #[test]
    fn test_filter_by_session_id() {
        let filter = EventFilter::new().with_session_ids(vec!["session1".to_string()]);

        let event = Event::SessionCreated {
            session_id: "session1".to_string(),
            user_id: "test_user".to_string(),
        };
        assert!(filter.matches(&event));

        let event2 = Event::SessionCreated {
            session_id: "session2".to_string(),
            user_id: "test_user".to_string(),
        };
        assert!(!filter.matches(&event2));
    }

    #[test]
    fn test_filter_by_user_id() {
        let filter = EventFilter::new().with_user_ids(vec!["user1".to_string()]);

        let event = Event::SessionCreated {
            session_id: "session1".to_string(),
            user_id: "user1".to_string(),
        };
        assert!(filter.matches(&event));

        let event2 = Event::SessionCreated {
            session_id: "session1".to_string(),
            user_id: "user2".to_string(),
        };
        assert!(!filter.matches(&event2));
    }
}
