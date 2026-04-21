use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use async_trait::async_trait;
use serde_json::Value;
use talos_error::{TalosError, TalosResult};
use talos_storage::{Row, Storage};

use crate::{CreateUserRecord, UserPatch, UserRecord, UserRole, UserTokenMeta};

#[derive(Debug, Clone)]
pub struct CreateUserTokenRecord {
    pub token_id: String,
    pub user_id: String,
    pub name: String,
    pub token_hash: String,
    pub prefix: String,
    pub created_at: i64,
    pub expires_at: Option<i64>,
}

#[derive(Debug, Clone)]
pub struct UserTokenRecord {
    pub token_id: String,
    pub user_id: String,
    pub name: String,
    pub token_hash: String,
    pub prefix: String,
    pub created_at: i64,
    pub last_used_at: Option<i64>,
    pub expires_at: Option<i64>,
}

impl From<UserTokenRecord> for UserTokenMeta {
    fn from(value: UserTokenRecord) -> Self {
        Self {
            token_id: value.token_id,
            name: value.name,
            prefix: value.prefix,
            created_at: value.created_at,
            last_used_at: value.last_used_at,
            expires_at: value.expires_at,
        }
    }
}

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

pub type DatabaseUserTokenStore = DatabaseUserStore;

#[async_trait]
pub trait UserTokenStore: Send + Sync {
    async fn create_token(&self, input: CreateUserTokenRecord) -> TalosResult<UserTokenRecord>;
    async fn get_token_by_id(&self, token_id: &str) -> TalosResult<Option<UserTokenRecord>>;
    async fn get_token_by_hash(&self, token_hash: &str) -> TalosResult<Option<UserTokenRecord>>;
    async fn list_tokens_for_user(&self, user_id: &str) -> TalosResult<Vec<UserTokenRecord>>;
    async fn delete_token(&self, user_id: &str, token_id: &str) -> TalosResult<()>;
    async fn touch_token_last_used(&self, token_id: &str, last_used_at: i64) -> TalosResult<()>;
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
                input
                    .email
                    .clone()
                    .map(Value::String)
                    .unwrap_or(Value::Null),
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

impl DatabaseUserStore {
    fn parse_token(row: &Row) -> UserTokenRecord {
        UserTokenRecord {
            token_id: row.get_string("token_id").unwrap_or_default(),
            user_id: row.get_string("user_id").unwrap_or_default(),
            name: row.get_string("name").unwrap_or_default(),
            token_hash: row.get_string("token_hash").unwrap_or_default(),
            prefix: row.get_string("prefix").unwrap_or_default(),
            created_at: row.get_i64("created_at").unwrap_or_default(),
            last_used_at: row.get_i64("last_used_at"),
            expires_at: row.get_i64("expires_at"),
        }
    }

    async fn fetch_token_row(
        &self,
        where_clause: &str,
        params: &[&str],
    ) -> TalosResult<Option<Row>> {
        let sql = format!(
            "SELECT token_id, user_id, name, token_hash, prefix, created_at, last_used_at, expires_at FROM user_tokens WHERE {} LIMIT 1",
            where_clause
        );
        let rows = self.storage.query(&sql, params).await?;
        Ok(rows.into_iter().next())
    }
}

#[async_trait]
impl UserTokenStore for DatabaseUserStore {
    async fn create_token(&self, input: CreateUserTokenRecord) -> TalosResult<UserTokenRecord> {
        let prefix = input.prefix.clone();
        let name = input.name.clone();
        let created_at = input.created_at;
        let user_id = input.user_id.clone();
        let token_id = input.token_id.clone();
        let token_hash = input.token_hash.clone();
        let expires_at = input.expires_at;
        let row = Row::new(
            vec![
                "token_id".to_string(),
                "user_id".to_string(),
                "name".to_string(),
                "token_hash".to_string(),
                "prefix".to_string(),
                "created_at".to_string(),
                "expires_at".to_string(),
            ],
            vec![
                Value::String(input.token_id.clone()),
                Value::String(input.user_id.clone()),
                Value::String(input.name),
                Value::String(token_hash.clone()),
                Value::String(input.prefix),
                Value::Number(input.created_at.into()),
                input
                    .expires_at
                    .map(|value| Value::Number(value.into()))
                    .unwrap_or(Value::Null),
            ],
        );
        self.storage.insert("user_tokens", &row).await?;
        Ok(UserTokenRecord {
            token_id,
            user_id,
            name,
            token_hash,
            prefix,
            created_at,
            last_used_at: None,
            expires_at,
        })
    }

    async fn get_token_by_id(&self, token_id: &str) -> TalosResult<Option<UserTokenRecord>> {
        Ok(self
            .fetch_token_row("token_id = ?", &[token_id])
            .await?
            .as_ref()
            .map(Self::parse_token))
    }

    async fn get_token_by_hash(&self, token_hash: &str) -> TalosResult<Option<UserTokenRecord>> {
        Ok(self
            .fetch_token_row("token_hash = ?", &[token_hash])
            .await?
            .as_ref()
            .map(Self::parse_token))
    }

    async fn list_tokens_for_user(&self, user_id: &str) -> TalosResult<Vec<UserTokenRecord>> {
        let sql = "SELECT token_id, user_id, name, token_hash, prefix, created_at, last_used_at, expires_at FROM user_tokens WHERE user_id = ? ORDER BY created_at DESC";
        Ok(self
            .storage
            .query(sql, &[user_id])
            .await?
            .iter()
            .map(Self::parse_token)
            .collect())
    }

    async fn delete_token(&self, user_id: &str, token_id: &str) -> TalosResult<()> {
        let affected = self
            .storage
            .delete(
                "user_tokens",
                "token_id = ? AND user_id = ?",
                &[token_id, user_id],
            )
            .await?;
        if affected == 0 {
            return Err(TalosError::ResourceNotFound(format!(
                "user token {} for user {}",
                token_id, user_id
            )));
        }
        Ok(())
    }

    async fn touch_token_last_used(&self, token_id: &str, last_used_at: i64) -> TalosResult<()> {
        let row = Row::new(
            vec!["last_used_at".to_string()],
            vec![Value::Number(last_used_at.into())],
        );
        let affected = self
            .storage
            .update("user_tokens", &row, "token_id = ?", &[token_id])
            .await?;
        if affected == 0 {
            return Err(TalosError::ResourceNotFound(format!(
                "user token {}",
                token_id
            )));
        }
        Ok(())
    }
}

#[derive(Default)]
pub struct InMemoryUserStore {
    users: RwLock<HashMap<String, UserRecord>>,
}

#[derive(Default)]
pub struct InMemoryUserTokenStore {
    tokens_by_id: RwLock<HashMap<String, UserTokenRecord>>,
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

#[async_trait]
impl UserTokenStore for InMemoryUserTokenStore {
    async fn create_token(&self, input: CreateUserTokenRecord) -> TalosResult<UserTokenRecord> {
        let record = UserTokenRecord {
            token_id: input.token_id,
            user_id: input.user_id,
            name: input.name,
            token_hash: input.token_hash,
            prefix: input.prefix,
            created_at: input.created_at,
            last_used_at: None,
            expires_at: input.expires_at,
        };
        self.tokens_by_id
            .write()
            .expect("in-memory user tokens write lock")
            .insert(record.token_id.clone(), record.clone());
        Ok(record)
    }

    async fn get_token_by_id(&self, token_id: &str) -> TalosResult<Option<UserTokenRecord>> {
        Ok(self
            .tokens_by_id
            .read()
            .expect("in-memory user tokens read lock")
            .get(token_id)
            .cloned())
    }

    async fn get_token_by_hash(&self, token_hash: &str) -> TalosResult<Option<UserTokenRecord>> {
        Ok(self
            .tokens_by_id
            .read()
            .expect("in-memory user tokens read lock")
            .values()
            .find(|token| token.token_hash == token_hash)
            .cloned())
    }

    async fn list_tokens_for_user(&self, user_id: &str) -> TalosResult<Vec<UserTokenRecord>> {
        Ok(self
            .tokens_by_id
            .read()
            .expect("in-memory user tokens read lock")
            .values()
            .filter(|token| token.user_id == user_id)
            .cloned()
            .collect())
    }

    async fn delete_token(&self, user_id: &str, token_id: &str) -> TalosResult<()> {
        let mut tokens = self
            .tokens_by_id
            .write()
            .expect("in-memory user tokens write lock");
        let token = tokens
            .get(token_id)
            .ok_or_else(|| TalosError::ResourceNotFound(format!("user token {}", token_id)))?;
        if token.user_id != user_id {
            return Err(TalosError::ResourceNotFound(format!(
                "user token {}",
                token_id
            )));
        }
        tokens.remove(token_id);
        Ok(())
    }

    async fn touch_token_last_used(&self, token_id: &str, last_used_at: i64) -> TalosResult<()> {
        let mut tokens = self
            .tokens_by_id
            .write()
            .expect("in-memory user tokens write lock");
        let token = tokens
            .get_mut(token_id)
            .ok_or_else(|| TalosError::ResourceNotFound(format!("user token {}", token_id)))?;
        token.last_used_at = Some(last_used_at);
        Ok(())
    }
}

impl InMemoryUserTokenStore {}

fn current_timestamp_millis() -> i64 {
    chrono::Utc::now().timestamp_millis()
}
