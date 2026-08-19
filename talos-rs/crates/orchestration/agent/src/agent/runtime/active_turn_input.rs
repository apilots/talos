use super::*;

impl KernelAgent {
    pub(crate) async fn prepare_active_turn_input(
        &self,
        session: &talos_types::TalosSession,
        items: Vec<TalosInputItem>,
    ) -> TalosResult<talos_agent_kernel::ActiveTurnInput> {
        if items.iter().any(|item| {
            !matches!(
                item,
                TalosInputItem::Text { .. } | TalosInputItem::Files { .. }
            )
        }) {
            return Err(TalosError::public(
                ErrorCode::InvalidRequest,
                "active turn input supports only text and files",
            ));
        }
        let input = AgentKernelParams::extract_user_input(&items)?;
        let config = AgentRunConfig::from_session(session);
        let provider = self.llm_provider_for_run(&config)?;
        let selected_model_info = self.resolve_selected_model_info(&config)?;
        let resolved_attachments = self
            .attachment_reader
            .read(
                &TurnFileScope {
                    owner_user_id: session.user_id.clone(),
                    project_id: session.project_id.clone(),
                },
                &input.file_ids,
            )
            .await?;
        talos_agent_kernel::build_active_turn_input(
            input,
            resolved_attachments,
            &selected_model_info,
            provider.as_ref(),
        )
    }
}

pub(super) fn emit_user_input_item(
    timeline: &mut TimelineBuilder,
    input: &UserInput,
) -> TalosResult<Vec<AgentEvent>> {
    let started = timeline.start_user_input(input.text.clone(), input.file_ids.clone());
    let item_id = item_id_from_started(&started)?;
    let completed = timeline.complete_item(&item_id, TalosTimelineItemStatus::Completed, None)?;
    Ok(vec![started, completed])
}
