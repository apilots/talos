use super::*;
use std::collections::BTreeMap;
use talos_storage::SqliteStorage;
use talos_store::{file::DatabaseFileStore, project::StorageProjectStore};

#[tokio::test]
async fn screenshots_use_owned_files_and_preserve_completed_uploads_on_later_failure() {
    let storage = Arc::new(SqliteStorage::new(":memory:").await.unwrap());
    let migrations =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../../conf/migrations/sqlite");
    talos_store::migration::run_migrations(storage.pool(), &migrations)
        .await
        .unwrap();
    let root = tempfile::tempdir().unwrap();
    let operator = opendal::Operator::new(
        opendal::services::Fs::default().root(root.path().to_str().unwrap()),
    )
    .unwrap()
    .finish();
    let service = Arc::new(
        FileService::new(
            Arc::new(DatabaseFileStore::new(storage.pool().clone())),
            Arc::new(StorageProjectStore::new(storage).await.unwrap()),
            BTreeMap::from([("local".into(), operator)]),
            Default::default(),
        )
        .unwrap(),
    );
    let images = ToolImages(service.clone());
    let png = Attachment::new(
        AttachmentKind::Image,
        "screenshot.png",
        "image/png",
        Vec::from(&b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR"[..]).into(),
    );
    let saved = images.save("owner", vec![png.clone()]).await.unwrap();
    let ids = vec![saved[0].file_id.clone()];
    assert_eq!(images.read("owner", &ids).await.unwrap(), vec![png.clone()]);
    assert!(images
        .read("other-user", &ids)
        .await
        .unwrap_err()
        .is_code(talos_error::ErrorCode::ResourceNotFound));
    let mut mislabeled = png.clone();
    mislabeled.media_type = "image/jpeg".into();
    assert!(images
        .save("owner", vec![png, mislabeled])
        .await
        .unwrap_err()
        .is_code(talos_error::ErrorCode::UnsupportedMediaType));
    let listed = service
        .list(&Actor::regular("owner"), Default::default())
        .await
        .unwrap();
    assert_eq!(
        listed.data.len(),
        3,
        "completed files remain manageable after a later MIME mismatch"
    );
    assert!(listed.data.iter().all(|file| file.mime_type == "image/png"));
}
