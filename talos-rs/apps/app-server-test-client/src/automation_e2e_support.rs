//! Existing SDK and canonical History helpers shared by automation acceptance cases.
use anyhow::{anyhow, ensure, Context, Result};
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};
use talos_sdk::{
    ActiveTurnSnapshot, AgentMessage, AgentMessageBlockContent, AgentMessageRole, AgentRunResult,
    AgentTurnRef, AgentUserContent, AgentUserInput, AutomationAction, AutomationInputOrigin,
    AutomationRequest, GatewayClient, LoopAction, MessageQueueAddRequest, MessageQueueListRequest,
    ModelSelection, QueuedMessageSource, SessionActiveTurnRequest, SessionAutomationReadRequest,
    SessionAutomationSnapshot, SessionHistoryRequest, SessionHistoryTurn, SkillSummary,
    StartTurnRequest, SubmitTurnInput, SubmitTurnInputRequest, UserPermissionMode,
};
const POLL: Duration = Duration::from_millis(50);
const LIMIT: Duration = Duration::from_secs(140);

pub(super) fn directory() -> Result<PathBuf> {
    std::env::var_os("TALOS_AUTOMATION_FIXTURE_DIR")
        .map(PathBuf::from)
        .context("TALOS_AUTOMATION_FIXTURE_DIR is required")
}

pub(super) fn marker(kind: &str) -> String {
    format!("{kind}_{}", uuid::Uuid::new_v4().simple())
}
pub(super) fn text(value: &str) -> AgentUserInput {
    AgentUserInput {
        content: vec![AgentUserContent::Text { text: value.into() }],
    }
}
pub(super) fn selected(marker: &str, skill: &SkillSummary) -> AgentUserInput {
    let instruction = match marker.split_once('_').map(|(kind, _)| kind) {
        Some("fixed" | "stop") => "check progress every 1 minute",
        Some("fixedlong") => "check progress every 10081 minutes",
        Some("dynamic" | "dynamicstop" | "strict") => {
            "check progress; model decides the next check time"
        }
        _ => "complete the verifiable Goal objective",
    };
    let mut input = text(&format!(
        "AUTO_CASE={marker} controlled automation work; {instruction}"
    ));
    input.content.push(AgentUserContent::SkillSelection {
        name: skill.name.clone(),
        path: skill.path.to_string_lossy().into(),
    });
    input
}
pub(super) async fn start(
    client: &mut impl GatewayClient,
    session: &str,
    input: AgentUserInput,
) -> Result<()> {
    client
        .start_turn(StartTurnRequest {
            node_id: None,
            session_id: session.into(),
            input,
            model_selection: crate::test_model_ref().map(ModelSelection::without_reasoning),
            user_permission_mode: Some(UserPermissionMode::FullAccess),
            project_id: None,
            current_directory: None,
            client_request_id: Some(uuid::Uuid::new_v4().to_string()),
        })
        .await?;
    Ok(())
}
pub(super) async fn gate(marker: &str, name: &str) -> Result<()> {
    let path = directory()?.join(format!("{marker}.{name}"));
    let started = Instant::now();
    while !path.exists() {
        ensure!(
            started.elapsed() < LIMIT,
            "model did not reach {}",
            path.display()
        );
        tokio::time::sleep(POLL).await;
    }
    Ok(())
}
pub(super) fn release(marker: &str, name: &str) -> Result<()> {
    std::fs::write(
        directory()?.join(format!("{marker}.release-{name}")),
        "released",
    )?;
    Ok(())
}
pub(super) async fn snapshot(
    client: &mut impl GatewayClient,
    session: &str,
) -> Result<SessionAutomationSnapshot> {
    Ok(client
        .session_automation(SessionAutomationReadRequest {
            session_id: session.into(),
        })
        .await?)
}
pub(super) async fn wait_snapshot(
    client: &mut impl GatewayClient,
    session: &str,
    predicate: impl Fn(&SessionAutomationSnapshot) -> bool,
) -> Result<SessionAutomationSnapshot> {
    let started = Instant::now();
    loop {
        let value = snapshot(client, session).await?;
        if predicate(&value) {
            return Ok(value);
        }
        ensure!(
            started.elapsed() < LIMIT,
            "automation did not converge: {value:?}"
        );
        tokio::time::sleep(POLL).await;
    }
}
pub(super) async fn active(
    client: &mut impl GatewayClient,
    session: &str,
) -> Result<ActiveTurnSnapshot> {
    client
        .get_active_turn(SessionActiveTurnRequest {
            session_id: session.into(),
        })
        .await?
        .active_turn
        .context("expected a real active Turn")
}
pub(super) async fn history(
    client: &mut impl GatewayClient,
    session: &str,
    turn: &AgentTurnRef,
) -> Result<SessionHistoryTurn> {
    let started = Instant::now();
    loop {
        let history = client
            .get_session_history(SessionHistoryRequest {
                session_id: session.into(),
                cursor: None,
                limit: Some(100),
            })
            .await?;
        if let Some(found) = history.data.into_iter().find(|item| item.identity == *turn) {
            ensure!(
                matches!(found.result, AgentRunResult::Completed(_)),
                "work Turn failed: {found:?}"
            );
            return Ok(found);
        }
        ensure!(
            started.elapsed() < LIMIT,
            "Turn did not enter canonical History: {turn:?}"
        );
        tokio::time::sleep(POLL).await;
    }
}
pub(super) fn human_message<'a>(
    messages: &'a [AgentMessage],
    human: &str,
) -> Option<&'a AgentMessage> {
    messages.iter().find(|message| message.role == AgentMessageRole::User && message.blocks.iter().any(|block|
        matches!(&block.content, AgentMessageBlockContent::UserContent(AgentUserContent::Text { text }) if text == human)))
}
pub(super) async fn queue_and_steer(
    client: &mut impl GatewayClient,
    session: &str,
) -> Result<(AgentTurnRef, AutomationInputOrigin, String)> {
    let running = active(client, session).await?;
    let identity = running.active_turn.identity;
    let origin = running
        .active_turn
        .automation_input
        .context("automatic work has no Input provenance")?;
    let human = format!("HUMAN_STEER_INPUT_{}", uuid::Uuid::new_v4().simple());
    let queued = client
        .add_queued_message(MessageQueueAddRequest {
            session_id: session.into(),
            input: text(&human),
        })
        .await?;
    ensure!(
        queued.messages.len() == 1 && queued.messages[0].source == QueuedMessageSource::User,
        "ordinary User input did not enter the shared queue: {queued:?}"
    );
    ensure!(
        active(client, session).await?.active_turn.identity == identity,
        "queueing started a second Turn"
    );
    client
        .submit_turn_input(SubmitTurnInputRequest {
            identity: identity.clone(),
            input: SubmitTurnInput::QueuedMessage {
                message_id: queued.messages[0].id.clone(),
            },
            client_request_id: None,
        })
        .await?;
    let started = Instant::now();
    loop {
        let steered = active(client, session).await?;
        ensure!(
            steered.active_turn.identity == identity,
            "Steer changed the current Turn"
        );
        if let Some(message) = human_message(&steered.messages, &human) {
            ensure!(
                message.message_id != origin.message_id,
                "Steer inherited the automation Input badge"
            );
            ensure!(
                steered.active_turn.automation_input.as_ref() == Some(&origin),
                "Steer changed Input origin"
            );
            break;
        }
        ensure!(started.elapsed() < LIMIT, "Steer was not durably admitted");
        tokio::time::sleep(POLL).await;
    }
    let after = client
        .list_message_queue(MessageQueueListRequest {
            session_id: session.into(),
        })
        .await?;
    ensure!(
        after.messages.is_empty(),
        "Steer did not consume its queued User message"
    );
    Ok((identity, origin, human))
}
pub(super) fn assert_history_input(
    turn: &SessionHistoryTurn,
    origin: &AutomationInputOrigin,
    human: &str,
) -> Result<()> {
    ensure!(
        turn.automation_input.as_ref() == Some(origin),
        "History lost exact automation Input origin"
    );
    let AgentRunResult::Completed(output) = &turn.result else {
        return Err(anyhow!("work failed"));
    };
    let initial = output
        .messages
        .iter()
        .find(|message| message.message_id == origin.message_id)
        .context("origin does not reference a real Input message")?;
    ensure!(
        initial.role == AgentMessageRole::User,
        "automation badge points to a non-Input"
    );
    let steer =
        human_message(&output.messages, human).context("History lost ordinary Steer Input")?;
    ensure!(
        steer.message_id != initial.message_id,
        "ordinary Steer Input reused automatic Input identity"
    );
    Ok(())
}
pub(super) async fn control(
    client: &mut impl GatewayClient,
    session: &str,
    action: LoopAction,
) -> Result<()> {
    client
        .execute_automation(AutomationRequest {
            session_id: session.into(),
            action: AutomationAction::Loop { action },
        })
        .await?;
    Ok(())
}
