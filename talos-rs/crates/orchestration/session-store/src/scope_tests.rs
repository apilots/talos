use super::*;
use std::path::Path;
use std::sync::Arc;
use talos_storage::SqliteStorage;

fn sample_context() -> TalosContext {
    TalosContext {
        source_type: TalosSourceType::Im,
        provider: TalosProvider::Telegram,
        user_id: "user-1".to_string(),
        message_id: "message-1".to_string(),
        trace_id: "trace-1".to_string(),
        channel_id: Some("group-1".to_string()),
        thread_id: Some("topic-1".to_string()),
        project_root: None,
        environment: None,
    }
}

#[test]
fn test_stable_session_key_per_channel_peer_topic() {
    let key = DatabaseSessionStore::stable_session_key(
        &sample_context(),
        "agent-a",
        "per_channel_peer_topic",
    );
    assert!(key.contains("channel:group-1"));
    assert!(key.contains("thread:topic-1"));
}

#[test]
fn test_stable_session_key_per_channel_peer() {
    let key =
        DatabaseSessionStore::stable_session_key(&sample_context(), "agent-a", "per_channel_peer");
    assert!(key.contains("channel:group-1"));
    assert!(!key.contains("thread:"));
}

#[test]
fn test_stable_session_key_main() {
    let key = DatabaseSessionStore::stable_session_key(&sample_context(), "agent-a", "main");
    assert!(!key.contains("channel:"));
    assert!(!key.contains("thread:"));
    assert!(key.contains("user:user-1"));
}

#[test]
fn test_new_session_id_is_uuid_v7() {
    let session_id = DatabaseSessionStore::new_session_id();
    let parsed = uuid::Uuid::parse_str(&session_id).expect("must be valid uuid");
    assert_eq!(parsed.get_version_num(), 7);
}

#[test]
fn test_new_session_id_is_unique() {
    let id1 = DatabaseSessionStore::new_session_id();
    let id2 = DatabaseSessionStore::new_session_id();
    assert_ne!(id1, id2);
}

#[test]
fn test_normalized_optional_string_treats_blank_as_none() {
    assert_eq!(DatabaseSessionStore::normalized_optional_string(None), None);
    assert_eq!(
        DatabaseSessionStore::normalized_optional_string(Some(String::new())),
        None
    );
    assert_eq!(
        DatabaseSessionStore::normalized_optional_string(Some("   ".to_string())),
        None
    );
    assert_eq!(
        DatabaseSessionStore::normalized_optional_string(Some(" project-1 ".to_string())),
        Some("project-1".to_string())
    );
}

#[tokio::test]
async fn test_record_session_progress_returns_not_found_for_missing_session() {
    let storage = Arc::new(SqliteStorage::new("sqlite::memory:").await.unwrap());
    let store = DatabaseSessionStore::new(storage).await.unwrap();

    let result = store
        .record_session_progress("missing-session", 1, &TalosUsage::default(), None)
        .await;

    assert!(matches!(
        result,
        Err(talos_error::TalosError::ResourceNotFound(message))
        if message == "session missing-session"
    ));
}

#[tokio::test]
async fn test_create_session_sets_default_title() {
    let storage = Arc::new(SqliteStorage::new("sqlite::memory:").await.unwrap());
    let store = DatabaseSessionStore::new(storage).await.unwrap();

    let session = store
        .create_session(
            None,
            &sample_context(),
            "agent-a",
            "qwen-max",
            None,
            None,
            None,
            None,
            None,
        )
        .await
        .expect("create session should succeed");

    assert_eq!(session.title, DEFAULT_SESSION_TITLE);

    let loaded = store
        .get_session_record(&session.id)
        .await
        .expect("load session should succeed")
        .expect("session should exist");
    assert_eq!(loaded.title, DEFAULT_SESSION_TITLE);
}

#[tokio::test]
async fn test_reset_session_record_clears_title_message_count_and_usage() {
    let storage = Arc::new(SqliteStorage::new("sqlite::memory:").await.unwrap());
    let store = DatabaseSessionStore::new(storage).await.unwrap();

    let session = store
        .create_session(
            None,
            &sample_context(),
            "agent-a",
            "qwen-max",
            Some("gpt-5.3"),
            None,
            None,
            None,
            None,
        )
        .await
        .expect("create session should succeed");
    store
        .update_session_title(&session.id, "Custom Title")
        .await
        .expect("title update should succeed");
    store
        .record_session_progress(
            &session.id,
            4,
            &TalosUsage {
                input_tokens: 11,
                output_tokens: 7,
                total_tokens: 18,
                cache_creation_input_tokens: Some(3),
                cache_read_input_tokens: Some(5),
            },
            Some("gpt-5.4"),
        )
        .await
        .expect("progress update should succeed");

    let ack = store
        .reset_session_record(&session.id)
        .await
        .expect("reset should succeed");
    assert!(ack.success);

    let loaded = store
        .get_session_record(&session.id)
        .await
        .expect("load session should succeed")
        .expect("session should exist");
    assert_eq!(loaded.title, DEFAULT_SESSION_TITLE);
    assert_eq!(loaded.message_count, 0);
    assert_eq!(loaded.usage, TalosUsage::default());
    assert_eq!(loaded.model, "gpt-5.4");
    assert_eq!(
        loaded.conf,
        SessionConfigOverridePolicy::single_model_conf("gpt-5.3")
    );
}

#[tokio::test]
async fn test_delete_session_record_removes_session() {
    let storage = Arc::new(SqliteStorage::new("sqlite::memory:").await.unwrap());
    let store = DatabaseSessionStore::new(storage).await.unwrap();

    let session = store
        .create_session(
            None,
            &sample_context(),
            "agent-a",
            "qwen-max",
            None,
            None,
            None,
            None,
            None,
        )
        .await
        .expect("create session should succeed");

    let ack = store
        .delete_session_record(&session.id)
        .await
        .expect("delete should succeed");
    assert!(ack.success);

    let loaded = store
        .get_session_record(&session.id)
        .await
        .expect("load session should succeed");
    assert!(loaded.is_none());
}

#[tokio::test]
async fn test_resolve_or_create_session_preserves_existing_project_binding_fields() {
    let storage = Arc::new(SqliteStorage::new("sqlite::memory:").await.unwrap());
    let store = DatabaseSessionStore::new(storage).await.unwrap();

    let initial = store
        .resolve_or_create_session(
            &sample_context(),
            "agent-a",
            "qwen-max",
            None,
            Some("per_channel_peer_topic".to_string()),
            Some("project-1".to_string()),
            Some(PathBuf::from("/repo/one")),
        )
        .await
        .expect("initial resolve_or_create should succeed");

    let resolved = store
        .resolve_or_create_session(
            &sample_context(),
            "agent-a",
            "qwen-max",
            None,
            Some("per_channel_peer_topic".to_string()),
            Some("project-2".to_string()),
            Some(PathBuf::from("/repo/two")),
        )
        .await
        .expect("second resolve_or_create should succeed");

    assert_eq!(resolved.id, initial.id);
    assert_eq!(resolved.project_id.as_deref(), Some("project-1"));
    assert_eq!(
        resolved.current_directory.as_deref(),
        Some(Path::new("/repo/one"))
    );

    let persisted = store
        .get_session_record(&initial.id)
        .await
        .expect("load session should succeed")
        .expect("session should exist");
    assert_eq!(persisted.project_id.as_deref(), Some("project-1"));
    assert_eq!(
        persisted.current_directory.as_deref(),
        Some(Path::new("/repo/one"))
    );
}

#[tokio::test]
async fn test_list_session_records_filters_by_project_id() {
    let storage = Arc::new(SqliteStorage::new("sqlite::memory:").await.unwrap());
    let store = DatabaseSessionStore::new(storage).await.unwrap();

    let first = store
        .create_session(
            None,
            &sample_context(),
            "agent-a",
            "qwen-max",
            None,
            None,
            None,
            Some("project-1".to_string()),
            None,
        )
        .await
        .expect("create first session should succeed");
    let second = store
        .create_session(
            None,
            &sample_context(),
            "agent-a",
            "qwen-max",
            None,
            None,
            None,
            Some("project-2".to_string()),
            None,
        )
        .await
        .expect("create second session should succeed");

    let records = store
        .list_session_records(&SessionQuery {
            user_id: None,
            channel_id: None,
            provider: None,
            project_id: Some("project-1".to_string()),
            limit: None,
            offset: None,
        })
        .await
        .expect("list sessions should succeed");

    assert_eq!(records.len(), 1);
    assert_eq!(records[0].id, first.id);
    assert_ne!(records[0].id, second.id);
}

#[tokio::test]
async fn test_list_session_records_respects_limit_and_offset() {
    let storage = Arc::new(SqliteStorage::new("sqlite::memory:").await.unwrap());
    let store = DatabaseSessionStore::new(storage).await.unwrap();

    let first = store
        .create_session(
            None,
            &sample_context(),
            "agent-a",
            "qwen-max",
            None,
            None,
            None,
            None,
            None,
        )
        .await
        .expect("create first session should succeed");
    tokio::time::sleep(std::time::Duration::from_millis(2)).await;
    let second = store
        .create_session(
            None,
            &sample_context(),
            "agent-a",
            "qwen-max",
            None,
            None,
            None,
            None,
            None,
        )
        .await
        .expect("create second session should succeed");
    tokio::time::sleep(std::time::Duration::from_millis(2)).await;
    let third = store
        .create_session(
            None,
            &sample_context(),
            "agent-a",
            "qwen-max",
            None,
            None,
            None,
            None,
            None,
        )
        .await
        .expect("create third session should succeed");

    let records = store
        .list_session_records(&SessionQuery {
            user_id: None,
            channel_id: None,
            provider: None,
            project_id: None,
            limit: Some(1),
            offset: Some(1),
        })
        .await
        .expect("list paged sessions should succeed");

    assert_eq!(records.len(), 1);
    assert_eq!(records[0].id, second.id);
    assert_ne!(records[0].id, first.id);
    assert_ne!(records[0].id, third.id);
    assert_eq!(
        store
            .count_session_records(&SessionQuery {
                user_id: None,
                channel_id: None,
                provider: None,
                project_id: None,
                limit: Some(1),
                offset: Some(1),
            })
            .await
            .expect("count sessions should succeed"),
        3
    );
}

#[tokio::test]
async fn test_list_projects_returns_session_count() {
    let storage = Arc::new(SqliteStorage::new("sqlite::memory:").await.unwrap());
    let store = DatabaseSessionStore::new(storage).await.unwrap();

    let project = store
        .create_project("repo", &PathBuf::from("/repo"))
        .await
        .expect("create project should succeed");

    store
        .create_session(
            None,
            &sample_context(),
            "agent-a",
            "qwen-max",
            None,
            None,
            None,
            Some(project.project_id.clone()),
            None,
        )
        .await
        .expect("create bound session should succeed");

    let projects = store
        .list_projects()
        .await
        .expect("list projects should succeed");
    assert_eq!(projects.len(), 1);
    assert_eq!(projects[0].project_id, project.project_id);
    assert_eq!(projects[0].session_count, 1);

    let loaded = store
        .get_project(&project.project_id)
        .await
        .expect("get project should succeed")
        .expect("project should exist");
    assert_eq!(loaded.session_count, 1);
}
