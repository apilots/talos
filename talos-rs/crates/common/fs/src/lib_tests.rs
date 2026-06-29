use super::*;

#[derive(Debug, Serialize, serde::Deserialize, PartialEq, Eq)]
struct Sample {
    name: String,
}

#[test]
fn write_file_replaces_file_contents() {
    let dir = tempfile::tempdir().expect("temp dir should be created");
    let path = dir.path().join("state.txt");

    write_bytes(&path, b"old").expect("initial write should succeed");
    write_bytes(&path, b"new").expect("replace write should succeed");

    assert_eq!(
        std_fs()
            .read_file(&path)
            .expect("file should read")
            .expect("file should exist"),
        "new"
    );
}

#[test]
fn create_new_rejects_existing_file() {
    let dir = tempfile::tempdir().expect("temp dir should be created");
    let path = dir.path().join("state.txt");
    write_bytes(&path, b"old").expect("initial write should succeed");

    let err = std_fs()
        .write_file_with(&path, "new", WriteOptions::create_new())
        .expect_err("create-new should reject existing files");

    assert!(matches!(err, TalosError::Conflict(_)));
    assert_eq!(
        std_fs()
            .read_file(&path)
            .expect("file should read")
            .expect("file should exist"),
        "old"
    );
}

#[test]
fn json_helpers_round_trip_pretty_json() {
    let dir = tempfile::tempdir().expect("temp dir should be created");
    let path = dir.path().join("state.json");

    write_json_pretty(
        &path,
        &Sample {
            name: "demo".to_string(),
        },
    )
    .expect("json write should succeed");

    let value: Sample = read_json(&path).expect("json read should succeed");
    assert_eq!(
        value,
        Sample {
            name: "demo".to_string()
        }
    );
    assert!(std_fs()
        .read_file(&path)
        .expect("json file should be readable")
        .expect("json file should exist")
        .contains('\n'));
}

#[test]
fn copy_file_creates_parent_and_copies_contents() {
    let dir = tempfile::tempdir().expect("temp dir should be created");
    let source = dir.path().join("source.txt");
    let target = dir.path().join("nested").join("target.txt");
    write_bytes(&source, b"copy me").expect("source file should write");

    let copied = copy_file(&source, &target).expect("file should copy");

    assert_eq!(copied, "copy me".len() as u64);
    assert_eq!(
        std_fs()
            .read_file(&target)
            .expect("target file should read")
            .expect("target file should exist"),
        "copy me"
    );
}

#[tokio::test]
async fn file_helpers_read_write_and_metadata() {
    let dir = tempfile::tempdir().expect("temp dir should be created");
    let path = dir.path().join("state.txt");

    assert!(read_file(&path)
        .await
        .expect("missing file should read")
        .is_none());

    write_file(&path, "content")
        .await
        .expect("file should write");

    assert_eq!(
        read_file(&path).await.expect("file should read"),
        Some("content".to_string())
    );
    let metadata = get_file_metadata(&path)
        .await
        .expect("metadata should read")
        .expect("metadata should exist");
    assert_eq!(metadata.size_bytes, "content".len() as u64);
    assert!(metadata.is_file);
    assert!(!metadata.is_dir);
}

#[tokio::test]
async fn with_file_lock_runs_operation() {
    let dir = tempfile::tempdir().expect("temp dir should be created");
    let path = dir.path().join("state.txt");

    let value = with_file_lock(&path, || async { Ok::<_, TalosError>(42) })
        .await
        .expect("locked operation should run");

    assert_eq!(value, 42);
}
