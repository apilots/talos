use super::*;

#[test]
fn prepared_pdf_reuses_bounded_resource_reads() {
    let document = PreparedDocument {
        pdf: Bytes::from_static(b"%PDF-preview"),
    };
    assert!(document.handles(PDF_PATH));
    let part = document.read(5, 7);
    assert_eq!(part.data.as_ref(), b"preview");
    assert_eq!(part.metadata.mime_type, "application/pdf");
    assert_eq!(part.metadata.size_bytes, 12);
    assert_eq!(document.read(0, 0).data, Bytes::new());
    assert_eq!(document.read(9, u32::MAX).data.as_ref(), b"iew");
    assert_eq!(document.read(12, 1).data, Bytes::new());
    let beyond_end = document.read(u64::MAX, u32::MAX);
    assert_eq!(beyond_end.offset, u64::MAX);
    assert_eq!(beyond_end.data, Bytes::new());
}
