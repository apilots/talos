use super::*;

pub(super) async fn run(client: &mut AppServerClient) -> Result<()> {
    let key = format!("e2e-response-marker-{}", uuid::Uuid::new_v4().simple());
    let first_marker = "Cerulean Comet";
    let updated_marker = "Amber Orbit";

    eprintln!("memory-e2e explicit remember through model Tool call");
    let first_session = new_session_id(client).await?;
    let first_events = run_memory_turn(
        client,
        &first_session,
        &format!(
            "Use UserWorkspaceMemoryUpdate now. Remember my stable response marker '{first_marker}' with exact key '{key}', section interaction_guidance, kind preference. The Summary must include the exact phrase '{first_marker}'. Then confirm briefly."
        ),
    )
    .await?;
    ensure_successful_memory_tool_call(&first_events)?;
    let first_doc = read_workspace_doc(client, WorkspaceDocKind::Memory)
        .await?
        .doc;
    let first_document = validate_memory_markdown(&first_doc.content)?;
    let first_entry = first_document
        .get_entry(&key)
        .ok_or_else(|| anyhow!("model did not persist expected memory key {key}"))?;
    if !first_entry.summary.contains(first_marker) {
        return Err(anyhow!(
            "persisted memory summary is semantically wrong: {}",
            first_entry.summary
        ));
    }
    let first_semantic_matches = memory_entries_containing(&first_document, first_marker)?;
    if first_semantic_matches.len() != 1 || first_semantic_matches[0].key != key {
        return Err(anyhow!(
            "successful memory Tool produced duplicate semantic entries: {first_semantic_matches:?}"
        ));
    }

    eprintln!("memory-e2e next Session reloads dynamic memory");
    let read_session = new_session_id(client).await?;
    let read_events = run_memory_turn(
        client,
        &read_session,
        "From User Workspace Memory, reply with only my exact two-word response marker.",
    )
    .await?;
    let recalled = assistant_text_from_events(&read_events);
    if !recalled
        .to_ascii_lowercase()
        .contains(&first_marker.to_ascii_lowercase())
    {
        return Err(anyhow!(
            "next Session did not recall User Workspace Memory: {recalled}"
        ));
    }

    eprintln!("memory-e2e same key update replaces the current conclusion");
    let update_session = new_session_id(client).await?;
    let update_events = run_memory_turn(
        client,
        &update_session,
        &format!(
            "Use UserWorkspaceMemoryUpdate replace_entry now to completely replace memory key '{key}'. Keep section interaction_guidance, title 'Stable response marker', and kind preference. Set both Summary and Apply to instructions that include '{updated_marker}'. No field may contain '{first_marker}'. Then confirm briefly."
        ),
    )
    .await?;
    ensure_successful_memory_tool_call(&update_events)?;
    let updated_doc = read_workspace_doc(client, WorkspaceDocKind::Memory)
        .await?
        .doc;
    let updated_document = validate_memory_markdown(&updated_doc.content)?;
    let matching = updated_document
        .list_entries(talos_memory::UserWorkspaceMemoryEntryQuery::default())
        .map_err(|error| anyhow!("list updated User Workspace Memory entries: {error}"))?
        .into_iter()
        .filter(|entry| entry.key == key)
        .collect::<Vec<_>>();
    let stale_content = matching.first().is_some_and(|entry| {
        [
            Some(entry.title.as_str()),
            Some(entry.summary.as_str()),
            entry.why.as_deref(),
            entry.apply.as_deref(),
            entry.status.as_deref(),
            entry.next.as_deref(),
        ]
        .into_iter()
        .flatten()
        .any(|value| value.contains(first_marker))
    });
    if matching.len() != 1
        || !matching[0].summary.contains(updated_marker)
        || !matching[0]
            .apply
            .as_deref()
            .is_some_and(|apply| apply.contains(updated_marker))
        || stale_content
    {
        return Err(anyhow!(
            "same-key update did not replace the current conclusion: {matching:?}"
        ));
    }
    let updated_semantic_matches = memory_entries_containing(&updated_document, updated_marker)?;
    if updated_semantic_matches.len() != 1 || updated_semantic_matches[0].key != key {
        return Err(anyhow!(
            "successful memory update Tool produced duplicate semantic entries: {updated_semantic_matches:?}"
        ));
    }

    eprintln!("memory-e2e later Session observes the updated conclusion");
    let updated_read_session = new_session_id(client).await?;
    let updated_events = run_memory_turn(
        client,
        &updated_read_session,
        "From User Workspace Memory, reply with only my exact two-word response marker.",
    )
    .await?;
    let updated_recall = assistant_text_from_events(&updated_events);
    if !updated_recall
        .to_ascii_lowercase()
        .contains(&updated_marker.to_ascii_lowercase())
        || updated_recall
            .to_ascii_lowercase()
            .contains(&first_marker.to_ascii_lowercase())
    {
        return Err(anyhow!(
            "later Session did not observe the updated memory: {updated_recall}"
        ));
    }

    eprintln!("memory-e2e explicit forget removes the stable key");
    let forget_session = new_session_id(client).await?;
    let forget_events = run_memory_turn(
        client,
        &forget_session,
        &format!(
            "Use UserWorkspaceMemoryUpdate now to remove memory key '{key}'. Then confirm briefly."
        ),
    )
    .await?;
    ensure_successful_memory_tool_call(&forget_events)?;
    let final_doc = read_workspace_doc(client, WorkspaceDocKind::Memory)
        .await?
        .doc;
    let final_document = validate_memory_markdown(&final_doc.content)?;
    if final_document.get_entry(&key).is_some() {
        return Err(anyhow!("explicit forget did not remove memory key {key}"));
    }
    if !memory_entries_containing(&final_document, updated_marker)?.is_empty() {
        return Err(anyhow!(
            "explicit forget left a duplicate semantic memory entry for {updated_marker}"
        ));
    }

    let reflected_marker = "Violet Harbor";
    eprintln!("memory-e2e Turn-end reflection persists durable memory without a primary Tool call");
    let reflection_session = new_session_id(client).await?;
    let reflection_events = run_memory_turn(
        client,
        &reflection_session,
        &format!(
            "Do not call any tool in this turn. Just acknowledge this durable preference: my fallback response marker is the exact phrase '{reflected_marker}'."
        ),
    )
    .await?;
    if contains_memory_tool_call(&reflection_events) {
        return Err(anyhow!(
            "reflection e2e was not isolated because the primary model called the memory Tool"
        ));
    }
    let reflected_doc = read_workspace_doc(client, WorkspaceDocKind::Memory)
        .await?
        .doc;
    let reflected_document = validate_memory_markdown(&reflected_doc.content)?;
    let reflected_entry = reflected_document
        .list_entries(talos_memory::UserWorkspaceMemoryEntryQuery::default())
        .map_err(|error| anyhow!("list reflected User Workspace Memory entries: {error}"))?
        .into_iter()
        .find(|entry| entry.summary.contains(reflected_marker))
        .ok_or_else(|| anyhow!("Turn-end reflection did not persist {reflected_marker}"))?;

    eprintln!("memory-e2e later Session reloads reflected memory");
    let reflection_read_session = new_session_id(client).await?;
    let reflection_read_events = run_memory_turn(
        client,
        &reflection_read_session,
        "From User Workspace Memory, reply with only my exact fallback response marker.",
    )
    .await?;
    let reflected_recall = assistant_text_from_events(&reflection_read_events);
    if !reflected_recall
        .to_ascii_lowercase()
        .contains(&reflected_marker.to_ascii_lowercase())
    {
        return Err(anyhow!(
            "later Session did not recall reflected memory: {reflected_recall}"
        ));
    }

    eprintln!("memory-e2e clean up reflected entry through explicit forget");
    let reflection_cleanup_session = new_session_id(client).await?;
    let cleanup_events = run_memory_turn(
        client,
        &reflection_cleanup_session,
        &format!(
            "Use UserWorkspaceMemoryUpdate now to remove memory key '{}'. Then confirm briefly.",
            reflected_entry.key
        ),
    )
    .await?;
    ensure_successful_memory_tool_call(&cleanup_events)?;
    let cleanup_doc = read_workspace_doc(client, WorkspaceDocKind::Memory)
        .await?
        .doc;
    let cleanup_document = validate_memory_markdown(&cleanup_doc.content)?;
    if cleanup_document.get_entry(&reflected_entry.key).is_some() {
        return Err(anyhow!(
            "reflection cleanup did not remove memory key {}",
            reflected_entry.key
        ));
    }
    Ok(())
}

async fn run_memory_turn(
    client: &mut AppServerClient,
    session_id: &str,
    input: &str,
) -> Result<Vec<TimelineEventEnvelope>> {
    client
        .start_turn(start_turn_request(
            session_id,
            input,
            Some(format!("memory-e2e-{}", uuid::Uuid::new_v4().simple())),
        ))
        .await?;
    let subscription = subscribe_events(client, session_id).await?;
    let events = collect_until_terminal(client, &subscription.subscription_id).await?;
    ensure_turn_event_flow(&events)?;
    Ok(events)
}

pub(super) fn validate_memory_markdown(
    content: &str,
) -> Result<talos_memory::UserWorkspaceMemoryDocument> {
    for required in [
        "type: UserWorkspaceMemory",
        "status: stable",
        "# User Workspace Memory",
        "<!-- talos:user-workspace-memory:start -->",
        "## Interaction Guidance",
        "## Key Knowledge",
        "## Active Context",
        "<!-- talos:user-workspace-memory:end -->",
    ] {
        if !content.contains(required) {
            return Err(anyhow!(
                "MEMORY.md is missing canonical content: {required}"
            ));
        }
    }
    if content
        .matches("<!-- talos:user-workspace-memory:start -->")
        .count()
        != 1
        || content
            .matches("<!-- talos:user-workspace-memory:end -->")
            .count()
            != 1
    {
        return Err(anyhow!(
            "MEMORY.md contains duplicate managed region markers"
        ));
    }
    talos_memory::UserWorkspaceMemoryDocument::parse(content)
        .map_err(|error| anyhow!("MEMORY.md is not valid canonical OKF: {error}"))
}

fn memory_entries_containing(
    document: &talos_memory::UserWorkspaceMemoryDocument,
    marker: &str,
) -> Result<Vec<talos_memory::UserWorkspaceMemoryEntry>> {
    Ok(document
        .list_entries(talos_memory::UserWorkspaceMemoryEntryQuery::default())
        .map_err(|error| anyhow!("list User Workspace Memory entries: {error}"))?
        .into_iter()
        .filter(|entry| {
            [
                Some(entry.title.as_str()),
                Some(entry.summary.as_str()),
                entry.why.as_deref(),
                entry.apply.as_deref(),
                entry.status.as_deref(),
                entry.next.as_deref(),
            ]
            .into_iter()
            .flatten()
            .any(|value| value.contains(marker))
        })
        .collect())
}

fn contains_memory_tool_call(events: &[TimelineEventEnvelope]) -> bool {
    events.iter().any(|event| match &event.event.payload {
        AgentEventPayload::ItemStarted(started) => matches!(
            &started.item.content,
            TalosTimelineContent::ToolCall { name, .. }
                if name == "UserWorkspaceMemoryUpdate"
                    || name == "user_workspace_memory_update"
        ),
        _ => false,
    })
}

fn ensure_successful_memory_tool_call(events: &[TimelineEventEnvelope]) -> Result<()> {
    let memory_items: std::collections::HashSet<_> = events
        .iter()
        .filter_map(|event| match &event.event.payload {
            AgentEventPayload::ItemStarted(started)
                if matches!(
                    &started.item.content,
                    TalosTimelineContent::ToolCall { name, .. }
                        if name == "UserWorkspaceMemoryUpdate"
                            || name == "user_workspace_memory_update"
                ) =>
            {
                Some(started.item.item_id.clone())
            }
            _ => None,
        })
        .collect();
    if memory_items.is_empty() {
        return Err(anyhow!(
            "expected the primary model to call UserWorkspaceMemoryUpdate"
        ));
    }
    let succeeded = events.iter().any(|event| match &event.event.payload {
        AgentEventPayload::ItemDelta(delta) if memory_items.contains(&delta.item_id) => {
            matches!(
                &delta.delta,
                TalosTimelineDelta::ToolResult {
                    is_error: false,
                    ..
                }
            )
        }
        AgentEventPayload::ItemCompleted(completed)
            if memory_items.contains(&completed.item_id) =>
        {
            matches!(
                &completed.final_content,
                Some(TalosTimelineContent::ToolCall {
                    result: Some(result),
                    ..
                }) if !result.is_error
            )
        }
        _ => false,
    });
    if !succeeded {
        return Err(anyhow!(
            "UserWorkspaceMemoryUpdate was called but no successful Tool result was emitted"
        ));
    }
    Ok(())
}
