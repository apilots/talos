use super::*;
use std::{
    path::{Path, PathBuf},
    sync::atomic::{AtomicUsize, Ordering},
    time::Duration,
};
use talos_automation::{AutomationJob, AutomationJobControl};
use talos_config::{Config, TalosPaths};
use talos_service::automation::native_jobs::NativeJobService;
use talos_storage::SqliteStorage;
use talos_store::{
    automation::native_jobs::DatabaseNativeJobControlStore,
    session::{DatabaseSessionStore, SessionRecord, SessionStore, SessionTreeIdentity},
};
use talos_types::{
    talos::{SessionSourceProvider, SessionSourceType},
    BuiltinJobControlRequest, BuiltinJobListRequest, SessionConf, SessionUsage,
};

const OWNER: &str = "native-job-owner";
const FOREIGN: &str = "another-owner";
const CRON: &str = "0 0 0 1 1 *";

#[path = "application_native_job_identity_tests.rs"]
mod identity_tests;

fn migrations() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../../conf/migrations/sqlite")
}

fn assert_code<T>(result: TalosResult<T>, code: ErrorCode) {
    let error = result.err().expect("operation must be rejected");
    assert!(error.is_code(code), "unexpected error: {error}");
}

async fn storage() -> Arc<SqliteStorage> {
    let storage = Arc::new(
        SqliteStorage::new_with_max_connections(":memory:", 1)
            .await
            .unwrap(),
    );
    talos_store::migration::run_migrations(storage.pool(), &migrations())
        .await
        .unwrap();
    storage
}

async fn insert_session(storage: Arc<SqliteStorage>, owner: &str) {
    let store = DatabaseSessionStore::new(storage).await.unwrap();
    store
        .insert_session_record(&SessionRecord {
            session_kind: talos_types::SessionKind::Standard,
            id: format!("session-{owner}"),
            agent_id: "talos".into(),
            execution_node_id: "embedded".into(),
            tree_identity: SessionTreeIdentity::Primary,
            workspace_id: "default".into(),
            title: "Native maintenance scope".into(),
            user_id: owner.into(),
            source_type: SessionSourceType::System,
            source_provider: SessionSourceProvider::System,
            channel_id: None,
            thread_id: None,
            environment: None,
            created_at: 1,
            last_activity: 1,
            message_count: 0,
            latest_turn_seq: 0,
            memory_processed_until_event_seq: 0,
            usage: SessionUsage::default(),
            conf: SessionConf::default(),
            project_id: None,
            worktree_binding: None,
            current_directory: None,
            pinned: false,
        })
        .await
        .unwrap();
}

async fn insert_usage(storage: &dyn Storage, owner: &str, kind: &str, finalized: &str) {
    let request = uuid::Uuid::new_v4().to_string();
    // Retention also sees orphaned and non-Turn usage. No Session join may hide it.
    storage
        .execute(
            "INSERT INTO profile_turn_usage (user_id, usage_kind, session_id, turn_id, \
             last_request_id, model_provider, model_id, input_tokens, output_tokens, \
             total_tokens, usage_available, finalized, timezone, completed_at_ms) \
             VALUES (?, ?, 'deleted-session', ?, ?, 'fixture', 'fixture', 1, 1, 2, 1, ?, 'UTC', 1)",
            &[owner, kind, &request, &request, finalized],
        )
        .await
        .unwrap();
}

#[tokio::test]
async fn only_open_mode_accepts_local_anonymous_aliases() {
    let storage = storage().await;
    insert_session(storage.clone(), "anonymous").await;
    insert_usage(storage.as_ref(), "embedded", "turn", "1").await;
    verify_anonymous_data(storage.as_ref(), &Actor::auth_disabled())
        .await
        .unwrap();
    insert_usage(storage.as_ref(), FOREIGN, "memory", "1").await;
    assert_code(
        verify_anonymous_data(storage.as_ref(), &Actor::auth_disabled()).await,
        ErrorCode::PermissionDenied,
    );
}

fn controlled_job(id: BuiltinJobId, calls: Arc<AtomicUsize>) -> AutomationJob {
    AutomationJob::cron(
        id.as_str(),
        CRON,
        talos_config::parse_timezone("UTC").unwrap(),
        move || {
            let calls = calls.clone();
            async move {
                calls.fetch_add(1, Ordering::SeqCst);
                Ok(())
            }
        },
    )
    .with_control(AutomationJobControl {
        paused: true,
        run_once: true,
    })
}

struct HostFixture {
    runtime: crate::TalosApplicationRuntime,
    storage: Arc<SqliteStorage>,
    host: Arc<ApplicationNativeJobHost>,
    calls: Arc<AtomicUsize>,
    _home: tempfile::TempDir,
}

impl HostFixture {
    async fn start() -> Self {
        let home = tempfile::tempdir().unwrap();
        let paths = TalosPaths::from_home(home.path().into());
        paths.ensure_base_dirs().unwrap();
        let mut config = Config::default().with_runtime_paths(paths);
        config.apply_derived_paths();
        config.gateway.auth = true;
        config.sync.enabled = Some(false);
        config.agent.kernel.tools.enable_mcp_tools = false;
        config.automation.dream_memory_maintenance.cron = CRON.into();
        config.llm.anthropic.models = vec![talos_config::ProviderModelConfig {
            id: config.llm.model.clone(),
            name: "Native control fixture".into(),
            context_window: 32768,
            reasoning: false,
            reasoning_efforts: None,
            reasoning_effort: Default::default(),
            reasoning_replay: Default::default(),
            request_parameters: Default::default(),
            speed: Default::default(),
            input: Vec::new(),
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
        let runtime = crate::build_application_runtime_with_migration_dir(config, &migrations())
            .await
            .unwrap();
        // Keep real host ownership and its database, but never run real maintenance on fixtures.
        runtime.automation.shutdown().await.unwrap();
        *runtime.host.local_owner.write().unwrap() = Some(OWNER.into());
        let storage = crate::application_storage::connect_application_storage(&runtime.config)
            .await
            .unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let automation = Automation::start(vec![
            controlled_job(BuiltinJobId::ProfileUsageRetention, calls.clone()),
            controlled_job(BuiltinJobId::DreamMemoryMaintenance, calls.clone()),
        ])
        .await
        .unwrap();
        let host = Arc::new(ApplicationNativeJobHost {
            host: Arc::downgrade(&runtime.host),
            storage: storage.clone(),
            automation,
            execution_instance_id: runtime.host_id().into(),
        });
        Self {
            runtime,
            storage,
            host,
            calls,
            _home: home,
        }
    }

    async fn close(self) {
        self.host.automation.shutdown().await.unwrap();
        self.runtime.shutdown_host().await.unwrap();
        self.runtime.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn real_host_requires_local_owner_and_exact_computer_without_admin_override() {
    let fixture = HostFixture::start().await;
    insert_session(fixture.storage.clone(), OWNER).await;
    let scope = fixture
        .host
        .resolve_owned(&Actor::regular(OWNER), "embedded")
        .await
        .unwrap();
    assert_eq!(scope.computer_id, "embedded");
    assert_eq!(scope.execution_instance_id, fixture.runtime.host_id());
    for actor in [
        Actor::regular(FOREIGN),
        Actor::regular(FOREIGN).with_role(talos_auth::UserRole::Admin),
        Actor::regular("").with_auth_disabled(true),
    ] {
        assert_code(
            fixture.host.resolve_owned(&actor, "embedded").await,
            ErrorCode::PermissionDenied,
        );
    }
    assert_code(
        fixture
            .host
            .resolve_owned(&Actor::regular(OWNER), "another-computer")
            .await,
        ErrorCode::PermissionDenied,
    );
    let unavailable = ApplicationNativeJobHost {
        host: Weak::new(),
        storage: fixture.storage.clone(),
        automation: fixture.host.automation.clone(),
        execution_instance_id: fixture.runtime.host_id().into(),
    };
    assert_code(
        unavailable
            .resolve_owned(&Actor::regular(OWNER), "embedded")
            .await,
        ErrorCode::ServiceUnavailable,
    );
    fixture.close().await;
}

#[tokio::test]
async fn execution_host_owner_can_manage_existing_jobs_with_historical_data() {
    let fixture = HostFixture::start().await;
    insert_session(fixture.storage.clone(), "anonymous").await;
    insert_usage(fixture.storage.as_ref(), FOREIGN, "memory", "1").await;
    let controls = Arc::new(DatabaseNativeJobControlStore::new(
        fixture.storage.pool().clone(),
    ));
    let host: Arc<dyn NativeJobHost> = fixture.host.clone();
    let service = NativeJobService::new(controls, Arc::downgrade(&host));
    let owner = Actor::regular(OWNER);
    let request = BuiltinJobListRequest {
        computer_id: "embedded".into(),
    };
    let rows = service.list(&owner, request.clone()).await.unwrap().data;
    let job = rows
        .iter()
        .find(|job| job.id == BuiltinJobId::ProfileUsageRetention)
        .unwrap();
    assert_eq!(
        job.available_actions,
        [BuiltinJobAction::Resume, BuiltinJobAction::RunOnce],
    );
    for actor in [
        Actor::regular(FOREIGN),
        Actor::regular(FOREIGN).with_role(talos_auth::UserRole::Admin),
        Actor::auth_disabled(),
    ] {
        assert_code(
            service.list(&actor, request.clone()).await,
            ErrorCode::PermissionDenied,
        );
    }
    let control = |action| BuiltinJobControlRequest {
        computer_id: job.computer_id.clone(),
        execution_instance_id: job.execution_instance_id.clone(),
        job_id: job.id,
        action,
    };
    let mut remote = control(BuiltinJobAction::RunOnce);
    remote.computer_id = "another-computer".into();
    assert_code(
        service.control(&owner, remote).await,
        ErrorCode::PermissionDenied,
    );
    let mut stale = control(BuiltinJobAction::RunOnce);
    stale.execution_instance_id = "previous-instance".into();
    assert_code(service.control(&owner, stale).await, ErrorCode::Conflict);
    assert_eq!(fixture.calls.load(Ordering::SeqCst), 0);

    let resumed = service
        .control(&owner, control(BuiltinJobAction::Resume))
        .await
        .unwrap();
    assert!(!resumed.paused);
    let paused = service
        .control(&owner, control(BuiltinJobAction::Pause))
        .await
        .unwrap();
    assert!(paused.paused);
    service
        .control(&owner, control(BuiltinJobAction::RunOnce))
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(3), async {
        while fixture.calls.load(Ordering::SeqCst) == 0 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    fixture.close().await;
}

#[tokio::test]
async fn runtime_projection_tracks_actual_claims_outcomes_and_hides_internal_scan() {
    let entered = Arc::new(tokio::sync::Notify::new());
    let release = Arc::new(tokio::sync::Notify::new());
    let timezone = talos_config::parse_timezone("UTC").unwrap();
    let automation = Automation::start(vec![
        AutomationJob::cron(
            BuiltinJobId::ProfileUsageRetention.as_str(),
            CRON,
            timezone,
            {
                let entered = entered.clone();
                let release = release.clone();
                move || {
                    let entered = entered.clone();
                    let release = release.clone();
                    async move {
                        entered.notify_one();
                        release.notified().await;
                        Ok(())
                    }
                }
            },
        )
        .with_control(AutomationJobControl {
            paused: true,
            run_once: true,
        }),
        AutomationJob::cron(
            BuiltinJobId::DreamMemoryMaintenance.as_str(),
            CRON,
            timezone,
            || async {
                Err(TalosError::new(
                    ErrorCode::Conflict,
                    "controlled callback failure",
                ))
            },
        )
        .with_control(AutomationJobControl {
            paused: true,
            run_once: true,
        }),
        AutomationJob::cron("session-loop-scan", CRON, timezone, || async { Ok(()) }),
    ])
    .await
    .unwrap();
    let runtime = ApplicationNativeJobRuntime {
        automation: automation.clone(),
        computer_id: "actual-computer".into(),
        execution_instance_id: "actual-instance".into(),
    };
    let initial = runtime.list().await.unwrap();
    assert_eq!(initial.len(), 2);
    assert!(initial
        .iter()
        .all(|job| job.computer_id == "actual-computer"
            && job.execution_instance_id == "actual-instance"
            && job.timezone == "UTC"
            && job.paused
            && !job.running
            && job.next_due_at.is_none()
            && job.last_finished_at.is_none()
            && job.last_outcome.is_none()
            && job.available_actions == [BuiltinJobAction::Resume, BuiltinJobAction::RunOnce]));
    runtime
        .run_once(BuiltinJobId::ProfileUsageRetention)
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(3), entered.notified())
        .await
        .unwrap();
    let running = runtime
        .list()
        .await
        .unwrap()
        .into_iter()
        .find(|job| job.id == BuiltinJobId::ProfileUsageRetention)
        .unwrap();
    assert!(running.running && running.paused && running.last_outcome.is_none());
    assert_eq!(running.available_actions, [BuiltinJobAction::Resume]);
    assert_code(
        runtime.run_once(BuiltinJobId::ProfileUsageRetention).await,
        ErrorCode::Conflict,
    );
    runtime
        .run_once(BuiltinJobId::DreamMemoryMaintenance)
        .await
        .unwrap();
    release.notify_one();
    let final_jobs = tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            let jobs = runtime.list().await.unwrap();
            if jobs
                .iter()
                .all(|job| !job.running && job.last_finished_at.is_some())
            {
                break jobs;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert!(final_jobs
        .iter()
        .all(|job| job.paused && !job.running && job.last_finished_at.is_some()));
    assert_eq!(
        final_jobs[0].last_outcome,
        Some(BuiltinJobOutcome::Succeeded)
    );
    assert!(
        matches!(&final_jobs[1].last_outcome, Some(BuiltinJobOutcome::Failed { error })
        if error.code == ErrorCode::Conflict && error.message == "controlled callback failure")
    );
    automation.shutdown().await.unwrap();
}
