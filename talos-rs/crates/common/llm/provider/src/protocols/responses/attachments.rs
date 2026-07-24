use crate::{Attachment, AttachmentKind};
use serde::Serialize;
use serde_json::Value;

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ResponsesInputAttachmentBlock {
    InputImage { image_url: String },
    InputFile { filename: String, file_data: String },
}

pub(super) fn user_content(text: String, attachments: Option<Vec<Attachment>>) -> Vec<Value> {
    let mut content = Vec::new();
    if !text.is_empty() {
        content.push(serde_json::json!({
            "type": "input_text",
            "text": text,
        }));
    }
    if let Some(attachments) = attachments {
        content.extend(attachments.into_iter().map(attachment_block).map(|block| {
            serde_json::to_value(block).expect("responses attachment block should serialize")
        }));
    }
    content
}

fn attachment_block(attachment: Attachment) -> ResponsesInputAttachmentBlock {
    let data_url = super::super::data_url(&attachment.media_type, &attachment.bytes);
    match attachment.kind {
        AttachmentKind::Image => ResponsesInputAttachmentBlock::InputImage {
            image_url: data_url,
        },
        AttachmentKind::File => ResponsesInputAttachmentBlock::InputFile {
            filename: attachment.filename,
            file_data: data_url,
        },
    }
}
