//! Upgrade a real retained SDK 0.1.7 checkpoint through the actual WSS recovery flow.
use super::*;
use talos_store::session::AgentExecutionStore;
use talos_sync::SyncDataAdapter;
const SESSION: &str = "released-session";
pub(super) fn fixture_root() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../orchestration/sync-parser/testdata/released-v1")
}
pub(super) fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|value| format!("{value:02x}")).collect()
}
pub(super) async fn restart_with_v1(
    runtime: TalosApplicationRuntime,
    actor: &talos_service::Actor,
    binding: &talos_sync::SyncBinding,
) -> (
    TalosApplicationRuntime,
    Vec<talos_agent_api::TimelineEventPayload>,
) {
    runtime.service.suspend_account_sync(actor).await.unwrap();
    let expected = seed(&runtime, binding).await;
    let runtime = restart_seeded(runtime).await;
    runtime
        .auth_service
        .verify_access_token(TOKEN)
        .await
        .unwrap();
    runtime.service.resume_account_sync(actor);
    (runtime, expected)
}
pub(super) async fn restart_seeded(runtime: TalosApplicationRuntime) -> TalosApplicationRuntime {
    let config = runtime.config.clone();
    let state_path = config.talos_paths().talos_home.join("upgrades/state.json");
    close(runtime).await;
    // A released installation has never registered the new task. Keep its other
    // completed tasks while restoring that real pre-upgrade ledger condition.
    let mut state: Value = serde_json::from_slice(&std::fs::read(&state_path).unwrap()).unwrap();
    state["tasks"]
        .as_object_mut()
        .unwrap()
        .remove("202611220001_sync_replicas_v2");
    std::fs::write(&state_path, serde_json::to_vec_pretty(&state).unwrap()).unwrap();
    let migration =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../../conf/migrations/sqlite");
    let runtime = build_application_runtime_with_migration_dir(config, &migration)
        .await
        .unwrap();
    let rows = runtime
        .sync_host
        .sync_stores
        .storage
        .query(
            "SELECT replica_schema_version FROM sync_documents WHERE resource_id=?",
            &[SESSION],
        )
        .await
        .unwrap();
    assert!(rows
        .iter()
        .all(|row| row.get_i64("replica_schema_version") == Some(2)));
    runtime
}
pub(super) async fn seed(
    runtime: &TalosApplicationRuntime,
    binding: &talos_sync::SyncBinding,
) -> Vec<talos_agent_api::TimelineEventPayload> {
    let root = fixture_root();
    let fixture: Value =
        serde_json::from_slice(&std::fs::read(root.join("survivor-b.json")).unwrap()).unwrap();
    assert_eq!(fixture["sdk"], "0.1.7");
    let snapshot = std::fs::read(root.join("survivor-b.bin")).unwrap();
    let version: talos_sync::SyncVersion =
        serde_json::from_value(fixture["version"].clone()).unwrap();
    let changes: Vec<talos_sync::SyncChange> =
        serde_json::from_value(fixture["changes"].clone()).unwrap();
    let expected = changes
        .iter()
        .filter_map(|change| {
            if let talos_sync::SyncChange::Event(entry) = change {
                Some(entry.event.clone())
            } else {
                None
            }
        })
        .collect();
    let adapter = runtime.sync_host.install_capture(binding.clone()).unwrap();
    let batch = talos_sync::SyncImportBatch {
        scope: binding.scope.clone(),
        resource: talos_sync::SyncResourceKey::Session {
            session_id: SESSION.into(),
        },
        version: version.clone(),
        changes,
    };
    adapter.prepare_import(&batch).await.unwrap();
    let mut tx = runtime
        .sync_host
        .sync_stores
        .storage
        .begin_transaction()
        .await
        .unwrap();
    adapter.apply(&mut *tx, &batch).await.unwrap();
    tx.execute("INSERT INTO sync_documents(hub_id,account_id,kind,resource_id,computer_id,generation,deleted,snapshot_blob,version_vector,acknowledged_vector,acknowledged_generation,replica_schema_version) VALUES (?,?,'session',?,'d4383755-33d1-43fd-a6ef-cf14dc2a823d',0,0,unhex(?),unhex(?),unhex(?),0,1)",
        &[&binding.scope.hub_id,&binding.scope.account_id,SESSION,&hex(&snapshot),&hex(&version.version_vector),&hex(&version.version_vector)]).await.unwrap();
    tx.commit().await.unwrap();
    expected
}
pub(super) async fn verify(
    survivor: &TalosApplicationRuntime,
    reader: &TalosApplicationRuntime,
    actor: &talos_service::Actor,
    expected: &[talos_agent_api::TimelineEventPayload],
) {
    wait_for(
        "released SDK history recovered from surviving B while original sources remain offline",
        async || {
            reader
                .sync_host
                .sync_stores
                .sessions
                .list_timeline_events_after(SESSION, 0, 100)
                .await
                .is_ok_and(|events| events.iter().map(|entry| &entry.event).eq(expected.iter()))
        },
    )
    .await;
    let request = talos_types::SessionHistoryRequest {
        session_id: SESSION.into(),
        cursor: None,
        limit: None,
    };
    let history = reader
        .service
        .get_session_history(actor, request)
        .await
        .unwrap();
    assert_eq!(history.data.len(), 4);
    assert!(history.last_stable_event_id.is_some());
    survivor
        .sync_host
        .sync_stores
        .sessions
        .append_timeline_notice(talos_agent_api::TimelineSystemNotice {
            identity: talos_types::AgentTurnRef::new(SESSION, "turn-4"),
            occurred_at: 123,
            notice: talos_agent_api::AgentSystemNotice {
                level: talos_types::TalosTimelineNoticeLevel::Info,
                content: talos_agent_api::SystemNoticeContent::Text(
                    "Incremental notice after released replica upgrade".into(),
                ),
            },
        })
        .await
        .unwrap();
    wait_for(
        "incremental notice followed the upgraded released baseline",
        async || {
            reader
                .sync_host
                .sync_stores
                .sessions
                .list_timeline_events_after(SESSION, 0, 100)
                .await
                .is_ok_and(|events| events.len() == expected.len() + 1)
        },
    )
    .await;
    eprintln!("Released SDK 0.1.7 baseline: four complete Turns restored, original events retained, following incremental notice delivered");
}
