use super::*;
use crate::Memory;
use chrono::{TimeZone, Utc};
use talos_config::AgentKernelConfig;
use tempfile::TempDir;

fn new_temp_store() -> (TempDir, MemoryStore) {
    let temp_dir = tempfile::tempdir().expect("temp dir should be created");
    let store = MemoryStore::new(temp_dir.path().to_path_buf());
    (temp_dir, store)
}

#[tokio::test]
async fn test_append_workspace_memory_writes_structured_block() {
    let (_guard, store) = new_temp_store();
    let timestamp = Utc::now();
    store
        .append_workspace_memory(&WorkspaceMemoryRecord {
            timestamp,
            session_id: "session-1".to_string(),
            title: "Flush required".to_string(),
            content: "**Facts:**\n- flush on close\n".to_string(),
        })
        .await
        .expect("append should succeed");

    let content = talos_fs::read_file(&store.workspace_memory_file_for(timestamp))
        .await
        .expect("memory file should read")
        .expect("memory file should exist");
    assert!(content.contains("## ["));
    assert!(content.contains("session: session-1"));
    assert!(content.contains("Flush required"));
}

#[tokio::test]
async fn test_append_workspace_memory_dedupes_same_session_and_title() {
    let (_guard, store) = new_temp_store();
    let timestamp = Utc::now();
    let record = WorkspaceMemoryRecord {
        timestamp,
        session_id: "session-1".to_string(),
        title: "Flush required".to_string(),
        content: "**Facts:**\n- flush on close\n".to_string(),
    };

    store
        .append_workspace_memory(&record)
        .await
        .expect("first append should succeed");
    store
        .append_workspace_memory(&record)
        .await
        .expect("second append should dedupe");

    let content = talos_fs::read_file(&store.workspace_memory_file_for(timestamp))
        .await
        .expect("memory file should read")
        .expect("memory file should exist");
    assert_eq!(content.matches("session: session-1").count(), 1);
}

#[tokio::test]
async fn test_append_workspace_memory_allows_similar_cross_session_content() {
    let (_guard, store) = new_temp_store();
    let timestamp = Utc::now();
    for session_id in ["session-1", "session-2"] {
        store
            .append_workspace_memory(&WorkspaceMemoryRecord {
                timestamp,
                session_id: session_id.to_string(),
                title: "Flush required".to_string(),
                content: "**Facts:**\n- flush on close\n".to_string(),
            })
            .await
            .expect("append should succeed");
    }

    let content = talos_fs::read_file(&store.workspace_memory_file_for(timestamp))
        .await
        .expect("memory file should read")
        .expect("memory file should exist");
    assert_eq!(content.matches("Flush required").count(), 2);
}

#[tokio::test]
async fn test_append_workspace_memory_uses_configured_timezone_for_file_and_heading() {
    let temp_dir = tempfile::tempdir().expect("temp dir should be created");
    let mut kernel_config = AgentKernelConfig::default();
    kernel_config.timezone = "Asia/Shanghai".to_string();
    kernel_config.workspace_dir = temp_dir.path().to_string_lossy().to_string();
    let store = MemoryStore::from_config(&kernel_config);
    let timestamp = Utc
        .with_ymd_and_hms(2026, 4, 4, 23, 21, 0)
        .single()
        .expect("valid timestamp");

    store
        .append_workspace_memory(&WorkspaceMemoryRecord {
            timestamp,
            session_id: "session-1".to_string(),
            title: "Timezone aware record".to_string(),
            content: "**Facts:**\n- render local time\n".to_string(),
        })
        .await
        .expect("append should succeed");

    let file_path = temp_dir.path().join("memory").join("2026-04-05.md");
    let content = talos_fs::read_file(&file_path)
        .await
        .expect("timezone-adjusted memory file should read")
        .expect("timezone-adjusted memory file should exist");
    assert!(content.contains("## [07:21] Timezone aware record"));
}
