use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use async_trait::async_trait;
use serde_json::Value;
use talos_error::{TalosError, TalosResult};
use talos_storage::{Row, Storage};

use crate::{CreateUserRecord, UserPatch, UserRecord, UserRole};

#[async_trait]
pub trait UserStore: Send + Sync {
    async fn create_user(&self, input: CreateUserRecord) -> TalosResult<UserRecord>;
    async fn get_user_by_id(&self, user_id: &str) -> TalosResult<Option<UserRecord>>;
    async fn get_user_by_identifier(&self, identifier: &str) -> TalosResult<Option<UserRecord>>;
    async fn update_user(&self, user_id: &str, patch: UserPatch) -> TalosResult<UserRecord>;
    async fn update_password_hash(&self, user_id: &str, password_hash: &str) -> TalosResult<()>;
    async fn increment_token_version(&self, user_id: &str) -> TalosResult<u64>;
    async fn list_users(&self) -> TalosResult<Vec<UserRecord>>;
    async fn delete_user(&self, user_id: &str) -> TalosResult<()>;
    async fn count_users(&self) -> TalosResult<u64>;
}

pub struct DatabaseUserStore {
    storage: Arc<dyn Storage>,
}

impl DatabaseUserStore {
    pub async fn new(storage: Arc<dyn Storage>) -> TalosResult<Self> {
        storage.init_schema().await?;
        Ok(Self { storage })
    }

    fn parse_user(row: &Row) -> UserRecord {
        UserRecord {
            user_id: row.get_string("user_id").unwrap_or_default(),
            username: row.get_string("username").unwrap_or_default(),
            email: row.get_string("email"),
            password_hash: row.get_string("password_hash").unwrap_or_default(),
            role: match row.get_string("role").as_deref() {
                Some("admin") => UserRole::Admin,
                _ => UserRole::User,
            },
            token_version: row.get_i64("token_version").unwrap_or(1).max(1) as u64,
            disabled: row.get_i64("disabled").unwrap_or_default() != 0,
            created_at: row.get_i64("created_at").unwrap_or_default(),
            updated_at: row.get_i64("updated_at").unwrap_or_default(),
        }
    }

    async fn fetch_user_row(
        &self,
        where_clause: &str,
        params: &[&str],
    ) -> TalosResult<Option<Row>> {
        let sql = format!(
            "SELECT user_id, username, email, password_hash, role, token_version, disabled, created_at, updated_at FROM users WHERE {} LIMIT 1",
            where_clause
        );
        let rows = self.storage.query(&sql, params).await?;
        Ok(rows.into_iter().next())
    }
}

#[async_trait]
impl UserStore for DatabaseUserStore {
    async fn create_user(&self, input: CreateUserRecord) -> TalosResult<UserRecord> {
        let row = Row::new(
            vec![
                "user_id".to_string(),
                "username".to_string(),
                "email".to_string(),
                "password_hash".to_string(),
                "role".to_string(),
                "token_version".to_string(),
                "disabled".to_string(),
                "created_at".to_string(),
                "updated_at".to_string(),
            ],
            vec![
                Value::String(input.user_id.clone()),
                Value::String(input.username.clone()),
                input.email.clone().map(Value::String).unwrap_or(Value::Null),
                Value::String(input.password_hash.clone()),
                Value::String(input.role.as_storage_value().to_string()),
                Value::Number((input.token_version as i64).into()),
                Value::Number((if input.disabled { 1 } else { 0 }).into()),
                Value::Number(input.created_at.into()),
                Value::Number(input.updated_at.into()),
            ],
        );
        self.storage.insert("users", &row).await?;
        Ok(UserRecord::from(input))
    }

    async fn get_user_by_id(&self, user_id: &str) -> TalosResult<Option<UserRecord>> {
        Ok(self
            .fetch_user_row("user_id = ?", &[user_id])
            .await?
            .as_ref()
            .map(Self::parse_user))
    }

    async fn get_user_by_identifier(&self, identifier: &str) -> TalosResult<Option<UserRecord>> {
        Ok(self
            .fetch_user_row("username = ? OR email = ?", &[identifier, identifier])
            .await?
            .as_ref()
            .map(Self::parse_user))
    }

    async fn update_user(&self, user_id: &str, patch: UserPatch) -> TalosResult<UserRecord> {
        let mut user = self
            .get_user_by_id(user_id)
            .await?
            .ok_or_else(|| TalosError::ResourceNotFound(format!("user {}", user_id)))?;
        if let Some(username) = patch.username {
            user.username = username;
        }
        if let Some(email) = patch.email {
            user.email = Some(email);
        }
        if let Some(role) = patch.role {
            user.role = role;
        }
        if let Some(disabled) = patch.disabled {
            user.disabled = disabled;
        }
        user.updated_at = current_timestamp_millis();

        let row = Row::new(
            vec![
                "username".to_string(),
                "email".to_string(),
                "role".to_string(),
                "disabled".to_string(),
                "updated_at".to_string(),
            ],
            vec![
                Value::String(user.username.clone()),
                user.email.clone().map(Value::String).unwrap_or(Value::Null),
                Value::String(user.role.as_storage_value().to_string()),
                Value::Number((if user.disabled { 1 } else { 0 }).into()),
                Value::Number(user.updated_at.into()),
            ],
        );
        self.storage
            .update("users", &row, "user_id = ?", &[user_id])
            .await?;
        Ok(user)
    }

    async fn update_password_hash(&self, user_id: &str, password_hash: &str) -> TalosResult<()> {
        let row = Row::new(
            vec!["password_hash".to_string(), "updated_at".to_string()],
            vec![
                Value::String(password_hash.to_string()),
                Value::Number(current_timestamp_millis().into()),
            ],
        );
        let affected = self
            .storage
            .update("users", &row, "user_id = ?", &[user_id])
            .await?;
        if affected == 0 {
            return Err(TalosError::ResourceNotFound(format!("user {}", user_id)));
        }
        Ok(())
    }

    async fn increment_token_version(&self, user_id: &str) -> TalosResult<u64> {
        let user = self
            .get_user_by_id(user_id)
            .await?
            .ok_or_else(|| TalosError::ResourceNotFound(format!("user {}", user_id)))?;
        let next = user.token_version + 1;
        let row = Row::new(
            vec!["token_version".to_string(), "updated_at".to_string()],
            vec![
                Value::Number((next as i64).into()),
                Value::Number(current_timestamp_millis().into()),
            ],
        );
        self.storage
            .update("users", &row, "user_id = ?", &[user_id])
            .await?;
        Ok(next)
    }

    async fn list_users(&self) -> TalosResult<Vec<UserRecord>> {
        let sql = "SELECT user_id, username, email, password_hash, role, token_version, disabled, created_at, updated_at FROM users ORDER BY created_at ASC";
        Ok(self
            .storage
            .query(sql, &[])
            .await?
            .iter()
            .map(Self::parse_user)
            .collect())
    }

    async fn delete_user(&self, user_id: &str) -> TalosResult<()> {
        let affected = self
            .storage
            .delete("users", "user_id = ?", &[user_id])
            .await?;
        if affected == 0 {
            return Err(TalosError::ResourceNotFound(format!("user {}", user_id)));
        }
        Ok(())
    }

    async fn count_users(&self) -> TalosResult<u64> {
        let rows = self
            .storage
            .query("SELECT COUNT(*) AS count FROM users", &[])
            .await?;
        Ok(rows
            .first()
            .and_then(|row| row.get_i64("count"))
            .unwrap_or_default() as u64)
    }
}

#[derive(Default)]
pub struct InMemoryUserStore {
    users: RwLock<HashMap<String, UserRecord>>,
}

#[async_trait]
impl UserStore for InMemoryUserStore {
    async fn create_user(&self, input: CreateUserRecord) -> TalosResult<UserRecord> {
        let user = UserRecord::from(input);
        self.users
            .write()
            .expect("in-memory users write lock")
            .insert(user.user_id.clone(), user.clone());
        Ok(user)
    }

    async fn get_user_by_id(&self, user_id: &str) -> TalosResult<Option<UserRecord>> {
        Ok(self
            .users
            .read()
            .expect("in-memory users read lock")
            .get(user_id)
            .cloned())
    }

    async fn get_user_by_identifier(&self, identifier: &str) -> TalosResult<Option<UserRecord>> {
        Ok(self
            .users
            .read()
            .expect("in-memory users read lock")
            .values()
            .find(|user| user.username == identifier || user.email.as_deref() == Some(identifier))
            .cloned())
    }

    async fn update_user(&self, user_id: &str, patch: UserPatch) -> TalosResult<UserRecord> {
        let mut users = self.users.write().expect("in-memory users write lock");
        let user = users
            .get_mut(user_id)
            .ok_or_else(|| TalosError::ResourceNotFound(format!("user {}", user_id)))?;
        if let Some(username) = patch.username {
            user.username = username;
        }
        if let Some(email) = patch.email {
            user.email = Some(email);
        }
        if let Some(role) = patch.role {
            user.role = role;
        }
        if let Some(disabled) = patch.disabled {
            user.disabled = disabled;
        }
        user.updated_at = current_timestamp_millis();
        Ok(user.clone())
    }

    async fn update_password_hash(&self, user_id: &str, password_hash: &str) -> TalosResult<()> {
        let mut users = self.users.write().expect("in-memory users write lock");
        let user = users
            .get_mut(user_id)
            .ok_or_else(|| TalosError::ResourceNotFound(format!("user {}", user_id)))?;
        user.password_hash = password_hash.to_string();
        user.updated_at = current_timestamp_millis();
        Ok(())
    }

    async fn increment_token_version(&self, user_id: &str) -> TalosResult<u64> {
        let mut users = self.users.write().expect("in-memory users write lock");
        let user = users
            .get_mut(user_id)
            .ok_or_else(|| TalosError::ResourceNotFound(format!("user {}", user_id)))?;
        user.token_version += 1;
        user.updated_at = current_timestamp_millis();
        Ok(user.token_version)
    }

    async fn list_users(&self) -> TalosResult<Vec<UserRecord>> {
        Ok(self
            .users
            .read()
            .expect("in-memory users read lock")
            .values()
            .cloned()
            .collect())
    }

    async fn delete_user(&self, user_id: &str) -> TalosResult<()> {
        self.users
            .write()
            .expect("in-memory users write lock")
            .remove(user_id);
        Ok(())
    }

    async fn count_users(&self) -> TalosResult<u64> {
        Ok(self.users.read().expect("in-memory users read lock").len() as u64)
    }
}

fn current_timestamp_millis() -> i64 {
    chrono::Utc::now().timestamp_millis()
}
