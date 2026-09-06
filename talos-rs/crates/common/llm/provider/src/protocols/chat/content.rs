use crate::{Attachment, AttachmentKind, CacheControl};
use talos_error::{TalosError, TalosResult};

use super::{
    ChatCompletionContentBlock, ChatCompletionFile, ChatCompletionImageUrl,
    ChatCompletionMessageContent,
};

pub(super) fn message_content(
    content: String,
    cache_control: Option<CacheControl>,
    attachments: Option<Vec<Attachment>>,
    cache_control_enabled: bool,
) -> Option<ChatCompletionMessageContent> {
    if let Some(attachments) = attachments {
        let mut blocks = Vec::with_capacity(attachments.len().saturating_add(1));
        if !content.is_empty() {
            blocks.push(ChatCompletionContentBlock::Text {
                text: content,
                cache_control: cache_control_enabled.then_some(cache_control).flatten(),
            });
        }
        for attachment in attachments {
            let data_url = super::super::data_url(&attachment.media_type, &attachment.bytes);
            blocks.push(match attachment.kind {
                AttachmentKind::Image => ChatCompletionContentBlock::ImageUrl {
                    image_url: ChatCompletionImageUrl { url: data_url },
                },
                AttachmentKind::File => ChatCompletionContentBlock::File {
                    file: ChatCompletionFile {
                        filename: attachment.filename,
                        file_data: data_url,
                    },
                },
            });
        }
        return Some(ChatCompletionMessageContent::Blocks(blocks));
    }

    if cache_control_enabled {
        return match cache_control {
            Some(cache_control) => Some(ChatCompletionMessageContent::Blocks(vec![
                ChatCompletionContentBlock::Text {
                    text: content,
                    cache_control: Some(cache_control),
                },
            ])),
            None => Some(ChatCompletionMessageContent::Text(content)),
        };
    }

    Some(ChatCompletionMessageContent::Text(content))
}

pub fn content_text(
    content: Option<ChatCompletionMessageContent>,
    provider_name: &str,
) -> TalosResult<String> {
    match content {
        Some(ChatCompletionMessageContent::Text(text)) => Ok(text),
        Some(ChatCompletionMessageContent::Blocks(blocks)) => {
            let mut text = String::new();
            for block in blocks {
                match block {
                    ChatCompletionContentBlock::Text { text: block, .. } => {
                        text.push_str(&block);
                    }
                    ChatCompletionContentBlock::ImageUrl { .. } => {
                        return Err(TalosError::new(talos_error::ErrorCode::InvalidRequest, format!(
                            "{provider_name} response contains unsupported content block type: image_url"
                        )));
                    }
                    ChatCompletionContentBlock::File { .. } => {
                        return Err(TalosError::new(
                            talos_error::ErrorCode::InvalidRequest,
                            format!(
                            "{provider_name} response contains unsupported content block type: file"
                        ),
                        ));
                    }
                }
            }
            Ok(text)
        }
        None => Ok(String::new()),
    }
}
