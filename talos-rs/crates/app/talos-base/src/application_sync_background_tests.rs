use super::*;
use std::{path::Path, time::Duration};
use talos_config::{Config, TalosPaths};

#[tokio::test]
async fn account_maintenance_reuses_auth_context_and_records_owned_lifetime() {
    use axum::{extract::State, routing::post, Json, Router};
    use serde_json::{json, Value};
    let home = tempfile::tempdir().unwrap();
    let log_file = home.path().join("lifecycle.log");
    let logging = talos_logging::init_with_config(talos_logging::LoggingConfig {
        file: Some(log_file.to_string_lossy().into()),
        stdout: false,
        ..Default::default()
    })
    .unwrap();
    let (requests, mut observed) = tokio::sync::mpsc::unbounded_channel();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let mut config = snapshot_config(home.path());
    config.cloud.api_base_url = format!("http://{}", listener.local_addr().unwrap());
    // A regular central-server account has no Worker to enroll and H is disabled:
    // successful initialization is observable without any external dependencies.
    config.sync.enabled = Some(false);
    config.agent_node.control_plane = Some(talos_config::AgentNodeControlConfig {
        bind_address: "127.0.0.1:0".into(),
        tls_certificate_file: home.path().join("unused.crt"),
        tls_private_key_file: home.path().join("unused.key"),
        trusted_node_ca_file: None,
    });
    let mut persisted = serde_json::to_value(&config).unwrap();
    persisted["logging"].as_object_mut().unwrap().remove("file");
    persisted["storage"]
        .as_object_mut()
        .unwrap()
        .remove("connection_string");
    std::fs::write(
        config.source_path(),
        serde_json::to_vec(&persisted).unwrap(),
    )
    .unwrap();
    let app = Router::new().route("/v1/auth/introspect", post(
        |State(requests): State<tokio::sync::mpsc::UnboundedSender<String>>, Json(request): Json<Value>| async move {
            requests.send(request["token"].as_str().unwrap().into()).unwrap();
            Json(json!({"active": request["token"] != "rejected-credential",
                "user":{"user_id":"idle-account","email":"test@example.invalid","username":"test","role":"user","type":"free","status":"active"},
                "capabilities":{"is_admin":false,"role":"user","scopes":[]},
                "entitlement":{"can_use":true,"plan":"free","reason":null},"expires_at_ms":null}))
        })).with_state(requests);
    let server = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    let runtime = crate::build_application_runtime_with_migration_dir(
        config,
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../../conf/migrations/sqlite"),
    )
    .await
    .unwrap();
    let actor = Actor::regular("idle-account").with_access_token("initial-credential");
    runtime.service.resume_account_sync(&actor);
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(2), observed.recv())
            .await
            .unwrap()
            .unwrap(),
        "initial-credential"
    );
    for _ in 0..20 {
        runtime.service.resume_account_sync(&actor);
        runtime.service.sync_status(&actor).await.unwrap();
    }
    assert!(
        tokio::time::timeout(Duration::from_millis(200), observed.recv())
            .await
            .is_err(),
        "repeated status/restoration calls must reuse Auth's verified context"
    );
    let refreshed = Actor::regular(&actor.user_id).with_access_token("new-credential");
    runtime.service.resume_account_sync(&refreshed);
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(2), observed.recv())
            .await
            .unwrap()
            .unwrap(),
        "new-credential"
    );
    // Lifecycle events reuse Auth's verified context instead of revalidating Cloud.
    runtime.sync_host.notify_refresh(&actor.user_id);
    runtime.sync_host.worker_failures.send_replace(None);
    let rejected = Actor::regular(&actor.user_id).with_access_token("rejected-credential");
    runtime.service.resume_account_sync(&rejected);
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(2), observed.recv())
            .await
            .unwrap()
            .unwrap(),
        "rejected-credential"
    );
    assert!(tokio::time::timeout(Duration::from_secs(6), observed.recv()).await.is_err(),
        "known authentication/permission rejection must await changed credentials or account state, without polling");
    runtime.service.resume_account_sync(&refreshed);
    assert!(
        tokio::time::timeout(Duration::from_millis(200), observed.recv())
            .await
            .is_err(),
        "restoration and lifecycle events reuse an existing verified context"
    );
    runtime.shutdown_sync().await.unwrap();
    runtime.worker_rpc.shutdown().await.unwrap();
    runtime.resource_rpc.shutdown().await.unwrap();
    runtime.automation.shutdown().await.unwrap();
    runtime.worker.shutdown().await.unwrap();
    runtime.auth_service.shutdown().await;
    server.abort();
    drop(logging);
    let log = std::fs::read_to_string(log_file).unwrap();
    assert_eq!(log.matches("Sync account maintenance started").count(), 1);
    assert_eq!(log.matches("Sync account maintenance stopped").count(), 1);
    assert!(log.contains("Sync account initialized; waiting for lifecycle changes"));
    assert_eq!(
        log.matches("Cloud authorization refresh started").count(),
        2
    );
    assert_eq!(
        log.matches("Cloud authorization refresh stopped").count(),
        2
    );
    assert!(log.contains("sync_account_initialization_failed"));
    assert!(log.contains("application failure"));
    assert!(
        !log.contains("initial-credential")
            && !log.contains("new-credential")
            && !log.contains("rejected-credential")
    );
}

#[tokio::test]
async fn status_snapshots_bypass_busy_lifecycles_and_maintenance_is_owned_and_coalesced() {
    let home = tempfile::Builder::new()
        .prefix(".talos-sync-status-")
        .tempdir_in(std::env::var_os("HOME").unwrap())
        .unwrap();
    let config = snapshot_config(home.path());
    let runtime = crate::build_application_runtime_with_migration_dir(
        config.clone(),
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../../conf/migrations/sqlite"),
    )
    .await
    .unwrap();
    let actor = Actor::regular("snapshot-account").with_access_token("fixture-token");
    let host = &runtime.sync_host;
    let lifecycle = host.lifecycle.lock().await;
    let retirement = host.registration_retirement.lock().await;
    let before = std::fs::read(config.source_path()).unwrap();
    let first = tokio::time::timeout(Duration::from_secs(1), runtime.service.sync_status(&actor))
        .await
        .expect("snapshot must not wait for lifecycle or retirement")
        .unwrap();
    assert!(host.background.lock().unwrap().tasks.is_empty());
    runtime.service.resume_account_sync(&actor);
    let supervisor_id = host.background.lock().unwrap().tasks[&actor.user_id]
        .task
        .id();
    for _ in 0..20 {
        runtime.service.resume_account_sync(&actor);
        let snapshot =
            tokio::time::timeout(Duration::from_secs(1), runtime.service.sync_status(&actor))
                .await
                .expect("polls must return while background work is stalled")
                .unwrap();
        assert_eq!(snapshot, first);
    }
    assert_eq!(host.background.lock().unwrap().tasks.len(), 1);
    let refreshed = Actor::regular(&actor.user_id).with_access_token("refreshed-fixture-token");
    runtime.service.resume_account_sync(&refreshed);
    {
        let background = host.background.lock().unwrap();
        let refresh = &background.tasks[&actor.user_id];
        assert_eq!(refresh.task.id(), supervisor_id);
        assert_eq!(refresh.actor.borrow().access_token, refreshed.access_token);
    }
    assert_eq!(std::fs::read(config.source_path()).unwrap(), before);
    assert!(runtime.sync.saved_bindings().await.unwrap().is_empty());
    let pause = tokio::time::timeout(Duration::from_secs(1), host.pause_refresh(&actor.user_id))
        .await
        .expect("cancelling queued maintenance must not wait for lifecycle");
    for _ in 0..20 {
        runtime.service.resume_account_sync(&actor);
    }
    assert!(host.background.lock().unwrap().tasks.is_empty());
    drop(pause);
    assert!(host.background.lock().unwrap().tasks.is_empty());
    runtime.service.resume_account_sync(&actor);
    assert_eq!(host.background.lock().unwrap().tasks.len(), 1);
    let pause = host.suspend_refresh(&actor).await;
    runtime.service.resume_account_sync(&actor);
    assert!(host.background.lock().unwrap().tasks.is_empty());
    drop(pause);
    runtime.service.resume_account_sync(&actor);
    assert!(
        host.background.lock().unwrap().tasks.is_empty(),
        "late authenticated responses must not undo logout"
    );
    runtime.service.resume_account_sync_after_login(&actor);
    assert_eq!(host.background.lock().unwrap().tasks.len(), 1);
    // Reproduce enrollment holding the Worker lock while a remote RPC is stalled.
    let pause = host.pause_refresh(&actor.user_id).await;
    drop(pause);
    let worker = runtime.worker_registration.clone();
    let (entered, ready) = tokio::sync::oneshot::channel();
    let task = tokio::spawn(async move {
        let _guard = worker.lock().await;
        entered.send(()).unwrap();
        std::future::pending::<()>().await;
    });
    ready.await.unwrap();
    host.background.lock().unwrap().tasks.insert(
        actor.user_id.clone(),
        RefreshTask {
            task,
            actor: tokio::sync::watch::channel(actor.clone()).0,
        },
    );
    let desktop = crate::desktop_application::DesktopApplication::new(
        &runtime,
        "shutdown fixture".into(),
        std::sync::Arc::new(talos_app_server::TalosApplicationAdapter::new(
            runtime.service.clone(),
            128,
        )),
        talos_app_server::DesktopHostBridge::new(),
    );
    use talos_app_server::AppServerApplication;
    tokio::time::timeout(
        Duration::from_secs(1),
        desktop.drain_background_tasks(Duration::from_secs(1)),
    )
    .await
    .expect("Desktop shutdown must cancel enrollment before locking its Worker")
    .unwrap();
    assert!(runtime.worker_registration.try_lock().is_ok());
    tokio::time::timeout(Duration::from_secs(1), runtime.shutdown_sync())
        .await
        .expect("shutdown must cancel maintenance before draining Sync")
        .unwrap();
    assert!(host.background.lock().unwrap().tasks.is_empty());
    assert_eq!(runtime.service.sync_status(&actor).await.unwrap(), first);
    assert!(host.background.lock().unwrap().tasks.is_empty());
    runtime.service.resume_account_sync(&actor);
    assert!(host.background.lock().unwrap().tasks.is_empty());
    drop(retirement);
    drop(lifecycle);
    runtime.worker_rpc.shutdown().await.unwrap();
    runtime.resource_rpc.shutdown().await.unwrap();
    runtime.automation.shutdown().await.unwrap();
    runtime.worker.shutdown().await.unwrap();
    drop(desktop);
    drop(runtime);

    // Cached-login startup owns initialization even when no status query occurs.
    let cache = talos_auth::CloudSessionCache::for_talos_home(home.path());
    let now = talos_auth::CloudSessionCache::now_ms();
    cache
        .store(&talos_auth::CloudSessionCacheEntry {
            schema_version: 2,
            cloud_api_base_url: config.cloud.api_base_url.clone(),
            access_token: "cached-fixture-token".into(),
            token_type: "Bearer".into(),
            expires_at_ms: Some(now + 60_000),
            last_used_at_ms: now,
            user: talos_auth::CloudSessionUserSnapshot {
                user_id: actor.user_id.clone(),
                username: "fixture".into(),
                email: None,
                role: talos_auth::UserRole::User,
                r#type: talos_auth::UserType::Free,
            },
        })
        .unwrap();
    let restarted = crate::build_application_runtime_with_migration_dir(
        config,
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../../conf/migrations/sqlite"),
    )
    .await
    .unwrap();
    assert!(restarted
        .sync_host
        .background
        .lock()
        .unwrap()
        .tasks
        .contains_key(&actor.user_id));
    restarted.shutdown_sync().await.unwrap();
    restarted.worker_rpc.shutdown().await.unwrap();
    restarted.resource_rpc.shutdown().await.unwrap();
    restarted.automation.shutdown().await.unwrap();
    restarted.worker.shutdown().await.unwrap();
}

fn snapshot_config(home: &Path) -> Config {
    let paths = TalosPaths::from_home(home.into());
    paths.ensure_base_dirs().unwrap();
    let mut config = Config::default().with_runtime_paths(paths);
    config.apply_derived_paths();
    config.gateway.auth = true;
    config.cloud.api_base_url = "http://127.0.0.1:9".into();
    config.cloud.agent_base_url = "https://127.0.0.1:9".into();
    config.agent.kernel.tools.enable_mcp_tools = false;
    config.llm.anthropic.models = vec![talos_config::ProviderModelConfig {
        id: config.llm.model.clone(),
        name: config.llm.model.clone(),
        context_window: 32768,
        reasoning: false,
        reasoning_effort: Default::default(),
        speed: Default::default(),
        input: vec![],
        cost: Default::default(),
    }];
    let mut persisted = serde_json::to_value(&config).unwrap();
    persisted["logging"].as_object_mut().unwrap().remove("file");
    persisted["storage"]
        .as_object_mut()
        .unwrap()
        .remove("connection_string");
    std::fs::write(
        config.source_path(),
        serde_json::to_vec(&persisted).unwrap(),
    )
    .unwrap();
    config
}
