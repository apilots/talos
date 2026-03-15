use crate::types::Session;
use async_trait::async_trait;
use std::error::Error;
use uuid::Uuid;

/// Defines the interface for session storage implementations
#[async_trait]
pub trait SessionStore: Send + Sync {
    /// Error type for the store implementation
    type Error: Error + Send + Sync + 'static;

    /// Creates a new session
    async fn create_session(&self, session: Session) -> Result<(), Self::Error>;

    /// Gets a session by ID
    async fn get_session(&self, session_id: Uuid) -> Result<Option<Session>, Self::Error>;

    /// Updates an existing session
    async fn update_session(&self, session: Session) -> Result<(), Self::Error>;

    /// Deletes a session by ID
    async fn delete_session(&self, session_id: Uuid) -> Result<(), Self::Error>;

    /// Extends the expiration time of a session
    async fn extend_session(&self, session_id: Uuid, duration_seconds: i64) -> Result<(), Self::Error>;

    /// Cleans up expired sessions
    async fn cleanup_expired_sessions(&self) -> Result<(), Self::Error>;

    /// Gets all sessions for a specific user
    async fn get_sessions_by_user(&self, user_id: &str) -> Result<Vec<Session>, Self::Error>;
}

#[cfg(test)]
pub mod test_utils {
    use crate::types::Session;
    use chrono::Utc;
    use uuid::Uuid;

    /// Helper function to create a test session
    pub fn create_test_session(user_id: &str) -> Session {
        Session {
            id: Uuid::new_v4(),
            user_id: user_id.to_string(),
            metadata: std::collections::HashMap::new(),
            created_at: Utc::now(),
            last_activity_at: Utc::now(),
            expires_at: None,
            is_active: true,
            data: serde_json::Value::Object(serde_json::Map::new()),
        }
    }
}