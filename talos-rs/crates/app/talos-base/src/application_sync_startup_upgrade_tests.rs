//! Local acceptance with independently upgraded persisted Hub and native replicas.
use super::*;
use talos_sync::{SyncBindParams, SyncResourceKey};

const HUB: &str = "11111111-1111-4111-8111-111111111111";
const SESSION: &str = "released-session";
fn credentials() -> talos_sync_sdk::CredentialCallback {
    Arc::new(|| {
        Box::pin(async {
            Ok(talos_sync_sdk::VerifiedCredential {
                account_id: ACCOUNT.into(),
                access_token: TOKEN.into(),
            })
        })
    })
}
fn scope() -> talos_sync::SyncScope {
    talos_sync::SyncScope {
        hub_id: HUB.into(),
        account_id: ACCOUNT.into(),
    }
}
fn params() -> SyncBindParams {
    SyncBindParams {
        scope: scope(),
        source_user_id: ACCOUNT.into(),
        computers: vec![],
    }
}
async fn start(runtime: &TalosApplicationRuntime) -> Arc<talos_sync::SyncHandle> {
    let identity = runtime
        .auth_service
        .verify_access_token(TOKEN)
        .await
        .unwrap();
    let handle = runtime
        .start_account_sync(&identity, params(), credentials())
        .await
        .unwrap();
    wait_for("account document transport connected", async || {
        handle.status().connection == SyncConnectionState::Connected
    })
    .await;
    handle
}
async fn complete_turns(
    runtime: &TalosApplicationRuntime,
    actor: &talos_service::Actor,
    count: usize,
) -> bool {
    runtime
        .service
        .get_session_history(
            actor,
            talos_types::SessionHistoryRequest {
                session_id: SESSION.into(),
                cursor: None,
                limit: None,
            },
        )
        .await
        .is_ok_and(|history| history.data.len() == count && history.next_cursor.is_none())
}
#[tokio::test]
#[ignore = "requires TALOS_SYNC_V1_DATABASE and candidate TALOS_SYNC_BIN; explicit local upgrade acceptance"]
async fn upgraded_hub_and_local_replica_continue_from_the_original_cut() {
    let _logging = talos_logging::init().unwrap();
    let directory = tempfile::Builder::new()
        .prefix(".talos-startup-sync-")
        .tempdir()
        .unwrap();
    let old_database = std::env::var_os("TALOS_SYNC_V1_DATABASE")
        .map(PathBuf::from)
        .expect("supply a complete released-v1 Hub fixture database");
    let binary = std::env::var_os("TALOS_SYNC_BIN")
        .map(PathBuf::from)
        .expect("supply the current independently built Sync startup-upgrade binary");
    let hub_database = directory.path().join("hub.db");
    std::fs::copy(old_database, &hub_database).unwrap();
    let certificate = rcgen::generate_simple_self_signed(vec!["127.0.0.1".into()]).unwrap();
    let cert = directory.path().join("cert.pem");
    let key = directory.path().join("key.pem");
    std::fs::write(&cert, certificate.cert.pem()).unwrap();
    std::fs::write(&key, certificate.signing_key.serialize_pem()).unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let api = format!("http://{}", listener.local_addr().unwrap());
    let http = tokio::spawn(async move {
        axum::serve(
            listener,
            Router::new().route("/v1/auth/introspect", post(introspect)),
        )
        .await
        .unwrap();
    });
    let hub_address = port();
    let hub_config = directory.path().join("sync.yaml");
    let mapping =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../../conf/sync-v1-upgrade-mapping.json");
    std::fs::write(&hub_config, serde_json::to_vec_pretty(&json!({
        "listener":{"bind_address":hub_address.to_string(),"certificate_file":cert,"private_key_file":key},
        "storage":{"backend":"sqlite","path":hub_database,"max_connections":1,"busy_timeout_ms":5000},
        "auth":{"issuer_url":api,"timeout_ms":5000,"revalidate_interval_ms":1000},
        "upgrade":{"v1_mapping_file":mapping},"limits":{"heartbeat_interval_ms":300}
    })).unwrap()).unwrap();
    let log_path = directory.path().join("sync.log");
    let log = std::fs::File::create(&log_path).unwrap();
    let mut hub = HubProcess(
        std::process::Command::new(binary)
            .env("RUST_LOG", "info")
            .arg("--config")
            .arg(hub_config)
            .stdout(log.try_clone().unwrap())
            .stderr(log)
            .spawn()
            .unwrap(),
    );
    wait_for("upgraded retained Hub listener", async || {
        tokio::net::TcpStream::connect(hub_address).await.is_ok()
    })
    .await;
    let mut options = talos_sync_sdk::SyncOptions::new(
        format!("wss://{hub_address}/v1/sync"),
        ACCOUNT.into(),
        uuid::Uuid::new_v4().to_string(),
    );
    options.trusted_ca_pem = Some(std::fs::read(&cert).unwrap());
    let client = talos_sync_sdk::SyncClient::connect(options, credentials())
        .await
        .unwrap_or_else(|error| {
            panic!(
                "upgraded Hub handshake: {error:?}; {}",
                std::fs::read_to_string(&log_path).unwrap()
            )
        });
    assert_eq!(
        client.scope().await,
        scope(),
        "upgrade must preserve the original Hub identity"
    );
    client.shutdown().await.unwrap();
    let migration =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../../conf/migrations/sqlite");
    let actor = talos_service::Actor::regular(ACCOUNT).with_access_token(TOKEN);
    let local_config = |name: &str| {
        let mut config = config(&directory.path().join(name), &api);
        config.sync.hub_endpoint = format!("wss://{hub_address}/v1/sync");
        config.sync.trusted_ca_file = Some(cert.clone());
        config.cloud.agent_base_url = "https://127.0.0.1:9".into();
        std::fs::write(config.source_path(), persisted_config(&config)).unwrap();
        config
    };
    let reader = build_application_runtime_with_migration_dir(local_config("cold-c"), &migration)
        .await
        .unwrap();
    start(&reader).await;
    wait_for(
        "C restores two complete Turns from the upgraded Hub before B exists",
        async || complete_turns(&reader, &actor, 2).await,
    )
    .await;
    eprintln!(
        "Hub startup upgrade independently retained source-A's original two Turns and Hub ID"
    );

    // Prepare an old B installation with four native Turns but only A's first
    // two acknowledged. The SDK0.1.7 snapshots retain the exact shared op IDs.
    let old_b = build_application_runtime_with_migration_dir(local_config("offline-b"), &migration)
        .await
        .unwrap();
    let identity = RequestIdentity {
        user_id: ACCOUNT.into(),
        username: None,
        role: talos_auth::UserRole::User,
        user_type: Default::default(),
        auth_disabled: false,
        access_token: None,
    };
    let binding = old_b.sync.bind_account(&identity, params()).await.unwrap();
    let expected = legacy::seed(&old_b, &binding).await;
    let fixture = legacy::fixture_root();
    let a: Value =
        serde_json::from_slice(&std::fs::read(fixture.join("source-a.json")).unwrap()).unwrap();
    let b: Value =
        serde_json::from_slice(&std::fs::read(fixture.join("survivor-b.json")).unwrap()).unwrap();
    let a_version: talos_sync::SyncVersion = serde_json::from_value(a["version"].clone()).unwrap();
    let mut tx = old_b.host.stores.storage.begin_transaction().await.unwrap();
    tx.execute(
        "UPDATE sync_documents SET acknowledged_vector=unhex(?) WHERE resource_id=?",
        &[&legacy::hex(&a_version.version_vector), SESSION],
    )
    .await
    .unwrap();
    tx.execute("INSERT INTO sync_outbox(hub_id,account_id,source_user_id,kind,resource_id,changes_json,exported_version_json,created_at) VALUES (?, ?, ?, 'session', ?, '[]', ?, 99)",
        &[HUB,ACCOUNT,ACCOUNT,SESSION,&serde_json::to_string(&b["version"]).unwrap()]).await.unwrap();
    tx.commit().await.unwrap();
    let survivor = legacy::restart_seeded(old_b).await;
    let key = SyncResourceKey::Session {
        session_id: SESSION.into(),
    };
    let upload = survivor
        .sync
        .export_pending(&binding, key.clone(), 128)
        .await
        .unwrap()
        .unwrap();
    let full = survivor
        .host
        .stores
        .storage
        .query(
            "SELECT length(snapshot_blob) AS bytes FROM sync_documents WHERE resource_id=?",
            &[SESSION],
        )
        .await
        .unwrap();
    let full_bytes = full[0].get_i64("bytes").unwrap() as usize;
    let delta = loro::LoroDoc::decode_import_blob_meta(&upload.update, true).unwrap();
    let acknowledged = loro::VersionVector::decode(&a_version.version_vector).unwrap();
    assert!(
        delta.partial_end_vv.iter().all(|(peer, _)| *peer != 101),
        "the acknowledged original-A prefix must not be exported again"
    );
    assert!(
        delta.partial_end_vv.iter().any(|(peer, _)| *peer == 202),
        "the original-B offline operations must retain their writer identity"
    );
    for (peer, _) in delta.partial_end_vv.iter() {
        let start = delta.partial_start_vv.get(peer).copied().unwrap_or(0);
        assert!(start >= acknowledged.get(peer).copied().unwrap_or(0));
    }
    let handle = start(&survivor).await;
    handle.flush(key).await.unwrap();
    wait_for(
        "B's original-source increment extends C to four complete Turns",
        async || complete_turns(&reader, &actor, 4).await,
    )
    .await;
    legacy::verify(&survivor, &reader, &actor, &expected).await;
    eprintln!(
        "Local startup upgrade preserved B's pending increment: {} bytes vs {} byte full replica",
        upload.update.len(),
        full_bytes
    );
    close(survivor).await;
    close(reader).await;
    hub.stop();
    http.abort();
}
