use std::sync::Arc;

use talos_storage::SqliteStorage;
use talos_store::session::{DatabaseSessionStore, SessionStore};
use talos_types::talos::{TalosContext, TalosProvider, TalosSourceType, TalosUsage};

use super::*;

fn context() -> TalosContext {
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

async fn service() -> (SessionRecordService, Arc<DatabaseSessionStore>) {
    let storage = Arc::new(SqliteStorage::new(":memory:").await.unwrap());
    let store = Arc::new(DatabaseSessionStore::new(storage).await.unwrap());
    let session_store: Arc<dyn SessionStore> = store.clone();
    (SessionRecordService::new(session_store), store)
}

#[tokio::test]
async fn resolve_or_create_session_uses_default_scope() {
    let (service, _store) = service().await;
    let session = service
        .resolve_or_create_session_record(&context(), "agent-a", "gpt-5.3", None, None, None, None)
        .await
        .unwrap();

    assert_eq!(
        session.id,
        scope::stable_session_key(
            &context(),
            "agent-a",
            scope::SESSION_SCOPE_PER_CHANNEL_PEER_TOPIC,
        )
    );
}

#[tokio::test]
async fn create_session_generates_record() {
    let (service, _store) = service().await;
    let session = service
        .create_session_record(
            Some("session-1"),
            &context(),
            "agent-a",
            "gpt-5.3",
            None,
            None,
            None,
            None,
            None,
        )
        .await
        .unwrap();

    assert_eq!(session.id, "session-1");
    assert_eq!(session.model, "gpt-5.3");
}

#[tokio::test]
async fn record_session_progress_accumulates_usage() {
    let (service, store) = service().await;
    service
        .create_session_record(
            Some("session-1"),
            &context(),
            "agent-a",
            "gpt-5.3",
            None,
            None,
            None,
            None,
            None,
        )
        .await
        .unwrap();
    service
        .record_session_progress(
            "session-1",
            2,
            &TalosUsage {
                input_tokens: 1,
                output_tokens: 2,
                total_tokens: 3,
                ..TalosUsage::default()
            },
            Some("gpt-5.4"),
        )
        .await
        .unwrap();

    let record = store
        .get_session_record("session-1")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(record.message_count, 2);
    assert_eq!(record.usage.total_tokens, 3);
    assert_eq!(record.model, "gpt-5.4");
}
