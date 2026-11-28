//! Automatic enrollment upgrades an existing document association without a Hub.
use super::*;
use talos_store::project::ProjectStore;
use talos_sync::{ComputerAlias, SyncBindParams, SyncResourceKey, SyncScope};

#[derive(Clone, Copy)]
enum DocumentTransport {
    Live,
    Retained,
    Disabled,
}

async fn export_project(
    fixture: &Fixture,
    binding: &talos_sync::SyncBinding,
    project_id: &str,
) -> talos_sync::SyncExportBatch {
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            if let Some(batch) = fixture
                .runtime
                .sync
                .export_pending(
                    binding,
                    SyncResourceKey::Project {
                        project_id: project_id.into(),
                    },
                    128,
                )
                .await
                .unwrap()
            {
                break batch;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("the local baseline or increment must be captured without a Hub")
}

async fn reconciles_automatic_enrollment(transport: DocumentTransport) {
    let fixture = Fixture::start().await;
    let host = &fixture.runtime.host;
    let actor = talos_service::Actor::regular("account").with_access_token("credential");
    let params = SyncBindParams {
        scope: SyncScope {
            hub_id: "gated-hub".into(),
            account_id: "account".into(),
        },
        source_user_id: "account".into(),
        computers: vec![],
    };
    let original = match transport {
        DocumentTransport::Live => Some(
            host.start_account_sync(
                &fixture.identity,
                params,
                host.credentials(&fixture.identity).unwrap(),
            )
            .await
            .unwrap(),
        ),
        DocumentTransport::Retained | DocumentTransport::Disabled => {
            let binding = fixture
                .runtime
                .sync
                .bind_account(&fixture.identity, params)
                .await
                .unwrap();
            host.install_capture(binding).unwrap();
            None
        }
    };
    if matches!(transport, DocumentTransport::Disabled) {
        fixture
            .runtime
            .sync
            .set_enabled("account", false)
            .await
            .unwrap();
    }
    let project = host
        .stores
        .projects
        .create_project_for_owner(
            "Created before registration",
            &fixture._home.path().join("project"),
            "account",
        )
        .await
        .unwrap();
    let computer = uuid::Uuid::new_v4().to_string();
    let mut node = directory_node(&computer);
    node.credential_expires_at = Some(chrono::Utc::now().timestamp() + 3600);
    fixture.directory.write().unwrap().push(node);
    let runner = talos_config::AgentNodeRunnerConfig {
        transport: Some(talos_config::AgentNodeTransport::Outbound),
        node_id: computer.clone(),
        control_plane_endpoint: host.config.cloud.agent_base_url.clone(),
        credential_file: host.config.talos_paths().conf_dir.join("worker.credential"),
        trusted_ca_file: host.config.cloud.agent_trusted_ca_file.clone(),
    };
    talos_config::YamlConfigStore::new(host.config.source_path())
        .save_node_identity(&runner, "controlled-worker-secret")
        .unwrap();
    let mut phase = super::super::RefreshPhase::AwaitLifecycle;
    let lifecycle = host.lifecycle.lock().await;
    tokio::time::timeout(
        Duration::from_secs(3),
        host.refresh_account(&actor, &mut phase),
    )
    .await
    .expect("automatic capture reconciliation must not acquire its lifecycle lock again")
    .unwrap();
    drop(lifecycle);
    let binding = fixture
        .runtime
        .sync
        .account_binding(&fixture.identity)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        binding.computers,
        vec![ComputerAlias {
            source_node_id: "embedded".into(),
            computer_id: computer.clone()
        }]
    );
    if matches!(transport, DocumentTransport::Disabled) {
        assert!(!host.stores.handles.lock().await.contains_key("account"));
    } else {
        let current = host.stores.handles.lock().await["account"].clone();
        if let Some(original) = original {
            assert!(!Arc::ptr_eq(&original, &current));
        }
        let baseline = export_project(&fixture, &binding, &project.project_id).await;
        let mut replica = talos_sync_sdk::SyncReplica::new(
            talos_sync_types::SyncResourceKey::new("project", &project.project_id),
            1,
        )
        .unwrap();
        replica
            .import(std::slice::from_ref(&baseline.update))
            .unwrap();
        assert_eq!(
            replica.header().unwrap().unwrap().related,
            Some(talos_sync_types::SyncResourceKey::new(
                "computer", &computer
            ))
        );
        assert_eq!(
            replica.fields().unwrap()["/data/name"],
            serde_json::json!("Created before registration")
        );
        fixture.runtime.sync.acknowledge(&baseline).await.unwrap();
        host.stores
            .projects
            .patch_project(
                &project.project_id,
                &talos_types::ProjectPatch {
                    name: Some("Changed after registration".into()),
                },
            )
            .await
            .unwrap();
        let delta = export_project(&fixture, &binding, &project.project_id).await;
        replica.import(&[delta.update]).unwrap();
        assert_eq!(
            replica.fields().unwrap()["/data/name"],
            serde_json::json!("Changed after registration")
        );
        assert!(Arc::ptr_eq(
            &current,
            &host.stores.handles.lock().await["account"]
        ));
    }
    fixture.finish().await;
}

#[tokio::test]
async fn automatic_enrollment_upgrades_live_binding_and_captures_project_baseline_and_delta() {
    reconciles_automatic_enrollment(DocumentTransport::Live).await;
}

#[tokio::test]
async fn automatic_enrollment_upgrades_retained_binding_and_captures_project_baseline_and_delta() {
    reconciles_automatic_enrollment(DocumentTransport::Retained).await;
}

#[tokio::test]
async fn automatic_enrollment_restores_capture_alias_without_enabling_document_sync() {
    reconciles_automatic_enrollment(DocumentTransport::Disabled).await;
}
