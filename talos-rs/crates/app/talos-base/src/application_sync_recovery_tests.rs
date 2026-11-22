//! Real surviving-host recovery against a replacement Hub with an empty database.
use super::*;
use talos_store::session::{AgentExecutionStore, SessionStore};
#[path = "application_sync_legacy_tests.rs"]
mod legacy;
#[path = "application_sync_startup_upgrade_tests.rs"]
mod startup_upgrade;

pub(super) async fn verify(
    survivor: TalosApplicationRuntime,
    actor: &talos_service::Actor,
    hub_config: &Path,
    binary: &Path,
    hub: &mut HubProcess,
) {
    let identity = survivor
        .auth_service
        .verify_access_token(TOKEN)
        .await
        .unwrap();
    let old = survivor
        .sync
        .account_binding(&identity)
        .await
        .unwrap()
        .unwrap();
    let (survivor, released_history) = legacy::restart_with_v1(survivor, actor, &old).await;
    let session = "paused-sharing";
    let expected = survivor
        .sync_host
        .sync_stores
        .sessions
        .list_timeline_events_after(session, 0, 10_000)
        .await
        .unwrap();
    let request = talos_types::SessionHistoryRequest {
        session_id: session.into(),
        cursor: None,
        limit: None,
    };
    let before = survivor
        .service
        .get_session_history(actor, request.clone())
        .await
        .unwrap();
    assert!(!before.data.is_empty());
    let expected_node = survivor
        .service
        .get_session(actor, session)
        .await
        .unwrap()
        .node_id;
    let storage = &survivor.sync_host.sync_stores.storage;
    storage.execute(
        "CREATE TRIGGER interrupt_hub_rebind BEFORE DELETE ON sync_bindings BEGIN SELECT RAISE(ABORT, 'controlled Hub migration interruption'); END",
        &[],
    ).await.unwrap();
    let mut config_value: Value =
        serde_json::from_slice(&std::fs::read(hub_config).unwrap()).unwrap();
    config_value["storage"]["path"] =
        json!(hub_config.parent().unwrap().join("replacement-hub.db"));
    std::fs::write(
        hub_config,
        serde_json::to_vec_pretty(&config_value).unwrap(),
    )
    .unwrap();
    let log =
        std::fs::File::create(hub_config.parent().unwrap().join("replacement-hub.log")).unwrap();
    hub.0 = std::process::Command::new(binary)
        .arg("--config")
        .arg(hub_config)
        .stdout(log.try_clone().unwrap())
        .stderr(log)
        .spawn()
        .unwrap();
    wait_for(
        "first automatic migration interrupted after stopping old transport",
        async || {
            survivor
                .sync_host
                .sync_stores
                .handles
                .lock()
                .await
                .get(&actor.user_id)
                .is_some_and(|handle| handle.is_stopped())
        },
    )
    .await;
    assert_eq!(
        survivor
            .sync
            .account_binding(&identity)
            .await
            .unwrap()
            .unwrap()
            .scope,
        old.scope
    );
    storage
        .execute("DROP TRIGGER interrupt_hub_rebind", &[])
        .await
        .unwrap();
    // No enable/login/resume call: owned maintenance must retry the failed transition.
    wait_for(
        "survivor automatically rebound to replacement Hub",
        async || {
            survivor
                .sync
                .account_binding(&identity)
                .await
                .unwrap()
                .is_some_and(|binding| binding.scope.hub_id != old.scope.hub_id)
                && survivor
                    .service
                    .sync_status(actor)
                    .await
                    .unwrap()
                    .connection
                    == SyncConnectionState::Connected
        },
    )
    .await;
    let home = survivor
        .config
        .talos_paths()
        .talos_home
        .parent()
        .unwrap()
        .join("recovery-reader");
    let mut reader_config = config(&home, &survivor.config.cloud.api_base_url);
    reader_config.cloud.agent_base_url = survivor.config.cloud.agent_base_url.clone();
    reader_config.cloud.agent_trusted_ca_file = survivor.config.cloud.agent_trusted_ca_file.clone();
    reader_config.sync = survivor.config.sync.clone();
    std::fs::write(
        reader_config.source_path(),
        persisted_config(&reader_config),
    )
    .unwrap();
    let migration =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../../conf/migrations/sqlite");
    let reader = build_application_runtime_with_migration_dir(reader_config, &migration)
        .await
        .unwrap();
    reader.service.resume_account_sync(actor);
    let recovery_started = std::time::Instant::now();
    let mut diagnosed = false;
    wait_for(
        "cold reader recovered complete native history from replacement Hub",
        async || {
            if !diagnosed && recovery_started.elapsed() > Duration::from_secs(25) {
                diagnosed = true;
                for (label, runtime) in [("survivor", &survivor), ("reader", &reader)] {
                    eprintln!("Recovery diagnostic {label}: {:?}", runtime.service.sync_status(actor).await);
                    for sql in [
                        "SELECT hub_id,kind,resource_id,deleted,generation,length(version_vector) AS version_bytes FROM sync_documents",
                        "SELECT hub_id,kind,resource_id,exported_version_json FROM visible_sync_outbox",
                        "SELECT session_id,COUNT(*) AS count FROM committed_agent_events GROUP BY session_id",
                    ] {
                        eprintln!("Recovery diagnostic {label} {sql}: {:?}", runtime.sync_host.sync_stores.storage.query(sql, &[]).await);
                    }
                }
            }
            reader
                .sync_host
                .sync_stores
                .sessions
                .list_timeline_events_after(session, 0, 10_000)
                .await
                .is_ok_and(|events| {
                    events
                        .iter()
                        .map(|entry| &entry.event)
                        .eq(expected.iter().map(|entry| &entry.event))
                })
        },
    )
    .await;
    legacy::verify(&survivor, &reader, actor, &released_history).await;
    let restored = reader
        .service
        .get_session_history(actor, request.clone())
        .await
        .unwrap();
    assert_eq!(restored.data.len(), before.data.len());
    assert!(restored.next_cursor.is_none());
    assert!(restored.last_stable_event_id.is_some());
    assert_eq!(
        reader
            .service
            .get_session(actor, session)
            .await
            .unwrap()
            .node_id,
        expected_node
    );
    let source_computers = survivor
        .service
        .list_computers(actor, Default::default())
        .await
        .unwrap();
    wait_for(
        "cold reader recovered the complete Computer directory",
        async || {
            let computers = reader
                .service
                .list_computers(actor, Default::default())
                .await
                .unwrap();
            computers.data.len() == source_computers.data.len()
                && source_computers.data.iter().all(|source| {
                    computers.data.iter().any(|node| {
                        node.node_id == source.node_id
                            && node.display_name == source.display_name
                            && node.owner_user_id == source.owner_user_id
                    })
                })
        },
    )
    .await;
    let other = talos_service::Actor::regular("other-account").with_access_token(OTHER_TOKEN);
    assert!(reader
        .service
        .get_session_history(&other, request.clone())
        .await
        .is_err());
    assert!(reader
        .service
        .list_computers(&other, Default::default())
        .await
        .unwrap()
        .data
        .is_empty());
    let event = expected.first().unwrap().event.identity().clone();
    survivor
        .sync_host
        .sync_stores
        .sessions
        .append_timeline_notice(talos_agent_api::TimelineSystemNotice {
            identity: event,
            occurred_at: chrono::Utc::now().timestamp(),
            notice: talos_agent_api::AgentSystemNotice {
                level: talos_types::TalosTimelineNoticeLevel::Info,
                content: talos_agent_api::SystemNoticeContent::Text(
                    "Incremental notice after full recovery".into(),
                ),
            },
        })
        .await
        .unwrap();
    wait_for(
        "incremental notice followed the replacement baseline",
        async || {
            reader
                .sync_host
                .sync_stores
                .sessions
                .list_timeline_events_after(session, 0, 10_000)
                .await
                .is_ok_and(|events| events.len() == expected.len() + 1)
        },
    )
    .await;
    assert!(reader
        .sync_host
        .sync_stores
        .sessions
        .get_session_record("delete-session")
        .await
        .unwrap()
        .is_none());
    hub.stop();
    let local_read_started = std::time::Instant::now();
    tokio::time::timeout(Duration::from_secs(1), async {
        let history = reader
            .service
            .get_session_history(actor, request)
            .await
            .unwrap();
        assert_eq!(history.data.len(), before.data.len());
        let computers = reader
            .service
            .list_computers(actor, Default::default())
            .await
            .unwrap();
        assert!(!computers.data.is_empty());
    })
    .await
    .expect("local History/directory reads must not wait for Hub availability");
    eprintln!(
        "Recovered History and Computer directory local reads with Hub stopped: {:?}",
        local_read_started.elapsed()
    );
    close(reader).await;
    close(survivor).await;
}
