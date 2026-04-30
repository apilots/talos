use super::*;

pub async fn persist_general_turn_artifacts(
    prepared: &PreparedGeneralTurn,
    input: &str,
    executed: &ExecutedTurnArtifacts,
    llm_provider: Arc<dyn LlmProvider>,
    memory_store: Arc<MemoryStore>,
    kernel_config: &AgentKernelConfig,
) {
    let seq = match memory_store.next_turn_seq(&prepared.session_id).await {
        Ok(seq) => seq,
        Err(err) => {
            warn!(
                session_id = %prepared.session_id,
                error = %err,
                "failed to allocate next turn seq"
            );
            return;
        }
    };
    let turn_id = Uuid::new_v4().to_string();
    let now = Utc::now();
    let mut items = Vec::with_capacity(executed.output_items.len().saturating_add(1));
    items.push(TalosOutputItem::UserInput {
        text: input.to_string(),
    });
    items.extend(executed.output_items.clone());
    let talos_turn = TalosTurn {
        turn_id: turn_id.clone(),
        session_id: prepared.session_id.to_string(),
        seq,
        timestamp: now,
        items,
        usage: executed.usage.clone(),
        stop_reason: to_api_stop_reason(executed.stop_reason),
    };
    if let Err(err) = memory_store.append_turn(&talos_turn).await {
        warn!(
            session_id = %prepared.session_id,
            error = %err,
            "failed to append memory store turn"
        );
    }

    if let Err(err) = compact_workspace_context_if_needed(
        &prepared.session_id,
        &prepared.selected_model,
        llm_provider.clone(),
        memory_store.as_ref(),
        kernel_config,
    )
    .await
    {
        warn!(
            session_id = %prepared.session_id,
            error = %err,
            "failed to compact workspace memory context"
        );
    }
}
