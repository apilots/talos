use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use super::{
    FileStorageBackendConfig, FileStorageConfig, FileStorageLimits, FsFileStorageConfig,
    S3FileStorageConfig, StorageBackendConfig,
};
use crate::Config;

#[test]
fn default_config_resolves_the_local_file_root() {
    let config = Config::default();

    assert_eq!(
        config.storage.files,
        FileStorageConfig {
            default: "local".to_string(),
            backends: BTreeMap::from([(
                "local".to_string(),
                FileStorageBackendConfig::Fs(FsFileStorageConfig {
                    root: config.talos_paths().data_dir.join("files"),
                }),
            )]),
            limits: FileStorageLimits::default(),
            allowed_mime_types: BTreeSet::from([
                "application/msword".to_string(),
                "application/pdf".to_string(),
                "application/vnd.ms-excel".to_string(),
                "application/vnd.openxmlformats-officedocument.presentationml.presentation"
                    .to_string(),
                "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet".to_string(),
                "application/vnd.openxmlformats-officedocument.wordprocessingml.document"
                    .to_string(),
                "image/jpeg".to_string(),
                "image/png".to_string(),
                "image/webp".to_string(),
                "text/csv".to_string(),
            ]),
        }
    );
}

#[test]
fn s3_file_backend_deserializes_as_a_closed_variant() {
    let storage: StorageBackendConfig = serde_yaml::from_str(
        r#"
files:
  default: archive
  backends:
    archive:
      type: s3
      root: /tenant-files
      bucket: talos-files
      region: us-east-1
      endpoint: https://s3.example.com
      access_key_id: key
      secret_access_key: secret
"#,
    )
    .unwrap();

    assert_eq!(
        storage.files.backends.get("archive"),
        Some(&FileStorageBackendConfig::S3(S3FileStorageConfig {
            root: Some("/tenant-files".to_string()),
            bucket: "talos-files".to_string(),
            region: Some("us-east-1".to_string()),
            endpoint: Some("https://s3.example.com".to_string()),
            access_key_id: Some("key".to_string()),
            secret_access_key: Some("secret".to_string()),
        }))
    );
}

#[test]
fn relative_file_roots_resolve_from_the_talos_data_directory() {
    let mut config = Config::default();
    config.storage.files.backends.insert(
        "custom".to_string(),
        FileStorageBackendConfig::Fs(FsFileStorageConfig {
            root: PathBuf::from("custom-files"),
        }),
    );

    config.apply_derived_paths();

    assert_eq!(
        config.storage.files.backends.get("custom"),
        Some(&FileStorageBackendConfig::Fs(FsFileStorageConfig {
            root: config.talos_paths().data_dir.join("custom-files"),
        }))
    );
}

#[test]
fn file_storage_rejects_a_missing_default_backend() {
    let mut config = Config::default();
    config.storage.files.default = "missing".to_string();

    let error = config.validate_runtime().unwrap_err();

    assert!(error
        .to_string()
        .contains("storage.files.default references unknown backend"));
}

#[test]
fn file_storage_rejects_partial_s3_credentials() {
    let mut config = Config::default();
    config.storage.files.backends.insert(
        "archive".to_string(),
        FileStorageBackendConfig::S3(S3FileStorageConfig {
            root: None,
            bucket: "talos-files".to_string(),
            region: None,
            endpoint: None,
            access_key_id: Some("key".to_string()),
            secret_access_key: None,
        }),
    );

    let error = config.validate_runtime().unwrap_err();

    assert!(error
        .to_string()
        .contains("access_key_id and secret_access_key must be configured together"));
}

#[test]
fn file_storage_rejects_s3_root_dot_segments() {
    let mut config = Config::default();
    config.storage.files.backends.insert(
        "archive".to_string(),
        FileStorageBackendConfig::S3(S3FileStorageConfig {
            root: Some("tenant/../files".to_string()),
            bucket: "talos-files".to_string(),
            region: None,
            endpoint: None,
            access_key_id: None,
            secret_access_key: None,
        }),
    );

    let error = config.validate_runtime().unwrap_err();

    assert!(error
        .to_string()
        .contains("storage.files.backends.archive.root must not contain `.` or `..` segments"));
}

#[test]
fn file_storage_rejects_zero_limits() {
    let mut config = Config::default();
    config.storage.files.limits.max_files_per_turn = 0;

    let error = config.validate_runtime().unwrap_err();

    assert!(error
        .to_string()
        .contains("storage.files.limits.max_files_per_turn must be > 0"));
}

#[test]
fn file_storage_rejects_mime_types_unknown_to_the_detector() {
    let mut config = Config::default();
    config
        .storage
        .files
        .allowed_mime_types
        .insert("application/x-unknown-talos".to_string());

    let error = config.validate_runtime().unwrap_err();

    assert!(error
        .to_string()
        .contains("storage.files.allowed_mime_types contains unsupported MIME"));
}

#[test]
fn file_storage_accepts_suffix_and_parser_validated_mime_types() {
    let mut config = Config::default();
    config.storage.files.allowed_mime_types = BTreeSet::from([
        "application/msword".to_string(),
        "application/vnd.ms-excel".to_string(),
        "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet".to_string(),
        "application/vnd.openxmlformats-officedocument.wordprocessingml.document".to_string(),
        "text/csv".to_string(),
    ]);

    config.validate_runtime().unwrap();
}
