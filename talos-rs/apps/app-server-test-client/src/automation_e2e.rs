//! Real SDK -> System Skill -> model Tool -> Service -> queue/settlement proof.
use crate::automation_e2e_support::*;
use anyhow::{ensure, Context, Result};
use serde_json::json;
use std::time::{Duration, Instant};
use talos_sdk::{
    AgentMessageBlockContent, AgentRunResult, AutomationAction, AutomationListEntry,
    AutomationListRequest, AutomationListScope, AutomationRequest, AutomationTurnOrigin,
    BuiltinJobAction, BuiltinJobControlRequest, BuiltinJobId, BuiltinJobListRequest, GatewayClient,
    GoalStatus, LoopAction, LoopSchedule, LoopStatus, MessageQueueAddRequest,
    MessageQueueDeleteRequest, MessageQueueListRequest, QueuedMessageSource, SkillScope,
    SkillSummary, SkillsListRequest, SubmitTurnInput, SubmitTurnInputRequest,
};

pub(super) async fn run(client: &mut impl GatewayClient) -> Result<()> {
    let skills = client
        .list_skills(SkillsListRequest {
            node_id: Some("embedded".into()),
            include_disabled: false,
        })
        .await?;
    let skill = |name: &str| {
        skills
            .data
            .iter()
            .find(|skill| {
                skill.name == name
                    && skill.scope == SkillScope::System
                    && skill.enabled
                    && skill.read_only
            })
            .cloned()
            .with_context(|| format!("actual System Skill {name} unavailable"))
    };
    let goal_skill = skill("goal")?;
    let loop_skill = skill("loop")?;
    let mut evidence = Vec::new();

    eprintln!(
        "automation Goal: real Skill/Tool registration, automatic work, User Queue then Steer"
    );
    let session = uuid::Uuid::new_v4().to_string();
    let case = marker("goal");
    start(client, &session, selected(&case, &goal_skill)).await?;
    gate(&case, "work-1").await?;
    let before = snapshot(client, &session).await?;
    ensure!(
        before
            .goal
            .as_ref()
            .is_some_and(|goal| goal.status == GoalStatus::Active),
        "model Tool did not persist Goal"
    );
    let rejected = client
        .execute_automation(AutomationRequest {
            session_id: session.clone(),
            action: AutomationAction::Loop {
                action: LoopAction::Create {
                    prompt: "must not replace Goal".into(),
                    schedule: LoopSchedule::ModelDecided {},
                },
            },
        })
        .await;
    ensure!(
        rejected.is_err(),
        "active Goal allowed conflicting Loop creation"
    );
    let (identity, origin, human) = queue_and_steer(client, &session).await?;
    ensure!(
        matches!(origin.origin, AutomationTurnOrigin::Goal { .. }),
        "Goal work has wrong provenance"
    );
    release(&case, "work-1")?;
    let complete = wait_snapshot(client, &session, |value| {
        value
            .goal
            .as_ref()
            .is_some_and(|goal| goal.status == GoalStatus::Complete)
    })
    .await?;
    let turn = history(client, &session, &identity).await?;
    assert_history_input(&turn, &origin, &human)?;
    evidence.push(json!({"case":"goal", "snapshot":complete, "work":turn}));

    eprintln!("automation fixed Loop: first automatic work and one real minute before the second");
    let session = uuid::Uuid::new_v4().to_string();
    let case = marker("fixed");
    start(client, &session, selected(&case, &loop_skill)).await?;
    gate(&case, "work-1").await?;
    let (identity, origin, human) = queue_and_steer(client, &session).await?;
    ensure!(
        matches!(origin.origin, AutomationTurnOrigin::Loop { .. }),
        "Loop work has wrong provenance"
    );
    release(&case, "work-1")?;
    let first = history(client, &session, &identity).await?;
    assert_history_input(&first, &origin, &human)?;
    let scheduled = wait_snapshot(client, &session, |value| {
        value
            .loops
            .first()
            .is_some_and(|task| task.next_due_at.is_some())
    })
    .await?;
    let task = &scheduled.loops[0];
    ensure!(
        task.next_due_at.unwrap() >= first.completed_at + 59,
        "fixed cadence was not measured from settlement: {scheduled:?}"
    );
    std::fs::write(directory()?.join(format!("{case}.iteration")), "2")?;
    let wall_started = Instant::now();
    gate(&case, "work-2").await?;
    ensure!(
        wall_started.elapsed() >= Duration::from_secs(55),
        "second Loop work ran before a real minute cadence"
    );
    let current = active(client, &session).await?.active_turn.identity;
    control(
        client,
        &session,
        LoopAction::Stop {
            loop_id: task.id.clone(),
        },
    )
    .await?;
    ensure!(
        active(client, &session).await?.active_turn.identity == current,
        "Stop Loop cancelled its admitted Turn"
    );
    release(&case, "work-2")?;
    let second = history(client, &session, &current).await?;
    let stopped = client
        .list_automation(AutomationListRequest {
            session_id: Some(session.clone()),
            scope: AutomationListScope::Ended,
            cursor: None,
            limit: Some(100),
        })
        .await?;
    ensure!(
        stopped.data.iter().any(|entry| matches!(entry, AutomationListEntry::Loop { loop_task }
            if loop_task.id == task.id && loop_task.status == LoopStatus::Stopped && loop_task.next_due_at.is_none())),
        "Stop left a future wakeup"
    );
    evidence.push(
        json!({"case":"fixed-real-minute", "first":first, "second":second, "snapshot":stopped}),
    );

    eprintln!(
        "automation dynamic Loop: zero-minute decision rejection and late old-revision settlement"
    );
    let session = uuid::Uuid::new_v4().to_string();
    let case = marker("dynamic");
    start(client, &session, selected(&case, &loop_skill)).await?;
    gate(&case, "work-1").await?;
    let current = active(client, &session).await?.active_turn.identity;
    release(&case, "work-1")?;
    gate(&case, "declared").await?;
    let previous = snapshot(client, &session).await?.loops.remove(0);
    control(
        client,
        &session,
        LoopAction::Edit {
            loop_id: previous.id.clone(),
            expected_revision: previous.config_revision,
            prompt: format!("AUTO_CASE={case} edited future work"),
            schedule: LoopSchedule::FixedInterval { minutes: 2 },
        },
    )
    .await?;
    release(&case, "declared")?;
    let completed = history(client, &session, &current).await?;
    let edited = wait_snapshot(client, &session, |value| {
        value.loops[0].next_due_at.is_some()
    })
    .await?;
    ensure!(
        edited.loops[0].config_revision == previous.config_revision + 1
            && edited.loops[0].next_due_at.unwrap() >= completed.completed_at + 119,
        "old model decision overwrote edited configuration: {edited:?}"
    );
    let AgentRunResult::Completed(output) = &completed.result else {
        unreachable!("history requires normal settlement")
    };
    ensure!(
        output
            .messages
            .iter()
            .flat_map(|message| &message.blocks)
            .any(
                |block| matches!(&block.content, AgentMessageBlockContent::ToolResult(result)
                if result.call_id == format!("zero-{case}") && result.is_error)
            ),
        "zero-minute model decision was not rejected by the real Tool path"
    );
    control(
        client,
        &session,
        LoopAction::Stop {
            loop_id: previous.id,
        },
    )
    .await?;
    evidence.push(json!({"case":"dynamic-revision", "work":completed, "snapshot":edited}));

    lifecycle::run(client, &goal_skill, &loop_skill, &mut evidence).await?;
    stop_before_admission(client, &loop_skill).await?;
    pending_exclusion(client, &goal_skill, &loop_skill).await?;
    native_job_roundtrip(client).await?;
    let label = std::env::var("TALOS_AUTOMATION_TRANSPORT").unwrap_or_else(|_| "app-server".into());
    std::fs::write(
        directory()?.join(format!("{label}-business.json")),
        serde_json::to_vec_pretty(&evidence)?,
    )?;
    eprintln!("CASE automation_business PASS transport={label} skill=actual tool=actual queue=shared steer=sameTurn history=exactInput cadence=realMinute");
    Ok(())
}

async fn stop_before_admission(
    client: &mut impl GatewayClient,
    skill: &SkillSummary,
) -> Result<()> {
    let session = uuid::Uuid::new_v4().to_string();
    let case = marker("stop");
    start(client, &session, selected(&case, skill)).await?;
    gate(&case, "registered").await?;
    let task = snapshot(client, &session).await?.loops.remove(0);
    ensure!(
        task.next_due_at == Some(task.created_at + 10),
        "registration did not retain the initial grace deadline: {task:?}"
    );
    let registration = active(client, &session).await?.active_turn.identity;
    wait_snapshot(client, &session, |state| {
        state
            .loops
            .iter()
            .any(|current| current.id == task.id && current.next_due_at.is_none())
    })
    .await?;
    let queue = client
        .list_message_queue(MessageQueueListRequest {
            session_id: session.clone(),
        })
        .await?;
    ensure!(queue.messages.iter().any(|message| matches!(&message.source, QueuedMessageSource::Loop { loop_id, .. } if loop_id == &task.id)), "new Loop did not use the original queue");
    control(client, &session, LoopAction::Stop { loop_id: task.id }).await?;
    ensure!(
        client
            .list_message_queue(MessageQueueListRequest {
                session_id: session.clone()
            })
            .await?
            .messages
            .is_empty(),
        "Stop did not retract queued occurrence"
    );
    ensure!(
        active(client, &session).await?.active_turn.identity == registration,
        "Stop interrupted registration Turn"
    );
    release(&case, "registered")?;
    let completed = history(client, &session, &registration).await?;
    ensure!(
        completed.automation_input.is_none(),
        "Loop registration was mislabeled as triggered Loop work"
    );
    Ok(())
}

async fn pending_exclusion(
    client: &mut impl GatewayClient,
    goal: &SkillSummary,
    loop_skill: &SkillSummary,
) -> Result<()> {
    let session = uuid::Uuid::new_v4().to_string();
    let case = marker("pending");
    start(
        client,
        &session,
        text(&format!("AUTO_CASE={case} ordinary User work")),
    )
    .await?;
    gate(&case, "busy").await?;
    let running = active(client, &session).await?.active_turn.identity;
    let pending = client
        .add_queued_message(MessageQueueAddRequest {
            session_id: session.clone(),
            input: selected(&marker("goal"), goal),
        })
        .await?;
    let selection = pending.messages[0].id.clone();
    ensure!(
        client
            .add_queued_message(MessageQueueAddRequest {
                session_id: session.clone(),
                input: selected(&marker("fixed"), loop_skill)
            })
            .await
            .is_err(),
        "pending Goal allowed opposite Loop intent"
    );
    ensure!(
        client
            .submit_turn_input(SubmitTurnInputRequest {
                identity: running.clone(),
                input: SubmitTurnInput::QueuedMessage {
                    message_id: selection.clone()
                },
                client_request_id: None
            })
            .await
            .is_err(),
        "typed activation was accepted as Steer"
    );
    client
        .delete_queued_message(MessageQueueDeleteRequest {
            session_id: session.clone(),
            message_id: selection,
        })
        .await?;
    release(&case, "busy")?;
    let turn = history(client, &session, &running).await?;
    ensure!(
        turn.automation_input.is_none(),
        "ordinary User acquired automatic provenance"
    );
    ensure!(
        snapshot(client, &session).await?.goal.is_none(),
        "deleting pending intent activated a Goal"
    );
    Ok(())
}

async fn native_job_roundtrip(client: &mut impl GatewayClient) -> Result<()> {
    let jobs = client
        .list_builtin_jobs(BuiltinJobListRequest {
            computer_id: "embedded".into(),
        })
        .await?;
    let job = jobs
        .data
        .into_iter()
        .find(|job| job.id == BuiltinJobId::ProfileUsageRetention)
        .context("native job not projected for actual owner")?;
    for action in [BuiltinJobAction::Pause, BuiltinJobAction::RunOnce] {
        let changed = client
            .control_builtin_job(BuiltinJobControlRequest {
                computer_id: job.computer_id.clone(),
                execution_instance_id: job.execution_instance_id.clone(),
                job_id: job.id,
                action,
            })
            .await?;
        ensure!(
            changed.paused,
            "manual native run implicitly enabled its schedule"
        );
    }
    client
        .control_builtin_job(BuiltinJobControlRequest {
            computer_id: job.computer_id,
            execution_instance_id: job.execution_instance_id,
            job_id: job.id,
            action: BuiltinJobAction::Resume,
        })
        .await?;
    Ok(())
}

#[path = "automation_lifecycle_e2e.rs"]
mod lifecycle;
