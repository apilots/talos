use std::collections::BTreeMap;

use talos_config::{
    FileStorageBackendConfig, FileStorageConfig, FsFileStorageConfig, S3FileStorageConfig,
};

use super::build_file_operators;

#[tokio::test]
async fn configured_file_backends_build_one_operator_per_storage_id() {
    let root = tempfile::tempdir().unwrap();
    let config = FileStorageConfig {
        default: "local".to_string(),
        backends: BTreeMap::from([
            (
                "local".to_string(),
                FileStorageBackendConfig::Fs(FsFileStorageConfig {
                    root: root.path().to_path_buf(),
                }),
            ),
            (
                "archive".to_string(),
                FileStorageBackendConfig::S3(S3FileStorageConfig {
                    root: Some("talos-files".to_string()),
                    bucket: "talos-files".to_string(),
                    region: Some("us-east-1".to_string()),
                    endpoint: Some("http://127.0.0.1:9000".to_string()),
                    access_key_id: Some("access-key".to_string()),
                    secret_access_key: Some("secret-key".to_string()),
                }),
            ),
        ]),
        ..FileStorageConfig::default()
    };

    let operators = build_file_operators(&config).unwrap();
    let local = operators.get("local").unwrap();
    local.write("probe.txt", "ready").await.unwrap();

    assert_eq!(
        (
            operators.keys().cloned().collect::<Vec<_>>(),
            local.read("probe.txt").await.unwrap().to_vec(),
            operators.get("archive").unwrap().info().scheme(),
            operators.get("archive").unwrap().info().root(),
        ),
        (
            vec!["archive".to_string(), "local".to_string()],
            b"ready".to_vec(),
            "s3",
            "/talos-files/".to_string(),
        )
    );
}
