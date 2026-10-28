use super::*;
use std::io::Write;

#[tokio::test]
async fn archive_members_share_metadata_and_bounded_reads_without_workspace_extraction() {
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    zip.start_file("docs/notes.txt", zip::write::SimpleFileOptions::default())
        .unwrap();
    zip.write_all(b"archive preview").unwrap();
    let bytes = zip.finish().unwrap().into_inner();
    let metadata = FilePreviewMetadata {
        name: "bundle.zip".into(),
        mime_type: "application/zip".into(),
        size_bytes: bytes.len() as u64,
    };
    let (view, prepared) = prepare(&metadata, bytes.into(), 1024).await;
    let FilePreviewDocument::Archive { entries } = view else {
        panic!("archive expected")
    };
    assert_eq!(entries[0].path, "docs/notes.txt");
    let resource = prepared.unwrap();
    let content = resource.read(&entries[0].resource_path, 8, 7).unwrap();
    assert_eq!(content.data.as_ref(), b"preview");
    assert!(resource
        .read(&entries[0].resource_path, 0, 0)
        .unwrap()
        .data
        .is_empty());
}

#[tokio::test]
async fn tar_and_gzip_tar_use_the_same_member_contract() {
    let mut builder = tar::Builder::new(Vec::new());
    let mut header = tar::Header::new_gnu();
    header.set_size(7);
    header.set_mode(0o600);
    header.set_cksum();
    builder
        .append_data(&mut header, "docs/a.txt", Cursor::new(b"preview"))
        .unwrap();
    let bytes = builder.into_inner().unwrap();
    let mut gzip = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    gzip.write_all(&bytes).unwrap();
    for (name, mime, bytes) in [
        ("bundle.tar", "application/x-tar", bytes),
        ("bundle.tar.gz", "application/gzip", gzip.finish().unwrap()),
    ] {
        let metadata = FilePreviewMetadata {
            name: name.into(),
            mime_type: mime.into(),
            size_bytes: bytes.len() as u64,
        };
        let (view, resource) = prepare(&metadata, bytes.into(), 4096).await;
        let FilePreviewDocument::Archive { entries } = view else {
            panic!("archive expected")
        };
        let content = resource
            .unwrap()
            .read(&entries[0].resource_path, 0, 7)
            .unwrap();
        assert_eq!(content.metadata.mime_type, "text/plain");
        assert_eq!(content.data.as_ref(), b"preview");
    }
}

#[tokio::test]
async fn archive_browser_paths_and_source_metadata_share_the_resource_contract() {
    let members = [
        ("./docs/source.ts", "export const value = '模块';"),
        ("./docs/index.html", "<p>preview</p>"),
    ];
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let mut tar = tar::Builder::new(Vec::new());
    for (name, content) in members {
        zip.start_file(name, zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(content.as_bytes()).unwrap();
        let mut header = tar::Header::new_gnu();
        // Preserve the literal ./ entry emitted by `tar -cf bundle.tar .`.
        header.as_mut_bytes()[..name.len()].copy_from_slice(name.as_bytes());
        header.set_size(content.len() as u64);
        header.set_mode(0o600);
        header.set_cksum();
        tar.append(&header, Cursor::new(content.as_bytes()))
            .unwrap();
    }
    let tar_bytes = tar.into_inner().unwrap();
    let mut gzip = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    gzip.write_all(&tar_bytes).unwrap();
    for (name, mime, bytes) in [
        (
            "bundle.zip",
            "application/zip",
            zip.finish().unwrap().into_inner(),
        ),
        ("bundle.tar", "application/x-tar", tar_bytes),
        ("bundle.tgz", "application/gzip", gzip.finish().unwrap()),
    ] {
        let metadata = FilePreviewMetadata {
            name: name.into(),
            mime_type: mime.into(),
            size_bytes: bytes.len() as u64,
        };
        let (view, resource) = prepare(&metadata, bytes.into(), 8192).await;
        let FilePreviewDocument::Archive { entries } = view else {
            panic!("archive expected")
        };
        let resource = resource.unwrap();
        for entry in entries {
            let url = url::Url::parse(&format!("https://preview.invalid/{}", entry.resource_path))
                .unwrap();
            let normalized = url.path().trim_start_matches('/');
            let expected = members
                .iter()
                .find(|(name, _)| *name == entry.path)
                .unwrap()
                .1
                .as_bytes();
            let info = resource.read(normalized, 0, 0).unwrap();
            assert!(info.data.is_empty());
            assert_eq!(
                info.metadata.mime_type,
                if entry.path.ends_with(".ts") {
                    "text/plain"
                } else {
                    "text/html"
                }
            );
            assert_eq!(
                resource.read(normalized, 7, 8).unwrap().data.as_ref(),
                &expected[7..expected.len().min(15)]
            );
        }
    }
}
