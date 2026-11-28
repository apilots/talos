use super::*;

#[tokio::test]
async fn enrolled_native_jobs_share_public_identity_across_local_entries() {
    let fixture = HostFixture::start().await;
    let computer = uuid::Uuid::new_v4().to_string();
    let config = &fixture.runtime.config;
    talos_config::YamlConfigStore::new(config.source_path())
        .save_node_identity(
            &talos_config::AgentNodeRunnerConfig {
                transport: Some(talos_config::AgentNodeTransport::Outbound),
                node_id: computer.clone(),
                control_plane_endpoint: config.cloud.agent_base_url.clone(),
                credential_file: config
                    .source_path()
                    .parent()
                    .unwrap()
                    .join("node.credential"),
                trusted_ca_file: None,
            },
            "owned-fixture-token",
        )
        .unwrap();
    let host: Arc<dyn NativeJobHost> = fixture.host.clone();
    let service = NativeJobService::new(
        Arc::new(DatabaseNativeJobControlStore::new(
            fixture.storage.pool().clone(),
        )),
        Arc::downgrade(&host),
    );
    let owner = Actor::regular(OWNER);
    for selected in [&computer, "embedded"] {
        let rows = service
            .list(
                &owner,
                BuiltinJobListRequest {
                    computer_id: selected.into(),
                },
            )
            .await
            .unwrap()
            .data;
        assert!(rows.iter().all(|job| job.computer_id == computer
            && job.execution_instance_id == fixture.runtime.host_id()));
        let job = &rows[0];
        for action in [BuiltinJobAction::Resume, BuiltinJobAction::Pause] {
            let result = service
                .control(
                    &owner,
                    BuiltinJobControlRequest {
                        computer_id: selected.into(),
                        execution_instance_id: job.execution_instance_id.clone(),
                        job_id: job.id,
                        action,
                    },
                )
                .await
                .unwrap();
            assert_eq!(result.computer_id, computer);
            assert_eq!(result.paused, action == BuiltinJobAction::Pause);
        }
        assert_code(
            service
                .control(
                    &owner,
                    BuiltinJobControlRequest {
                        computer_id: selected.into(),
                        execution_instance_id: "stale-instance".into(),
                        job_id: job.id,
                        action: BuiltinJobAction::RunOnce,
                    },
                )
                .await,
            ErrorCode::Conflict,
        );
        for actor in [
            Actor::regular(FOREIGN),
            Actor::regular(FOREIGN).with_role(talos_auth::UserRole::Admin),
        ] {
            assert_code(
                service
                    .list(
                        &actor,
                        BuiltinJobListRequest {
                            computer_id: selected.into(),
                        },
                    )
                    .await,
                ErrorCode::PermissionDenied,
            );
        }
    }
    assert_code(
        service
            .list(
                &owner,
                BuiltinJobListRequest {
                    computer_id: uuid::Uuid::new_v4().to_string(),
                },
            )
            .await,
        ErrorCode::PermissionDenied,
    );
    fixture.close().await;
}
