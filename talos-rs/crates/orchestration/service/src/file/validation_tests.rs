use std::collections::BTreeMap;

use super::{filename, metadata, normalized_suffix, tags};
use talos_config::FileStorageLimits;

fn is_invalid_request<T>(result: talos_error::TalosResult<T>) -> bool {
    result.is_err_and(|error| error.is_code(talos_error::ErrorCode::InvalidRequest))
}

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
    assert!(is_invalid_request(filename("toolong".to_string(), &limits)));
    assert!(is_invalid_request(filename("a/b".to_string(), &limits)));
    assert_eq!(tags(vec![" a ".to_string()], &limits).unwrap(), vec!["a"]);
    assert!(is_invalid_request(tags(
        vec!["a".to_string(), "b".to_string()],
        &limits
    )));
    assert!(is_invalid_request(tags(
        vec!["a".to_string(), " a ".to_string()],
        &FileStorageLimits::default()
    )));
    assert!(metadata(
        &BTreeMap::from([("key".to_string(), "val".to_string())]),
        &limits
    )
    .is_ok());
    assert!(is_invalid_request(metadata(
        &BTreeMap::from([(" key".to_string(), "v".to_string())]),
        &limits
    )));
    assert!(is_invalid_request(metadata(
        &BTreeMap::from([("key".to_string(), "long".to_string())]),
        &limits
    )));
    let total_limit = FileStorageLimits {
        max_metadata_bytes: 5,
        ..limits
    };
    assert!(is_invalid_request(metadata(
        &BTreeMap::from([("k".to_string(), "v".to_string())]),
        &total_limit
    )));
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
