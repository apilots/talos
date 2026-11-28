use std::fs;

use talos_config::{Config, TalosPaths, WorkspaceDefinition, DEFAULT_WORKSPACE_ID};

use super::*;

#[tokio::test]
async fn startup_runs_versioned_product_tasks_in_real_migration_order() {
    let temp = tempfile::tempdir().expect("temp Talos home");
    let paths = TalosPaths::from_home(temp.path().to_path_buf());
    let config = Config::default()
        .with_runtime_paths(paths.clone())
        .with_workspace_definition(WorkspaceDefinition::new(
            DEFAULT_WORKSPACE_ID,
            "workspace-talos",
        ));
    let legacy_user = paths
        .legacy_workspace_dir_for_agent(&config.agent.name)
        .join("users/alice");
    fs::create_dir_all(legacy_user.join("memory")).unwrap();
    fs::write(
        legacy_user.join("memory/2026-07-17.md"),
        "## Legacy memory\n\nUser-authored body.\n",
    )
    .unwrap();
    let migrations =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../../conf/migrations/sqlite");
    let storage = Arc::new(
        SqliteStorage::new("sqlite::memory:")
            .await
            .expect("connect sqlite"),
    );

    let error = run_startup_upgrades_for_version(
        StartupUpgradeParams {
            config: &config,
            storage: &storage,
            database_migration_dir: &migrations,
        },
        "0.1.7",
    )
    .await
    .expect_err("0.1.7 must not expose the new layout before its task runs");
    assert!(error.is_code(talos_error::ErrorCode::InvalidRequest));
    let converted_legacy = fs::read_to_string(legacy_user.join("memory/2026-07-17.md"))
        .expect("read Dream Memory converted by 0.1.7 task");
    assert!(converted_legacy.contains("talos:dream-memory"));
    assert!(!paths.workspace_prefix().join("alice").exists());

    let report = run_startup_upgrades_for_version(
        StartupUpgradeParams {
            config: &config,
            storage: &storage,
            database_migration_dir: &migrations,
        },
        "0.1.8-SNAPSHOT",
    )
    .await
    .expect("complete 0.1.8 workspace upgrade during development");

    assert_eq!(
        (
            report.database.applied_versions,
            report.product.previous_version,
            report.product.current_version,
            report.product.applied,
            report.product.skipped,
        ),
        (
            Vec::<i64>::new(),
            "0.1.7".to_string(),
            "0.1.8-SNAPSHOT".to_string(),
            vec![
                "202608290001_workspace_layout_v1".to_string(),
                "202609140001_history_repair_0_1_8".to_string()
            ],
            vec!["202608290002_dream_memory_okf".to_string()],
        )
    );
    let converted = fs::read_to_string(report.workspace_prefix.join("alice/memory/2026-07-17.md"))
        .expect("read migrated Dream Memory");
    assert!(converted.contains("talos:dream-memory"));
    assert!(!paths
        .legacy_workspace_dir_for_agent(&config.agent.name)
        .join("users")
        .exists());
}

#[tokio::test]
async fn pending_legacy_artifact_repair_uses_canonical_waiting_action_after_private_table_removal()
{
    use talos_agent_api::{AgentEventPayload, AgentPendingInput, AgentTurnStarted};
    use talos_agent_kernel::{
        EventAuthor, ExecutionBranch, KernelTranscriptEvent, KernelTranscriptPayload,
        KernelTranscriptUserActionRequestPayload,
    };
    use talos_types::{PendingUserAction, QuestionParams, UserActionParams, UserActionState};
    let temp = tempfile::tempdir().unwrap();
    let paths = TalosPaths::from_home(temp.path().to_path_buf());
    let config = Config::default()
        .with_runtime_paths(paths.clone())
        .with_workspace_definition(WorkspaceDefinition::new(
            DEFAULT_WORKSPACE_ID,
            "workspace-talos",
        ));
    let migrations =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../../conf/migrations/sqlite");
    let storage = Arc::new(
        SqliteStorage::new_with_max_connections(
            temp.path().join("product.sqlite").to_str().unwrap(),
            1,
        )
        .await
        .unwrap(),
    );
    let all = sqlx::migrate::Migrator::new(migrations.clone())
        .await
        .unwrap();
    let prior = sqlx::migrate::Migrator {
        migrations: std::borrow::Cow::Owned(
            all.iter()
                .filter(|entry| entry.version < 202611280007)
                .cloned()
                .collect(),
        ),
        ..sqlx::migrate::Migrator::DEFAULT
    };
    prior.run(storage.pool()).await.unwrap();
    sqlx::query("INSERT INTO sessions(session_key,agent_id,workspace_id,user_id,source_type,source_provider,execution_node_id,created_at,last_activity,environment_json,conf_json,usage_json) VALUES ('session','default',?,'alice','system','system','embedded',1,1,'null',?,?)")
        .bind(DEFAULT_WORKSPACE_ID).bind(serde_json::to_string(&talos_types::SessionConf::default()).unwrap()).bind(serde_json::to_string(&talos_types::SessionUsage::default()).unwrap()).execute(storage.pool()).await.unwrap();
    let params = UserActionParams::Question(QuestionParams {
        header: "Continue".into(),
        question: "How should I continue?".into(),
        options: vec![],
    });
    let waiting = AgentEventPayload::WaitingForInput(AgentPendingInput::UserAction {
        action: PendingUserAction {
            action_id: "action-1".into(),
            call_id: "call-1".into(),
            item_id: None,
            state: UserActionState::Pending,
            params: params.clone(),
        },
    });
    for (sequence, payload) in [
        (1, AgentEventPayload::TurnStarted(AgentTurnStarted {})),
        (2, waiting.clone()),
    ] {
        sqlx::query("INSERT INTO agent_events(session_id,turn_id,sequence,occurred_at,persisted_at,payload_json,history_generation) VALUES ('session','turn',?,1,1,?,0)").bind(sequence).bind(serde_json::to_string(&payload).unwrap()).execute(storage.pool()).await.unwrap();
    }
    sqlx::query("INSERT INTO agent_active_turns(session_id,turn_id,state,last_event_seq,updated_at) VALUES ('session','turn','waiting_for_input',2,1)").execute(storage.pool()).await.unwrap();
    sqlx::query("INSERT INTO agent_continuation_bindings VALUES ('session','turn','action-1','{\"opaqueId\":\"obsolete-private-key\"}',2,'waiting',NULL)").execute(storage.pool()).await.unwrap();
    let request: KernelTranscriptUserActionRequestPayload = serde_json::from_value(serde_json::json!({
        "action_id":"action-1","call_id":"call-1","tool_name":"AskUserQuestion","arguments":{},"params":params,"sandbox_request_fingerprint":null,"reasoning_continuation":null,
        "tool_calls":[{"id":"call-1","name":"AskUserQuestion","arguments":{"question":"How should I continue?"}}],"tool_messages":[],"deferred_tool_calls":[]
    })).unwrap();
    let transcript = KernelTranscriptEvent::new(
        "session",
        "invocation",
        ExecutionBranch::ReactiveLoop,
        EventAuthor::Agent,
        KernelTranscriptPayload::UserActionRequest { payload: request },
    );
    let complete = serde_json::json!({"kind":"transcript","record":transcript});
    let mut incomplete = complete.clone();
    incomplete["record"]["payload"]["payload"]
        .as_object_mut()
        .unwrap()
        .remove("tool_calls");
    let directory = config
        .resolve_workspace(DEFAULT_WORKSPACE_ID, "alice")
        .unwrap()
        .root
        .join("sessions/session");
    fs::create_dir_all(&directory).unwrap();
    let artifact = directory.join("transcript.jsonl");
    let broken = format!("{incomplete}\n");
    fs::write(&artifact, &broken).unwrap();
    let error = run_startup_upgrades_for_version(
        StartupUpgradeParams {
            config: &config,
            storage: &storage,
            database_migration_dir: &migrations,
        },
        "0.1.8-SNAPSHOT",
    )
    .await
    .unwrap_err();
    assert!(error.is_code(talos_error::ErrorCode::InternalError));
    assert_eq!(fs::read_to_string(&artifact).unwrap(), broken);
    let removed: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM sqlite_master WHERE name='agent_continuation_bindings'",
    )
    .fetch_one(storage.pool())
    .await
    .unwrap();
    assert_eq!(removed, 0);
    fs::write(&artifact, format!("{complete}\n")).unwrap();
    let report = run_startup_upgrades_for_version(
        StartupUpgradeParams {
            config: &config,
            storage: &storage,
            database_migration_dir: &migrations,
        },
        "0.1.8-SNAPSHOT",
    )
    .await
    .unwrap();
    assert!(report
        .product
        .applied
        .contains(&"202609140001_history_repair_0_1_8".to_owned()));
    let repaired: serde_json::Value =
        serde_json::from_str(fs::read_to_string(&artifact).unwrap().trim()).unwrap();
    assert_eq!(repaired["schema_version"], 2);
    assert_eq!(repaired["record"], complete["record"]);
    let canonical: String =
        sqlx::query_scalar("SELECT payload_json FROM visible_agent_events WHERE event_seq=2")
            .fetch_one(storage.pool())
            .await
            .unwrap();
    assert_eq!(
        serde_json::from_str::<AgentEventPayload>(&canonical).unwrap(),
        waiting
    );
}
