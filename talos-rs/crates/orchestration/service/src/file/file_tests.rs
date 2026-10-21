use std::collections::BTreeMap;
use std::io;
use std::sync::Arc;

use crate::test_support::run_test_migrations;
use base64::Engine;
use bytes::Bytes;
use futures_util::stream;
use opendal::{services::Fs, Operator};
use talos_agent::{AttachmentReader, TurnFileScope};
use talos_config::{FileStorageConfig, FileStorageLimits};
use talos_error::ErrorCode;
use talos_storage::SqliteStorage;
use talos_store::file::{DatabaseFileStore, FileStatus, FileStore};
use talos_store::project::{ProjectStore, StorageProjectStore};
use talos_types::{
    FileDeleteRequest, FileListRequest, FileReadRequest, FileScope, FileUpdateRequest,
    FileUploadRequest,
};
use tempfile::TempDir;

use crate::Actor;

use super::{FileByteStream, FileService};

const PNG: &[u8] = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR";
#[path = "peer_tests.rs"]
mod peer_tests;
const CSV: &[u8] = b"name,severity\nfire,high\nsmoke,medium\n";
// Derived from calamine's MIT-licensed empty_sheet.xlsx fixture.
const XLSX_BASE64: &str = concat!(
    "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",
    "LnhtbFBLBQYAAAAACQAJAFkCAABwCwAAAAA=",
);

struct TestFiles {
    service: FileService,
    storage: Arc<SqliteStorage>,
    store: Arc<DatabaseFileStore>,
    project_store: Arc<StorageProjectStore>,
    operator: Operator,
    root: TempDir,
}

impl TestFiles {
    async fn new(configure: impl FnOnce(&mut FileStorageConfig)) -> Self {
        let storage = Arc::new(SqliteStorage::new(":memory:").await.unwrap());
        run_test_migrations(storage.pool()).await;
        let store = Arc::new(DatabaseFileStore::new(storage.pool().clone()));
        let project_store = Arc::new(StorageProjectStore::new(storage.clone()).await.unwrap());
        let root = tempfile::tempdir().unwrap();
        let operator = Operator::new(Fs::default().root(root.path().to_str().unwrap()))
            .unwrap()
            .finish();
        let mut config = FileStorageConfig::default();
        configure(&mut config);
        let service = FileService::new(
            store.clone(),
            project_store.clone(),
            BTreeMap::from([("local".to_string(), operator.clone())]),
            config,
        )
        .unwrap();
        Self {
            service,
            storage,
            store,
            project_store,
            operator,
            root,
        }
    }

    async fn upload(&self, actor: &Actor, scope: FileScope) -> talos_types::FileInfo {
        self.service
            .upload(
                actor,
                FileUploadRequest {
                    filename: " Example.PNG ".to_string(),
                    scope,
                    tags: vec!["zeta".to_string(), "alpha".to_string()],
                    metadata: BTreeMap::from([("source".to_string(), "test".to_string())]),
                },
                bytes_stream(PNG),
            )
            .await
            .unwrap()
            .file
    }
}

fn bytes_stream(bytes: &'static [u8]) -> FileByteStream {
    Box::pin(stream::iter([Ok(Bytes::from_static(bytes))]))
}

#[tokio::test]
async fn local_files_complete_the_public_lifecycle_without_exposing_storage_details() {
    let files = TestFiles::new(|_| {}).await;
    let actor = Actor::regular("user-1");
    let uploaded = files.upload(&actor, FileScope::Personal).await;

    let record = files
        .store
        .get_file_for_owner(&talos_store::file::FileOwnerKey {
            file_id: uploaded.file_id.clone(),
            owner_user_id: actor.user_id.clone(),
        })
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        record.local_location().unwrap().1,
        format!("{}/content.png", uploaded.file_id)
    );
    let object = std::fs::read(files.root.path().join(record.local_location().unwrap().1)).unwrap();
    let listed = files
        .service
        .list(&actor, FileListRequest::default())
        .await
        .unwrap();
    let read = files
        .service
        .read(
            &actor,
            FileReadRequest {
                file_id: uploaded.file_id.clone(),
            },
        )
        .await
        .unwrap();
    let updated = files
        .service
        .update(
            &actor,
            FileUpdateRequest {
                file_id: uploaded.file_id.clone(),
                tags: Some(Vec::new()),
                metadata: Some(BTreeMap::new()),
            },
        )
        .await
        .unwrap();

    let content = files
        .service
        .read_content(
            &actor,
            FileReadRequest {
                file_id: uploaded.file_id.clone(),
            },
        )
        .await
        .unwrap();
    assert_eq!(content.bytes, PNG);
    assert_eq!(content.file, updated.file);
    assert_eq!(object, PNG);
    assert_eq!(listed.data, vec![uploaded.clone()]);
    assert_eq!(read.file, uploaded);
    assert_eq!(
        (updated.file.tags, updated.file.metadata),
        (Vec::new(), BTreeMap::new())
    );

    let deleted = files
        .service
        .delete(
            &actor,
            FileDeleteRequest {
                file_id: updated.file.file_id.clone(),
            },
        )
        .await
        .unwrap();
    assert_eq!(deleted.file_id, updated.file.file_id);
    assert!(!files
        .operator
        .exists(record.local_location().unwrap().1)
        .await
        .unwrap());
}

#[tokio::test]
async fn csv_files_are_detected_from_valid_utf8_records() {
    let files = TestFiles::new(|_| {}).await;
    let actor = Actor::regular("user-1");
    let uploaded = files
        .service
        .upload(
            &actor,
            FileUploadRequest {
                filename: "dataset.csv".to_string(),
                scope: FileScope::Personal,
                tags: Vec::new(),
                metadata: BTreeMap::new(),
            },
            bytes_stream(CSV),
        )
        .await
        .unwrap()
        .file;

    assert_eq!(uploaded.mime_type, "text/csv");
}

#[tokio::test]
async fn csv_validation_reads_the_complete_upload() {
    let files = TestFiles::new(|_| {}).await;
    let mut content = b"name,value\n".to_vec();
    content.extend(std::iter::repeat_n(b"row,1\n".as_slice(), 12_000).flatten());
    content.push(0xff);
    let error = files
        .service
        .upload(
            &Actor::regular("user-1"),
            FileUploadRequest {
                filename: "dataset.csv".to_string(),
                scope: FileScope::Personal,
                tags: Vec::new(),
                metadata: BTreeMap::new(),
            },
            Box::pin(stream::iter([Ok(Bytes::from(content))])),
        )
        .await
        .unwrap_err();

    assert!(error.is_code(ErrorCode::UnsupportedMediaType));
}

#[tokio::test]
async fn office_documents_use_normalized_suffix_mime_types() {
    let files = TestFiles::new(|_| {}).await;
    let actor = Actor::regular("user-1");
    let mut detected = Vec::new();

    for filename in ["notes.DOC", "notes.DoCx"] {
        let uploaded = files
            .service
            .upload(
                &actor,
                FileUploadRequest {
                    filename: filename.to_string(),
                    scope: FileScope::Personal,
                    tags: Vec::new(),
                    metadata: BTreeMap::new(),
                },
                bytes_stream(b"opaque document content"),
            )
            .await
            .unwrap()
            .file;
        detected.push((uploaded.original_filename, uploaded.mime_type));
    }

    assert_eq!(
        detected,
        vec![
            ("notes.DOC".to_string(), "application/msword".to_string()),
            (
                "notes.DoCx".to_string(),
                "application/vnd.openxmlformats-officedocument.wordprocessingml.document"
                    .to_string(),
            ),
        ]
    );
}

#[tokio::test]
async fn xlsx_files_are_validated_with_a_workbook_reader() {
    let files = TestFiles::new(|_| {}).await;
    let content = base64::engine::general_purpose::STANDARD
        .decode(XLSX_BASE64)
        .unwrap();
    let uploaded = files
        .service
        .upload(
            &Actor::regular("user-1"),
            FileUploadRequest {
                filename: "workbook.xlsx".to_string(),
                scope: FileScope::Personal,
                tags: Vec::new(),
                metadata: BTreeMap::new(),
            },
            Box::pin(stream::iter([Ok(Bytes::from(content))])),
        )
        .await
        .unwrap()
        .file;

    assert_eq!(
        uploaded.mime_type,
        "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet"
    );
}

#[tokio::test]
async fn malformed_excel_content_is_rejected() {
    let files = TestFiles::new(|_| {}).await;
    let mut content = base64::engine::general_purpose::STANDARD
        .decode(XLSX_BASE64)
        .unwrap();
    content.truncate(content.len() - 64);
    let error = files
        .service
        .upload(
            &Actor::regular("user-1"),
            FileUploadRequest {
                filename: "workbook.xlsx".to_string(),
                scope: FileScope::Personal,
                tags: Vec::new(),
                metadata: BTreeMap::new(),
            },
            Box::pin(stream::iter([Ok(Bytes::from(content))])),
        )
        .await
        .unwrap_err();

    assert!(error.is_code(ErrorCode::UnsupportedMediaType));
}

#[tokio::test]
async fn binary_content_is_not_misclassified_as_csv() {
    let files = TestFiles::new(|config| {
        config.allowed_mime_types = std::collections::BTreeSet::from(["text/csv".to_string()]);
    })
    .await;
    let error = files
        .service
        .upload(
            &Actor::regular("user-1"),
            FileUploadRequest {
                filename: "dataset.csv".to_string(),
                scope: FileScope::Personal,
                tags: Vec::new(),
                metadata: BTreeMap::new(),
            },
            bytes_stream(b"\xff\xfe\x00\x01"),
        )
        .await
        .unwrap_err();

    assert!(error.is_code(ErrorCode::UnsupportedMediaType));
}

#[tokio::test]
async fn public_file_cursor_pages_without_duplicates() {
    let files = TestFiles::new(|_| {}).await;
    let actor = Actor::regular("user-1");
    let first = files.upload(&actor, FileScope::Personal).await;
    let second = files.upload(&actor, FileScope::Personal).await;

    let first_page = files
        .service
        .list(
            &actor,
            FileListRequest {
                limit: Some(1),
                ..FileListRequest::default()
            },
        )
        .await
        .unwrap();
    let second_page = files
        .service
        .list(
            &actor,
            FileListRequest {
                cursor: first_page.next_cursor.clone(),
                limit: Some(1),
                ..FileListRequest::default()
            },
        )
        .await
        .unwrap();
    let ids = first_page
        .data
        .iter()
        .chain(&second_page.data)
        .map(|file| file.file_id.as_str())
        .collect::<std::collections::BTreeSet<_>>();

    assert_eq!(
        ids,
        std::collections::BTreeSet::from([first.file_id.as_str(), second.file_id.as_str()])
    );
    assert!(first_page.next_cursor.is_some());
    assert!(second_page.next_cursor.is_none());
}

#[tokio::test]
async fn file_scope_and_owner_are_enforced_as_not_found() {
    let files = TestFiles::new(|_| {}).await;
    let owner = Actor::regular("user-1");
    let other = Actor::regular("user-2");
    let project = files
        .project_store
        .create_project_for_owner("Project", files.root.path(), &owner.user_id)
        .await
        .unwrap();
    let uploaded = files
        .upload(
            &owner,
            FileScope::Project {
                project_id: project.project_id,
            },
        )
        .await;

    let denied = files
        .service
        .read_content(
            &other,
            FileReadRequest {
                file_id: uploaded.file_id.clone(),
            },
        )
        .await;
    assert!(denied.err().unwrap().is_code(ErrorCode::ResourceNotFound));
    let error = files
        .service
        .read(
            &other,
            FileReadRequest {
                file_id: uploaded.file_id,
            },
        )
        .await
        .unwrap_err();
    assert!(error.is_code(talos_error::ErrorCode::ResourceNotFound));
}

#[tokio::test]
async fn project_file_relation_is_preserved_by_store_writes() {
    let files = TestFiles::new(|_| {}).await;
    let owner = Actor::regular("user-1");
    let project = files
        .project_store
        .create_project_for_owner("Project", files.root.path(), &owner.user_id)
        .await
        .unwrap();
    let uploaded = files
        .upload(
            &owner,
            FileScope::Project {
                project_id: project.project_id.clone(),
            },
        )
        .await;
    let record = files
        .store
        .get_file_for_owner(&talos_store::file::FileOwnerKey {
            file_id: uploaded.file_id,
            owner_user_id: owner.user_id.clone(),
        })
        .await
        .unwrap()
        .unwrap();

    let conflict = files
        .project_store
        .delete_project(&project.project_id)
        .await
        .unwrap_err();
    assert!(conflict.is_code(talos_error::ErrorCode::Conflict));

    files
        .service
        .delete(
            &owner,
            FileDeleteRequest {
                file_id: record.file_id.clone(),
            },
        )
        .await
        .unwrap();
    files
        .project_store
        .delete_project(&project.project_id)
        .await
        .unwrap();

    let missing_project = files
        .store
        .create_file(talos_store::file::FileRecord {
            file_id: "file-after-project-delete".to_string(),
            location: talos_store::file::FileLocation::Local {
                storage_id: record.local_location().unwrap().0.to_owned(),
                object_key: "file-after-project-delete/content.png".to_string(),
            },
            mime_type: None,
            size_bytes: None,
            status: FileStatus::Uploading,
            ..record
        })
        .await
        .unwrap_err();
    assert!(missing_project.is_code(talos_error::ErrorCode::ResourceNotFound));
}

#[tokio::test]
async fn attachment_reader_preserves_order_and_allows_personal_plus_session_project() {
    let files = TestFiles::new(|_| {}).await;
    let owner = Actor::regular("user-1");
    let project = files
        .project_store
        .create_project_for_owner("Project", files.root.path(), &owner.user_id)
        .await
        .unwrap();
    let personal = files.upload(&owner, FileScope::Personal).await;
    let project_file = files
        .upload(
            &owner,
            FileScope::Project {
                project_id: project.project_id.clone(),
            },
        )
        .await;

    let resolved = AttachmentReader::read(
        &files.service,
        &TurnFileScope {
            owner_user_id: owner.user_id,
            project_id: Some(project.project_id),
        },
        &[project_file.file_id.clone(), personal.file_id.clone()],
    )
    .await
    .unwrap();

    assert_eq!(
        resolved
            .iter()
            .map(|attachment| attachment.file_id.as_str())
            .collect::<Vec<_>>(),
        vec![project_file.file_id, personal.file_id]
    );
    assert!(resolved.iter().all(|attachment| attachment.bytes == PNG));
}

#[tokio::test]
async fn attachment_reader_rejects_project_mismatch_and_count_limit() {
    let files = TestFiles::new(|config| config.limits.max_files_per_turn = 1).await;
    let owner = Actor::regular("user-1");
    let project = files
        .project_store
        .create_project_for_owner("Project", files.root.path(), &owner.user_id)
        .await
        .unwrap();
    let project_file = files
        .upload(
            &owner,
            FileScope::Project {
                project_id: project.project_id,
            },
        )
        .await;

    let mismatch = AttachmentReader::read(
        &files.service,
        &TurnFileScope {
            owner_user_id: owner.user_id.clone(),
            project_id: Some("other-project".to_string()),
        },
        std::slice::from_ref(&project_file.file_id),
    )
    .await
    .unwrap_err();
    assert!(mismatch.is_code(talos_error::ErrorCode::ResourceNotFound));

    let count = AttachmentReader::read(
        &files.service,
        &TurnFileScope {
            owner_user_id: owner.user_id,
            project_id: None,
        },
        &[project_file.file_id.clone(), "file-2".to_string()],
    )
    .await
    .unwrap_err();
    assert!(count.is_code(ErrorCode::AttachmentLimitExceeded));
}

#[tokio::test]
async fn attachment_reader_rejects_an_oversized_object_before_loading_it() {
    let files = TestFiles::new(|config| {
        config.limits.max_upload_bytes = PNG.len() as u64;
    })
    .await;
    let owner = Actor::regular("user-1");
    let uploaded = files.upload(&owner, FileScope::Personal).await;
    let record = files
        .store
        .get_file_for_owner(&talos_store::file::FileOwnerKey {
            file_id: uploaded.file_id.clone(),
            owner_user_id: owner.user_id.clone(),
        })
        .await
        .unwrap()
        .unwrap();
    std::fs::write(
        files.root.path().join(record.local_location().unwrap().1),
        vec![0_u8; PNG.len() + 1],
    )
    .unwrap();

    let error = AttachmentReader::read(
        &files.service,
        &TurnFileScope {
            owner_user_id: owner.user_id,
            project_id: None,
        },
        &[uploaded.file_id],
    )
    .await
    .unwrap_err();

    assert!(error.is_code(ErrorCode::AttachmentLimitExceeded));
}

#[tokio::test]
async fn upload_rejects_invalid_metadata_and_stream_limits_before_visibility() {
    let files = TestFiles::new(|config| {
        config.limits = FileStorageLimits {
            max_upload_bytes: PNG.len() as u64 - 1,
            ..FileStorageLimits::default()
        };
    })
    .await;
    let actor = Actor::regular("user-1");
    let too_large = files
        .service
        .upload(
            &actor,
            FileUploadRequest {
                filename: "image.png".to_string(),
                scope: FileScope::Personal,
                tags: Vec::new(),
                metadata: BTreeMap::new(),
            },
            bytes_stream(PNG),
        )
        .await
        .unwrap_err();
    let invalid_name = files
        .service
        .upload(
            &actor,
            FileUploadRequest {
                filename: "../image.png".to_string(),
                scope: FileScope::Personal,
                tags: Vec::new(),
                metadata: BTreeMap::new(),
            },
            bytes_stream(PNG),
        )
        .await
        .unwrap_err();
    let visible = files
        .service
        .list(&actor, FileListRequest::default())
        .await
        .unwrap();

    assert!(too_large.is_code(ErrorCode::UploadTooLarge));
    assert!(invalid_name.is_code(talos_error::ErrorCode::InvalidRequest));
    assert!(visible.data.is_empty());
}

#[tokio::test]
async fn failed_input_stream_aborts_the_object_and_uploading_record() {
    let files = TestFiles::new(|_| {}).await;
    let actor = Actor::regular("user-1");
    let body: FileByteStream = Box::pin(stream::iter([Err(io::Error::other("broken body"))]));

    let error = files
        .service
        .upload(
            &actor,
            FileUploadRequest {
                filename: "image.png".to_string(),
                scope: FileScope::Personal,
                tags: Vec::new(),
                metadata: BTreeMap::new(),
            },
            body,
        )
        .await
        .unwrap_err();

    assert!(error.is_internal());
    assert!(files
        .store
        .list_incomplete_files(None, 100)
        .await
        .unwrap()
        .is_empty());
}

#[tokio::test]
async fn unsupported_media_and_ready_transition_failure_leave_no_visible_file() {
    let files = TestFiles::new(|_| {}).await;
    let actor = Actor::regular("user-1");
    let unsupported = files
        .service
        .upload(
            &actor,
            FileUploadRequest {
                filename: "plain.txt".to_string(),
                scope: FileScope::Personal,
                tags: Vec::new(),
                metadata: BTreeMap::new(),
            },
            bytes_stream(b"plain text"),
        )
        .await
        .unwrap_err();
    sqlx::query(
        r#"
        CREATE TRIGGER fail_file_ready
        BEFORE UPDATE OF status ON files
        WHEN NEW.status = 'ready'
        BEGIN
            SELECT RAISE(ABORT, 'injected ready failure');
        END
        "#,
    )
    .execute(files.storage.pool())
    .await
    .unwrap();
    let ready_failure = files
        .service
        .upload(
            &actor,
            FileUploadRequest {
                filename: "image.png".to_string(),
                scope: FileScope::Personal,
                tags: Vec::new(),
                metadata: BTreeMap::new(),
            },
            bytes_stream(PNG),
        )
        .await
        .unwrap_err();

    assert!(unsupported.is_code(ErrorCode::UnsupportedMediaType));
    assert!(ready_failure.is_internal());
    assert!(files
        .store
        .list_incomplete_files(None, 100)
        .await
        .unwrap()
        .is_empty());
}

#[tokio::test]
async fn failed_object_delete_remains_recoverable() {
    let files = TestFiles::new(|_| {}).await;
    let actor = Actor::regular("user-1");
    let uploaded = files.upload(&actor, FileScope::Personal).await;
    std::fs::remove_dir_all(files.root.path()).unwrap();
    std::fs::write(files.root.path(), b"not a directory").unwrap();

    let error = files
        .service
        .delete(
            &actor,
            FileDeleteRequest {
                file_id: uploaded.file_id.clone(),
            },
        )
        .await
        .unwrap_err();
    let incomplete = files.store.list_incomplete_files(None, 100).await.unwrap();

    assert!(error.is_internal());
    assert_eq!(
        incomplete
            .iter()
            .map(|record| (record.file_id.as_str(), record.status))
            .collect::<Vec<_>>(),
        vec![(
            uploaded.file_id.as_str(),
            talos_store::file::FileStatus::Deleting
        )]
    );

    std::fs::remove_file(files.root.path()).unwrap();
    std::fs::create_dir(files.root.path()).unwrap();
    files.service.reconcile_incomplete().await.unwrap();
    assert!(files
        .store
        .list_incomplete_files(None, 100)
        .await
        .unwrap()
        .is_empty());
}

#[tokio::test]
async fn recovery_rejects_records_for_unconfigured_storage() {
    let files = TestFiles::new(|_| {}).await;
    let record = talos_store::file::FileRecord {
        file_id: "file-unknown-storage".to_string(),
        owner_user_id: "user-1".to_string(),
        project_id: None,
        location: talos_store::file::FileLocation::Local {
            storage_id: "archive".to_string(),
            object_key: "files/file-unknown-storage/content.png".to_string(),
        },
        original_filename: "content.png".to_string(),
        mime_type: None,
        size_bytes: None,
        status: talos_store::file::FileStatus::Uploading,
        tags: Vec::new(),
        metadata: BTreeMap::new(),
        created_at: 1,
        updated_at: 1,
    };
    files.store.create_file(record.clone()).await.unwrap();

    let error = files.service.reconcile_incomplete().await.unwrap_err();

    assert!(error.is_code(talos_error::ErrorCode::InvalidRequest));
    assert_eq!(
        files.store.list_incomplete_files(None, 100).await.unwrap(),
        vec![record]
    );
}

#[tokio::test]
async fn recovery_removes_more_than_one_bounded_batch() {
    let files = TestFiles::new(|_| {}).await;
    for index in 0..101 {
        let file_id = format!("file-{index:03}");
        files
            .store
            .create_file(talos_store::file::FileRecord {
                file_id: file_id.clone(),
                owner_user_id: "user-1".to_string(),
                project_id: None,
                location: talos_store::file::FileLocation::Local {
                    storage_id: "local".to_string(),
                    object_key: format!("files/{file_id}/content.png"),
                },
                original_filename: "content.png".to_string(),
                mime_type: None,
                size_bytes: None,
                status: talos_store::file::FileStatus::Uploading,
                tags: Vec::new(),
                metadata: BTreeMap::new(),
                created_at: index,
                updated_at: index,
            })
            .await
            .unwrap();
    }

    files.service.reconcile_incomplete().await.unwrap();

    assert!(files
        .store
        .list_incomplete_files(None, 100)
        .await
        .unwrap()
        .is_empty());
}

#[tokio::test]
async fn markdown_upload_reads_original_bytes_and_keeps_owner_and_allowlist_checks() {
    let files = TestFiles::new(|_| {}).await;
    let owner = Actor::regular("markdown-owner");
    let content =
        "# 模块化工作区\n\n| Resource | Owner |\n| --- | --- |\n| Markdown | FileService |\n";
    let uploaded = files
        .service
        .upload(
            &owner,
            FileUploadRequest {
                filename: "workspace.Markdown".into(),
                scope: FileScope::Personal,
                tags: vec![],
                metadata: BTreeMap::new(),
            },
            Box::pin(stream::iter(
                content
                    .as_bytes()
                    .chunks(1)
                    .map(|chunk| Ok(Bytes::copy_from_slice(chunk)))
                    .collect::<Vec<_>>(),
            )),
        )
        .await
        .unwrap()
        .file;
    assert_eq!(uploaded.mime_type, "text/markdown");
    assert_eq!(uploaded.size_bytes, content.len() as u64);
    let request = FileReadRequest {
        file_id: uploaded.file_id,
    };
    let read = files
        .service
        .read_content(&owner, request.clone())
        .await
        .unwrap();
    assert_eq!(read.bytes.as_ref(), content.as_bytes());
    let error = files
        .service
        .read_content(&Actor::regular("other-user"), request)
        .await
        .err()
        .unwrap();
    assert!(error.is_code(ErrorCode::ResourceNotFound));

    let restricted = TestFiles::new(|config| {
        config.allowed_mime_types.remove("text/markdown");
    })
    .await;
    let error = restricted
        .service
        .upload(
            &owner,
            FileUploadRequest {
                filename: "review.md".into(),
                scope: FileScope::Personal,
                tags: vec![],
                metadata: BTreeMap::new(),
            },
            bytes_stream(b"# Review\n"),
        )
        .await
        .unwrap_err();
    assert!(error.is_code(ErrorCode::UnsupportedMediaType));
    assert!(restricted
        .service
        .list(&owner, FileListRequest::default())
        .await
        .unwrap()
        .data
        .is_empty());
}
