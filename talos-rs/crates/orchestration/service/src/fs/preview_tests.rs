use super::*;

#[tokio::test]
async fn preview_handle_pins_the_directory_and_enforces_owner_and_close() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("workspace");
    std::fs::create_dir(&root).unwrap();
    std::fs::write(root.join("report.html"), b"<p>report</p>").unwrap();
    let resources = PreviewRoots::default();
    let result = resources
        .operate(
            "owner".into(),
            WorkerPreviewRequest::Open {
                root: root.clone(),
                path: "report.html".into(),
                expires_at: chrono::Utc::now().timestamp() + 60,
            },
        )
        .await
        .unwrap();
    let WorkerPreviewResponse::Opened {
        handle, metadata, ..
    } = result
    else {
        panic!("expected open")
    };
    assert_eq!(metadata.mime_type, "text/html");
    std::fs::rename(&root, temp.path().join("original")).unwrap();
    std::fs::create_dir(&root).unwrap();
    std::fs::write(root.join("report.html"), b"outside").unwrap();
    let read = WorkerPreviewRequest::Read {
        handle: handle.clone(),
        path: "report.html".into(),
        offset: 3,
        limit: 6,
    };
    let WorkerPreviewResponse::Content(content) = resources
        .operate("owner".into(), read.clone())
        .await
        .unwrap()
    else {
        panic!("expected content")
    };
    assert_eq!(content.data.as_ref(), b"report");
    assert!(resources
        .operate("other".into(), read.clone())
        .await
        .unwrap_err()
        .is_code(ErrorCode::ResourceNotFound));
    resources
        .operate("owner".into(), WorkerPreviewRequest::Close { handle })
        .await
        .unwrap();
    assert!(resources
        .operate("owner".into(), read)
        .await
        .unwrap_err()
        .is_code(ErrorCode::ResourceNotFound));
}

#[cfg(unix)]
#[tokio::test]
async fn preview_uses_scoped_symlinks_and_rejects_non_regular_files() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("source.txt"), b"inside").unwrap();
    std::os::unix::fs::symlink("source.txt", root.path().join("link.txt")).unwrap();
    let directory = talos_fs::RootedDirectory::open(root.path()).unwrap();
    assert_eq!(
        directory
            .read(std::path::Path::new("link.txt"), 0..6)
            .unwrap()
            .bytes,
        b"inside"
    );
    let outside = tempfile::NamedTempFile::new().unwrap();
    std::os::unix::fs::symlink(outside.path(), root.path().join("escape.txt")).unwrap();
    assert!(directory
        .read(std::path::Path::new("escape.txt"), 0..6)
        .is_err());
    let status = std::process::Command::new("mkfifo")
        .arg(root.path().join("pipe"))
        .status()
        .unwrap();
    assert!(status.success());
    assert!(directory
        .read(std::path::Path::new("pipe"), 0..6)
        .unwrap_err()
        .is_code(ErrorCode::InvalidRequest));
}

#[tokio::test(start_paused = true)]
async fn preview_directory_is_released_at_expiry_without_another_request() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(temp.path().join("file.txt"), b"preview").unwrap();
    let resources = PreviewRoots::default();
    let WorkerPreviewResponse::Opened { handle, .. } = resources
        .operate(
            "owner".into(),
            WorkerPreviewRequest::Open {
                root: temp.path().to_owned(),
                path: "file.txt".into(),
                expires_at: chrono::Utc::now().timestamp() + 1,
            },
        )
        .await
        .unwrap()
    else {
        panic!("expected open")
    };
    let WorkerPreviewResponse::Opened {
        handle: valid_handle,
        ..
    } = resources
        .operate(
            "owner".into(),
            WorkerPreviewRequest::Open {
                root: temp.path().to_owned(),
                path: "file.txt".into(),
                expires_at: chrono::Utc::now().timestamp() + 60,
            },
        )
        .await
        .unwrap()
    else {
        panic!("expected open")
    };
    // Expiry must reject reads even before the scheduled cleanup gets to run.
    resources
        .0
        .lock()
        .unwrap()
        .get_mut(&handle)
        .unwrap()
        .expires_at = chrono::Utc::now().timestamp() - 1;
    let read = |handle| WorkerPreviewRequest::Read {
        handle,
        path: "file.txt".into(),
        offset: 0,
        limit: 7,
    };
    assert!(resources
        .operate("owner".into(), read(handle.clone()))
        .await
        .unwrap_err()
        .is_code(ErrorCode::ResourceNotFound));
    let WorkerPreviewResponse::Content(content) = resources
        .operate("owner".into(), read(valid_handle.clone()))
        .await
        .unwrap()
    else {
        panic!("expected content")
    };
    assert_eq!(content.data.as_ref(), b"preview");
    tokio::task::yield_now().await;
    tokio::time::advance(std::time::Duration::from_secs(2)).await;
    tokio::task::yield_now().await;
    let roots = resources.0.lock().unwrap();
    assert!(!roots.contains_key(&handle));
    assert!(roots.contains_key(&valid_handle));
}

#[test]
fn bounded_metadata_recognizes_legacy_office_compound_containers() {
    let root = tempfile::tempdir().unwrap();
    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../../talos-e2e/fixtures/resource-preview");
    for (name, mime) in [
        ("matrix-word.doc", "application/msword"),
        ("matrix-sheet.xls", "application/vnd.ms-excel"),
        ("matrix-slides.ppt", "application/vnd.ms-powerpoint"),
    ] {
        if name.ends_with(".ppt") {
            let compressed = std::fs::read(fixture.join(format!("{name}.gz"))).unwrap();
            let mut bytes = Vec::new();
            std::io::Read::read_to_end(
                &mut flate2::read::GzDecoder::new(compressed.as_slice()),
                &mut bytes,
            )
            .unwrap();
            std::fs::write(root.path().join(name), bytes).unwrap();
        } else {
            std::fs::copy(fixture.join(name), root.path().join(name)).unwrap();
        }
        let directory = talos_fs::RootedDirectory::open(root.path()).unwrap();
        let read = directory.read(std::path::Path::new(name), 0..0).unwrap();
        assert!(read.bytes.is_empty());
        assert_eq!(content(read, 0).metadata.mime_type, mime);
    }
}

#[test]
fn source_text_takes_precedence_over_ambiguous_binary_filename_hints() {
    let root = tempfile::tempdir().unwrap();
    let mut long_source = vec![b'a'; 64 * 1024 - 1];
    long_source.extend_from_slice("模块".as_bytes());
    for (name, bytes) in [
        ("source.ts", "export type Name = '模块';".as_bytes()),
        ("README", b"generated source text".as_slice()),
        ("large.ts", long_source.as_slice()),
    ] {
        std::fs::write(root.path().join(name), bytes).unwrap();
        let directory = talos_fs::RootedDirectory::open(root.path()).unwrap();
        let read = directory.read(std::path::Path::new(name), 0..0).unwrap();
        assert_eq!(content(read, 0).metadata.mime_type, "text/plain");
    }
}
