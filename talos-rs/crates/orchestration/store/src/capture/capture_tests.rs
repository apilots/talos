use crate::{
    project::{ProjectStore, StorageProjectStore},
    session::{DatabaseSessionStore, SessionRecord, SessionStore},
};
use std::{path::Path, sync::Arc};
use talos_storage::{SqliteStorage, Storage};

async fn storage() -> (tempfile::TempDir, Arc<SqliteStorage>) {
    let home = tempfile::tempdir().unwrap();
    let storage = Arc::new(
        SqliteStorage::new(home.path().join("facts.db").to_str().unwrap())
            .await
            .unwrap(),
    );
    crate::migration::run_migrations(storage.pool(), &crate::migration::test_migrations_dir())
        .await
        .unwrap();
    (home, storage)
}
fn record(id: &str) -> SessionRecord {
    SessionRecord {
        session_kind: talos_types::SessionKind::Standard,
        id: id.into(),
        agent_id: "default".into(),
        execution_node_id: "node-a".into(),
        tree_identity: talos_types::SessionTreeIdentity::Primary,
        workspace_id: "workspace-a".into(),
        title: "Original".into(),
        user_id: "account-a".into(),
        source_type: talos_types::SessionSourceType::Desktop,
        source_provider: talos_types::SessionSourceProvider::Sdk,
        channel_id: None,
        thread_id: None,
        environment: None,
        created_at: 1,
        last_activity: 1,
        message_count: 0,
        latest_turn_seq: 0,
        memory_processed_until_event_seq: 0,
        usage: Default::default(),
        conf: Default::default(),
        project_id: None,
        worktree_binding: None,
        current_directory: None,
        pinned: false,
    }
}
#[tokio::test]
async fn no_adapter_project_changes_and_deletion_preserve_original_owner_and_before_image() {
    let (_home, storage) = storage().await;
    let projects = StorageProjectStore::new(storage.clone()).await.unwrap();
    let project = projects
        .create_project_on_node("Before", Path::new("/workspace"), "account-a", "node-a")
        .await
        .unwrap();
    projects
        .patch_project(
            &project.project_id,
            &talos_types::ProjectPatch {
                name: Some("After".into()),
            },
        )
        .await
        .unwrap();
    projects.delete_project(&project.project_id).await.unwrap();
    let rows=storage.query("SELECT owner_user_id, execution_node_id, changes_json FROM resource_revisions WHERE kind='project' AND resource_id=? ORDER BY revision", &[&project.project_id]).await.unwrap();
    assert_eq!(rows.len(), 3);
    assert!(rows.iter().all(
        |row| row.get_string("owner_user_id").as_deref() == Some("account-a")
            && row.get_string("execution_node_id").as_deref() == Some("node-a")
    ));
    let deleted: super::MetadataRevision =
        serde_json::from_str(&rows[2].get_string("changes_json").unwrap()).unwrap();
    assert_eq!(
        deleted
            .before
            .unwrap()
            .fields
            .row()
            .get_string("name")
            .as_deref(),
        Some("After")
    );
    assert!(deleted.after.is_none());
}
#[tokio::test]
async fn no_adapter_clear_delete_and_new_generation_remain_ordered_after_restart() {
    let (home, storage) = storage().await;
    let sessions = DatabaseSessionStore::new(storage.clone()).await.unwrap();
    sessions
        .insert_session_record(&record("session-a"))
        .await
        .unwrap();
    sessions
        .clear_session_history("session-a", 2)
        .await
        .unwrap();
    sessions.delete_session_record("session-a").await.unwrap();
    sessions
        .insert_session_record(&record("session-a"))
        .await
        .unwrap();
    let reopened = SqliteStorage::new(home.path().join("facts.db").to_str().unwrap())
        .await
        .unwrap();
    let rows=reopened.query("SELECT owner_user_id,changes_json FROM resource_revisions WHERE kind='session' AND resource_id='session-a' ORDER BY revision", &[]).await.unwrap();
    assert_eq!(rows.len(), 4);
    let revisions: Vec<super::source::SessionRevision> = rows
        .iter()
        .map(|row| serde_json::from_str(&row.get_string("changes_json").unwrap()).unwrap())
        .collect();
    assert!(revisions[2].after.is_none());
    assert!(
        revisions[3].after.as_ref().unwrap().generation
            > revisions[1].after.as_ref().unwrap().generation
    );
    assert!(rows
        .iter()
        .all(|row| row.get_string("owner_user_id").as_deref() == Some("account-a")));
}
#[tokio::test]
async fn capture_failure_rolls_back_the_original_business_transaction() {
    let (_home, storage) = storage().await;
    storage.execute("CREATE TRIGGER reject_revision BEFORE INSERT ON resource_revisions BEGIN SELECT RAISE(ABORT,'capture unavailable'); END",&[]).await.unwrap();
    let projects = StorageProjectStore::new(storage.clone()).await.unwrap();
    assert!(projects
        .create_project_on_node(
            "Must rollback",
            Path::new("/workspace"),
            "account-a",
            "node-a"
        )
        .await
        .is_err());
    assert!(projects.list_projects().await.unwrap().is_empty());
    assert!(storage
        .query("SELECT revision FROM resource_revisions", &[])
        .await
        .unwrap()
        .is_empty());
}

#[tokio::test]
async fn explicit_claim_transfers_only_existing_local_resources_not_deleted_anonymous_history() {
    let (_home, storage) = storage().await;
    let sessions = DatabaseSessionStore::new(storage.clone()).await.unwrap();
    for id in ["claimable", "deleted-anonymous"] {
        let mut local = record(id);
        local.user_id = "anonymous".into();
        local.execution_node_id = "embedded".into();
        sessions.insert_session_record(&local).await.unwrap();
    }
    sessions
        .delete_session_record("deleted-anonymous")
        .await
        .unwrap();
    sessions.claim_local_history("account-a").await.unwrap();
    let rows = storage
        .query(
            "SELECT resource_id,owner_user_id FROM resource_revisions ORDER BY revision",
            &[],
        )
        .await
        .unwrap();
    assert!(rows
        .iter()
        .filter(|row| row.get_string("resource_id").as_deref() == Some("claimable"))
        .all(|row| row.get_string("owner_user_id").as_deref() == Some("account-a")));
    assert!(rows
        .iter()
        .filter(|row| row.get_string("resource_id").as_deref() == Some("deleted-anonymous"))
        .all(|row| row.get_string("owner_user_id").as_deref() == Some("anonymous")));
    assert_eq!(
        sessions
            .get_workspace_user_id("claimable")
            .await
            .unwrap()
            .as_deref(),
        Some("anonymous")
    );
}

#[tokio::test]
async fn retiring_registration_without_an_adapter_freezes_persisted_resource_identity() {
    let (_home, storage) = storage().await;
    let sessions = DatabaseSessionStore::new(storage.clone()).await.unwrap();
    let mut local = record("retired-local");
    local.execution_node_id = "embedded".into();
    sessions.insert_session_record(&local).await.unwrap();
    let projects = StorageProjectStore::new(storage.clone()).await.unwrap();
    let project = projects
        .create_project_on_node("Existing", Path::new("/workspace"), "account-a", "embedded")
        .await
        .unwrap();
    storage.execute("INSERT INTO sync_bindings(hub_id,account_id,source_user_id,computers_json) VALUES ('hub-a','account-a','account-a','[{\"sourceNodeId\":\"embedded\",\"computerId\":\"old-computer\"}]')",&[]).await.unwrap();
    sessions
        .retire_local_registration("account-a", "old-computer")
        .await
        .unwrap();
    assert_eq!(
        sessions
            .get_session_record("retired-local")
            .await
            .unwrap()
            .unwrap()
            .execution_node_id,
        "old-computer"
    );
    assert_eq!(
        projects
            .get_project(&project.project_id)
            .await
            .unwrap()
            .unwrap()
            .execution_node_id,
        "old-computer"
    );
    let rows=storage.query("SELECT execution_node_id,changes_json FROM resource_revisions WHERE owner_user_id='account-a'",&[]).await.unwrap();
    assert!(rows
        .iter()
        .all(
            |row| row.get_string("execution_node_id").as_deref() == Some("old-computer")
                && !row
                    .get_string("changes_json")
                    .unwrap()
                    .contains("\"execution_node_id\":\"embedded\"")
        ));
    assert_eq!(
        storage
            .query("SELECT computers_json FROM sync_bindings", &[])
            .await
            .unwrap()[0]
            .get_string("computers_json")
            .as_deref(),
        Some("[]")
    );
}

#[tokio::test]
async fn sqlite_file_capture_preserves_tags_and_rolls_back_failed_deletion_capture() {
    use crate::file::{
        DatabaseFileStore, FileLocation, FileOwnerKey, FileRecord, FileStatus, FileStore,
    };
    let (_home, storage) = storage().await;
    let files = DatabaseFileStore::new(&storage);
    let key = FileOwnerKey {
        file_id: "local-file".into(),
        owner_user_id: "account-a".into(),
    };
    files
        .create_file(FileRecord {
            file_id: key.file_id.clone(),
            owner_user_id: key.owner_user_id.clone(),
            project_id: None,
            location: FileLocation::Local {
                storage_id: "local".into(),
                object_key: "local-file".into(),
            },
            original_filename: "notes.txt".into(),
            mime_type: Some("text/plain".into()),
            size_bytes: Some(4),
            status: FileStatus::Ready,
            tags: vec!["notes".into()],
            metadata: Default::default(),
            created_at: 1,
            updated_at: 1,
        })
        .await
        .unwrap();
    storage.execute("CREATE TRIGGER reject_file_revision BEFORE INSERT ON resource_revisions WHEN NEW.kind='file' BEGIN SELECT RAISE(ABORT,'capture unavailable'); END",&[]).await.unwrap();
    assert!(files.mark_deleting_for_owner(&key, 2).await.is_err());
    assert_eq!(
        files
            .get_file_for_owner(&key)
            .await
            .unwrap()
            .unwrap()
            .status,
        FileStatus::Ready
    );
    storage
        .execute("DROP TRIGGER reject_file_revision", &[])
        .await
        .unwrap();
    files.mark_deleting_for_owner(&key, 3).await.unwrap();
    files.delete_file_record(&key.file_id).await.unwrap();
    let rows = storage
        .query(
            "SELECT changes_json FROM resource_revisions WHERE kind='file' ORDER BY revision",
            &[],
        )
        .await
        .unwrap();
    assert_eq!(rows.len(), 2);
    let tombstone: super::MetadataRevision =
        serde_json::from_str(&rows[1].get_string("changes_json").unwrap()).unwrap();
    assert_eq!(tombstone.before.unwrap().tags, vec!["notes".to_owned()]);
    assert!(tombstone.after.is_none());
}

#[tokio::test]
async fn session_capture_failure_rolls_back_session_creation() {
    let (_home, storage) = storage().await;
    storage.execute("CREATE TRIGGER reject_session_revision BEFORE INSERT ON resource_revisions WHEN NEW.kind='session' BEGIN SELECT RAISE(ABORT,'capture unavailable'); END",&[]).await.unwrap();
    let sessions = DatabaseSessionStore::new(storage.clone()).await.unwrap();
    assert!(sessions
        .insert_session_record(&record("must-not-exist"))
        .await
        .is_err());
    assert!(sessions
        .get_session_record("must-not-exist")
        .await
        .unwrap()
        .is_none());
}
