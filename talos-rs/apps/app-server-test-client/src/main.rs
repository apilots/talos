use std::process::Stdio;

use anyhow::{anyhow, Context, Result};
use serde_json::{json, Value};
use talos_agent::AgentEventReducer;
use talos_sdk::{
    timeline_event_method, AgentAssistantTextPhase, AgentEventPayload, AgentMessageBlockContent,
    AgentTurnRef, AppServerClient, AppServerClientConfig, ConfigGetRequest, DynamicToolResult,
    ExpectedFileVersion, GatewayClient, MemoryFileListRequest, MemoryFileReadRequest,
    MemoryFileWriteRequest, ModelListRequest, ProjectDeleteRequest, ProjectListRequest,
    ProjectReadRequest, RecoverTimelineRequest, SessionGetRequest, SubmitTurnInput,
    SubmitTurnInputRequest, SubscribeTimelineRequest, TalosError, TimelineEvent,
    UserActionResponse, WorkspaceDocKind, WorkspaceDocListRequest, WorkspaceDocReadRequest,
    WorkspaceDocWriteRequest,
};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;

mod mcp_e2e;
mod memory_e2e;
mod message_queue_e2e;
mod request;
mod resource_matrix;
use resource_matrix::resource_matrix;
mod scenario;
mod skills_e2e;
use mcp_e2e::{mcp_e2e, prepare_mcp_fixture};
use message_queue_e2e::message_queue_e2e;
use request::{start_turn_request, test_model_ref};
use scenario::Scenario;
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
        Scenario::MemoryE2e => memory_e2e::run(&mut client).await?,
        Scenario::CloudSessionCache => cloud_session_cache(&mut client).await?,
        Scenario::SkillsE2e => skills_e2e(&mut client).await?,
        Scenario::McpE2e => mcp_e2e(&mut client).await?,
        Scenario::MessageQueueE2e => message_queue_e2e(&mut client).await?,
    }
    GatewayClient::shutdown(&mut client).await?;
    Ok(())
}

async fn smoke(client: &mut AppServerClient) -> Result<()> {
    eprintln!("model/list");
    list_models(client).await?;
    eprintln!("session id");
    new_session_id(client).await?;
    eprintln!("config/get");
    get_config(client).await?;
    eprintln!("memory/file/list");
    list_memory_files(client).await?;
    eprintln!("workspace/doc/list");
    list_workspace_docs(client).await?;
    Ok(())
}

async fn cloud_session_cache(client: &mut AppServerClient) -> Result<()> {
    eprintln!("cloud-session-cache model.list");
    list_models(client).await?;
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
        .start_turn(start_turn_request(
            &session_id,
            "answer with ok",
            Some("app-server-test-conflict".to_string()),
        ))
        .await
        .expect_err("second turn should conflict");
    if err.is_code(talos_sdk::ErrorCode::ActiveTurnRunning) {
        Ok(())
    } else {
        Err(anyhow!("expected active turn conflict, got {err}"))
    }
}

async fn new_session_id(client: &mut AppServerClient) -> Result<String> {
    let _ = client;
    Ok(uuid::Uuid::new_v4().to_string())
}

async fn list_models(
    client: &mut AppServerClient,
) -> talos_sdk::TalosResult<talos_sdk::ModelListResponse> {
    GatewayClient::list_models(client, ModelListRequest { node_id: None }).await
}

async fn get_config(
    client: &mut AppServerClient,
) -> talos_sdk::TalosResult<talos_sdk::ConfigResponse> {
    GatewayClient::get_config(client, ConfigGetRequest::default()).await
}

async fn list_memory_files(
    client: &mut AppServerClient,
) -> talos_sdk::TalosResult<talos_sdk::MemoryFileListResponse> {
    GatewayClient::list_memory_files(
        client,
        MemoryFileListRequest {
            node_id: None,
            limit: Some(32),
            before_date: None,
            date_from: None,
            date_to: None,
        },
    )
    .await
}

async fn read_memory_file(
    client: &mut AppServerClient,
    date: impl Into<String>,
) -> talos_sdk::TalosResult<talos_sdk::MemoryFileReadResponse> {
    GatewayClient::read_memory_file(
        client,
        MemoryFileReadRequest {
            node_id: None,
            date: date.into(),
        },
    )
    .await
}

async fn write_memory_file(
    client: &mut AppServerClient,
    date: impl Into<String>,
    content: String,
    expected_version: Option<String>,
) -> talos_sdk::TalosResult<talos_sdk::MemoryFileWriteResponse> {
    GatewayClient::write_memory_file(
        client,
        MemoryFileWriteRequest {
            node_id: None,
            date: date.into(),
            content,
            expected_version,
        },
    )
    .await
}

async fn list_workspace_docs(
    client: &mut AppServerClient,
) -> talos_sdk::TalosResult<talos_sdk::WorkspaceDocListResponse> {
    GatewayClient::list_workspace_docs(client, WorkspaceDocListRequest { node_id: None }).await
}

async fn read_workspace_doc(
    client: &mut AppServerClient,
    kind: WorkspaceDocKind,
) -> talos_sdk::TalosResult<talos_sdk::WorkspaceDocReadResponse> {
    GatewayClient::read_workspace_doc(
        client,
        WorkspaceDocReadRequest {
            node_id: None,
            kind,
        },
    )
    .await
}

async fn write_workspace_doc(
    client: &mut AppServerClient,
    kind: WorkspaceDocKind,
    content: String,
    expected_version: ExpectedFileVersion,
) -> talos_sdk::TalosResult<talos_sdk::WorkspaceDocWriteResponse> {
    GatewayClient::write_workspace_doc(
        client,
        WorkspaceDocWriteRequest {
            node_id: None,
            kind,
            content,
            expected_version: expected_file_version_token(expected_version),
        },
    )
    .await
}

async fn list_all_projects(
    client: &mut AppServerClient,
) -> talos_sdk::TalosResult<talos_sdk::ProjectListResponse> {
    GatewayClient::list_projects(client, ProjectListRequest { node_id: None }).await
}

async fn get_project_by_id(
    client: &mut AppServerClient,
    project_id: &str,
) -> talos_sdk::TalosResult<talos_sdk::ProjectReadResponse> {
    GatewayClient::get_project(
        client,
        ProjectReadRequest {
            project_id: project_id.to_string(),
        },
    )
    .await
}

async fn delete_project_by_id(
    client: &mut AppServerClient,
    project_id: &str,
) -> talos_sdk::TalosResult<talos_sdk::ProjectDeleteResponse> {
    GatewayClient::delete_project(
        client,
        ProjectDeleteRequest {
            project_id: project_id.to_string(),
        },
    )
    .await
}

async fn get_session_by_id(
    client: &mut AppServerClient,
    session_id: &str,
) -> talos_sdk::TalosResult<talos_sdk::SessionGetResponse> {
    GatewayClient::get_session(
        client,
        SessionGetRequest {
            session_id: session_id.to_string(),
        },
    )
    .await
}

async fn recover_timeline_after(
    client: &mut AppServerClient,
    session_id: &str,
    after_event_id: Option<String>,
) -> talos_sdk::TalosResult<talos_sdk::RecoverTimelineResponse> {
    GatewayClient::recover_timeline(
        client,
        RecoverTimelineRequest {
            session_id: session_id.to_string(),
            after_event_id,
            replay_limit: None,
        },
    )
    .await
}

async fn submit_dynamic_tool_result_for_missing_session(
    client: &mut AppServerClient,
    session_id: &str,
    output: String,
    success: bool,
) -> talos_sdk::TalosResult<talos_sdk::SubmitTurnInputResponse> {
    GatewayClient::submit_turn_input(
        client,
        SubmitTurnInputRequest {
            identity: AgentTurnRef::new(session_id, "missing-turn"),
            input: SubmitTurnInput::UserActionResponse {
                action_id: "missing-action".to_string(),
                response: UserActionResponse::DynamicToolResult(DynamicToolResult {
                    output,
                    success,
                }),
            },
            client_request_id: None,
        },
    )
    .await
}

fn expected_file_version_token(expected_version: ExpectedFileVersion) -> Option<String> {
    match expected_version {
        ExpectedFileVersion::MustNotExist => None,
        ExpectedFileVersion::Match { version_token } => Some(version_token),
    }
}

async fn subscribe_events(
    client: &mut AppServerClient,
    session_id: &str,
) -> Result<talos_sdk::SubscribeTimelineResponse> {
    subscribe_events_with_limit(client, session_id, 32).await
}

async fn subscribe_events_with_limit(
    client: &mut AppServerClient,
    session_id: &str,
    replay_limit: usize,
) -> Result<talos_sdk::SubscribeTimelineResponse> {
    subscribe_events_with_cursor(client, session_id, None, replay_limit).await
}

async fn subscribe_events_with_cursor(
    client: &mut AppServerClient,
    session_id: &str,
    since_event_id: Option<String>,
    replay_limit: usize,
) -> Result<talos_sdk::SubscribeTimelineResponse> {
    Ok(GatewayClient::subscribe_timeline(
        client,
        SubscribeTimelineRequest {
            session_id: session_id.to_string(),
            turn_id: None,
            since_event_id,
            replay_limit: Some(replay_limit),
            close_when_idle: None,
            close_on_terminal: None,
        },
    )
    .await?)
}

async fn start_turn(client: &mut AppServerClient, session_id: &str) -> Result<()> {
    client
        .start_turn(start_turn_request(
            session_id,
            "answer with ok",
            Some(format!("app-server-test-{}", uuid::Uuid::new_v4().simple())),
        ))
        .await?;
    Ok(())
}

async fn collect_until_terminal(
    client: &mut AppServerClient,
    _subscription_id: &str,
) -> Result<Vec<TimelineEvent>> {
    tokio::time::timeout(std::time::Duration::from_secs(180), async {
        let mut events = Vec::new();
        loop {
            let event = next_event_with_timeout(client).await?;
            let terminal = is_terminal_event(&event);
            events.push(event);
            if terminal {
                return Ok(events);
            }
        }
    })
    .await
    .map_err(|_| anyhow!("timed out waiting for terminal event"))?
}

async fn collect_events(
    client: &mut AppServerClient,
    _subscription_id: &str,
    expected_count: usize,
) -> Result<Vec<TimelineEvent>> {
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

async fn next_event_with_timeout(client: &mut AppServerClient) -> Result<TimelineEvent> {
    tokio::time::timeout(std::time::Duration::from_secs(30), client.next_event())
        .await
        .map_err(|_| anyhow!("timed out waiting for app-server event"))?
        .map_err(Into::into)
}

async fn collect_two_subscriptions_until_terminal(
    client: &mut AppServerClient,
    first_subscription_id: &str,
    second_subscription_id: &str,
) -> Result<(Vec<TimelineEvent>, Vec<TimelineEvent>)> {
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
                "version": env!("CARGO_PKG_VERSION"),
                "kind": "sdk"
            },
            "capabilities": {
                "protocol_version": talos_app_protocol::version::ProtocolVersion::CURRENT,
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

fn is_terminal_event(event: &TimelineEvent) -> bool {
    matches!(
        event.event.agent_event().map(|event| &event.payload),
        Some(AgentEventPayload::TurnCompleted(_) | AgentEventPayload::TurnFailed(_))
    )
}

fn is_replayable_event(_event: &TimelineEvent) -> bool {
    true
}

fn ensure_turn_event_flow(events: &[TimelineEvent]) -> Result<()> {
    if let Some(error) = events
        .iter()
        .filter_map(|event| event.event.agent_event())
        .find_map(|event| match &event.payload {
            AgentEventPayload::TurnFailed(error) => Some(&error.error),
            _ => None,
        })
    {
        return Err(anyhow!("turn failed during app-server e2e: {error:?}"));
    }
    for required in [
        "timeline/turn/started",
        "timeline/message/block/delta",
        "timeline/turn/completed",
    ] {
        if !events
            .iter()
            .any(|event| timeline_event_method(&event.event) == required)
        {
            return Err(anyhow!("turn event flow missing {required}"));
        }
    }
    Ok(())
}

fn assistant_text_from_events(events: &[TimelineEvent]) -> Result<String> {
    let first = events
        .first()
        .ok_or_else(|| anyhow!("Agent Turn emitted no Timeline events"))?;
    let mut reducer = AgentEventReducer::new(first.event.identity().clone());
    for event in events.iter().filter_map(|event| event.event.agent_event()) {
        reducer
            .apply(event.clone())
            .map_err(|error| anyhow!("invalid Agent event sequence: {error}"))?;
    }
    Ok(reducer
        .checkpoint()
        .messages
        .into_iter()
        .flat_map(|message| message.blocks)
        .filter_map(|block| match block.content {
            AgentMessageBlockContent::AssistantText(text)
                if text.phase == AgentAssistantTextPhase::FinalAnswer =>
            {
                Some(text.text)
            }
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n"))
}

fn assert_matching_event_sequence(left: &[TimelineEvent], right: &[TimelineEvent]) -> Result<()> {
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

fn event_projection(event: &TimelineEvent) -> (&str, &str, Option<&str>, &'static str) {
    (
        event.event_id.as_str(),
        event.event.identity().session_id.as_str(),
        Some(event.event.identity().turn_id.as_str()),
        timeline_event_method(&event.event),
    )
}

fn expect_validation<T>(result: Result<T, TalosError>, context: &str) -> Result<()> {
    match result {
        Err(error) if error.is_code(talos_sdk::ErrorCode::InvalidRequest) => Ok(()),
        Ok(_) => Err(anyhow!("{context}: expected validation error, got success")),
        Err(error) => Err(anyhow!("{context}: expected validation error, got {error}")),
    }
}

fn expect_conflict<T>(result: Result<T, TalosError>, context: &str) -> Result<()> {
    match result {
        Err(error) if error.is_code(talos_sdk::ErrorCode::Conflict) => Ok(()),
        Ok(_) => Err(anyhow!("{context}: expected conflict, got success")),
        Err(error) => Err(anyhow!("{context}: expected conflict, got {error}")),
    }
}

fn expect_not_found<T>(result: Result<T, TalosError>, context: &str) -> Result<()> {
    match result {
        Err(error) if error.is_code(talos_sdk::ErrorCode::ResourceNotFound) => Ok(()),
        Ok(_) => Err(anyhow!("{context}: expected not found, got success")),
        Err(error) => Err(anyhow!("{context}: expected not found, got {error}")),
    }
}
