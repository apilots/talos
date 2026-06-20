use std::{collections::HashMap, process::Stdio};

use anyhow::{anyhow, Context, Result};
use serde_json::{json, Value};
use talos_sdk::{
    canonical_method, AppServerClient, AppServerClientConfig, EventSubscribeParams,
    ExpectedFileVersion, ProjectPatch, TalosContext, TalosError, TalosInputItem,
    TalosMessageRequest, TalosProvider, TalosSourceType, TalosTimelineContent, TalosTimelineDelta,
    TalosTimelineEventPayload, TalosTimelineEventType, TalosTimelineItemKind,
    TimelineEventEnvelope, WorkspaceDocKind,
};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;

mod mcp_e2e;
mod skills_e2e;
use mcp_e2e::{mcp_e2e, prepare_mcp_fixture};
use skills_e2e::{prepare_skills_fixture, skills_e2e};

#[tokio::main]
async fn main() -> Result<()> {
    let scenario = Scenario::parse(std::env::args().nth(1).as_deref())?;
    let binary = std::env::var("TALOS_APP_SERVER_BIN").unwrap_or_else(|_| "app-server".to_string());

    if scenario == Scenario::Malformed {
        return malformed(&binary).await;
    }
    if scenario == Scenario::SkillsE2e {
        prepare_skills_fixture()?;
    }
    if scenario == Scenario::McpE2e {
        prepare_mcp_fixture()?;
    }

    let mut client = AppServerClient::connect(AppServerClientConfig {
        app_server_bin: binary,
        client_name: "talos-app-server-test-client".to_string(),
        client_version: env!("CARGO_PKG_VERSION").to_string(),
        actor_credentials: None,
        use_cloud_session_cache: scenario.uses_cloud_session_cache(),
    })
    .await?;
    match scenario {
        Scenario::Smoke => smoke(&mut client).await?,
        Scenario::Malformed => unreachable!("malformed scenario uses raw protocol harness"),
        Scenario::Replay => replay(&mut client).await?,
        Scenario::MultiSubscriber => multi_subscriber(&mut client).await?,
        Scenario::TurnSmoke => turn_smoke(&mut client).await?,
        Scenario::ActiveTurnConflict => active_turn_conflict(&mut client).await?,
        Scenario::ResourceMatrix => resource_matrix(&mut client).await?,
        Scenario::CloudSessionCache => cloud_session_cache(&mut client).await?,
        Scenario::SkillsE2e => skills_e2e(&mut client).await?,
        Scenario::McpE2e => mcp_e2e(&mut client).await?,
    }
    client.shutdown().await?;
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Scenario {
    Smoke,
    Malformed,
    Replay,
    MultiSubscriber,
    TurnSmoke,
    ActiveTurnConflict,
    ResourceMatrix,
    CloudSessionCache,
    SkillsE2e,
    McpE2e,
}

impl Scenario {
    fn parse(value: Option<&str>) -> Result<Self> {
        match value.unwrap_or("smoke") {
            "smoke" => Ok(Self::Smoke),
            "malformed" => Ok(Self::Malformed),
            "replay" => Ok(Self::Replay),
            "multi-subscriber" => Ok(Self::MultiSubscriber),
            "turn-smoke" => Ok(Self::TurnSmoke),
            "active-turn-conflict" => Ok(Self::ActiveTurnConflict),
            "resource-matrix" => Ok(Self::ResourceMatrix),
            "cloud-session-cache" => Ok(Self::CloudSessionCache),
            "skills-e2e" => Ok(Self::SkillsE2e),
            "mcp-e2e" => Ok(Self::McpE2e),
            other => Err(anyhow!("unsupported scenario: {other}")),
        }
    }

    fn uses_cloud_session_cache(self) -> bool {
        matches!(self, Self::CloudSessionCache)
    }
}

async fn smoke(client: &mut AppServerClient) -> Result<()> {
    eprintln!("model.list");
    client.list_models().await?;
    eprintln!("session id");
    new_session_id(client).await?;
    eprintln!("config.read");
    client.config_read().await?;
    eprintln!("memory.file.list");
    client.list_memory_files().await?;
    eprintln!("workspace.doc.list");
    client.list_workspace_docs().await?;
    Ok(())
}

async fn cloud_session_cache(client: &mut AppServerClient) -> Result<()> {
    eprintln!("cloud-session-cache model.list");
    client.list_models().await?;
    eprintln!("cloud-session-cache session id");
    new_session_id(client).await?;
    Ok(())
}

async fn replay(client: &mut AppServerClient) -> Result<()> {
    eprintln!("replay session id");
    let session_id = session_id_from_env().unwrap_or(new_session_id(client).await?);
    eprintln!("replay turn.start");
    start_turn(client, &session_id).await?;
    eprintln!("replay event.subscribe");
    let initial_subscription = subscribe_events(client, &session_id).await?;
    eprintln!("replay collect initial events");
    let initial_events =
        collect_until_terminal(client, &initial_subscription.subscription_id).await?;
    let replayable_initial_events: Vec<_> = initial_events
        .iter()
        .filter(|event| is_replayable_event(event))
        .cloned()
        .collect();
    let first_event_id = replayable_initial_events
        .first()
        .map(|event| event.event_id.clone())
        .ok_or_else(|| anyhow!("expected initial events before replay"))?;
    eprintln!("replay event.subscribe replay");
    let replay = subscribe_events_with_limit(client, &session_id, initial_events.len() + 8).await?;
    eprintln!("replay collect replay events");
    let replay_events = collect_events(
        client,
        &replay.subscription_id,
        replayable_initial_events.len(),
    )
    .await?;
    if replay_events.len() != replayable_initial_events.len() {
        return Err(anyhow!(
            "replay event count mismatch: initial={} replay={}",
            replayable_initial_events.len(),
            replay_events.len()
        ));
    }
    if replay_events.first().map(|event| event.event_id.as_str()) != Some(first_event_id.as_str()) {
        return Err(anyhow!("replay did not start from first persisted event"));
    }
    assert_matching_event_sequence(&replayable_initial_events, &replay_events)
}

async fn multi_subscriber(client: &mut AppServerClient) -> Result<()> {
    eprintln!("multi-subscriber session id");
    let session_id = session_id_from_env().unwrap_or(new_session_id(client).await?);
    eprintln!("multi-subscriber turn.start");
    start_turn(client, &session_id).await?;
    eprintln!("multi-subscriber first subscribe");
    let first = subscribe_events(client, &session_id).await?;
    eprintln!("multi-subscriber second subscribe");
    let second = subscribe_events(client, &session_id).await?;
    eprintln!("multi-subscriber collect events");
    let (first_events, second_events) = collect_two_subscriptions_until_terminal(
        client,
        &first.subscription_id,
        &second.subscription_id,
    )
    .await?;
    assert_matching_event_sequence(&first_events, &second_events)
}

async fn turn_smoke(client: &mut AppServerClient) -> Result<()> {
    eprintln!("turn-smoke session id");
    let session_id = new_session_id(client).await?;
    eprintln!("turn-smoke turn.start");
    start_turn(client, &session_id).await?;
    eprintln!("turn-smoke event.subscribe");
    let subscription = subscribe_events(client, &session_id).await?;
    eprintln!("turn-smoke collect events");
    let events = collect_until_terminal(client, &subscription.subscription_id).await?;
    ensure_turn_event_flow(&events)
}

async fn active_turn_conflict(client: &mut AppServerClient) -> Result<()> {
    eprintln!("active-turn-conflict session id");
    let session_id = new_session_id(client).await?;
    eprintln!("active-turn-conflict first turn.start");
    start_turn(client, &session_id).await?;
    eprintln!("active-turn-conflict second turn.start");
    let err = client
        .start_turn(
            &session_id,
            message_request("answer with ok"),
            Some("app-server-test-conflict".to_string()),
        )
        .await
        .expect_err("second turn should conflict");
    if matches!(err, talos_sdk::TalosError::Conflict(_)) {
        Ok(())
    } else {
        Err(anyhow!("expected active turn conflict, got {err}"))
    }
}

async fn resource_matrix(client: &mut AppServerClient) -> Result<()> {
    eprintln!("resource-matrix config/model list");
    client.list_models().await?;
    client.config_read().await?;

    eprintln!("resource-matrix memory roundtrip");
    let memory_path = "2099-12-31.md";
    let memory_content =
        "## [00:00] App Server\nsession: sdk\n\n**Facts:**\n- app-server resource parity\n";
    let memory_expected_version = match client.read_memory_file(memory_path).await {
        Ok(existing) => Some(existing.version_token),
        Err(TalosError::ResourceNotFound(_)) => None,
        Err(error) => return Err(error.into()),
    };
    let written_memory = client
        .write_memory_file(
            memory_path,
            memory_content.to_string(),
            memory_expected_version,
        )
        .await?;
    if !written_memory
        .content
        .contains("app-server resource parity")
    {
        return Err(anyhow!("memory write did not persist expected content"));
    }
    let read_memory = client.read_memory_file(memory_path).await?;
    if read_memory.version_token != written_memory.version_token {
        return Err(anyhow!("memory read returned a different version token"));
    }
    expect_conflict(
        client
            .write_memory_file(
                memory_path,
                "stale write".to_string(),
                Some("stale-version-token".to_string()),
            )
            .await,
        "memory stale write should conflict",
    )?;
    expect_validation(
        client.read_memory_file("not-a-date.md").await,
        "invalid memory path should validate",
    )?;
    client.list_memory_files().await?;

    eprintln!("resource-matrix workspace doc read/conflict");
    let read_workspace = client.read_workspace_doc(WorkspaceDocKind::Memory).await?;
    if !read_workspace.exists {
        return Err(anyhow!("expected bootstrap workspace memory doc to exist"));
    }
    expect_conflict(
        client
            .write_workspace_doc(
                WorkspaceDocKind::Memory,
                "blind overwrite".to_string(),
                ExpectedFileVersion::MustNotExist,
            )
            .await,
        "workspace blind overwrite should conflict",
    )?;
    client.list_workspace_docs().await?;

    eprintln!("resource-matrix project roundtrip");
    let project_dir = std::env::temp_dir().join(format!(
        "talos-app-server-{}",
        uuid::Uuid::new_v4().simple()
    ));
    std::fs::create_dir_all(&project_dir)
        .with_context(|| format!("create project test directory {}", project_dir.display()))?;
    let project_name = format!("App Server {}", uuid::Uuid::new_v4().simple());
    let project = client
        .create_project(project_name.clone(), project_dir.clone())
        .await?;
    let read_project = client.get_project(&project.project_id).await?;
    if read_project.directory != project_dir {
        return Err(anyhow!("project.read returned a different directory"));
    }
    let listed_projects = client.list_projects().await?;
    if !listed_projects
        .iter()
        .any(|candidate| candidate.project_id == project.project_id)
    {
        return Err(anyhow!("project.list did not include created project"));
    }
    let patched_name = format!("{project_name} patched");
    let patched_project = client
        .patch_project(
            &project.project_id,
            ProjectPatch {
                name: Some(patched_name.clone()),
            },
        )
        .await?;
    if patched_project.name != patched_name {
        return Err(anyhow!("project.patch did not update name"));
    }
    client.delete_project(&project.project_id).await?;
    expect_not_found(
        client.get_project(&project.project_id).await,
        "deleted project should map to not found",
    )?;

    eprintln!("resource-matrix session error mapping");
    let session_id = new_session_id(client).await?;
    start_turn(client, &session_id).await?;
    let listed_sessions = client.list_sessions(Default::default()).await?;
    if !listed_sessions
        .iter()
        .any(|candidate| candidate.context.session_id == session_id)
    {
        return Err(anyhow!("session.list did not include created session"));
    }
    let _snapshot = client.recover_session(&session_id).await?;
    expect_not_found(
        client.get_session("missing-session").await,
        "missing session should map to not found",
    )?;
    expect_not_found(
        client.recover_session("missing-session").await,
        "missing session recovery should map to not found",
    )?;
    expect_not_found(
        client
            .submit_dynamic_tool_result(
                "missing-session",
                "call-1",
                "DynamicTool",
                "result".to_string(),
                true,
            )
            .await,
        "missing dynamic tool result session should map to not found",
    )?;

    Ok(())
}

async fn new_session_id(client: &mut AppServerClient) -> Result<String> {
    let _ = client;
    Ok(uuid::Uuid::new_v4().to_string())
}

async fn subscribe_events(
    client: &mut AppServerClient,
    session_id: &str,
) -> Result<talos_sdk::EventSubscribeResponse> {
    subscribe_events_with_limit(client, session_id, 32).await
}

async fn subscribe_events_with_limit(
    client: &mut AppServerClient,
    session_id: &str,
    replay_limit: usize,
) -> Result<talos_sdk::EventSubscribeResponse> {
    subscribe_events_with_cursor(client, session_id, None, replay_limit).await
}

async fn subscribe_events_with_cursor(
    client: &mut AppServerClient,
    session_id: &str,
    since_event_id: Option<String>,
    replay_limit: usize,
) -> Result<talos_sdk::EventSubscribeResponse> {
    Ok(client
        .subscribe_events(EventSubscribeParams {
            session_id: session_id.to_string(),
            since_event_id,
            replay_limit: Some(replay_limit),
            close_when_idle: None,
            close_on_terminal: None,
        })
        .await?)
}

async fn start_turn(client: &mut AppServerClient, session_id: &str) -> Result<()> {
    client
        .start_turn(
            session_id,
            message_request("answer with ok"),
            Some(format!("app-server-test-{}", uuid::Uuid::new_v4().simple())),
        )
        .await?;
    Ok(())
}

fn message_request(text: &str) -> TalosMessageRequest {
    TalosMessageRequest {
        input: vec![TalosInputItem::Text {
            text: text.to_string(),
        }],
        workspace_id: None,
        context: Some(test_talos_context()),
        model: std::env::var("TALOS_APP_SERVER_TEST_MODEL")
            .ok()
            .filter(|value| !value.trim().is_empty()),
        project_id: None,
        current_directory: None,
    }
}

async fn collect_until_terminal(
    client: &mut AppServerClient,
    _subscription_id: &str,
) -> Result<Vec<TimelineEventEnvelope>> {
    let mut events = Vec::new();
    for _ in 0..1024 {
        let event = next_event_with_timeout(client).await?;
        let terminal = is_terminal_event(&event);
        events.push(event);
        if terminal {
            return Ok(events);
        }
    }
    Err(anyhow!("timed out waiting for terminal event"))
}

async fn collect_events(
    client: &mut AppServerClient,
    _subscription_id: &str,
    expected_count: usize,
) -> Result<Vec<TimelineEventEnvelope>> {
    let mut events = Vec::new();
    for _ in 0..expected_count.saturating_add(16).max(64) {
        let event = next_event_with_timeout(client).await?;
        events.push(event);
        if events.len() >= expected_count {
            return Ok(events);
        }
    }
    Err(anyhow!(
        "timed out waiting for {expected_count} events; got {}",
        events.len()
    ))
}

async fn next_event_with_timeout(client: &mut AppServerClient) -> Result<TimelineEventEnvelope> {
    tokio::time::timeout(std::time::Duration::from_secs(30), client.next_event())
        .await
        .map_err(|_| anyhow!("timed out waiting for app-server event"))?
        .map_err(Into::into)
}

async fn collect_two_subscriptions_until_terminal(
    client: &mut AppServerClient,
    first_subscription_id: &str,
    second_subscription_id: &str,
) -> Result<(Vec<TimelineEventEnvelope>, Vec<TimelineEventEnvelope>)> {
    let mut first_events = Vec::new();
    let mut second_events = Vec::new();
    for _ in 0..256 {
        let notification = client.next_notification().await?;
        let terminal = is_terminal_event(&notification.event);
        if notification.subscription_id == first_subscription_id {
            first_events.push(notification.event);
        } else if notification.subscription_id == second_subscription_id {
            second_events.push(notification.event);
        }
        if first_events.last().is_some_and(is_terminal_event)
            && second_events.last().is_some_and(is_terminal_event)
        {
            return Ok((first_events, second_events));
        }
        if terminal
            && notification.subscription_id != first_subscription_id
            && notification.subscription_id != second_subscription_id
        {
            return Err(anyhow!(
                "received terminal event for unexpected subscription {}",
                notification.subscription_id
            ));
        }
    }
    Err(anyhow!("timed out waiting for terminal event"))
}

async fn malformed(binary: &str) -> Result<()> {
    let mut child = Command::new(binary)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .with_context(|| format!("spawn {binary}"))?;
    let mut stdin = child
        .stdin
        .take()
        .context("app-server stdin is not piped")?;
    let stdout = child
        .stdout
        .take()
        .context("app-server stdout is not piped")?;
    let mut stdout = BufReader::new(stdout);

    stdin.write_all(b"{\n").await?;
    let response = read_message(&mut stdout).await?;
    if response["error"]["code"] != -32700 {
        return Err(anyhow!("expected parse error, got {response}"));
    }
    let initialize = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "client_info": {
                "name": "talos-app-server-test-client",
                "version": env!("CARGO_PKG_VERSION")
            },
            "capabilities": {
                "protocol_version": { "major": 0, "minor": 2 },
                "supports_server_requests": false,
                "supports_event_replay": true
            }
        }
    });
    stdin.write_all(initialize.to_string().as_bytes()).await?;
    stdin.write_all(b"\n").await?;
    stdin.flush().await?;
    let response = read_message(&mut stdout).await?;
    if response.get("error").is_some() {
        return Err(anyhow!(
            "initialize returned error after malformed line: {response}"
        ));
    }
    stdin.shutdown().await?;
    drop(stdin);
    let status = child.wait().await?;
    if status.success() {
        Ok(())
    } else {
        Err(anyhow!("app-server exited with {status}"))
    }
}

async fn read_message(stdout: &mut BufReader<tokio::process::ChildStdout>) -> Result<Value> {
    let mut line = String::new();
    let bytes = stdout.read_line(&mut line).await?;
    if bytes == 0 {
        return Err(anyhow!("app-server stdout closed"));
    }
    serde_json::from_str(line.trim_end()).context("parse app-server response")
}

fn session_id_from_env() -> Option<String> {
    std::env::var("TALOS_APP_SERVER_TEST_SESSION_ID")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn test_talos_context() -> TalosContext {
    TalosContext {
        source_type: TalosSourceType::Cli,
        provider: TalosProvider::Cli,
        user_id: "app-server-test-client".to_string(),
        channel_id: Some("app-server-test-client".to_string()),
        thread_id: None,
        message_id: format!("msg-{}", uuid::Uuid::new_v4().simple()),
        trace_id: format!("trace-{}", uuid::Uuid::new_v4().simple()),
        project_root: None,
        environment: None,
    }
}

fn is_terminal_event(event: &TimelineEventEnvelope) -> bool {
    matches!(
        event.event.event_type,
        TalosTimelineEventType::TurnCompleted | TalosTimelineEventType::TurnError
    )
}

fn is_replayable_event(event: &TimelineEventEnvelope) -> bool {
    event.event.event_type != TalosTimelineEventType::TurnMetrics
}

fn ensure_turn_event_flow(events: &[TimelineEventEnvelope]) -> Result<()> {
    if events
        .iter()
        .any(|event| event.event.event_type == TalosTimelineEventType::TurnError)
    {
        return Err(anyhow!("turn failed during app-server e2e"));
    }
    for required in [
        "timeline.turn.started",
        "timeline.item.delta",
        "timeline.turn.completed",
    ] {
        if !events
            .iter()
            .any(|event| canonical_method(&event.event.event_type) == required)
        {
            return Err(anyhow!("turn event flow missing {required}"));
        }
    }
    Ok(())
}

fn assistant_text_from_events(events: &[TimelineEventEnvelope]) -> String {
    let mut output = String::new();
    let mut item_kinds = HashMap::new();
    for event in events {
        match &event.event.payload {
            TalosTimelineEventPayload::ItemStarted(started) => {
                item_kinds.insert(started.item.item_id.clone(), started.item.kind);
                if started.item.kind == TalosTimelineItemKind::AssistantText {
                    if let TalosTimelineContent::Text { text } = &started.item.content {
                        output.push_str(text);
                    }
                }
            }
            TalosTimelineEventPayload::ItemDelta(delta) => {
                if item_kinds.get(&delta.item_id) == Some(&TalosTimelineItemKind::AssistantText) {
                    if let TalosTimelineDelta::Text { text } = &delta.delta {
                        output.push_str(text);
                    }
                }
            }
            TalosTimelineEventPayload::ItemCompleted(completed) => {
                if item_kinds.get(&completed.item_id) == Some(&TalosTimelineItemKind::AssistantText)
                {
                    if let Some(TalosTimelineContent::Text { text }) = &completed.final_content {
                        if !text.is_empty() && !output.contains(text) {
                            output.push_str(text);
                        }
                    }
                }
            }
            _ => {}
        }
    }
    output
}

fn assert_matching_event_sequence(
    left: &[TimelineEventEnvelope],
    right: &[TimelineEventEnvelope],
) -> Result<()> {
    ensure_turn_event_flow(left)?;
    ensure_turn_event_flow(right)?;
    let left_projection: Vec<_> = left.iter().map(event_projection).collect();
    let right_projection: Vec<_> = right.iter().map(event_projection).collect();
    if left_projection != right_projection {
        return Err(anyhow!(
            "event sequence mismatch:\nleft={left_projection:?}\nright={right_projection:?}"
        ));
    }
    Ok(())
}

fn event_projection(event: &TimelineEventEnvelope) -> (&str, &str, Option<&str>, &'static str) {
    (
        event.event_id.as_str(),
        event.session_id.as_str(),
        Some(event.turn_id.as_str()),
        canonical_method(&event.event.event_type),
    )
}

fn expect_validation<T>(result: Result<T, TalosError>, context: &str) -> Result<()> {
    match result {
        Err(TalosError::ValidationError(_)) => Ok(()),
        Ok(_) => Err(anyhow!("{context}: expected validation error, got success")),
        Err(error) => Err(anyhow!("{context}: expected validation error, got {error}")),
    }
}

fn expect_conflict<T>(result: Result<T, TalosError>, context: &str) -> Result<()> {
    match result {
        Err(TalosError::Conflict(_)) => Ok(()),
        Ok(_) => Err(anyhow!("{context}: expected conflict, got success")),
        Err(error) => Err(anyhow!("{context}: expected conflict, got {error}")),
    }
}

fn expect_not_found<T>(result: Result<T, TalosError>, context: &str) -> Result<()> {
    match result {
        Err(TalosError::ResourceNotFound(_)) => Ok(()),
        Ok(_) => Err(anyhow!("{context}: expected not found, got success")),
        Err(error) => Err(anyhow!("{context}: expected not found, got {error}")),
    }
}
