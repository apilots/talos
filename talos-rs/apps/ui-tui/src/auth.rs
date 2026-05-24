use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use talos_config::TalosPaths;
use talos_error::{TalosError, TalosResult};
use talos_sdk::{AuthResponse, UserRole};

const AUTH_FILE_NAME: &str = "auth.json";
const FILE_PERMISSIONS: u32 = 0o600;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthSession {
    pub access_token: String,
    pub token_type: String,
    pub expires_at: u64,
    pub user_id: String,
    pub username: String,
    pub role: String,
    pub is_admin: bool,
    pub scopes: Vec<String>,
}

impl AuthSession {
    pub fn from_auth_response(auth_response: &AuthResponse) -> Self {
        Self {
            access_token: auth_response.access_token.clone(),
            token_type: auth_response.token_type.clone(),
            expires_at: auth_response.expires_at,
            user_id: auth_response.user.user_id.clone(),
            username: auth_response.user.username.clone(),
            role: role_to_string(&auth_response.user.role),
            is_admin: auth_response.capabilities.is_admin,
            scopes: auth_response.capabilities.scopes.clone(),
        }
    }

    pub fn is_expired(&self) -> bool {
        if self.expires_at == 0 {
            return false;
        }

        Self::unix_timestamp_secs() >= self.expires_at
    }

    fn unix_timestamp_secs() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
    }
}

pub fn auth_file_path() -> PathBuf {
    TalosPaths::resolve().talos_home.join(AUTH_FILE_NAME)
}

pub fn read_auth_session() -> Option<AuthSession> {
    let path = auth_file_path();
    let content = fs::read_to_string(path).ok()?;
    let session: AuthSession = serde_json::from_str(&content).ok()?;
    if session.access_token.is_empty() || session.is_expired() {
        clear_auth_session().ok();
        return None;
    }
    Some(session)
}

pub fn write_auth_session(session: &AuthSession) -> TalosResult<()> {
    let path = auth_file_path();
    TalosPaths::resolve()
        .ensure_base_dirs()
        .map_err(|err| TalosError::ClientError(format!("prepare auth directory: {}", err)))?;
    fs::create_dir_all(path.parent().unwrap_or_else(|| std::path::Path::new("."))).map_err(
        |err| {
            TalosError::StorageError(format!("create auth file parent directory failed: {}", err))
        },
    )?;
    let payload = serde_json::to_string_pretty(session)
        .map_err(|err| TalosError::ClientError(format!("serialize auth session: {}", err)))?;
    fs::write(&path, payload).map_err(|err| {
        TalosError::StorageError(format!("write auth file {}: {}", path.display(), err))
    })?;
    set_permissions(&path).ok();
    Ok(())
}

pub fn clear_auth_session() -> TalosResult<()> {
    let path = auth_file_path();
    match fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(TalosError::StorageError(format!(
            "remove auth file {}: {}",
            path.display(),
            err
        ))),
    }
}

fn role_to_string(role: &UserRole) -> String {
    match role {
        UserRole::Admin => "admin".to_string(),
        UserRole::User => "user".to_string(),
    }
}

#[cfg(unix)]
fn set_permissions(path: &PathBuf) -> TalosResult<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, std::fs::Permissions::from_mode(FILE_PERMISSIONS)).map_err(
        |err| {
            TalosError::StorageError(format!(
                "set auth file permission {}: {}",
                path.display(),
                err
            ))
        },
    )?;
    Ok(())
}

#[cfg(not(unix))]
fn set_permissions(_path: &PathBuf) {}
