//! Combine bounded completed execution facts, without a recording lifecycle.
use std::collections::HashSet;
use talos_agent_api::trajectory::*;
use talos_error::{ErrorCode, TalosError, TalosResult};

fn invalid() -> TalosError {
    TalosError::new(
        ErrorCode::InvalidRequest,
        "Execution history identity or content linkage is invalid.",
    )
}
fn conflict() -> TalosError {
    TalosError::new(
        ErrorCode::Conflict,
        "Previously saved execution input changed.",
    )
}
pub(super) fn append_empty(
    value: SessionHistorySupplement,
) -> TalosResult<SessionHistorySupplement> {
    append(
        SessionHistorySupplement {
            identity: value.identity.clone(),
            clock_segments: vec![],
            requests: vec![],
            tools: vec![],
            payloads: Default::default(),
        },
        value,
    )
}
pub(super) fn append(
    mut old: SessionHistorySupplement,
    mut incoming: SessionHistorySupplement,
) -> TalosResult<SessionHistorySupplement> {
    if old.identity != incoming.identity {
        return Err(invalid());
    }
    // Share only fully equal typed payloads, including role/source/cache metadata.
    let mut shared = std::collections::HashMap::new();
    for (id, payload) in incoming.payloads {
        if old.payloads.get(&id).is_some_and(|saved| saved != &payload) {
            return Err(conflict());
        }
        if let Some((saved_id, _)) = old.payloads.iter().find(|(_, saved)| **saved == payload) {
            shared.insert(id, saved_id.clone());
        } else {
            old.payloads.insert(id, payload);
        }
    }
    for request in &mut incoming.requests {
        for reference in [
            &mut request.prompt,
            &mut request.tools,
            &mut request.parameters,
        ]
        .into_iter()
        .chain(
            request
                .outputs
                .iter_mut()
                .map(|output| &mut output.response),
        )
        .flatten()
        {
            if let Some(id) = shared.get(&reference.payload_id) {
                reference.payload_id = id.clone();
            }
        }
    }
    for tool in &mut incoming.tools {
        if let Some(reference) = &mut tool.prepared_result {
            if let Some(id) = shared.get(&reference.payload_id) {
                reference.payload_id = id.clone();
            }
        }
    }
    for clock in incoming.clock_segments {
        if let Some(saved) = old
            .clock_segments
            .iter_mut()
            .find(|c| c.segment_id == clock.segment_id)
        {
            if saved.source_node_id != clock.source_node_id
                || saved.started_at != clock.started_at
                || saved.relation != clock.relation
            {
                return Err(conflict());
            }
            saved.observed_through_offset_ms = saved
                .observed_through_offset_ms
                .max(clock.observed_through_offset_ms);
        } else {
            old.clock_segments.push(clock);
        }
    }
    let mut ordinal = old
        .requests
        .iter()
        .map(|r| r.request.ordinal)
        .max()
        .unwrap_or(0);
    for mut detail in incoming.requests {
        if let Some(saved) = old
            .requests
            .iter_mut()
            .find(|r| r.request.model_request_id == detail.request.model_request_id)
        {
            // Each real request freezes once; later invocation checkpoints may
            // repeat that same immutable request, never revise its result.
            detail.request.ordinal = saved.request.ordinal;
            if saved != &detail {
                return Err(conflict());
            }
        } else {
            ordinal += 1;
            detail.request.ordinal = ordinal;
            old.requests.push(detail);
        }
    }
    for detail in incoming.tools {
        if let Some(saved) = old
            .tools
            .iter_mut()
            .find(|t| t.tool.call_id == detail.tool.call_id)
        {
            if saved.identity != detail.identity {
                return Err(invalid());
            }
            for (before, after) in [
                (
                    &saved.tool.source_model_request_id,
                    &detail.tool.source_model_request_id,
                ),
                (
                    &saved.tool.source_call_record_id,
                    &detail.tool.source_call_record_id,
                ),
            ] {
                if before.is_some() && after.is_some() && before != after {
                    return Err(conflict());
                }
            }
            saved.tool.source_model_request_id = detail
                .tool
                .source_model_request_id
                .or(saved.tool.source_model_request_id.take());
            saved.tool.source_call_record_id = detail
                .tool
                .source_call_record_id
                .or(saved.tool.source_call_record_id.take());
            saved.prepared_result = detail.prepared_result.or(saved.prepared_result.take());
            for interval in detail.tool.intervals.into_iter().flatten() {
                let intervals = saved.tool.intervals.get_or_insert_default();
                if let Some(existing) = intervals.iter_mut().find(|i| {
                    i.kind == interval.kind
                        && i.interval.segment_id == interval.interval.segment_id
                        && i.interval.started_offset_ms == interval.interval.started_offset_ms
                }) {
                    *existing = interval;
                } else {
                    intervals.push(interval);
                }
            }
            for usage in detail.usages.into_iter().flatten() {
                let usages = saved.usages.get_or_insert_default();
                if let Some(existing) = usages
                    .iter_mut()
                    .find(|u| u.model_request_id == usage.model_request_id)
                {
                    *existing = usage;
                } else {
                    usages.push(usage);
                }
            }
        } else {
            old.tools.push(detail);
        }
    }
    validate(&old)?;
    let referenced: HashSet<_> = old
        .requests
        .iter()
        .flat_map(|r| {
            [r.prompt.as_ref(), r.tools.as_ref(), r.parameters.as_ref()]
                .into_iter()
                .chain(r.outputs.iter().map(|o| o.response.as_ref()))
        })
        .chain(old.tools.iter().map(|t| t.prepared_result.as_ref()))
        .flatten()
        .map(|r| r.payload_id.clone())
        .collect();
    old.payloads.retain(|key, _| referenced.contains(key));
    Ok(old)
}
fn snapshot(
    value: &SessionHistorySupplement,
    reference: &Option<PayloadRef>,
    matches: impl FnOnce(&TrajectoryPayload) -> bool,
) -> TalosResult<()> {
    if let Some(reference) = reference {
        if !value
            .payloads
            .get(&reference.payload_id)
            .is_some_and(matches)
        {
            return Err(invalid());
        }
    }
    Ok(())
}
fn interval(value: &ExecutionInterval) -> TalosResult<()> {
    if value.segment_id.is_empty()
        || !value.started_offset_ms.is_finite()
        || value.started_offset_ms < 0.0
    {
        return Err(invalid());
    }
    match (value.ended_offset_ms, value.elapsed_ms) {
        (None, None) => Ok(()),
        (Some(end), Some(elapsed))
            if end.is_finite()
                && elapsed.is_finite()
                && end >= value.started_offset_ms
                && (end - value.started_offset_ms - elapsed).abs() < 0.001 =>
        {
            Ok(())
        }
        _ => Err(invalid()),
    }
}
fn validate(value: &SessionHistorySupplement) -> TalosResult<()> {
    let mut requests = HashSet::new();
    let mut calls = HashSet::new();
    for clock in &value.clock_segments {
        if clock.segment_id.is_empty()
            || !clock.observed_through_offset_ms.is_finite()
            || clock.observed_through_offset_ms < 0.0
        {
            return Err(invalid());
        }
    }
    if value.requests.len() + value.tools.len() + value.payloads.len() > 1024 {
        return Err(TalosError::new(
            ErrorCode::SessionBudgetExceeded,
            "Execution history supplement exceeded its record limit.",
        ));
    }
    for detail in &value.requests {
        if detail.identity != value.identity
            || detail.request.model_request_id.is_empty()
            || !requests.insert(&detail.request.model_request_id)
        {
            return Err(invalid());
        }
        snapshot(value, &detail.prompt, |p| {
            matches!(p, TrajectoryPayload::Prompt { .. })
        })?;
        snapshot(value, &detail.tools, |p| {
            matches!(p, TrajectoryPayload::Tools { .. })
        })?;
        snapshot(value, &detail.parameters, |p| {
            matches!(p, TrajectoryPayload::Parameters { .. })
        })?;
        let mut call_records = HashSet::new();
        for attempt in &detail.request.call_records {
            if attempt.call_record_id.is_empty() || !call_records.insert(&attempt.call_record_id) {
                return Err(invalid());
            }
            if let Some(timing) = &attempt.timing {
                interval(&timing.interval)?;
            }
        }
        for output in &detail.outputs {
            if !call_records.contains(&output.call_record_id) {
                return Err(invalid());
            }
            snapshot(value, &output.response, |p| {
                matches!(p, TrajectoryPayload::Response { .. })
            })?;
        }
    }
    for detail in &value.tools {
        if detail.identity != value.identity
            || detail.tool.call_id.is_empty()
            || !calls.insert(&detail.tool.call_id)
        {
            return Err(invalid());
        }
        snapshot(value, &detail.prepared_result, |p| {
            matches!(p, TrajectoryPayload::PreparedResult { .. })
        })?;
        for phase in detail.tool.intervals.iter().flatten() {
            interval(&phase.interval)?;
        }
        if let Some(id) = &detail.tool.source_model_request_id {
            let source = value
                .requests
                .iter()
                .find(|r| &r.request.model_request_id == id)
                .ok_or_else(invalid)?;
            if detail
                .tool
                .source_call_record_id
                .as_ref()
                .is_some_and(|id| {
                    !source
                        .request
                        .call_records
                        .iter()
                        .any(|a| &a.call_record_id == id)
                })
            {
                return Err(invalid());
            }
        } else if detail.tool.source_call_record_id.is_some() {
            return Err(invalid());
        }
        for usage in detail.usages.iter().flatten() {
            let request = value
                .requests
                .iter()
                .find(|r| r.request.model_request_id == usage.model_request_id)
                .ok_or_else(invalid)?;
            if matches!(
                usage.usage,
                ToolInputUsageKind::Direct | ToolInputUsageKind::Transformed
            ) {
                if let Some(reference) = &request.prompt {
                    let prompt = value.payloads.get(&reference.payload_id);
                    if !matches!(prompt,Some(TrajectoryPayload::Prompt{messages}) if messages.iter().any(|m|Some(m.index)==usage.message_index && m.role==SnapshotRole::Tool && m.tool_call_id.as_ref()==Some(&detail.tool.call_id)))
                    {
                        return Err(invalid());
                    }
                }
            }
        }
    }
    Ok(())
}
