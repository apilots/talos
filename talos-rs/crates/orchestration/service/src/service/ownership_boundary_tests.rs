//! Product ownership and durable admission use the existing execution fixture.
use super::*;

#[tokio::test]
async fn product_ownership_rejects_personal_worker_without_relay_scope() {
    let (service, store) = test_application(
        Arc::new(TestAgentExecution {
            mode: TestAgentExecutionMode::Accepted,
        }),
        configured_test_config(),
    )
    .await;
    let actor = Actor::regular("member-user");
    let created = service
        .nodes
        .create_node(
            &actor,
            crate::CreateNodeRequest {
                display_name: "Personal Computer".into(),
                endpoint: None,
                owner_user_id: None,
                credential_expires_at: chrono::Utc::now().timestamp() + 3600,
            },
        )
        .await
        .unwrap();
    let principal = service
        .nodes
        .server()
        .unwrap()
        .authenticate_registration(&agent_protocol::NodeAuth {
            node_id: created.node.node_id.clone(),
            token: created.credential.token,
        })
        .await
        .unwrap();
    service
        .nodes
        .server()
        .unwrap()
        .register(
            &principal,
            agent_protocol::RegisterParams {
                product_owner: talos_types::NodeProductOwner::Computer,
                system_info: None,
                deployments: vec![agent_protocol::Deployment {
                    agent_id: "default".into(),
                }],
            },
        )
        .await
        .unwrap();
    let mut request = create_session_params("personal-without-relay", request_source(), None);
    request.node_id = Some(created.node.node_id.clone());
    let error = service.create_session(&actor, request).await.unwrap_err();
    assert!(error.is_code(talos_error::ErrorCode::PermissionDenied));
    assert!(store
        .get_session_record("personal-without-relay")
        .await
        .unwrap()
        .is_none());
    let mut request = start_turn_request("personal-turn-without-relay", "hello");
    request.node_id = Some(created.node.node_id.clone());
    assert!(service
        .start_turn(&actor, request_source(), request, None)
        .await
        .unwrap_err()
        .is_code(talos_error::ErrorCode::PermissionDenied));
    assert!(store
        .get_session_record("personal-turn-without-relay")
        .await
        .unwrap()
        .is_none());
    let session = service
        .create_session(
            &actor,
            create_session_params("wrong-product-binding", request_source(), None),
        )
        .await
        .unwrap();
    let mut record = store
        .get_session_record(&session.id)
        .await
        .unwrap()
        .unwrap();
    record.id = "persisted-personal-binding".into();
    assert!(service
        .access
        .worker_environment(&actor, &created.node.node_id)
        .await
        .err()
        .unwrap()
        .is_code(talos_error::ErrorCode::PermissionDenied));
    assert!(service
        .access
        .desktop_environment(&actor, &created.node.node_id)
        .await
        .is_ok());
    assert!(service
        .access
        .desktop_environment(&Actor::regular("other-owner"), &created.node.node_id)
        .await
        .is_err());
    record.execution_node_id = created.node.node_id;
    store.insert_session_record(&record).await.unwrap();
    assert!(service.get_session(&actor, &record.id).await.is_ok());
    assert!(service
        .sessions
        .ensure_execution_product_binding(&record)
        .await
        .unwrap_err()
        .is_code(talos_error::ErrorCode::PermissionDenied));
}

#[tokio::test]
async fn permanently_removed_cloud_worker_retains_idle_product_history_and_cleanup() {
    let (service, store) = test_application(
        Arc::new(TestAgentExecution {
            mode: TestAgentExecutionMode::Accepted,
        }),
        configured_test_config(),
    )
    .await;
    let actor = Actor::regular("member-user");
    let node = service
        .nodes
        .create_node(
            &actor,
            crate::CreateNodeRequest {
                display_name: "Removed Cloud Worker".into(),
                endpoint: None,
                owner_user_id: None,
                credential_expires_at: chrono::Utc::now().timestamp() + 3600,
            },
        )
        .await
        .unwrap();
    let principal = service
        .nodes
        .server()
        .unwrap()
        .authenticate_registration(&agent_protocol::NodeAuth {
            node_id: node.node.node_id.clone(),
            token: node.credential.token,
        })
        .await
        .unwrap();
    let registration = service
        .nodes
        .server()
        .unwrap()
        .register(
            &principal,
            agent_protocol::RegisterParams {
                product_owner: talos_types::NodeProductOwner::Server,
                system_info: None,
                deployments: vec![agent_protocol::Deployment {
                    agent_id: "default".into(),
                }],
            },
        )
        .await
        .unwrap();
    service
        .nodes
        .server()
        .unwrap()
        .set_registration_deletion_hook(
            &agent_protocol::NodeAuth {
                node_id: node.node.node_id.clone(),
                token: registration.lease_token,
            },
            Arc::new(|_| Box::pin(async { Ok(()) })),
        )
        .await
        .unwrap();
    let mut request = create_session_params("retained-cloud-history", request_source(), None);
    request.node_id = Some(node.node.node_id.clone());
    let session = service.create_session(&actor, request).await.unwrap();
    service
        .nodes
        .delete_computer(
            &actor,
            talos_types::DeleteComputerRequest {
                node_id: node.node.node_id,
            },
        )
        .await
        .unwrap();
    assert_eq!(
        service.get_session(&actor, &session.id).await.unwrap().id,
        session.id
    );
    assert!(service
        .get_session_history(
            &actor,
            talos_types::SessionHistoryRequest {
                session_id: session.id.clone(),
                cursor: None,
                limit: Some(20)
            }
        )
        .await
        .is_ok());
    assert!(service
        .recover_timeline(
            &actor,
            talos_types::RecoverTimelineRequest {
                session_id: session.id.clone(),
                after_event_id: None,
                replay_limit: Some(20)
            }
        )
        .await
        .is_ok());
    assert!(service.delete_session(&actor, &session.id).await.unwrap());
    assert!(store
        .get_session_record(&session.id)
        .await
        .unwrap()
        .is_none());
}

#[tokio::test]
async fn newly_created_session_keeps_accepted_worker_after_lost_confirmation() {
    let (service, product, journal) = test_application_with_worker(
        Arc::new(TestAgentExecution {
            mode: TestAgentExecutionMode::LostAcceptedConfirmation,
        }),
        Arc::new(TestAgentCoordinator {
            cancel_mode: TestCancelMode::Cancelled,
            cancelled_turn_ids: Arc::new(Mutex::new(Vec::new())),
            compaction_usage: Default::default(),
            compaction_error: None,
        }),
        configured_test_config(),
    )
    .await;
    let actor = Actor::regular("member-user");
    let request = start_turn_request("new-session-lost-ack", "keep executing");
    let error = service
        .start_turn(
            &actor,
            request_source(),
            request.clone(),
            Some("stable-request".into()),
        )
        .await
        .unwrap_err();
    assert!(agent_protocol::AgentAdmissionUnconfirmed::contains(&error));
    assert!(product
        .get_session_record(&request.session_id)
        .await
        .unwrap()
        .is_some());
    let scope = journal
        .session_scope(&request.session_id)
        .await
        .unwrap()
        .unwrap();
    let original = journal
        .execution_state(&scope)
        .await
        .unwrap()
        .unwrap()
        .active_turn
        .unwrap();
    service.reconcile_worker_events().await.unwrap();
    let replay = service
        .start_turn(
            &actor,
            request_source(),
            request,
            Some("stable-request".into()),
        )
        .await
        .unwrap();
    assert_eq!(replay.turn_id, original.turn_id);
    assert_eq!(
        product
            .get_agent_active_turn(&original.session_id)
            .await
            .unwrap()
            .unwrap()
            .identity,
        original
    );
    assert!(journal
        .execution_state(&scope)
        .await
        .unwrap()
        .unwrap()
        .active_turn
        .is_some());
}
