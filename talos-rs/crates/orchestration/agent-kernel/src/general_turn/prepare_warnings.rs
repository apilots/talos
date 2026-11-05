//! Prompt warning deduplication and event projection.
use super::*;
use talos_prompt::PromptSectionKind;

pub(super) async fn emit_prompt_warning_events(
    ctx: &mut InvocationContext,
    warnings: &[KernelWarning],
) {
    for warning in warnings {
        let payload = PromptWarningEventData {
            warning: warning.clone(),
            agent_id: ctx.agent_id.clone(),
            session_id: ctx.session_id.clone(),
            phase: ctx.phase.clone(),
        };
        emit_stream_event(
            ctx.stream_tx.as_ref(),
            AgentKernelEventPayload::PromptWarning(payload),
        )
        .await;
    }
}

pub(crate) fn prompt_warning_key(warning: &KernelWarning) -> Option<String> {
    match warning {
        KernelWarning::PromptBootstrapFileMissing(payload) => {
            Some(format!("missing:{:?}:{}", payload.file_kind, payload.path))
        }
        KernelWarning::PromptBootstrapFileTruncated(payload) => Some(format!(
            "truncated:{:?}:{}",
            payload.file_kind, payload.path
        )),
        KernelWarning::PromptBootstrapFileDropped(payload) => {
            Some(format!("dropped:{:?}:{}", payload.file_kind, payload.path))
        }
        KernelWarning::PromptDynamicSectionDropped(payload) => Some(format!(
            "dynamic_dropped:{}",
            serialize_section_kinds(&payload.sections)
        )),
    }
}

fn serialize_section_kinds(sections: &[PromptSectionKind]) -> String {
    sections
        .iter()
        .map(|section| format!("{section:?}"))
        .collect::<Vec<_>>()
        .join(",")
}

pub(super) fn filter_prompt_warnings(
    ctx: &InvocationContext,
    warnings: Vec<KernelWarning>,
) -> (Vec<KernelWarning>, Option<PromptWarningState>) {
    let mut warning_state = ctx.session.state.prompt_warning.clone().unwrap_or_default();
    let mut changed = false;
    let mut filtered = Vec::new();

    for warning in warnings {
        if let Some(key) = prompt_warning_key(&warning) {
            if warning_state.seen_warning_keys.insert(key) {
                changed = true;
                filtered.push(warning);
            }
        } else {
            filtered.push(warning);
        }
    }

    (filtered, changed.then_some(warning_state))
}
