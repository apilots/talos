use super::tests::{
    configured_test_config, test_application, TestAgentExecution, TestAgentExecutionMode,
};
use crate::Actor;
use std::sync::Arc;
use talos_config::{ConfigStore, InMemoryLlmConfigStore, SessionMessageSendPolicy};
use talos_types::{ConfigGetRequest, ConfigPath, ConfigResponse, ConfigUpdateRequest, ConfigValue};

#[tokio::test]
async fn product_policy_uses_product_store_without_connecting_selected_worker() {
    let product_store = Arc::new(InMemoryLlmConfigStore::new(Default::default()));
    let (service, _) = test_application(
        Arc::new(TestAgentExecution {
            mode: TestAgentExecutionMode::Accepted,
        }),
        configured_test_config(),
    )
    .await;
    let service = service.with_product_config_store(product_store.clone());
    let admin = Actor::regular("member").with_role(talos_auth::UserRole::Admin);
    let response = service
        .update_config(
            &admin,
            ConfigUpdateRequest {
                node_id: Some("offline-worker-with-no-registration".into()),
                value: ConfigValue::SessionMessageSendPolicy(SessionMessageSendPolicy::Steer),
            },
        )
        .await
        .unwrap();
    assert_eq!(
        response,
        ConfigResponse::SessionMessageSendPolicy {
            policy: SessionMessageSendPolicy::Steer,
        }
    );
    assert_eq!(
        product_store.load_session_message_send_policy().unwrap(),
        SessionMessageSendPolicy::Steer
    );
    let response = service
        .get_config(
            &admin,
            ConfigGetRequest {
                node_id: Some("another-unreachable-worker".into()),
                path: ConfigPath::SessionMessageSendPolicy,
            },
        )
        .await
        .unwrap();
    assert_eq!(
        response,
        ConfigResponse::SessionMessageSendPolicy {
            policy: SessionMessageSendPolicy::Steer,
        }
    );
    let computer = service.with_product_owner(talos_types::NodeProductOwner::Computer);
    let response = computer
        .get_config(
            &Actor::regular("member"),
            ConfigGetRequest {
                node_id: None,
                path: ConfigPath::SessionMessageSendPolicy,
            },
        )
        .await
        .unwrap();
    assert_eq!(
        response,
        ConfigResponse::SessionMessageSendPolicy {
            policy: SessionMessageSendPolicy::Steer
        }
    );
}
