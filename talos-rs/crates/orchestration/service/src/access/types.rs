use talos_auth::UserRole;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Actor {
    pub user_id: String,
    pub username: Option<String>,
    pub role: UserRole,
    pub access_token: Option<String>,
}

impl Actor {
    pub fn new(user_id: impl Into<String>) -> Self {
        Self {
            user_id: user_id.into(),
            username: None,
            role: UserRole::User,
            access_token: None,
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
}
