use std::collections::BTreeMap;

use talos_config::FileStorageLimits;
use talos_error::TalosError;

use super::{filename, metadata, normalized_suffix, tags};

fn tight_limits() -> FileStorageLimits {
    FileStorageLimits {
        max_upload_bytes: 10,
        max_files_per_turn: 1,
        max_turn_file_bytes: 10,
        max_filename_bytes: 5,
        max_tags: 1,
        max_tag_bytes: 3,
        max_metadata_entries: 1,
        max_metadata_key_bytes: 3,
        max_metadata_value_bytes: 3,
        max_metadata_bytes: 16,
    }
}

#[test]
fn file_metadata_limits_have_one_deterministic_validation_boundary() {
    let limits = tight_limits();

    assert_eq!(filename(" ok ".to_string(), &limits).unwrap(), "ok");
    assert!(matches!(
        filename("toolong".to_string(), &limits),
        Err(TalosError::ValidationError(_))
    ));
    assert!(matches!(
        filename("a/b".to_string(), &limits),
        Err(TalosError::ValidationError(_))
    ));
    assert_eq!(tags(vec![" a ".to_string()], &limits).unwrap(), vec!["a"]);
    assert!(matches!(
        tags(vec!["a".to_string(), "b".to_string()], &limits),
        Err(TalosError::ValidationError(_))
    ));
    assert!(matches!(
        tags(
            vec!["a".to_string(), " a ".to_string()],
            &FileStorageLimits::default()
        ),
        Err(TalosError::ValidationError(_))
    ));
    assert!(metadata(
        &BTreeMap::from([("key".to_string(), "val".to_string())]),
        &limits
    )
    .is_ok());
    assert!(matches!(
        metadata(
            &BTreeMap::from([(" key".to_string(), "v".to_string())]),
            &limits
        ),
        Err(TalosError::ValidationError(_))
    ));
    assert!(matches!(
        metadata(
            &BTreeMap::from([("key".to_string(), "long".to_string())]),
            &limits
        ),
        Err(TalosError::ValidationError(_))
    ));
    let total_limit = FileStorageLimits {
        max_metadata_bytes: 5,
        ..limits
    };
    assert!(matches!(
        metadata(
            &BTreeMap::from([("k".to_string(), "v".to_string())]),
            &total_limit
        ),
        Err(TalosError::ValidationError(_))
    ));
}

#[test]
fn object_suffix_is_normalized_only_for_the_closed_safe_alphabet() {
    assert_eq!(
        (
            normalized_suffix("Report.PDF"),
            normalized_suffix("archive.tar.gz"),
            normalized_suffix("unsafe.file-name"),
            normalized_suffix("no-extension"),
        ),
        (
            ".pdf".to_string(),
            ".gz".to_string(),
            String::new(),
            String::new(),
        )
    );
}
