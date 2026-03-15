use crate::{
    types::Session,
    SessionStore,
};
use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;
use thiserror::Error;
use tokio::sync::RwLock;
use uuid::Uuid;

/// Errors that can occur in the InMemorySessionStore
#[derive(Error, Debug)]
pub enum InMemorySessionStoreError {
    #[error("Session not found")]
    SessionNotFound,
    #[error("Storage error: {0}")]
    StorageError(String),
}

/// In-memory implementation of SessionStore
pub struct InMemorySessionStore {
    /// Storage for sessions
    sessions: Arc<RwLock<HashMap<Uuid, Session>>>,
}

impl InMemorySessionStore {
    /// Creates a new instance of InMemorySessionStore
    pub fn new() -> Self {
        Self {
            sessions: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Creates a new instance with pre-populated sessions (for testing)
    pub fn with_sessions(sessions: HashMap<Uuid, Session>) -> Self {
        Self {
            sessions: Arc::new(RwLock::new(sessions)),
        }
    }

    /// Gets a reference to the internal storage (for testing purposes)
    pub fn sessions(&self) -> &Arc<RwLock<HashMap<Uuid, Session>>> {
        &self.sessions
    }
}

#[async_trait]
impl SessionStore for InMemorySessionStore {
    type Error = InMemorySessionStoreError;

    async fn create_session(&self, session: Session) -> Result<(), Self::Error> {
        let mut sessions = self.sessions.write().await;
        sessions.insert(session.id, session);
        Ok(())
    }

    async fn get_session(&self, session_id: Uuid) -> Result<Option<Session>, Self::Error> {
        let sessions = self.sessions.read().await;
        if let Some(session) = sessions.get(&session_id) {
            // Check if the session is expired
            if session.is_expired() {
                drop(sessions); // Release the read lock before acquiring write lock
                let mut sessions = self.sessions.write().await;
                sessions.remove(&session_id);
                return Ok(None);
            }
            Ok(Some(session.clone()))
        } else {
            Ok(None)
        }
    }

    async fn update_session(&self, session: Session) -> Result<(), Self::Error> {
        let mut sessions = self.sessions.write().await;
        sessions.insert(session.id, session);
        Ok(())
    }

    async fn delete_session(&self, session_id: Uuid) -> Result<(), Self::Error> {
        let mut sessions = self.sessions.write().await;
        sessions.remove(&session_id);
        Ok(())
    }

    async fn extend_session(&self, session_id: Uuid, duration_seconds: i64) -> Result<(), Self::Error> {
        let mut sessions = self.sessions.write().await;
        if let Some(session) = sessions.get_mut(&session_id) {
            session.set_expires_in(duration_seconds);
            Ok(())
        } else {
            Err(InMemorySessionStoreError::SessionNotFound)
        }
    }

    async fn cleanup_expired_sessions(&self) -> Result<(), Self::Error> {
        let mut sessions = self.sessions.write().await;
        sessions.retain(|_, session| !session.is_expired());
        Ok(())
    }

    async fn get_sessions_by_user(&self, user_id: &str) -> Result<Vec<Session>, Self::Error> {
        let sessions = self.sessions.read().await;
        let user_sessions: Vec<Session> = sessions
            .values()
            .filter(|session| session.user_id == user_id && !session.is_expired() && session.is_active)
            .cloned()
            .collect();
        Ok(user_sessions)
    }
}

impl Default for InMemorySessionStore {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::r#trait::test_utils::create_test_session;
    use tokio;

    #[tokio::test]
    async fn test_create_and_get_session() {
        let store = InMemorySessionStore::new();
        let session = create_test_session("user1");

        // Create session
        store.create_session(session.clone()).await.unwrap();

        // Retrieve session
        let retrieved = store.get_session(session.id).await.unwrap();
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().id, session.id);
    }

    #[tokio::test]
    async fn test_get_nonexistent_session() {
        let store = InMemorySessionStore::new();
        let fake_id = Uuid::new_v4();

        let retrieved = store.get_session(fake_id).await.unwrap();
        assert!(retrieved.is_none());
    }

    #[tokio::test]
    async fn test_update_session() {
        let store = InMemorySessionStore::new();
        let mut session = create_test_session("user1");

        // Create session
        store.create_session(session.clone()).await.unwrap();

        // Modify session
        session.add_metadata("updated".to_string(), "true".to_string());

        // Update session
        store.update_session(session.clone()).await.unwrap();

        // Retrieve and verify update
        let updated = store.get_session(session.id).await.unwrap().unwrap();
        assert_eq!(updated.get_metadata("updated"), Some(&"true".to_string()));
    }

    #[tokio::test]
    async fn test_delete_session() {
        let store = InMemorySessionStore::new();
        let session = create_test_session("user1");

        // Create session
        store.create_session(session.clone()).await.unwrap();

        // Verify it exists
        assert!(store.get_session(session.id).await.unwrap().is_some());

        // Delete session
        store.delete_session(session.id).await.unwrap();

        // Verify it's gone
        assert!(store.get_session(session.id).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn test_extend_session() {
        let store = InMemorySessionStore::new();
        let mut session = create_test_session("user1");
        session.set_expires_in(1); // Expires in 1 second

        // Create session
        store.create_session(session.clone()).await.unwrap();

        // Extend session by 3600 seconds (1 hour)
        store.extend_session(session.id, 3600).await.unwrap();

        // Retrieve and verify it's not expired
        let extended = store.get_session(session.id).await.unwrap();
        assert!(extended.is_some());
    }

    #[tokio::test]
    async fn test_extend_nonexistent_session() {
        let store = InMemorySessionStore::new();
        let fake_id = Uuid::new_v4();

        let result = store.extend_session(fake_id, 3600).await;
        match result {
            Err(InMemorySessionStoreError::SessionNotFound) => (), // Expected
            _ => panic!("Expected SessionNotFound error"),
        }
    }

    #[tokio::test]
    async fn test_cleanup_expired_sessions() {
        let store = InMemorySessionStore::new();
        
        // Create an expired session
        let mut expired_session = create_test_session("user1");
        expired_session.set_expires_in(-1); // Expired 1 second ago
        
        // Create a valid session
        let valid_session = create_test_session("user2");

        // Add both sessions to store
        store.create_session(expired_session.clone()).await.unwrap();
        store.create_session(valid_session.clone()).await.unwrap();

        // Verify both sessions exist initially
        assert!(store.get_session(expired_session.id).await.unwrap().is_none()); // Already cleaned due to expiry
        assert!(store.get_session(valid_session.id).await.unwrap().is_some());

        // Add expired session directly to bypass expiry check during insertion
        {
            let mut sessions = store.sessions.write().await;
            sessions.insert(expired_session.id, expired_session.clone());
        }

        // Verify both exist now
        {
            let sessions = store.sessions.read().await;
            assert_eq!(sessions.len(), 2);
        }

        // Clean up expired sessions
        store.cleanup_expired_sessions().await.unwrap();

        // Verify only the valid session remains
        {
            let sessions = store.sessions.read().await;
            assert_eq!(sessions.len(), 1);
            assert!(sessions.contains_key(&valid_session.id));
        }
    }

    #[tokio::test]
    async fn test_get_sessions_by_user() {
        let store = InMemorySessionStore::new();
        
        // Create sessions for different users
        let mut session1 = create_test_session("user1");
        session1.add_metadata("test".to_string(), "session1".to_string());
        
        let mut session2 = create_test_session("user1");
        session2.add_metadata("test".to_string(), "session2".to_string());
        
        let session3 = create_test_session("user2");

        // Add sessions to store
        store.create_session(session1.clone()).await.unwrap();
        store.create_session(session2.clone()).await.unwrap();
        store.create_session(session3.clone()).await.unwrap();

        // Get sessions for user1
        let user1_sessions = store.get_sessions_by_user("user1").await.unwrap();
        assert_eq!(user1_sessions.len(), 2);
        
        let user2_sessions = store.get_sessions_by_user("user2").await.unwrap();
        assert_eq!(user2_sessions.len(), 1);

        let user3_sessions = store.get_sessions_by_user("user3").await.unwrap();
        assert_eq!(user3_sessions.len(), 0);
    }

    #[tokio::test]
    async fn test_session_expiry_during_get() {
        let store = InMemorySessionStore::new();
        
        // Create an expired session
        let mut expired_session = create_test_session("user1");
        expired_session.set_expires_in(-1); // Expired 1 second ago

        // Add session directly to bypass expiry check
        {
            let mut sessions = store.sessions.write().await;
            sessions.insert(expired_session.id, expired_session.clone());
        }

        // Attempt to get the expired session - should return None and remove it
        let retrieved = store.get_session(expired_session.id).await.unwrap();
        assert!(retrieved.is_none());

        // Verify it's been removed from storage
        {
            let sessions = store.sessions.read().await;
            assert!(!sessions.contains_key(&expired_session.id));
        }
    }
}