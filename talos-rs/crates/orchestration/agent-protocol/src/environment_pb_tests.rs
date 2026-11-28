use super::*;

#[test]
fn preview_conversion_contract_preserves_chunk_and_document_results() {
    let metadata = FilePreviewMetadata {
        name: "report.docx".into(),
        mime_type: "application/vnd.openxmlformats-officedocument.wordprocessingml.document".into(),
        size_bytes: 7,
    };
    for request in [
        WorkerPreviewRequest::OpenFile {
            metadata,
            expires_at: 123,
            max_bytes: 456,
        },
        WorkerPreviewRequest::Write {
            handle: "worker-handle".into(),
            offset: 42,
            data: b"office".to_vec().into(),
        },
        WorkerPreviewRequest::PrepareDocument {
            handle: "worker-handle".into(),
            max_bytes: 456,
        },
    ] {
        assert_eq!(
            decode_preview_read(Some(encode_preview_read(request.clone()).unwrap())).unwrap(),
            request
        );
    }
    for response in [
        WorkerPreviewResponse::Written,
        WorkerPreviewResponse::Document(FilePreviewDocument::Pdf {
            resource_path: "__preview__/document.pdf".into(),
        }),
        WorkerPreviewResponse::Document(FilePreviewDocument::Unavailable {
            reason: FilePreviewDocumentUnavailable::ConverterUnavailable,
        }),
        WorkerPreviewResponse::Document(FilePreviewDocument::Unavailable {
            reason: FilePreviewDocumentUnavailable::ConversionFailed,
        }),
        WorkerPreviewResponse::Document(FilePreviewDocument::Unavailable {
            reason: FilePreviewDocumentUnavailable::TooLarge,
        }),
    ] {
        assert_eq!(
            decode_preview_content(encode_preview_content(response.clone()).unwrap()).unwrap(),
            response
        );
    }
}
