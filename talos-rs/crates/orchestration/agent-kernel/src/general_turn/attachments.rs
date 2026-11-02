use talos_config::ModelInputKind;
use talos_error::{ErrorCode, TalosError, TalosResult};
use talos_llm_provider::{
    validate_attachment_usage, Attachment, AttachmentKind, ChatMessage, LlmProvider,
    MessageInputKind, MessageInputOrigin, MessageInputSource, Role,
};
use talos_prompt::PromptAssembly;
use talos_types::ModelInfo;

use crate::{ActiveTurnInput, InvocationContext, ResolvedAttachment, UserInput};

pub(super) fn prepare_model_attachments(
    ctx: &InvocationContext,
    provider: &dyn LlmProvider,
) -> TalosResult<(Vec<Attachment>, usize)> {
    prepare_resolved_attachments(
        &ctx.input,
        &ctx.resolved_attachments,
        &ctx.selected_model_info,
        provider,
    )
}

fn prepare_resolved_attachments(
    input: &UserInput,
    resolved_attachments: &[ResolvedAttachment],
    selected_model_info: &ModelInfo,
    provider: &dyn LlmProvider,
) -> TalosResult<(Vec<Attachment>, usize)> {
    if input.file_ids.len() != resolved_attachments.len()
        || input
            .file_ids
            .iter()
            .zip(resolved_attachments)
            .any(|(file_id, attachment)| file_id != &attachment.file_id)
    {
        return Err(TalosError::internal(
            "attachment reader returned incomplete or reordered results",
        ));
    }

    let mut attachments = Vec::with_capacity(resolved_attachments.len());
    for resolved in resolved_attachments {
        let actual_size = u64::try_from(resolved.bytes.len())
            .map_err(|_| TalosError::internal("resolved attachment size overflow"))?;
        if actual_size != resolved.size_bytes {
            return Err(TalosError::internal(
                "attachment reader returned inconsistent content size",
            ));
        }
        let kind = if resolved.media_type.starts_with("image/") {
            AttachmentKind::Image
        } else {
            AttachmentKind::File
        };
        let model_input = match kind {
            AttachmentKind::Image => ModelInputKind::Image,
            AttachmentKind::File => ModelInputKind::File,
        };
        if !selected_model_info.model.input.contains(&model_input) {
            return Err(TalosError::new(
                ErrorCode::UnsupportedAttachment,
                format!(
                    "model {} does not support {:?} input",
                    selected_model_info.model.id, kind
                ),
            ));
        }
        attachments.push(Attachment {
            file_id: Some(resolved.file_id.clone()),
            kind,
            filename: resolved.filename.clone(),
            media_type: resolved.media_type.clone(),
            bytes: resolved.bytes.clone(),
        });
    }
    let usage = provider.validate_attachments(&selected_model_info.model.id, &attachments)?;
    let total_tokens = validate_attachment_usage(attachments.len(), &usage)?;
    Ok((attachments, total_tokens))
}

/// Build one active-turn input using the same attachment validation as initial input.
pub fn build_active_turn_input(
    input: UserInput,
    resolved_attachments: Vec<ResolvedAttachment>,
    selected_model_info: &ModelInfo,
    provider: &dyn LlmProvider,
) -> TalosResult<ActiveTurnInput> {
    let (attachments, attachment_tokens) =
        prepare_resolved_attachments(&input, &resolved_attachments, selected_model_info, provider)?;
    let message = if attachments.is_empty() {
        ChatMessage::user(input.text.clone())
    } else {
        ChatMessage::user_with_attachments(input.text.clone(), attachments)?
    }
    .with_input_origin(MessageInputOrigin {
        kind: MessageInputKind::User,
        source: MessageInputSource::User,
    });
    Ok(ActiveTurnInput {
        browser_uploads: browser_uploads(&resolved_attachments),
        input,
        message,
        attachment_tokens,
    })
}

/// Preserve original File identities and shared bytes for browser upload admission.
pub(super) fn browser_uploads(
    attachments: &[ResolvedAttachment],
) -> Vec<talos_types::ToolInputFile> {
    attachments
        .iter()
        .map(|file| talos_types::ToolInputFile {
            file_id: file.file_id.clone(),
            filename: file.filename.clone(),
            bytes: file.bytes.clone(),
        })
        .collect()
}

pub(super) fn reserve_prompt_attachment_tokens(
    assembly: &mut PromptAssembly,
    attachment_tokens: usize,
) -> TalosResult<()> {
    if attachment_tokens == 0 {
        return Ok(());
    }
    let diagnostics = &mut assembly.diagnostics;
    diagnostics.dynamic_estimated_tokens = diagnostics
        .dynamic_estimated_tokens
        .checked_add(attachment_tokens)
        .ok_or_else(token_overflow)?;
    diagnostics.total_estimated_tokens = diagnostics
        .total_estimated_tokens
        .checked_add(attachment_tokens)
        .ok_or_else(token_overflow)?;
    let window = &mut diagnostics.window_usage;
    window.dynamic_suffix_tokens = window
        .dynamic_suffix_tokens
        .checked_add(attachment_tokens)
        .ok_or_else(token_overflow)?;
    window.total_tokens = window
        .total_tokens
        .checked_add(attachment_tokens)
        .ok_or_else(token_overflow)?;
    window.within_effective_budget =
        !window.window_resolved || window.total_tokens <= window.effective_prompt_budget;
    Ok(())
}

fn attach_to_current_user_message(
    messages: &mut [ChatMessage],
    input: &str,
    attachments: Vec<Attachment>,
) -> TalosResult<()> {
    if attachments.is_empty() {
        return Ok(());
    }
    let message = messages
        .last_mut()
        .ok_or_else(|| TalosError::internal("prompt omitted the current user message"))?;
    if message.role != Role::User || message.content != input {
        return Err(TalosError::internal(
            "prompt current user message does not match normalized input",
        ));
    }
    message.attachments = Some(attachments);
    message.validate_attachments()?;
    Ok(())
}

pub(super) fn render_prompt_messages(
    assembly: &PromptAssembly,
    input: &str,
    attachments: Vec<Attachment>,
) -> TalosResult<Vec<ChatMessage>> {
    let mut messages = talos_prompt::render_prompt_assembly(assembly);
    attach_to_current_user_message(&mut messages, input, attachments)?;
    Ok(messages)
}

fn token_overflow() -> TalosError {
    TalosError::new(
        talos_error::ErrorCode::ContextWindowExceeded,
        "attachment token estimate overflow".to_string(),
    )
}

/// Keep newest tool images that fit beside user attachments, before loading bytes.
pub(super) fn limit_tool_images(
    prepared: &super::PreparedGeneralTurn,
    messages: &mut [ChatMessage],
) {
    let user_images = prepared
        .messages
        .iter()
        .chain(messages.iter())
        .filter(|message| message.role == Role::User)
        .map(|message| message.attachments.as_ref().map_or(0, Vec::len))
        .sum::<usize>();
    let mut remaining = talos_llm_provider::MAX_ATTACHMENTS.saturating_sub(user_images);
    for message in messages
        .iter_mut()
        .rev()
        .filter(|message| message.role == Role::Tool)
    {
        let keep = remaining.min(message.image_file_ids.len());
        if keep < message.image_file_ids.len() {
            message.image_file_ids.truncate(keep);
            if let Some(images) = &mut message.attachments {
                images.truncate(keep);
            }
            if keep == 0 {
                message.attachments = None;
            }
            message.content.push_str("\nSome tool images were omitted from model input to fit request limits; file references remain available.");
        }
        remaining -= keep;
    }
}

/// Resolve current and resumed tool references after context trimming.
pub(super) async fn resolve_tool_images(
    prepared: &super::PreparedGeneralTurn,
    messages: &mut [ChatMessage],
    tools: &dyn talos_tools::ToolExecutorPort,
    provider: &dyn LlmProvider,
) -> TalosResult<()> {
    let user_bytes = prepared
        .messages
        .iter()
        .chain(messages.iter())
        .filter(|message| message.role == Role::User)
        .flat_map(|message| message.attachments.iter().flatten())
        .map(|image| image.bytes.len())
        .sum::<usize>();
    let mut remaining_bytes =
        talos_llm_provider::MAX_TOTAL_ATTACHMENT_BYTES.saturating_sub(user_bytes);
    for message in messages.iter_mut().rev() {
        if message.image_file_ids.is_empty() {
            continue;
        }
        if message.role != Role::Tool || !prepared.image_input_enabled {
            return Err(TalosError::new(
                ErrorCode::UnsupportedAttachment,
                "The selected model does not support tool image results.",
            ));
        }
        let store = tools.image_store().ok_or_else(|| {
            TalosError::new(
                ErrorCode::UnsupportedMediaType,
                "Tool image storage is unavailable.",
            )
        })?;
        let mut cached = message.attachments.take().unwrap_or_default().into_iter();
        let mut retained_ids = Vec::new();
        let mut images = Vec::new();
        for id in &message.image_file_ids {
            let image = match cached.next() {
                Some(image) => image,
                None => {
                    let mut resolved = store
                        .read(&prepared.user_id, std::slice::from_ref(id))
                        .await?;
                    if resolved.len() != 1 {
                        return Err(TalosError::internal(
                            "Tool image storage returned incomplete results.",
                        ));
                    }
                    resolved.remove(0)
                }
            };
            if image.bytes.len() > remaining_bytes {
                continue;
            }
            remaining_bytes -= image.bytes.len();
            retained_ids.push(id.clone());
            images.push(image);
        }
        if retained_ids.len() != message.image_file_ids.len() {
            message.content.push_str("\nSome tool images were omitted from model input to fit the byte limit; file references remain available.");
        }
        message.image_file_ids = retained_ids;
        if !images.is_empty() {
            let usage = provider.validate_attachments(&prepared.selected_model, &images)?;
            validate_attachment_usage(images.len(), &usage)?;
            message.attachments = Some(images);
            message.validate_attachments()?;
        }
    }
    Ok(())
}
