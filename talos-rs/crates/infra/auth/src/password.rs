use argon2::password_hash::{
    rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString,
};
use argon2::Argon2;
use talos_error::{TalosError, TalosResult};

#[derive(Clone, Default)]
pub struct PasswordService;

impl PasswordService {
    pub fn hash_password(&self, password: &str) -> TalosResult<String> {
        validate_password(password)?;
        let salt = SaltString::generate(&mut OsRng);
        Argon2::default()
            .hash_password(password.as_bytes(), &salt)
            .map(|hash| hash.to_string())
            .map_err(|err| TalosError::AuthError(format!("failed to hash password: {}", err)))
    }

    pub fn verify_password(&self, password: &str, hash: &str) -> TalosResult<()> {
        let parsed = PasswordHash::new(hash)
            .map_err(|err| TalosError::AuthError(format!("invalid password hash: {}", err)))?;
        Argon2::default()
            .verify_password(password.as_bytes(), &parsed)
            .map_err(|_| TalosError::AuthError("invalid username or password".to_string()))
    }
}

pub fn validate_password(password: &str) -> TalosResult<()> {
    if password.trim().len() < 7 {
        return Err(TalosError::ValidationError(
            "password must contain at least 7 characters".to_string(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hash_and_verify_password() {
        let service = PasswordService;
        let hash = service.hash_password("password123").unwrap();
        service.verify_password("password123", &hash).unwrap();
        assert!(service.verify_password("wrongpass", &hash).is_err());
    }
}
