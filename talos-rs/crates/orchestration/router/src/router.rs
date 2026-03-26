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
                        return Err(talos_error::TalosError::InvalidConfiguration(
                            "Peer rule requires rule_value".to_string(),
                        ));
                    }
                }
                "channel" => {
                    if let Some(value) = binding_config.rule_value {
                        MatchRule::Channel(value)
                    } else {
                        return Err(talos_error::TalosError::InvalidConfiguration(
                            "Channel rule requires rule_value".to_string(),
                        ));
                    }
                }
                "default" => MatchRule::Default,
                _ => {
                    return Err(talos_error::TalosError::InvalidConfiguration(format!(
                        "Invalid rule type: {}",
                        binding_config.rule_type
                    )));
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
mod tests {
    use super::*;
    use crate::types::BindingConfig;

    #[tokio::test]
    async fn test_add_and_list_bindings() {
        let router = RouterImpl::new();

        let binding1 = Binding {
            id: "b1".to_string(),
            agent_id: "agent1".to_string(),
            rule: MatchRule::Peer("user1".to_string()),
            priority: 10,
        };

        router.add_binding(binding1).await.unwrap();

        let bindings = router.list_bindings().await.unwrap();
        assert_eq!(bindings.len(), 1);
    }

    #[tokio::test]
    async fn test_route_peer_match() {
        let router = RouterImpl::new();

        router
            .add_binding(Binding {
                id: "b1".to_string(),
                agent_id: "agent1".to_string(),
                rule: MatchRule::Peer("user1".to_string()),
                priority: 10,
            })
            .await
            .unwrap();

        let result = router.route("telegram", "user1").await.unwrap();
        assert_eq!(result, Some("agent1".to_string()));

        let result2 = router.route("telegram", "user2").await.unwrap();
        assert_eq!(result2, None);
    }

    #[tokio::test]
    async fn test_route_priority() {
        let router = RouterImpl::new();

        router
            .add_binding(Binding {
                id: "b1".to_string(),
                agent_id: "agent1".to_string(),
                rule: MatchRule::Default,
                priority: 5,
            })
            .await
            .unwrap();

        router
            .add_binding(Binding {
                id: "b2".to_string(),
                agent_id: "agent2".to_string(),
                rule: MatchRule::Peer("user1".to_string()),
                priority: 10,
            })
            .await
            .unwrap();

        let result = router.route("telegram", "user1").await.unwrap();
        assert_eq!(result, Some("agent2".to_string()));
    }

    #[tokio::test]
    async fn test_from_config() {
        let config = RouterConfig {
            bindings: vec![
                BindingConfig {
                    id: "b1".to_string(),
                    agent_id: "agent1".to_string(),
                    rule_type: "peer".to_string(),
                    rule_value: Some("user1".to_string()),
                    priority: 10,
                },
                BindingConfig {
                    id: "b2".to_string(),
                    agent_id: "agent2".to_string(),
                    rule_type: "channel".to_string(),
                    rule_value: Some("telegram".to_string()),
                    priority: 5,
                },
                BindingConfig {
                    id: "b3".to_string(),
                    agent_id: "agent3".to_string(),
                    rule_type: "default".to_string(),
                    rule_value: None,
                    priority: 1,
                },
            ],
        };

        let router = RouterImpl::from_config(config).await.unwrap();
        let bindings = router.list_bindings().await.unwrap();

        assert_eq!(bindings.len(), 3);

        // Test peer rule
        let result = router.route("any_channel", "user1").await.unwrap();
        assert_eq!(result, Some("agent1".to_string()));

        // Test channel rule
        let result = router.route("telegram", "any_user").await.unwrap();
        assert_eq!(result, Some("agent2".to_string()));

        // Test default rule
        let result = router.route("other_channel", "other_user").await.unwrap();
        assert_eq!(result, Some("agent3".to_string()));
    }

    #[tokio::test]
    async fn test_from_config_invalid_rule_type() {
        let config = RouterConfig {
            bindings: vec![BindingConfig {
                id: "b1".to_string(),
                agent_id: "agent1".to_string(),
                rule_type: "invalid".to_string(),
                rule_value: Some("value".to_string()),
                priority: 10,
            }],
        };

        let result = RouterImpl::from_config(config).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_from_config_missing_rule_value() {
        let config = RouterConfig {
            bindings: vec![BindingConfig {
                id: "b1".to_string(),
                agent_id: "agent1".to_string(),
                rule_type: "peer".to_string(),
                rule_value: None, // Missing required value
                priority: 10,
            }],
        };

        let result = RouterImpl::from_config(config).await;
        assert!(result.is_err());
    }
}
