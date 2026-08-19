use talos_config::ModelInputKind;
use talos_error::{TalosError, TalosFileErrorKind, TalosResult};
use talos_llm_provider::{
    validate_attachment_usage, Attachment, AttachmentKind, ChatMessage, LlmProvider, Role,
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
            "attachment reader returned incomplete or reordered results".to_string(),
        ));
    }

    let mut attachments = Vec::with_capacity(resolved_attachments.len());
    for resolved in resolved_attachments {
        let actual_size = u64::try_from(resolved.bytes.len())
            .map_err(|_| TalosError::internal("resolved attachment size overflow".to_string()))?;
        if actual_size != resolved.size_bytes {
            return Err(TalosError::internal(
                "attachment reader returned inconsistent content size".to_string(),
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
            return Err(TalosError::file_error(
                TalosFileErrorKind::UnsupportedAttachment,
                format!(
                    "model {} does not support {:?} input",
                    selected_model_info.model.id, kind
                ),
            ));
        }
        attachments.push(Attachment {
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
    };
    Ok(ActiveTurnInput {
        input,
        message,
        attachment_tokens,
    })
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
    let message = messages.last_mut().ok_or_else(|| {
        TalosError::internal("prompt omitted the current user message".to_string())
    })?;
    if message.role != Role::User || message.content != input {
        return Err(TalosError::internal(
            "prompt current user message does not match normalized input".to_string(),
        ));
    }
    *message = ChatMessage::user_with_attachments(message.content.clone(), attachments)?;
    Ok(())
}

pub(super) fn render_prompt_messages(
    assembly: &PromptAssembly,
    input: &str,
    attachments: Vec<Attachment>,
) -> TalosResult<(Vec<ChatMessage>, Vec<ChatMessage>)> {
    let mut messages = talos_prompt::render_prompt_assembly(assembly);
    let prompt_prefix_messages = match messages.last() {
        Some(message) if message.role == Role::User && message.content == input => {
            messages[..messages.len().saturating_sub(1)].to_vec()
        }
        _ => messages.clone(),
    };
    attach_to_current_user_message(&mut messages, input, attachments)?;
    Ok((messages, prompt_prefix_messages))
}

fn token_overflow() -> TalosError {
    TalosError::public(
        talos_error::ErrorCode::ContextWindowExceeded,
        "attachment token estimate overflow".to_string(),
    )
}
