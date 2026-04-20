use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};
use talos_error::{TalosError, TalosResult};

use crate::{AuthScope, UserRecord, UserRole};

#[derive(Debug, Clone)]
pub struct JwtConfig {
    pub secret: String,
    pub expiry_days: u32,
    pub issuer: String,
}

impl Default for JwtConfig {
    fn default() -> Self {
        Self {
            secret: "change-me-in-production".to_string(),
            expiry_days: 30,
            issuer: "talos".to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccessTokenClaims {
    pub sub: String,
    #[serde(default)]
    pub username: Option<String>,
    pub role: UserRole,
    #[serde(default)]
    pub scopes: Vec<String>,
    pub token_version: u64,
    pub exp: usize,
    pub iat: usize,
    pub iss: String,
}

#[derive(Debug, Clone)]
pub struct IssuedAccessToken {
    pub token: String,
    pub expires_at: u64,
}

pub struct JwtAuth {
    config: JwtConfig,
    encoding_key: EncodingKey,
    decoding_key: DecodingKey,
}

impl JwtAuth {
    pub fn new(secret: &str) -> Self {
        Self::with_config(JwtConfig {
            secret: secret.to_string(),
            ..Default::default()
        })
    }

    pub fn with_config(config: JwtConfig) -> Self {
        Self {
            encoding_key: EncodingKey::from_secret(config.secret.as_bytes()),
            decoding_key: DecodingKey::from_secret(config.secret.as_bytes()),
            config,
        }
    }

    pub fn issue_access_token(
        &self,
        user: &UserRecord,
        scopes: &[AuthScope],
    ) -> TalosResult<IssuedAccessToken> {
        let issued_at = current_unix_seconds();
        let expires_at = issued_at + u64::from(self.config.expiry_days) * 24 * 60 * 60;
        let claims = AccessTokenClaims {
            sub: user.user_id.clone(),
            username: Some(user.username.clone()),
            role: user.role,
            scopes: scopes
                .iter()
                .map(AuthScope::as_str)
                .map(str::to_string)
                .collect(),
            token_version: user.token_version,
            exp: expires_at as usize,
            iat: issued_at as usize,
            iss: self.config.issuer.clone(),
        };
        let token = encode(&Header::default(), &claims, &self.encoding_key).map_err(|err| {
            TalosError::AuthError(format!("failed to issue access token: {}", err))
        })?;
        Ok(IssuedAccessToken { token, expires_at })
    }

    pub fn decode_access_token(&self, token: &str) -> TalosResult<AccessTokenClaims> {
        let validation = Validation::default();
        let token_data = decode::<AccessTokenClaims>(token, &self.decoding_key, &validation)
            .map_err(|err| TalosError::AuthError(format!("invalid token: {}", err)))?;
        Ok(token_data.claims)
    }
}

fn current_unix_seconds() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{default_scopes_for_role, UserRecord};

    fn test_user() -> UserRecord {
        UserRecord {
            user_id: "user-1".to_string(),
            username: "alice".to_string(),
            email: Some("alice@example.com".to_string()),
            password_hash: "hash".to_string(),
            role: UserRole::Admin,
            token_version: 2,
            disabled: false,
            created_at: 1,
            updated_at: 1,
        }
    }

    #[test]
    fn test_issue_and_decode_access_token() {
        let jwt = JwtAuth::new("test-secret");
        let user = test_user();
        let issued = jwt
            .issue_access_token(&user, &default_scopes_for_role(user.role))
            .unwrap();

        let claims = jwt.decode_access_token(&issued.token).unwrap();
        assert_eq!(claims.sub, user.user_id);
        assert_eq!(claims.username.as_deref(), Some("alice"));
        assert_eq!(claims.role, UserRole::Admin);
        assert_eq!(claims.token_version, 2);
        assert!(!claims.scopes.is_empty());
    }
}
