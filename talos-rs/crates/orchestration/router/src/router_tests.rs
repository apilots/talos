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
