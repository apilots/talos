//! JWT Authentication Implementation

use super::{Auth, AuthResult, TokenType};
use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};
use talos_error::{TalosError, TalosResult};
use tracing::{debug, error, info};

/// JWT Configuration
#[derive(Debug, Clone)]
pub struct JwtConfig {
    pub secret: String,
    pub expiry_hours: u64,
    pub issuer: String,
}

impl Default for JwtConfig {
    fn default() -> Self {
        Self {
            secret: "change-me-in-production".to_string(),
            expiry_hours: 24,
            issuer: "talos".to_string(),
        }
    }
}

/// JWT Claims
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Claims {
    sub: String, // Subject (user_id)
    exp: usize,  // Expiration time
    iat: usize,  // Issued at
    iss: String, // Issuer
    #[serde(default)]
    token_type: Option<String>,
    #[serde(default)]
    scopes: Vec<String>,
    #[serde(default)]
    channels: Vec<String>,
    #[serde(default)]
    act_as: Option<String>,
}

/// JWT Authentication implementation
pub struct JwtAuth {
    config: JwtConfig,
    encoding_key: EncodingKey,
    decoding_key: DecodingKey,
}

impl JwtAuth {
    /// Create a new JWT auth instance
    pub fn new(secret: &str) -> Self {
        Self::with_config(JwtConfig {
            secret: secret.to_string(),
            ..Default::default()
        })
    }

    /// Create with custom configuration
    pub fn with_config(config: JwtConfig) -> Self {
        let encoding_key = EncodingKey::from_secret(config.secret.as_bytes());
        let decoding_key = DecodingKey::from_secret(config.secret.as_bytes());

        info!("JWT authentication initialized");

        Self {
            config,
            encoding_key,
            decoding_key,
        }
    }

    /// Generate expiration timestamp
    fn generate_exp(&self) -> usize {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        (now + self.config.expiry_hours * 3600) as usize
    }

    /// Generate issued-at timestamp
    fn generate_iat(&self) -> usize {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as usize
    }

    /// Issue a service token with optional act_as and scope constraints.
    pub async fn issue_service(
        &self,
        service_id: &str,
        scopes: Vec<String>,
        channels: Vec<String>,
        act_as: Option<String>,
    ) -> TalosResult<String> {
        let claims = Claims {
            sub: service_id.to_string(),
            exp: self.generate_exp(),
            iat: self.generate_iat(),
            iss: self.config.issuer.clone(),
            token_type: Some("service".to_string()),
            scopes,
            channels,
            act_as,
        };

        encode(&Header::default(), &claims, &self.encoding_key).map_err(|e| {
            error!("Failed to issue service token: {}", e);
            TalosError::AuthError(format!("Failed to issue service token: {}", e))
        })
    }
}

#[async_trait::async_trait]
impl Auth for JwtAuth {
    async fn verify(&self, token: &str) -> TalosResult<AuthResult> {
        debug!("Verifying JWT token");

        let validation = Validation::default();
        let token_data = decode::<Claims>(token, &self.decoding_key, &validation).map_err(|e| {
            error!("JWT verification failed: {}", e);
            TalosError::AuthError(format!("Invalid token: {}", e))
        })?;

        let claims = token_data.claims;
        let is_valid = claims.exp > self.generate_iat();

        debug!("Token verified for user: {}", claims.sub);

        Ok(AuthResult {
            user_id: claims.sub,
            expires_at: claims.exp as u64,
            is_valid,
            token_type: match claims.token_type.as_deref() {
                Some("service") => TokenType::Service,
                _ => TokenType::User,
            },
            scopes: claims.scopes,
            allowed_channels: claims.channels,
            act_as: claims.act_as,
            issuer: Some(claims.iss),
        })
    }

    async fn issue(&self, user_id: &str) -> TalosResult<String> {
        debug!("Issuing JWT token for user: {}", user_id);

        let claims = Claims {
            sub: user_id.to_string(),
            exp: self.generate_exp(),
            iat: self.generate_iat(),
            iss: self.config.issuer.clone(),
            token_type: Some("user".to_string()),
            scopes: vec![],
            channels: vec![],
            act_as: None,
        };

        let token = encode(&Header::default(), &claims, &self.encoding_key).map_err(|e| {
            error!("Failed to issue token: {}", e);
            TalosError::AuthError(format!("Failed to issue token: {}", e))
        })?;

        info!("Token issued for user: {}", user_id);

        Ok(token)
    }

    async fn refresh(&self, old_token: &str) -> TalosResult<String> {
        // Verify old token first
        let auth_result = self.verify(old_token).await?;

        if !auth_result.is_valid {
            return Err(TalosError::AuthError("Token expired".to_string()));
        }

        // Issue new token
        self.issue(&auth_result.user_id).await
    }

    async fn revoke(&self, _token: &str) -> TalosResult<()> {
        // JWT tokens are stateless, so we can't truly revoke them
        // In production, you'd maintain a blacklist in Redis/database
        debug!("Token revocation requested (stateless - not actually revoked)");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_jwt_auth() {
        let auth = JwtAuth::new("test-secret");

        // Issue token
        let token = auth.issue("user-123").await.unwrap();
        assert!(!token.is_empty());

        // Verify token
        let result = auth.verify(&token).await.unwrap();
        assert_eq!(result.user_id, "user-123");
        assert!(result.is_valid);

        // Refresh token
        let new_token = auth.refresh(&token).await.unwrap();
        assert!(!new_token.is_empty());
        // Note: Tokens might be the same if issued within the same second
        // assert_ne!(token, new_token);
    }

    #[tokio::test]
    async fn test_invalid_token() {
        let auth = JwtAuth::new("test-secret");

        // Invalid token should fail
        let result = auth.verify("invalid-token").await;
        assert!(result.is_err());
    }
}
