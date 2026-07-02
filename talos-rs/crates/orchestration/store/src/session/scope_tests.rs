use std::path::PathBuf;
use std::sync::Arc;

use super::*;
use crate::project::{ProjectStore, StorageProjectStore};
use talos_storage::SqliteStorage;
use talos_types::talos::{TalosContext, TalosProvider, TalosSourceType, TokenUsage};
use talos_types::{SessionConf, DEFAULT_SESSION_TITLE};

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

fn record(
    id: &str,
    user_id: &str,
    project_id: Option<String>,
    current_directory: Option<PathBuf>,
) -> SessionRecord {
    let context = TalosContext {
        user_id: user_id.to_string(),
        ..sample_context()
    };
    SessionRecord {
        id: id.to_string(),
        title: DEFAULT_SESSION_TITLE.to_string(),
        user_id: user_id.to_string(),
        context,
        model: "gpt-5.3".to_string(),
        created_at: 1,
        last_activity: 1,
        message_count: 0,
        usage: TokenUsage::default(),
        conf: SessionConf::default(),
        workspace_id: Some("workspace-alpha".to_string()),
        project_id,
        current_directory,
    }
}

async fn store() -> DatabaseSessionStore {
    let storage = Arc::new(SqliteStorage::new(":memory:").await.unwrap());
    DatabaseSessionStore::new(storage).await.unwrap()
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
async fn test_insert_and_get_session_record_roundtrip() {
    let store = store().await;
    let record = record(
        "session-1",
        "user-1",
        Some("project-1".to_string()),
        Some(PathBuf::from("/repo")),
    );

    store
        .insert_session_record(&record, "agent-a", "api")
        .await
        .unwrap();
    let loaded = store
        .get_session_record("session-1")
        .await
        .unwrap()
        .unwrap();

    assert_eq!(loaded.id, record.id);
    assert_eq!(loaded.workspace_id, record.workspace_id);
    assert_eq!(loaded.project_id, record.project_id);
    assert_eq!(loaded.current_directory, record.current_directory);
    assert_eq!(loaded.title, DEFAULT_SESSION_TITLE);
}

#[tokio::test]
async fn test_update_session_record_persists_fields() {
    let store = store().await;
    let mut record = record("session-1", "user-1", None, None);
    store
        .insert_session_record(&record, "agent-a", "api")
        .await
        .unwrap();

    record.title = "Custom Title".to_string();
    record.message_count = 2;
    record.usage = TokenUsage {
        input_tokens: 10,
        output_tokens: 5,
        total_tokens: 15,
        ..TokenUsage::default()
    };
    record.model = "gpt-5.4".to_string();
    assert!(store.update_session_record(&record).await.unwrap());

    let loaded = store
        .get_session_record("session-1")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(loaded.title, "Custom Title");
    assert_eq!(loaded.message_count, 2);
    assert_eq!(loaded.usage.total_tokens, 15);
    assert_eq!(loaded.model, "gpt-5.4");
}

#[tokio::test]
async fn test_delete_session_record_removes_session() {
    let store = store().await;
    let record = record("session-1", "user-1", None, None);
    store
        .insert_session_record(&record, "agent-a", "api")
        .await
        .unwrap();

    assert!(store.delete_session_record("session-1").await.unwrap());
    assert!(store
        .get_session_record("session-1")
        .await
        .unwrap()
        .is_none());
}

#[tokio::test]
async fn test_list_session_records_filters_by_project_id() {
    let store = store().await;
    store
        .insert_session_record(
            &record("session-1", "user-1", Some("project-1".to_string()), None),
            "agent-a",
            "api",
        )
        .await
        .unwrap();
    store
        .insert_session_record(
            &record("session-2", "user-1", Some("project-2".to_string()), None),
            "agent-a",
            "api",
        )
        .await
        .unwrap();

    let records = store
        .list_session_records(&SessionQuery {
            project_id: Some("project-1".to_string()),
            ..SessionQuery::default()
        })
        .await
        .unwrap();

    assert_eq!(records.len(), 1);
    assert_eq!(records[0].id, "session-1");
}

#[tokio::test]
async fn test_list_session_records_respects_limit_and_offset() {
    let store = store().await;
    store
        .insert_session_record(&record("session-1", "user-1", None, None), "agent-a", "api")
        .await
        .unwrap();
    store
        .insert_session_record(&record("session-2", "user-1", None, None), "agent-a", "api")
        .await
        .unwrap();
    store
        .insert_session_record(&record("session-3", "user-1", None, None), "agent-a", "api")
        .await
        .unwrap();

    let records = store
        .list_session_records(&SessionQuery {
            limit: Some(1),
            offset: Some(1),
            ..SessionQuery::default()
        })
        .await
        .unwrap();

    assert_eq!(records.len(), 1);
}

#[tokio::test]
async fn test_list_projects_returns_session_count() {
    let storage = Arc::new(SqliteStorage::new(":memory:").await.unwrap());
    let store = DatabaseSessionStore::new(storage.clone()).await.unwrap();
    let project_store = StorageProjectStore::new(storage).await.unwrap();
    let project = project_store
        .create_project("repo", &PathBuf::from("/repo"))
        .await
        .unwrap();
    store
        .insert_session_record(
            &record(
                "session-1",
                "user-1",
                Some(project.project_id.clone()),
                None,
            ),
            "agent-a",
            "api",
        )
        .await
        .unwrap();

    let projects = project_store.list_projects().await.unwrap();

    assert_eq!(projects[0].session_count, 1);
}
