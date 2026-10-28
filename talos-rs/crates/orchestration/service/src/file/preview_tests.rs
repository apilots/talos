use super::*;

#[test]
fn prepared_pdf_reuses_bounded_resource_reads() {
    let document = PreparedDocument::Pdf(Bytes::from_static(b"%PDF-preview"));
    assert!(document.handles(PDF_PATH));
    let part = document.read(PDF_PATH, 5, 7).unwrap();
    assert_eq!(part.data.as_ref(), b"preview");
    assert_eq!(part.metadata.mime_type, "application/pdf");
    assert_eq!(part.metadata.size_bytes, 12);
}
