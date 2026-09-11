use base64::Engine;
use talos_error::{ErrorCode, TalosError, TalosResult};
use talos_llm_provider::{validate_inline_attachments, Attachment, AttachmentKind};
use talos_types::{FileInfo, McpContent};

use crate::ToolContext;

pub(super) async fn persist_images(
    content: &[McpContent],
    context: &ToolContext,
) -> TalosResult<Vec<FileInfo>> {
    let mut images = Vec::new();
    let mut encoded_bytes = 0usize;
    for item in content {
        let McpContent::Image { data, mime_type } = item else {
            continue;
        };
        encoded_bytes = encoded_bytes.saturating_add(data.len());
        if encoded_bytes > 8 * 1024 * 1024 {
            return Err(TalosError::new(
                ErrorCode::AttachmentLimitExceeded,
                "MCP images exceed the response byte limit.",
            ));
        }
        let suffix = match mime_type.as_str() {
            "image/png" => "png",
            "image/jpeg" => "jpg",
            "image/webp" => "webp",
            "image/gif" => "gif",
            _ => {
                return Err(TalosError::new(
                    ErrorCode::UnsupportedMediaType,
                    "The MCP image format is unsupported.",
                ))
            }
        };
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(data)
            .map_err(|error| {
                TalosError::from_source(
                    ErrorCode::ToolExecutionFailed,
                    "The MCP image is not valid base64.",
                    error,
                )
            })?;
        images.push(Attachment::new(
            AttachmentKind::Image,
            format!("mcp-image.{suffix}"),
            mime_type,
            bytes.into(),
        ));
    }
    if images.is_empty() {
        return Ok(Vec::new());
    }
    validate_inline_attachments(&images, |_| true)?;
    context.image_store()?.save(context.user_id(), images).await
}
