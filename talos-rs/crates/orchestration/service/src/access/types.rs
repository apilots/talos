use talos_auth::UserRole;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Actor {
    pub user_id: String,
    pub username: Option<String>,
    pub role: UserRole,
    pub access_token: Option<String>,
    pub auth_disabled: bool,
}

impl Actor {
    pub fn new(user_id: impl Into<String>) -> Self {
        Self {
            user_id: user_id.into(),
            username: None,
            role: UserRole::User,
            access_token: None,
            auth_disabled: false,
        }
    }

    pub fn regular(user_id: impl Into<String>) -> Self {
        Self::new(user_id)
    }

    pub fn with_access_token(mut self, token: impl Into<String>) -> Self {
        self.access_token = Some(token.into());
        self
    }

    pub fn with_username(mut self, username: Option<String>) -> Self {
        self.username = username;
        self
    }

    pub fn with_role(mut self, role: UserRole) -> Self {
        self.role = role;
        self
    }

    pub fn with_auth_disabled(mut self, auth_disabled: bool) -> Self {
        self.auth_disabled = auth_disabled;
        self
    }

    pub fn auth_disabled() -> Self {
        Self::regular("anonymous").with_auth_disabled(true)
    }
}
