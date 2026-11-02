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

#[test]
fn markdown_validation_checks_complete_utf8_content_across_upload_chunks() {
    use super::MediaTypeValidator;

    for filename in ["review.md", "review.MarkDown"] {
        let mut validator = MediaTypeValidator::new(filename);
        // A character split between chunks remains valid after the complete body arrives.
        let content = "# Review\n\n模块边界\n".as_bytes();
        for byte in content {
            validator.observe(std::slice::from_ref(byte));
        }
        assert_eq!(validator.detected_mime_type(), Some("text/markdown".into()));
    }
    for tail in [b"\xff".as_slice(), b"\0", b"\x1b[31m", b"\xe4\xb8"] {
        let mut validator = MediaTypeValidator::new("review.md");
        validator.observe(&vec![b'a'; super::MIME_SNIFF_BYTES + 1]);
        validator.observe(tail);
        assert_eq!(validator.detected_mime_type(), None);
    }
    assert_eq!(
        MediaTypeValidator::new("empty.md").detected_mime_type(),
        Some("text/markdown".into())
    );
}

#[test]
fn generated_text_uses_complete_content_and_preserves_detected_binary_mime() {
    use super::MediaTypeValidator;

    let mut code = MediaTypeValidator::new("module.ts");
    for chunk in "export const label = '模块';\n".as_bytes().chunks(1) {
        code.observe(chunk);
    }
    assert_eq!(code.detected_mime_type(), Some("text/plain".into()));
    code.observe(&vec![b'a'; super::MIME_SNIFF_BYTES + 1]);
    code.observe(&[0xff]);
    assert_eq!(code.detected_mime_type(), None);

    let mut misleading_html = MediaTypeValidator::new("report.html");
    misleading_html.observe(b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR");
    misleading_html.observe(&vec![0; super::MIME_SNIFF_BYTES * 2]);
    assert!(misleading_html.content.len() <= super::MIME_SNIFF_BYTES);
    assert_eq!(
        misleading_html.detected_mime_type(),
        Some("image/png".into())
    );
}

#[test]
fn json_requires_parser_success_and_html_fragments_remain_browser_content() {
    use super::MediaTypeValidator;

    let mut json = MediaTypeValidator::new("result.json");
    json.observe(br#"{"preview":true}"#);
    assert_eq!(json.detected_mime_type(), Some("application/json".into()));
    json.observe(b" incomplete");
    assert_eq!(json.detected_mime_type(), Some("text/plain".into()));

    let mut html = MediaTypeValidator::new("report.HTML");
    html.observe(b"<section>Report</section>");
    assert_eq!(html.detected_mime_type(), Some("text/html".into()));
    html.observe(&[0]);
    assert_eq!(html.detected_mime_type(), None);
}

#[test]
fn presentation_detection_reads_the_zip_directory_beyond_the_sniff_window() {
    use super::MediaTypeValidator;
    use std::io::{Cursor, Write};

    let mut package = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options =
        zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    package
        .start_file("docProps/thumbnail.bin", options)
        .unwrap();
    package
        .write_all(&vec![0; super::MIME_SNIFF_BYTES + 1])
        .unwrap();
    package.start_file("ppt/presentation.xml", options).unwrap();
    package.write_all(br#"<p:presentation xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main"/>"#).unwrap();
    let bytes = package.finish().unwrap().into_inner();
    let mut validator = MediaTypeValidator::new("slides.PPTX");
    for chunk in bytes.chunks(8192) {
        validator.observe(chunk);
    }
    assert_eq!(
        validator.detected_mime_type(),
        Some("application/vnd.openxmlformats-officedocument.presentationml.presentation".into())
    );
}

#[test]
fn legacy_presentation_detection_has_the_complete_compound_file() {
    use super::MediaTypeValidator;
    // Shared real LibreOffice-generated fixture; its CFB directory is beyond
    // the magic-byte prefix, so infer needs the complete bounded upload.
    let bytes = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../../talos-e2e/fixtures/resource-preview/matrix-slides.ppt.gz"
    ));
    let mut decoded = Vec::new();
    std::io::Read::read_to_end(
        &mut flate2::read::GzDecoder::new(bytes.as_slice()),
        &mut decoded,
    )
    .unwrap();
    let mut validator = MediaTypeValidator::new("slides.ppt");
    for chunk in decoded.chunks(8192) {
        validator.observe(chunk);
    }
    assert_eq!(
        validator.detected_mime_type(),
        Some("application/vnd.ms-powerpoint".into())
    );
}
