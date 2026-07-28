use std::collections::{BTreeMap, BTreeSet};

use talos_storage::SqliteStorage;
use talos_types::FileScope;

use crate::migration::{run_migrations, test_migrations_dir};

use super::{
    DatabaseFileStore, FileIncompleteCursor, FileListCursor, FileListQuery, FileOwnerKey,
    FileReadyUpdate, FileRecord, FileRecordUpdate, FileStatus, FileStore,
};

async fn store() -> DatabaseFileStore {
    let storage = SqliteStorage::new_with_max_connections(":memory:", 1)
        .await
        .unwrap();
    let pool = storage.pool().clone();
    run_migrations(&pool, &test_migrations_dir()).await.unwrap();
    sqlx::query(
        "INSERT INTO projects (project_id, name, user_id, directory, created_at, updated_at) VALUES ('project-1', 'Project 1', 'user-1', '/project-1', 1, 1)",
    )
        .execute(&pool)
        .await
        .unwrap();
    DatabaseFileStore::new(pool)
}

fn uploading(file_id: &str, owner_user_id: &str, project_id: Option<&str>, at: i64) -> FileRecord {
    FileRecord {
        file_id: file_id.to_string(),
        owner_user_id: owner_user_id.to_string(),
        project_id: project_id.map(str::to_string),
        storage_id: "local".to_string(),
        object_key: format!("files/{file_id}/content.pdf"),
        original_filename: format!("{file_id}.pdf"),
        mime_type: None,
        size_bytes: None,
        status: FileStatus::Uploading,
        tags: vec!["alpha".to_string(), "shared".to_string()],
        metadata: BTreeMap::from([("source".to_string(), "test".to_string())]),
        created_at: at,
        updated_at: at,
    }
}

fn owner_key(file_id: &str, owner_user_id: &str) -> FileOwnerKey {
    FileOwnerKey {
        file_id: file_id.to_string(),
        owner_user_id: owner_user_id.to_string(),
    }
}

async fn make_ready(store: &DatabaseFileStore, record: FileRecord) -> FileRecord {
    let file_id = record.file_id.clone();
    let owner_user_id = record.owner_user_id.clone();
    let updated_at = record.updated_at + 1;
    store.create_file(record).await.unwrap();
    assert!(store
        .mark_ready(FileReadyUpdate {
            file_id: file_id.clone(),
            mime_type: "application/pdf".to_string(),
            size_bytes: 42,
            updated_at,
        })
        .await
        .unwrap());
    store
        .get_file_for_owner(&owner_key(&file_id, &owner_user_id))
        .await
        .unwrap()
        .unwrap()
}

#[tokio::test]
async fn file_reads_are_owner_scoped() {
    let store = store().await;
    let record = uploading("file-1", "user-1", None, 100);
    store.create_file(record.clone()).await.unwrap();

    assert_eq!(
        (
            store
                .get_file_for_owner(&owner_key("file-1", "user-1"))
                .await
                .unwrap(),
            store
                .get_file_for_owner(&owner_key("file-1", "user-2"))
                .await
                .unwrap(),
        ),
        (Some(record), None)
    );
}

#[tokio::test]
async fn batch_file_reads_return_only_requested_owner_records() {
    let store = store().await;
    let first = make_ready(&store, uploading("file-1", "user-1", None, 100)).await;
    let second = make_ready(&store, uploading("file-2", "user-1", None, 200)).await;
    make_ready(&store, uploading("file-3", "user-2", None, 300)).await;

    let mut actual = store
        .get_files_for_owner(
            "user-1",
            &[
                "file-2".to_string(),
                "file-1".to_string(),
                "file-3".to_string(),
            ],
        )
        .await
        .unwrap();
    actual.sort_by(|left, right| left.file_id.cmp(&right.file_id));

    assert_eq!(actual, vec![first, second]);
}

#[tokio::test]
async fn file_list_applies_project_and_all_tag_filters() {
    let store = store().await;
    let expected = make_ready(
        &store,
        uploading("file-1", "user-1", Some("project-1"), 100),
    )
    .await;
    let mut missing_tag = uploading("file-2", "user-1", Some("project-1"), 200);
    missing_tag.tags = vec!["alpha".to_string()];
    make_ready(&store, missing_tag).await;
    make_ready(
        &store,
        uploading("file-3", "user-2", Some("project-1"), 300),
    )
    .await;

    let actual = store
        .list_files_for_owner(FileListQuery {
            owner_user_id: "user-1".to_string(),
            scope: Some(FileScope::Project {
                project_id: "project-1".to_string(),
            }),
            tags: vec!["alpha".to_string(), "shared".to_string()],
            cursor: None,
            limit: 10,
        })
        .await
        .unwrap();

    assert_eq!(actual, vec![expected]);
}

#[tokio::test]
async fn file_list_cursor_is_stable_for_equal_timestamps() {
    let store = store().await;
    let first = make_ready(&store, uploading("file-b", "user-1", None, 100)).await;
    let second = make_ready(&store, uploading("file-a", "user-1", None, 100)).await;

    let first_page = store
        .list_files_for_owner(FileListQuery {
            owner_user_id: "user-1".to_string(),
            scope: None,
            tags: Vec::new(),
            cursor: None,
            limit: 1,
        })
        .await
        .unwrap();
    let second_page = store
        .list_files_for_owner(FileListQuery {
            owner_user_id: "user-1".to_string(),
            scope: None,
            tags: Vec::new(),
            cursor: Some(FileListCursor {
                created_at: first.created_at,
                file_id: first.file_id.clone(),
            }),
            limit: 1,
        })
        .await
        .unwrap();

    assert_eq!((first_page, second_page), (vec![first], vec![second]));
}

#[tokio::test]
async fn file_update_distinguishes_unchanged_from_cleared_fields() {
    let store = store().await;
    make_ready(&store, uploading("file-1", "user-1", None, 100)).await;

    let updated = store
        .update_file_for_owner(
            &owner_key("file-1", "user-1"),
            FileRecordUpdate {
                tags: Some(Vec::new()),
                metadata: Some(BTreeMap::new()),
                updated_at: 300,
            },
        )
        .await
        .unwrap()
        .unwrap();

    assert_eq!(
        (updated.tags, updated.metadata, updated.updated_at),
        (Vec::new(), BTreeMap::new(), 300)
    );
}

#[tokio::test]
async fn deleting_transition_is_idempotent_and_recoverable() {
    let store = store().await;
    let ready = make_ready(&store, uploading("file-1", "user-1", None, 100)).await;
    let key = owner_key("file-1", "user-1");

    let first = store
        .mark_deleting_for_owner(&key, 300)
        .await
        .unwrap()
        .unwrap();
    let second = store
        .mark_deleting_for_owner(&key, 400)
        .await
        .unwrap()
        .unwrap();
    let incomplete = store.list_incomplete_files(None, 100).await.unwrap();

    assert_eq!(
        (first.status, second.status, second.updated_at, incomplete),
        (
            FileStatus::Deleting,
            FileStatus::Deleting,
            300,
            vec![FileRecord {
                status: FileStatus::Deleting,
                updated_at: 300,
                ..ready
            }]
        )
    );
}

#[tokio::test]
async fn incomplete_cursor_project_count_and_storage_ids_are_deterministic() {
    let store = store().await;
    let first = uploading("file-a", "user-1", Some("project-1"), 100);
    let mut second = uploading("file-b", "user-2", Some("project-1"), 200);
    second.storage_id = "archive".to_string();
    store.create_file(first.clone()).await.unwrap();
    store.create_file(second.clone()).await.unwrap();

    let after_first = store
        .list_incomplete_files(
            Some(FileIncompleteCursor {
                updated_at: first.updated_at,
                file_id: first.file_id,
            }),
            100,
        )
        .await
        .unwrap();

    assert_eq!(
        (
            after_first,
            store.count_by_project("project-1").await.unwrap(),
            store.list_storage_ids().await.unwrap(),
        ),
        (
            vec![second],
            2,
            BTreeSet::from(["archive".to_string(), "local".to_string()]),
        )
    );
}
