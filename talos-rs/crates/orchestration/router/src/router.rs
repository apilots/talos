use async_trait::async_trait;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::debug;

use talos_error::TalosResult;

use crate::router_trait::Router;
use crate::types::{Binding, MatchRule, RouterConfig};

/// 路由器实现
pub struct RouterImpl {
    bindings: Arc<RwLock<Vec<Binding>>>,
}

impl RouterImpl {
    /// 创建新的路由器实例
    pub fn new() -> Self {
        Self {
            bindings: Arc::new(RwLock::new(Vec::new())),
        }
    }

    /// 从配置创建路由器实例
    pub async fn from_config(config: RouterConfig) -> TalosResult<Self> {
        let router = Self::new();

        for binding_config in config.bindings {
            let rule = match binding_config.rule_type.as_str() {
                "peer" => {
                    if let Some(value) = binding_config.rule_value {
                        MatchRule::Peer(value)
                    } else {
                        return Err(talos_error::TalosError::public(
                            talos_error::ErrorCode::InvalidRequest,
                            "Peer rule requires rule_value".to_string(),
                        ));
                    }
                }
                "channel" => {
                    if let Some(value) = binding_config.rule_value {
                        MatchRule::Channel(value)
                    } else {
                        return Err(talos_error::TalosError::public(
                            talos_error::ErrorCode::InvalidRequest,
                            "Channel rule requires rule_value".to_string(),
                        ));
                    }
                }
                "default" => MatchRule::Default,
                _ => {
                    return Err(talos_error::TalosError::public(
                        talos_error::ErrorCode::InvalidRequest,
                        format!("Invalid rule type: {}", binding_config.rule_type),
                    ));
                }
            };

            let binding = Binding {
                id: binding_config.id,
                agent_id: binding_config.agent_id,
                rule,
                priority: binding_config.priority,
            };

            router.add_binding(binding).await?;
        }

        Ok(router)
    }

    /// 匹配用户到绑定规则
    fn matches_rule(&self, binding: &Binding, channel: &str, user_id: &str) -> bool {
        match &binding.rule {
            MatchRule::Peer(peer_id) => peer_id == user_id,
            MatchRule::Channel(chan) => chan == channel,
            MatchRule::Default => true,
        }
    }
}

#[async_trait]
impl Router for RouterImpl {
    async fn route(&self, channel: &str, user_id: &str) -> TalosResult<Option<String>> {
        let bindings = self.bindings.read().await;

        let mut matched: Vec<&Binding> = bindings
            .iter()
            .filter(|b| self.matches_rule(b, channel, user_id))
            .collect();

        matched.sort_by(|a, b| b.priority.cmp(&a.priority));

        Ok(matched.first().map(|b| b.agent_id.clone()))
    }

    async fn add_binding(&self, binding: Binding) -> TalosResult<()> {
        let mut bindings = self.bindings.write().await;
        bindings.retain(|b| b.id != binding.id);
        bindings.push(binding);
        debug!("Added routing binding");
        Ok(())
    }

    async fn remove_binding(&self, id: &str) -> TalosResult<()> {
        let mut bindings = self.bindings.write().await;
        bindings.retain(|b| b.id != id);
        debug!("Removed routing binding: {}", id);
        Ok(())
    }

    async fn list_bindings(&self) -> TalosResult<Vec<Binding>> {
        Ok(self.bindings.read().await.clone())
    }
}

#[cfg(test)]
#[path = "router_tests.rs"]
mod tests;
