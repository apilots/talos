use std::process::Stdio;

use anyhow::{anyhow, Context, Result};
use serde_json::{json, Value};
use talos_client::{
    canonical_method, AdminSessionListParams, AppServerClient, AppServerClientConfig,
    ApplicationEventEnvelope, EventSubscribeParams, ExpectedFileVersion, ProjectPatch,
    ProtocolTalosContext, SessionCreateParams, TalosError, TalosEventPayload, TalosEventType,
    TalosInputItem, TalosMessageRequest, WorkspaceDocKind,
};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;

#[tokio::main]
async fn main() -> Result<()> {
    let scenario = Scenario::parse(std::env::args().nth(1).as_deref())?;
    let token = std::env::var("TALOS_LOCAL_LAUNCH_TOKEN")
        .context("TALOS_LOCAL_LAUNCH_TOKEN is required")?;
    let binary = std::env::var("TALOS_APP_SERVER_BIN").unwrap_or_else(|_| "talos".to_string());

    if scenario == Scenario::Malformed {
        return malformed(&binary, &token).await;
    }

    let mut client = AppServerClient::connect(AppServerClientConfig {
        app_server_bin: binary,
        launch_token: token,
        client_name: "talos-app-server-test-client".to_string(),
        client_version: env!("CARGO_PKG_VERSION").to_string(),
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
        Scenario::ApprovalResume => approval_resume(&mut client).await?,
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
    ApprovalResume,
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
            "approval-resume" => Ok(Self::ApprovalResume),
            other => Err(anyhow!("unsupported scenario: {other}")),
        }
    }
}

async fn smoke(client: &mut AppServerClient) -> Result<()> {
    eprintln!("model.list");
    client.list_models().await?;
    eprintln!("session.create");
    let session_id = create_session(client).await?;
    eprintln!("config.read");
    client.config_read().await?;
    eprintln!("memory.file.list");
    client.list_memory_files().await?;
    eprintln!("workspace.doc.list");
    client.list_workspace_docs().await?;
    eprintln!("event.subscribe");
    subscribe_events(client, &session_id).await?;
    Ok(())
}

async fn replay(client: &mut AppServerClient) -> Result<()> {
    eprintln!("replay session.create");
    let session_id = session_id_from_env().unwrap_or(create_session(client).await?);
    eprintln!("replay event.subscribe");
    let live = subscribe_events(client, &session_id).await?;
    eprintln!("replay turn.start");
    start_turn(client, &session_id).await?;
    eprintln!("replay collect live events");
    let live_events = collect_until_terminal(client, &live.subscription_id).await?;
    let first_event_id = live_events
        .first()
        .map(|event| event.event_id.clone())
        .ok_or_else(|| anyhow!("expected live events before replay"))?;
    eprintln!("replay event.subscribe replay");
    let replay = subscribe_events_with_limit(client, &session_id, live_events.len() + 8).await?;
    eprintln!("replay collect replay events");
    let replay_events = collect_events(client, &replay.subscription_id, live_events.len()).await?;
    if replay_events.len() != live_events.len() {
        return Err(anyhow!(
            "replay event count mismatch: live={} replay={}",
            live_events.len(),
            replay_events.len()
        ));
    }
    if replay_events.first().map(|event| event.event_id.as_str()) != Some(first_event_id.as_str()) {
        return Err(anyhow!("replay did not start from first persisted event"));
    }
    assert_matching_event_sequence(&live_events, &replay_events)
}

async fn multi_subscriber(client: &mut AppServerClient) -> Result<()> {
    eprintln!("multi-subscriber session.create");
    let session_id = session_id_from_env().unwrap_or(create_session(client).await?);
    eprintln!("multi-subscriber first subscribe");
    let first = subscribe_events(client, &session_id).await?;
    eprintln!("multi-subscriber second subscribe");
    let second = subscribe_events(client, &session_id).await?;
    eprintln!("multi-subscriber turn.start");
    start_turn(client, &session_id).await?;
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
    eprintln!("turn-smoke session.create");
    let session_id = create_session(client).await?;
    eprintln!("turn-smoke event.subscribe");
    let subscription = subscribe_events(client, &session_id).await?;
    eprintln!("turn-smoke turn.start");
    start_turn(client, &session_id).await?;
    eprintln!("turn-smoke collect events");
    let events = collect_until_terminal(client, &subscription.subscription_id).await?;
    ensure_turn_event_flow(&events)
}

async fn active_turn_conflict(client: &mut AppServerClient) -> Result<()> {
    eprintln!("active-turn-conflict session.create");
    let session_id = create_session(client).await?;
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
    if matches!(err, talos_client::TalosError::Conflict(_)) {
        Ok(())
    } else {
        Err(anyhow!("expected active turn conflict, got {err}"))
    }
}

async fn approval_resume(client: &mut AppServerClient) -> Result<()> {
    eprintln!("approval-resume session.create");
    let session_id = create_session(client).await?;
    eprintln!("approval-resume event.subscribe");
    let subscription = subscribe_events_with_limit(client, &session_id, 128).await?;
    eprintln!("approval-resume turn.start pending approval");
    client
        .start_turn(
            &session_id,
            message_request(
                "Call the Bash tool exactly once with command `echo APPROVAL_RESUME_OK`, \
                 timeout 10, and dangerously_disable_sandbox true. Do not answer directly \
                 before calling the tool.",
            ),
            Some(format!(
                "app-server-approval-pending-{}",
                uuid::Uuid::new_v4().simple()
            )),
        )
        .await?;
    let pending =
        collect_pending_approval_and_terminal(client, &subscription.subscription_id).await?;
    if pending.session_id != session_id {
        return Err(anyhow!("pending approval session_id mismatch"));
    }
    if pending.turn_id.is_none() {
        return Err(anyhow!("pending approval missing turn_id"));
    }

    eprintln!("approval-resume resubscribe");
    let live_resume_subscription = subscribe_events_with_limit(client, &session_id, 128).await?;

    eprintln!("approval-resume submit approval");
    client
        .submit_tool_approval(
            &session_id,
            &pending.call_id,
            &pending.tool_name,
            true,
            Some("approved by protocol regression".to_string()),
        )
        .await?;

    eprintln!("approval-resume replay subscription");
    let resumed_subscription =
        subscribe_events_since(client, &session_id, pending.event_id.clone(), 512).await?;

    eprintln!("approval-resume collect resumed events");
    let resumed_events = collect_approval_resume_events(
        client,
        &[
            live_resume_subscription.subscription_id.as_str(),
            resumed_subscription.subscription_id.as_str(),
        ],
        pending.sequence,
    )
    .await?;
    assert_approval_resume_completed(&pending, &resumed_events)
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
    let admin_projects = client.list_admin_projects().await?;
    if !admin_projects
        .iter()
        .any(|candidate| candidate.project_id == project.project_id)
    {
        return Err(anyhow!(
            "admin.project.list did not include created project"
        ));
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
    let session_id = create_session(client).await?;
    let admin_sessions = client
        .list_admin_sessions(AdminSessionListParams {
            user_id: None,
            channel_id: None,
            provider: None,
            project_id: None,
            limit: Some(20),
            offset: Some(0),
        })
        .await?;
    if !admin_sessions
        .data
        .iter()
        .any(|candidate| candidate.context.session_id == session_id)
    {
        return Err(anyhow!(
            "admin.session.list did not include created session"
        ));
    }
    if admin_sessions.pagination.limit != 20 {
        return Err(anyhow!("admin.session.list returned unexpected page limit"));
    }
    let _turns = client.get_session_turns(&session_id).await?;
    expect_not_found(
        client.get_session("missing-session").await,
        "missing session should map to not found",
    )?;
    expect_not_found(
        client.get_session_turns("missing-session").await,
        "missing session turns should map to not found",
    )?;
    expect_not_found(
        client
            .submit_tool_approval("missing-session", "call-1", "Bash", false, None)
            .await,
        "missing tool approval session should map to not found",
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

async fn create_session(client: &mut AppServerClient) -> Result<String> {
    let session = client
        .create_session(SessionCreateParams {
            context: test_context(),
            model: std::env::var("TALOS_APP_SERVER_TEST_MODEL")
                .ok()
                .filter(|value| !value.trim().is_empty()),
            project_id: None,
            current_directory: std::env::current_dir().ok(),
        })
        .await?;
    Ok(session.context.session_id)
}

async fn subscribe_events(
    client: &mut AppServerClient,
    session_id: &str,
) -> Result<talos_client::EventSubscribeResponse> {
    subscribe_events_with_limit(client, session_id, 32).await
}

async fn subscribe_events_with_limit(
    client: &mut AppServerClient,
    session_id: &str,
    replay_limit: usize,
) -> Result<talos_client::EventSubscribeResponse> {
    subscribe_events_with_cursor(client, session_id, None, replay_limit).await
}

async fn subscribe_events_since(
    client: &mut AppServerClient,
    session_id: &str,
    since_event_id: String,
    replay_limit: usize,
) -> Result<talos_client::EventSubscribeResponse> {
    subscribe_events_with_cursor(client, session_id, Some(since_event_id), replay_limit).await
}

async fn subscribe_events_with_cursor(
    client: &mut AppServerClient,
    session_id: &str,
    since_event_id: Option<String>,
    replay_limit: usize,
) -> Result<talos_client::EventSubscribeResponse> {
    Ok(client
        .subscribe_events(EventSubscribeParams {
            session_id: session_id.to_string(),
            since_event_id,
            replay_limit: Some(replay_limit),
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
    }
}

async fn collect_until_terminal(
    client: &mut AppServerClient,
    subscription_id: &str,
) -> Result<Vec<ApplicationEventEnvelope>> {
    let mut events = Vec::new();
    for _ in 0..128 {
        let event = next_event_with_timeout(client).await?;
        if event.subscription_id == subscription_id {
            let terminal = is_terminal_event(&event);
            events.push(event);
            if terminal {
                return Ok(events);
            }
        }
    }
    Err(anyhow!("timed out waiting for terminal event"))
}

async fn collect_events(
    client: &mut AppServerClient,
    subscription_id: &str,
    expected_count: usize,
) -> Result<Vec<ApplicationEventEnvelope>> {
    let mut events = Vec::new();
    for _ in 0..64 {
        let event = next_event_with_timeout(client).await?;
        if event.subscription_id == subscription_id {
            events.push(event);
            if events.len() >= expected_count {
                return Ok(events);
            }
        }
    }
    Err(anyhow!(
        "timed out waiting for {expected_count} events on {subscription_id}; got {}",
        events.len()
    ))
}

struct PendingApprovalRecord {
    session_id: String,
    turn_id: Option<String>,
    event_id: String,
    call_id: String,
    tool_name: String,
    sequence: u64,
}

async fn collect_pending_approval_and_terminal(
    client: &mut AppServerClient,
    subscription_id: &str,
) -> Result<PendingApprovalRecord> {
    let mut pending = None;
    for _ in 0..1024 {
        let event = next_event_with_timeout(client).await?;
        if event.subscription_id != subscription_id {
            continue;
        }
        if let TalosEventPayload::MessageToolCallPendingApproval(payload) = &event.event.payload {
            pending = Some(PendingApprovalRecord {
                session_id: event.event.session_id.clone(),
                turn_id: event.turn_id.clone(),
                event_id: event.event_id.clone(),
                call_id: payload.call_id.clone(),
                tool_name: payload.tool_name.clone(),
                sequence: parse_event_sequence(&event.event_id)?,
            });
            continue;
        }
        if pending.is_some() && event.event.event_type == TalosEventType::MessageCompleted {
            return Ok(pending.expect("pending checked"));
        }
        if event.event.event_type == TalosEventType::MessageError {
            return Err(anyhow!("turn failed before pending approval event"));
        }
    }
    Err(anyhow!(
        "timed out waiting for pending approval terminal event"
    ))
}

async fn collect_approval_resume_events(
    client: &mut AppServerClient,
    subscription_ids: &[&str],
    after_sequence: u64,
) -> Result<Vec<ApplicationEventEnvelope>> {
    let mut events = Vec::new();
    let mut saw_tool_completed = false;
    for _ in 0..1024 {
        let event = next_event_with_timeout(client).await?;
        if !subscription_ids.contains(&event.subscription_id.as_str())
            || parse_event_sequence(&event.event_id)? <= after_sequence
        {
            continue;
        }
        match event.event.event_type {
            TalosEventType::MessageToolCallCompleted => saw_tool_completed = true,
            TalosEventType::MessageCompleted if saw_tool_completed => {
                events.push(event);
                return Ok(events);
            }
            TalosEventType::MessageError => {
                events.push(event);
                return Ok(events);
            }
            _ => {}
        }
        events.push(event);
    }
    Err(anyhow!(
        "timed out waiting for approval resume completion; events={:?}",
        events.iter().map(event_projection).collect::<Vec<_>>()
    ))
}

async fn next_event_with_timeout(client: &mut AppServerClient) -> Result<ApplicationEventEnvelope> {
    tokio::time::timeout(std::time::Duration::from_secs(30), client.next_event())
        .await
        .map_err(|_| anyhow!("timed out waiting for app-server event"))?
        .map_err(Into::into)
}

fn assert_approval_resume_completed(
    pending: &PendingApprovalRecord,
    events: &[ApplicationEventEnvelope],
) -> Result<()> {
    let mut saw_tool_completed = false;
    let mut saw_completed = false;
    let mut completed_call_ids = Vec::new();
    for event in events {
        if event.event.session_id != pending.session_id {
            return Err(anyhow!("resumed event session_id mismatch"));
        }
        if parse_event_sequence(&event.event_id)? <= pending.sequence {
            return Err(anyhow!("resumed event sequence did not advance"));
        }
        match &event.event.payload {
            TalosEventPayload::MessageToolCallCompleted(payload) => {
                completed_call_ids.push(payload.call_id.clone());
                if payload.call_id == pending.call_id {
                    saw_tool_completed = true;
                }
            }
            TalosEventPayload::MessageCompleted(_) => {
                saw_completed = true;
            }
            TalosEventPayload::MessageError(payload) => {
                return Err(anyhow!("resumed turn failed: {}", payload.error));
            }
            _ => {}
        }
    }
    if !saw_tool_completed {
        return Err(anyhow!(
            "resumed turn missing approved tool completion; pending_call_id={} completed_call_ids={:?}",
            pending.call_id,
            completed_call_ids
        ));
    }
    if !saw_completed {
        return Err(anyhow!("resumed turn missing message.completed"));
    }
    Ok(())
}

fn parse_event_sequence(event_id: &str) -> Result<u64> {
    event_id
        .strip_prefix("ev_")
        .ok_or_else(|| anyhow!("invalid event id prefix: {event_id}"))?
        .parse::<u64>()
        .with_context(|| format!("invalid event id sequence: {event_id}"))
}

async fn collect_two_subscriptions_until_terminal(
    client: &mut AppServerClient,
    first_subscription_id: &str,
    second_subscription_id: &str,
) -> Result<(Vec<ApplicationEventEnvelope>, Vec<ApplicationEventEnvelope>)> {
    let mut first_events = Vec::new();
    let mut second_events = Vec::new();
    let mut first_terminal = false;
    let mut second_terminal = false;
    for _ in 0..256 {
        let event = client.next_event().await?;
        if event.subscription_id == first_subscription_id {
            first_terminal |= is_terminal_event(&event);
            first_events.push(event);
        } else if event.subscription_id == second_subscription_id {
            second_terminal |= is_terminal_event(&event);
            second_events.push(event);
        }
        if first_terminal && second_terminal {
            return Ok((first_events, second_events));
        }
    }
    Err(anyhow!(
        "timed out waiting for two subscription terminal events: first={} second={}",
        first_terminal,
        second_terminal
    ))
}

async fn malformed(binary: &str, token: &str) -> Result<()> {
    let mut child = Command::new(binary)
        .arg("app-server")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .with_context(|| format!("spawn {binary} app-server"))?;
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
            },
            "actor_credentials": {
                "credential": {
                    "type": "local_launch_token",
                    "token": token
                }
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

fn test_context() -> ProtocolTalosContext {
    ProtocolTalosContext {
        source_type: "cli".to_string(),
        provider: "cli".to_string(),
        channel_id: "app-server-test-client".to_string(),
        thread_id: None,
        message_id: format!("msg-{}", uuid::Uuid::new_v4().simple()),
        trace_id: format!("trace-{}", uuid::Uuid::new_v4().simple()),
    }
}

fn is_terminal_event(event: &ApplicationEventEnvelope) -> bool {
    matches!(
        event.event.event_type,
        TalosEventType::MessageCompleted | TalosEventType::MessageError
    )
}

fn ensure_turn_event_flow(events: &[ApplicationEventEnvelope]) -> Result<()> {
    if events
        .iter()
        .any(|event| event.event.event_type == TalosEventType::MessageError)
    {
        return Err(anyhow!("turn failed during app-server e2e"));
    }
    for required in [
        "message.started",
        "message.output.delta",
        "message.completed",
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

fn assert_matching_event_sequence(
    left: &[ApplicationEventEnvelope],
    right: &[ApplicationEventEnvelope],
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

fn event_projection(event: &ApplicationEventEnvelope) -> (&str, &str, Option<&str>, &'static str) {
    (
        event.event_id.as_str(),
        event.event.session_id.as_str(),
        event.turn_id.as_deref(),
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
