//! Talos Authentication Layer
//!
//! Provides JWT-based authentication and device pairing.
//!
//! # Example
//!
//! ```rust,no_run
//! use talos_auth::{Auth, JwtAuth};
//!
//! #[tokio::main]
//! async fn main() {
//!     let auth = JwtAuth::new("secret-key");
//!     let token = auth.issue("user-123").await.unwrap();
//!     let result = auth.verify(&token).await.unwrap();
//!     assert_eq!(result.user_id, "user-123");
//! }
//! ```

mod jwt;
mod pairing;

pub use jwt::{JwtAuth, JwtConfig};
pub use pairing::{DevicePairing, PairingCode, PairingResult};

use async_trait::async_trait;
use talos_error::TalosResult;

/// Authentication result
#[derive(Debug, Clone)]
pub struct AuthResult {
    pub user_id: String,
    pub expires_at: u64,
    pub is_valid: bool,
    pub token_type: TokenType,
    pub scopes: Vec<String>,
    pub allowed_channels: Vec<String>,
    pub act_as: Option<String>,
    pub issuer: Option<String>,
}

/// Token role category.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenType {
    User,
    Service,
}

/// Auth trait - Authentication interface
#[async_trait]
pub trait Auth: Send + Sync {
    /// Verify a token and return auth result
    async fn verify(&self, token: &str) -> TalosResult<AuthResult>;

    /// Issue a new token for a user
    async fn issue(&self, user_id: &str) -> TalosResult<String>;

    /// Refresh an existing token
    async fn refresh(&self, old_token: &str) -> TalosResult<String>;

    /// Revoke a token
    async fn revoke(&self, token: &str) -> TalosResult<()>;
}

/// Device pairing code format
#[derive(Debug, Clone)]
pub struct DeviceCode {
    pub code: String,
    pub expires_at: u64,
    pub device_id: String,
}

impl DeviceCode {
    /// Generate a new device code (6 digits)
    pub fn generate(device_id: &str, expiry_secs: u64) -> Self {
        let code = format!("{:06}", rand::random::<u32>() % 1000000);
        let expires_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs()
            + expiry_secs;

        Self {
            code,
            expires_at,
            device_id: device_id.to_string(),
        }
    }

    /// Check if the code is expired
    pub fn is_expired(&self) -> bool {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        now > self.expires_at
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_device_code() {
        let code = DeviceCode::generate("device-123", 300);
        assert_eq!(code.code.len(), 6);
        assert_eq!(code.device_id, "device-123");
        assert!(!code.is_expired());
    }
}
