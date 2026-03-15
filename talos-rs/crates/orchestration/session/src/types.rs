use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

/// Represents a user session in the Talos system
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    /// Unique identifier for the session
    pub id: Uuid,
    
    /// User identifier associated with the session
    pub user_id: String,
    
    /// Session metadata
    pub metadata: HashMap<String, String>,
    
    /// Session creation timestamp
    pub created_at: DateTime<Utc>,
    
    /// Last activity timestamp
    pub last_activity_at: DateTime<Utc>,
    
    /// Session expiration timestamp
    pub expires_at: Option<DateTime<Utc>>,
    
    /// Whether the session is currently active
    pub is_active: bool,
    
    /// Additional session data
    pub data: serde_json::Value,
}

impl Session {
    /// Creates a new session with the given user ID
    pub fn new(user_id: String) -> Self {
        let now = Utc::now();
        
        Self {
            id: Uuid::new_v4(),
            user_id,
            metadata: HashMap::new(),
            created_at: now,
            last_activity_at: now,
            expires_at: None,
            is_active: true,
            data: serde_json::Value::Object(serde_json::Map::new()),
        }
    }

    /// Creates a new session with a specific ID (useful for testing or restoration)
    pub fn with_id(id: Uuid, user_id: String) -> Self {
        let now = Utc::now();
        
        Self {
            id,
            user_id,
            metadata: HashMap::new(),
            created_at: now,
            last_activity_at: now,
            expires_at: None,
            is_active: true,
            data: serde_json::Value::Object(serde_json::Map::new()),
        }
    }

    /// Updates the last activity timestamp to now
    pub fn touch(&mut self) {
        self.last_activity_at = Utc::now();
    }

    /// Sets the session expiration time
    pub fn set_expires_in(&mut self, duration_seconds: i64) {
        self.expires_at = Some(Utc::now() + chrono::Duration::seconds(duration_seconds));
    }

    /// Checks if the session has expired
    pub fn is_expired(&self) -> bool {
        if let Some(expires_at) = self.expires_at {
            Utc::now() > expires_at
        } else {
            false
        }
    }

    /// Deactivates the session
    pub fn deactivate(&mut self) {
        self.is_active = false;
    }

    /// Adds metadata to the session
    pub fn add_metadata(&mut self, key: String, value: String) {
        self.metadata.insert(key, value);
    }

    /// Gets metadata value by key
    pub fn get_metadata(&self, key: &str) -> Option<&String> {
        self.metadata.get(key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    #[test]
    fn test_session_creation() {
        let user_id = "test_user".to_string();
        let session = Session::new(user_id.clone());
        
        assert_eq!(session.user_id, user_id);
        assert!(session.id != Uuid::nil());
        assert!(session.is_active);
        assert!(session.created_at <= Utc::now());
        assert!(session.last_activity_at <= Utc::now());
        assert_eq!(session.expires_at, None);
        assert_eq!(session.metadata.len(), 0);
    }

    #[test]
    fn test_session_with_id() {
        let id = Uuid::new_v4();
        let user_id = "test_user".to_string();
        let session = Session::with_id(id, user_id.clone());
        
        assert_eq!(session.id, id);
        assert_eq!(session.user_id, user_id);
    }

    #[test]
    fn test_session_touch() {
        let mut session = Session::new("test_user".to_string());
        let original_time = session.last_activity_at;
        
        // Sleep briefly to ensure time difference
        std::thread::sleep(std::time::Duration::from_millis(1));
        session.touch();
        
        assert!(session.last_activity_at > original_time);
    }

    #[test]
    fn test_session_expiry() {
        let mut session = Session::new("test_user".to_string());
        assert!(!session.is_expired());
        
        session.set_expires_in(-1); // Expire 1 second ago
        assert!(session.is_expired());
        
        session.set_expires_in(3600); // Expire in 1 hour
        assert!(!session.is_expired());
    }

    #[test]
    fn test_session_deactivation() {
        let mut session = Session::new("test_user".to_string());
        assert!(session.is_active);
        
        session.deactivate();
        assert!(!session.is_active);
    }

    #[test]
    fn test_session_metadata() {
        let mut session = Session::new("test_user".to_string());
        
        session.add_metadata("key1".to_string(), "value1".to_string());
        assert_eq!(session.get_metadata("key1"), Some(&"value1".to_string()));
        assert_eq!(session.get_metadata("nonexistent"), None);
    }
}