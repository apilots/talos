use crate::{Attachment, AttachmentKind, CacheControl};
use serde::Serialize;
use serde_json::Value;

#[derive(Debug, Clone, Serialize)]
struct MessagesBase64Source {
    #[serde(rename = "type")]
    source_type: &'static str,
    media_type: String,
    data: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum MessagesAttachmentBlock {
    Image { source: MessagesBase64Source },
    Document { source: MessagesBase64Source },
}

pub fn text_block(text: String, cache_control: Option<CacheControl>) -> Value {
    serde_json::to_value(super::MessagesTextBlock {
        block_type: "text".to_string(),
        text,
        cache_control,
    })
    .expect("messages text block should serialize")
}

pub(super) fn user_content(
    text: String,
    cache_control: Option<CacheControl>,
    attachments: Option<Vec<Attachment>>,
) -> Vec<Value> {
    let mut content = Vec::new();
    if !text.is_empty() {
        content.push(text_block(text, cache_control));
    }
    if let Some(attachments) = attachments {
        content.extend(attachments.into_iter().map(|attachment| {
            serde_json::to_value(attachment_block(attachment))
                .expect("messages attachment block should serialize")
        }));
    }
    content
}

pub(super) fn tool_content(text: String, attachments: Option<Vec<Attachment>>) -> Value {
    if attachments.is_some() {
        Value::Array(user_content(text, None, attachments))
    } else {
        Value::String(text)
    }
}

fn attachment_block(attachment: Attachment) -> MessagesAttachmentBlock {
    let source = MessagesBase64Source {
        source_type: "base64",
        media_type: attachment.media_type,
        data: super::super::base64_data(&attachment.bytes),
    };
    match attachment.kind {
        AttachmentKind::Image => MessagesAttachmentBlock::Image { source },
        AttachmentKind::File => MessagesAttachmentBlock::Document { source },
    }
}
