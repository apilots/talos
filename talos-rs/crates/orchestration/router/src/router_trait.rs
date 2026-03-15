use async_trait::async_trait;
use talos_error::TalosResult;

use crate::types::Binding;

#[async_trait]
pub trait Router: Send + Sync {
    async fn route(&self, channel: &str, user_id: &str) -> TalosResult<Option<String>>;
    async fn add_binding(&self, binding: Binding) -> TalosResult<()>;
    async fn remove_binding(&self, id: &str) -> TalosResult<()>;
    async fn list_bindings(&self) -> TalosResult<Vec<Binding>>;
}
