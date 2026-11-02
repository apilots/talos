use crate::{ChatMessage, Role};
use bytes::Bytes;
use talos_error::{ErrorCode, TalosError, TalosResult};

pub const MAX_ATTACHMENTS: usize = 5;
const MAX_ATTACHMENT_BYTES: usize = 20 * 1024 * 1024;
pub const MAX_TOTAL_ATTACHMENT_BYTES: usize = 20 * 1024 * 1024;
pub const MAX_ATTACHMENT_TOKENS: usize = 10_000;

impl ChatMessage {
    /// Rejects ambiguous empty attachment state and unsupported message roles.
    pub fn validate_attachments(&self) -> TalosResult<()> {
        let Some(attachments) = self.attachments.as_ref() else {
            return Ok(());
        };
        if attachments.is_empty() {
            return Err(TalosError::new(
                ErrorCode::InvalidRequest,
                "attachments must be omitted instead of empty".to_string(),
            ));
        }
        if !matches!(self.role, Role::User | Role::Tool) {
            return Err(TalosError::new(
                ErrorCode::InvalidRequest,
                "only user or tool messages may contain attachments".to_string(),
            ));
        }
        if self.role == Role::Tool
            && attachments
                .iter()
                .any(|attachment| attachment.kind != AttachmentKind::Image)
        {
            return Err(TalosError::new(
                ErrorCode::UnsupportedMediaType,
                "Tool messages support image attachments only.",
            ));
        }
        Ok(())
    }
}

/// Binary input attached to one user or tool message.
#[derive(Debug, Clone, Eq)]
pub struct Attachment {
    /// Local FileService identity when known; never part of provider wire payloads.
    pub file_id: Option<String>,
    /// Wire representation selected by the shared protocol encoders.
    pub kind: AttachmentKind,
    /// Original filename sent to providers that accept file names.
    pub filename: String,
    /// Validated MIME type used by provider capability checks and wire encoding.
    pub media_type: String,
    /// Complete file bytes; provider adapters must not persist or log this value.
    pub bytes: Bytes,
}

// Local observation identity does not change provider message equality.
impl PartialEq for Attachment {
    fn eq(&self, other: &Self) -> bool {
        self.kind == other.kind
            && self.filename == other.filename
            && self.media_type == other.media_type
            && self.bytes == other.bytes
    }
}

impl Attachment {
    /// Creates one provider-neutral attachment for a user message.
    pub fn new(
        kind: AttachmentKind,
        filename: impl Into<String>,
        media_type: impl Into<String>,
        bytes: Bytes,
    ) -> Self {
        Self {
            file_id: None,
            kind,
            filename: filename.into(),
            media_type: media_type.into(),
            bytes,
        }
    }
}

/// Closed set of binary representations supported by shared protocol encoders.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttachmentKind {
    /// Image input encoded with the protocol's image block.
    Image,
    /// Non-image document encoded with the protocol's file or document block.
    File,
}

/// Conservative per-item token bounds returned in attachment order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttachmentUsage {
    /// One conservative token reservation for every validated attachment.
    pub item_tokens: Vec<usize>,
}

/// Validates common inline limits and applies one provider-owned MIME policy.
///
/// Accepted attachments reserve the full per-item context allowance. Binary
/// wire length is not a model token count, and providers remain responsible
/// for accepting only media types supported by their selected protocol.
pub fn validate_inline_attachments(
    attachments: &[Attachment],
    supports_media_type: impl Fn(&Attachment) -> bool,
) -> TalosResult<AttachmentUsage> {
    if attachments.len() > MAX_ATTACHMENTS {
        return Err(TalosError::localized(
            talos_error::ErrorLocalization::AttachmentCountExceeded {
                limit: MAX_ATTACHMENTS as u64,
                actual: attachments.len() as u64,
            },
        ));
    }
    let mut total_bytes = 0_usize;
    let mut item_tokens = Vec::with_capacity(attachments.len());
    for attachment in attachments {
        if attachment.bytes.len() > MAX_ATTACHMENT_BYTES {
            return Err(limit_error(format!(
                "one attachment exceeds {MAX_ATTACHMENT_BYTES} bytes"
            )));
        }
        total_bytes = total_bytes
            .checked_add(attachment.bytes.len())
            .ok_or_else(|| limit_error("attachment byte count overflow".to_string()))?;
        if !supports_media_type(attachment) {
            return Err(TalosError::new(
                ErrorCode::UnsupportedMediaType,
                format!(
                    "attachment MIME type {} is unsupported for {:?}",
                    attachment.media_type, attachment.kind
                ),
            ));
        }
        item_tokens.push(MAX_ATTACHMENT_TOKENS);
    }
    if total_bytes > MAX_TOTAL_ATTACHMENT_BYTES {
        return Err(limit_error(format!(
            "attachment bytes exceed {MAX_TOTAL_ATTACHMENT_BYTES}"
        )));
    }
    Ok(AttachmentUsage { item_tokens })
}

/// Validates one provider's usage result and returns its total token reservation.
pub fn validate_attachment_usage(
    attachment_count: usize,
    usage: &AttachmentUsage,
) -> TalosResult<usize> {
    if usage.item_tokens.len() != attachment_count {
        return Err(TalosError::internal(
            "Provider returned an inconsistent number of attachment estimates.",
        )
        .context(format!(
            "provider returned {} attachment estimates for {attachment_count} attachments",
            usage.item_tokens.len()
        )));
    }
    if usage
        .item_tokens
        .iter()
        .any(|tokens| *tokens > MAX_ATTACHMENT_TOKENS)
    {
        return Err(TalosError::new(
            talos_error::ErrorCode::ContextWindowExceeded,
            format!("one attachment exceeds the {MAX_ATTACHMENT_TOKENS} token input cap"),
        ));
    }
    Ok(usage.item_tokens.iter().copied().sum())
}

fn limit_error(message: String) -> TalosError {
    TalosError::new(ErrorCode::AttachmentLimitExceeded, message)
}
