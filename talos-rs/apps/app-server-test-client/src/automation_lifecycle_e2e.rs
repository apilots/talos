//! Real lifecycle branches using the existing controlled model and Gateway client.
use crate::automation_e2e_support::*;
use anyhow::{ensure, Context, Result};
use serde_json::{json, Value};
use std::time::Duration;
use talos_sdk::{
    AgentMessageBlockContent, AgentRunResult, AutomationListEntry, AutomationListRequest,
    AutomationListScope, GatewayClient, GoalStatus, LoopAction, LoopStatus, LoopStopReason,
    MessageQueueListRequest, QueuedMessageSource, SkillSummary,
};

pub(super) async fn run(
    client: &mut impl GatewayClient,
    goal_skill: &SkillSummary,
    loop_skill: &SkillSummary,
    evidence: &mut Vec<Value>,
) -> Result<()> {
    goal_cross_turn(client, goal_skill, evidence).await?;
    long_fixed(client, loop_skill, evidence).await?;
    model_stop(client, loop_skill, evidence, "dynamicstop").await?;
    model_stop(client, loop_skill, evidence, "strict").await
}

async fn goal_cross_turn(
    client: &mut impl GatewayClient,
    skill: &SkillSummary,
    evidence: &mut Vec<Value>,
) -> Result<()> {
    let session = uuid::Uuid::new_v4().to_string();
    let case = marker("goalcarry");
    start(client, &session, selected(&case, skill)).await?;
    gate(&case, "work-1").await?;
    let initial = snapshot(client, &session)
        .await?
        .goal
        .context("Goal missing")?;
    let first_id = active(client, &session).await?.active_turn.identity;
    release(&case, "work-1")?;
    let first = history(client, &session, &first_id).await?;
    let AgentRunResult::Completed(output) = &first.result else {
        anyhow::bail!("Goal carry work did not settle normally");
    };
    ensure!(
        output.metrics.tool_errors == 0
            && output
                .messages
                .iter()
                .flat_map(|message| &message.blocks)
                .any(
                    |block| matches!(&block.content, AgentMessageBlockContent::ToolResult(result)
            if result.tool_name == "Read" && !result.is_error)
                ),
        "Goal carry did not perform a successful actual Read"
    );
    gate(&case, "work-2").await?;
    let continued = snapshot(client, &session)
        .await?
        .goal
        .context("Goal missing")?;
    let second_id = active(client, &session).await?.active_turn.identity;
    ensure!(
        first_id != second_id,
        "Goal did not continue through another original Turn"
    );
    ensure!(
        continued.id == initial.id
            && continued.created_at == initial.created_at
            && continued.status == GoalStatus::Active,
        "Goal clock restarted between Turns"
    );
    // Span a real second so a frozen completion time is distinguishable from now.
    tokio::time::sleep(Duration::from_millis(1100)).await;
    release(&case, "work-2")?;
    let complete = wait_snapshot(client, &session, |state| {
        state
            .goal
            .as_ref()
            .is_some_and(|goal| goal.status == GoalStatus::Complete)
    })
    .await?;
    let finished = complete.goal.as_ref().unwrap();
    let second = history(client, &session, &second_id).await?;
    ensure!(
        finished.created_at == initial.created_at
            && finished.updated_at >= second.completed_at
            && finished.updated_at > finished.created_at,
        "Goal total time lost its original creation or completion fact"
    );
    tokio::time::sleep(Duration::from_millis(1100)).await;
    let reread = snapshot(client, &session)
        .await?
        .goal
        .context("completed Goal missing")?;
    ensure!(
        reread.updated_at == finished.updated_at && reread.created_at == initial.created_at,
        "completed Goal total time kept advancing"
    );
    ensure!(
        first.automation_input.is_some() && second.automation_input.is_some(),
        "cross-Turn Goal lost exact automatic Input facts"
    );
    evidence.push(json!({"case":"goal-cross-turn-time", "initial":initial,
        "continued":continued, "first":first, "second":second, "complete":finished, "reread":reread}));
    Ok(())
}

async fn long_fixed(
    client: &mut impl GatewayClient,
    skill: &SkillSummary,
    evidence: &mut Vec<Value>,
) -> Result<()> {
    let session = uuid::Uuid::new_v4().to_string();
    let case = marker("fixedlong");
    start(client, &session, selected(&case, skill)).await?;
    gate(&case, "work-1").await?;
    let turn = active(client, &session).await?.active_turn.identity;
    release(&case, "work-1")?;
    let work = history(client, &session, &turn).await?;
    let state = wait_snapshot(client, &session, |state| {
        state
            .loops
            .first()
            .is_some_and(|task| task.next_due_at.is_some())
    })
    .await?;
    let task = &state.loops[0];
    ensure!(
        task.status == LoopStatus::Active
            && task.next_due_at.unwrap() >= work.completed_at + 10081 * 60,
        "fixed Loop did not retain a next time beyond seven days"
    );
    ensure!(
        serde_json::to_value(task)?.get("expiresAt").is_none(),
        "removed Loop deadline leaked onto wire"
    );
    let loop_id = task.id.clone();
    evidence.push(json!({"case":"fixed-beyond-seven-days", "work":work, "snapshot":state}));
    control(client, &session, LoopAction::Stop { loop_id }).await
}

async fn model_stop(
    client: &mut impl GatewayClient,
    skill: &SkillSummary,
    evidence: &mut Vec<Value>,
    kind: &str,
) -> Result<()> {
    let session = uuid::Uuid::new_v4().to_string();
    let case = marker(kind);
    start(client, &session, selected(&case, skill)).await?;
    if kind == "strict" {
        gate(&case, "registered").await?;
        let registered = active(client, &session).await?.active_turn.identity;
        release(&case, "registered")?;
        let registration = history(client, &session, &registered).await?;
        assert_strict_correction(&registration, &case)?;
        evidence.push(json!({"case":"strict-model-tool-correction", "registration":registration}));
    }
    gate(&case, "work-1").await?;
    let turn = active(client, &session).await?.active_turn.identity;
    release(&case, "work-1")?;
    gate(&case, "declared").await?;
    let pending = snapshot(client, &session).await?;
    let task = &pending.loops[0];
    ensure!(
        task.status == LoopStatus::Active
            && task.next_due_at.is_none()
            && active(client, &session).await?.active_turn.identity == turn,
        "model Stop applied before original normal settlement"
    );
    release(&case, "declared")?;
    let work = history(client, &session, &turn).await?;
    wait_snapshot(client, &session, |state| {
        state.loops.iter().all(|loop_task| loop_task.id != task.id)
    })
    .await?;
    let ended = client
        .list_automation(AutomationListRequest {
            session_id: Some(session.clone()),
            scope: AutomationListScope::Ended,
            cursor: None,
            limit: Some(100),
        })
        .await?;
    ensure!(ended.data.iter().any(|entry| matches!(entry, AutomationListEntry::Loop { loop_task }
        if loop_task.id == task.id && loop_task.status == LoopStatus::Stopped
        && loop_task.stop_reason == Some(LoopStopReason::ModelStopped)
        && loop_task.next_due_at.is_none() && loop_task.reason.as_deref() == Some("Controlled task is complete; stop checking"))),
        "real model Stop declaration was not applied at settlement");
    let queue = client
        .list_message_queue(MessageQueueListRequest {
            session_id: session,
        })
        .await?;
    ensure!(
        !queue.messages.iter().any(|item| matches!(&item.source,
        QueuedMessageSource::Loop { loop_id, .. } if loop_id == &task.id)),
        "model Stop left an occurrence queued"
    );
    evidence
        .push(json!({"case":if kind == "strict" { "strict-corrected-model-stop" } else { "dynamic-model-stop" }, "pending":pending, "work":work, "ended":ended}));
    Ok(())
}

fn assert_strict_correction(turn: &talos_sdk::SessionHistoryTurn, case: &str) -> Result<()> {
    let AgentRunResult::Completed(output) = &turn.result else {
        anyhow::bail!("corrected registration did not settle normally");
    };
    let results = output
        .messages
        .iter()
        .flat_map(|message| &message.blocks)
        .filter_map(|block| match &block.content {
            AgentMessageBlockContent::ToolResult(result) => Some(result),
            _ => None,
        })
        .collect::<Vec<_>>();
    let rejected = results
        .iter()
        .find(|result| result.call_id == format!("bad-schedule-{case}"))
        .context("actual malformed Tool result missing")?;
    ensure!(
        rejected.is_error,
        "modelDecided with extra minutes reached control"
    );
    let listed = results
        .iter()
        .find(|result| result.call_id == format!("list-empty-{case}"))
        .context("actual post-rejection List Tool result missing")?;
    ensure!(
        !listed.is_error,
        "model could not inspect actual state after rejection"
    );
    let content = serde_json::to_value(&listed.content)?;
    let empty: talos_sdk::SessionAutomationSnapshot =
        serde_json::from_value(content[0]["value"]["snapshot"].clone())?;
    ensure!(
        empty.goal.is_none() && empty.loops.is_empty(),
        "invalid nested field had business side effects"
    );
    let corrected = results
        .iter()
        .find(|result| result.call_id == format!("create-{case}"))
        .context("actual corrected Create Tool result missing")?;
    ensure!(!corrected.is_error, "same-Turn corrected Create failed");
    let content = serde_json::to_value(&corrected.content)?;
    let created: talos_sdk::SessionAutomationSnapshot =
        serde_json::from_value(content[0]["value"]["snapshot"].clone())?;
    ensure!(
        created.loops.len() == 1,
        "corrected Create did not produce exactly one Loop"
    );
    Ok(())
}
