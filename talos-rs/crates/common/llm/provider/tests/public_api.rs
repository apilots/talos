use bytes::Bytes;
use talos_llm_provider::{Attachment, AttachmentKind, ChatMessage, Role};

#[test]
fn downstream_constructs_text_and_attachment_messages_through_public_apis() {
    let text = ChatMessage::user("hello");
    let attachment = Attachment::new(
        AttachmentKind::Image,
        "diagram.png",
        "image/png",
        Bytes::from_static(b"png"),
    );
    let attached = ChatMessage::user_with_attachments("inspect", vec![attachment.clone()])
        .expect("a non-empty user attachment must be accepted");

    assert_eq!(text, ChatMessage::user("hello"));
    assert_eq!(attached.role, Role::User);
    assert_eq!(attached.content, "inspect");
    assert_eq!(attached.attachments, Some(vec![attachment]));
}
