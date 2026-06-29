use super::{date_in_range, write_memory_file_transaction, MemoryDate};
use talos_error::TalosError;

#[test]
fn date_range_applies_before_date_after_primary_range_filter() {
    let target = MemoryDate::parse("2026-04-04").expect("date should be valid");
    let date_from = Some(MemoryDate::parse("2026-04-01").expect("date should be valid"));
    let date_to = Some(MemoryDate::parse("2026-04-30").expect("date should be valid"));

    assert!(date_in_range(target, date_from, date_to, None));
    assert!(date_in_range(
        target,
        date_from,
        date_to,
        Some(MemoryDate::parse("2026-04-05").expect("date should be valid"))
    ));
    assert!(!date_in_range(
        target,
        date_from,
        date_to,
        Some(MemoryDate::parse("2026-04-04").expect("date should be valid"))
    ));
    assert!(!date_in_range(
        target,
        date_from,
        date_to,
        Some(MemoryDate::parse("2026-04-03").expect("date should be valid"))
    ));
}

#[tokio::test]
async fn write_transaction_removes_new_file_on_rebuild_failure() {
    let temp_dir =
        std::env::temp_dir().join(format!("talos-memory-files-test-{}", uuid::Uuid::new_v4()));
    let path = temp_dir.join("memory").join("2026-04-04.md");

    let result = write_memory_file_transaction(&path, "new content", None, &|| async {
        Err(TalosError::StorageError(
            "forced rebuild failure".to_string(),
        ))
    })
    .await;

    assert!(result.is_err());
    assert!(!path.exists(), "new file should be removed after rollback");
    let _ = talos_fs::remove_directory(&temp_dir).await;
}

#[tokio::test]
async fn write_transaction_restores_previous_content_on_rebuild_failure() {
    let temp_dir =
        std::env::temp_dir().join(format!("talos-memory-files-test-{}", uuid::Uuid::new_v4()));
    let path = temp_dir.join("memory").join("2026-04-04.md");
    talos_fs::write_file(&path, "old content")
        .await
        .expect("seed file should write");

    let result =
        write_memory_file_transaction(&path, "new content", Some("old content"), &|| async {
            Err(TalosError::StorageError(
                "forced rebuild failure".to_string(),
            ))
        })
        .await;

    assert!(result.is_err());
    let restored = talos_fs::read_file(&path)
        .await
        .expect("restored file should read")
        .expect("restored file should exist");
    assert_eq!(restored, "old content");
    let _ = talos_fs::remove_directory(&temp_dir).await;
}
