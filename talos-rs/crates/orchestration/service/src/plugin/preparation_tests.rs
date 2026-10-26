use super::*;
use talos_types::*;

fn fixture() -> (
    tempfile::TempDir,
    PluginManagerService,
    PluginInstallRequest,
) {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("source/skills/writer");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(
        root.join("SKILL.md"),
        "---\nname: writer\ndescription: Write.\n---\nWrite.\n",
    )
    .unwrap();
    let manager = talos_plugin::PluginManager::with_state_store(
        talos_config::PluginsRuntimeConfig {
            sources: vec![],
            roots: vec![],
        },
        talos_plugin::PluginStateStore::new(directory.path().join("plugins")),
    );
    let source = manager
        .add_source(PluginSourceAddRequest {
            node_id: None,
            source: MarketplaceSourceInput::LocalFolder {
                path: directory.path().join("source"),
                sparse_paths: vec!["skills".into()],
            },
            refresh: true,
        })
        .unwrap();
    let request = PluginInstallRequest {
        node_id: None,
        target: PluginInstallTarget::Plugin {
            plugin_id: source.plugins[0].id.clone(),
            expected_source_commit: None,
        },
        enable: Some(true),
        conflict_policy: PluginInstallConflictPolicy::Reject,
    };
    (
        directory,
        PluginManagerService::with_manager(manager),
        request,
    )
}
fn actor(id: &str) -> WorkerEnvironmentContext {
    WorkerEnvironmentContext {
        user_id: id.into(),
        username: None,
        workspace_id: None,
    }
}

#[tokio::test]
async fn cancellation_is_actor_bound_and_releases_the_candidate_without_commit() {
    let (_directory, service, request) = fixture();
    let prepared = service
        .prepare_plugin_change(actor("alice"), WorkerPluginChange::Install(request.clone()))
        .await
        .unwrap();
    assert_eq!(
        service
            .cancel_plugin_change(actor("bob"), prepared.clone())
            .unwrap_err()
            .error_code(),
        ErrorCode::PermissionDenied
    );
    service
        .cancel_plugin_change(actor("alice"), prepared.clone())
        .unwrap();
    service
        .cancel_plugin_change(actor("alice"), prepared.clone())
        .unwrap();
    assert_eq!(
        service
            .commit_plugin_install(
                actor("alice"),
                WorkerPluginInstall {
                    request,
                    preparation_id: prepared.preparation_id,
                }
            )
            .await
            .unwrap_err()
            .error_code(),
        ErrorCode::Conflict
    );
    assert!(service.preparations.lock().unwrap().is_empty());
    assert_eq!(service.preparation_slots.available_permits(), 4);
}

#[tokio::test]
async fn preparation_does_not_hold_execution_gate_and_commit_still_requires_it() {
    let (_directory, service, request) = fixture();
    let gate = service.execution_gate();
    let executing = gate.read().await;
    let prepared = service
        .prepare_plugin_change(actor("alice"), WorkerPluginChange::Install(request.clone()))
        .await
        .unwrap();
    let error = service
        .commit_plugin_install(
            actor("alice"),
            WorkerPluginInstall {
                request,
                preparation_id: prepared.preparation_id,
            },
        )
        .await
        .unwrap_err();
    assert_eq!(error.error_code(), ErrorCode::Conflict);
    drop(executing);
    assert!(service
        .manager
        .list_plugins(PluginListRequest {
            node_id: None,
            kind: None,
            status: None,
            source_id: None,
            include_disabled: true
        })
        .unwrap()
        .plugins
        .is_empty());
}

#[tokio::test]
async fn preparation_is_bound_to_actor_and_request_and_can_only_commit_once() {
    let (_directory, service, request) = fixture();
    let prepared = service
        .prepare_plugin_change(actor("alice"), WorkerPluginChange::Install(request.clone()))
        .await
        .unwrap();
    let params = WorkerPluginInstall {
        request,
        preparation_id: prepared.preparation_id,
    };
    assert_eq!(
        service
            .commit_plugin_install(actor("bob"), params.clone())
            .await
            .unwrap_err()
            .error_code(),
        ErrorCode::PermissionDenied
    );
    let mut changed = params.clone();
    changed.request.enable = Some(false);
    assert_eq!(
        service
            .commit_plugin_install(actor("alice"), changed)
            .await
            .unwrap_err()
            .error_code(),
        ErrorCode::PermissionDenied
    );
    service
        .commit_plugin_install(actor("alice"), params.clone())
        .await
        .unwrap();
    assert_eq!(
        service
            .commit_plugin_install(actor("alice"), params)
            .await
            .unwrap_err()
            .error_code(),
        ErrorCode::Conflict
    );
}

#[tokio::test]
async fn preparation_admission_precedes_materialization_and_releases_after_drop() {
    let (_directory, service, request) = fixture();
    let held = service
        .preparation_slots
        .clone()
        .acquire_many_owned(4)
        .await
        .unwrap();
    let mut exhausted_request = request.clone();
    exhausted_request.target = PluginInstallTarget::Plugin {
        plugin_id: PluginId("missing-source-plugin".into()),
        expected_source_commit: None,
    };
    let error = service
        .prepare_plugin_change(
            actor("alice"),
            WorkerPluginChange::Install(exhausted_request),
        )
        .await
        .unwrap_err();
    assert_eq!(error.error_code(), ErrorCode::Conflict);
    assert!(service.preparations.lock().unwrap().is_empty());
    drop(held);
    let prepared = service
        .prepare_plugin_change(actor("alice"), WorkerPluginChange::Install(request.clone()))
        .await
        .unwrap();
    assert_eq!(service.preparation_slots.available_permits(), 3);
    service
        .commit_plugin_install(
            actor("alice"),
            WorkerPluginInstall {
                request,
                preparation_id: prepared.preparation_id,
            },
        )
        .await
        .unwrap();
    assert_eq!(service.preparation_slots.available_permits(), 4);
}
