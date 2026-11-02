use super::*;
use talos_llm_provider::{Attachment, AttachmentKind};

#[tokio::test]
async fn attachment_capture_keeps_user_and_tool_file_ids_without_binary_bodies() {
    let (writer, capture) = setup();
    let recorder = capture.recorder();
    let mut prepared = ChatMessage::tool("call", "screenshot", "image result");
    prepared.image_file_ids = vec!["tool-image".into()];
    recorder.prepared_tool_result(&prepared);

    let mut document = Attachment::new(
        AttachmentKind::File,
        "note.pdf",
        "application/pdf",
        b"%PDF".as_slice().into(),
    );
    document.file_id = Some("user-file".into());
    let user = ChatMessage::user_with_attachments("read", vec![document]).unwrap();
    let mut consumed = prepared.clone();
    consumed.attachments = Some(vec![Attachment::new(
        AttachmentKind::Image,
        "screen.png",
        "image/png",
        b"image-binary".as_slice().into(),
    )]);
    let request = ChatRequest {
        messages: vec![user, consumed],
        ..ChatRequest::new_user("", "model")
    };
    let observation = recorder
        .start_model(KernelObservedPurpose::Working, "connection", &request)
        .unwrap();
    observation.model_event(&talos_llm_provider::ModelBlockStreamEvent::Done {
        finish_reason: Some("stop".into()),
        usage: None,
    });
    observation.finished(None);
    drop(observation);
    drop(recorder);
    capture.flush(&writer).await;

    let records = writer.records.lock().unwrap();
    let payloads = records
        .iter()
        .flat_map(|batch| batch.payloads.values())
        .collect::<Vec<_>>();
    let prepared = payloads
        .iter()
        .find_map(|p| match p {
            TrajectoryPayload::PreparedResult { message } => Some(message),
            _ => None,
        })
        .unwrap();
    assert_eq!(
        prepared.attachments,
        Some(vec![SnapshotAttachment {
            file_id: Some("tool-image".into()),
            filename: None,
            kind: Some(SnapshotAttachmentKind::Image),
            mime_type: None,
            size_bytes: None,
            availability: SnapshotAvailability::NotRetained,
        }])
    );
    let messages = payloads
        .iter()
        .find_map(|p| match p {
            TrajectoryPayload::Prompt { messages } => Some(messages),
            _ => None,
        })
        .unwrap();
    assert_eq!(
        messages[0].attachments,
        Some(vec![SnapshotAttachment {
            file_id: Some("user-file".into()),
            filename: Some("note.pdf".into()),
            kind: Some(SnapshotAttachmentKind::File),
            mime_type: Some("application/pdf".into()),
            size_bytes: Some(4.0),
            availability: SnapshotAvailability::NotRetained,
        }])
    );
    assert_eq!(
        messages[1].attachments,
        Some(vec![SnapshotAttachment {
            file_id: Some("tool-image".into()),
            filename: Some("screen.png".into()),
            kind: Some(SnapshotAttachmentKind::Image),
            mime_type: Some("image/png".into()),
            size_bytes: Some(12.0),
            availability: SnapshotAvailability::NotRetained,
        }])
    );
    let serialized = serde_json::to_string(&payloads).unwrap();
    assert!(!serialized.contains("image-binary"));
    assert!(!serialized.contains("%PDF"));
}
