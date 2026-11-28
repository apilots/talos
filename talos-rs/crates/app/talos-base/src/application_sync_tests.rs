//! Real management runtimes, G talos-server, WSS/Loro H and one physical Worker.
//! The model HTTP fixture controls timing; paid-model deployment E2E is stage 5.
use super::*;
use axum::{extract::State, response::IntoResponse, routing::post, Json, Router};
use serde_json::{json, Value};
use std::{path::Path, time::Duration};
use talos_auth::RequestIdentity;
use talos_config::{AgentNodeRunnerConfig, AgentNodeTransport, Config, TalosPaths};
use talos_store::session::SessionQuery;
use talos_sync::SyncConnectionState;
use talos_types::{AgentUserContent, AgentUserInput, StartTurnRequest};

#[path = "application_sync_access_tests.rs"]
mod access;
#[path = "application_sync_enrollment_tests.rs"]
mod enrollment;
use enrollment::TopologyScenario;
#[path = "application_sync_login_tests.rs"]
mod login;
#[path = "application_sync_metrics_tests.rs"]
mod metrics;
#[path = "application_sync_recovery_tests.rs"]
mod recovery;
#[path = "application_sync_status_flow_tests.rs"]
mod status_flow;

struct HubProcess(std::process::Child);
impl HubProcess {
    fn stop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
impl Drop for HubProcess {
    fn drop(&mut self) {
        self.stop();
    }
}

const ACCOUNT: &str = "same-account";
const TOKEN: &str = "shared-session-fixture-token";
const OTHER_TOKEN: &str = "other-account-fixture-token";
const REFRESHED_TOKEN: &str = "refreshed-shared-session-fixture-token";
#[derive(Clone, Default)]
struct Model {
    blocked: Arc<std::sync::atomic::AtomicBool>,
    resume: Arc<tokio::sync::Notify>,
    prompts: Arc<std::sync::Mutex<Vec<Value>>>,
}
async fn introspect(Json(request): Json<Value>) -> Json<Value> {
    Json(
        json!({"active":request["token"] == TOKEN || request["token"] == OTHER_TOKEN || request["token"] == REFRESHED_TOKEN,
        "user":{"user_id":if request["token"] == OTHER_TOKEN { "other-account" } else { ACCOUNT },"email":"test@example.invalid","username":"test","role":"user","type":"free","status":"active"},
        "capabilities":{"is_admin":false,"role":"user","scopes":["sessions:read","sessions:write","projects:read","projects:write"]},
        "entitlement":{"can_use":true,"plan":"free","reason":null},"expires_at_ms":null}),
    )
}
async fn completion(
    State(model): State<Model>,
    Json(request): Json<Value>,
) -> axum::response::Response {
    model.prompts.lock().unwrap().push(request.clone());
    if request["stream"] == true {
        if model.blocked.load(std::sync::atomic::Ordering::SeqCst) {
            model.resume.notified().await;
        }
        let chunk = json!({"id":"fixture","object":"chat.completion.chunk","created":1,"model":"deepseek-chat","choices":[{"index":0,"delta":{"role":"assistant","content":"worker response"},"finish_reason":null}]});
        let end = json!({"id":"fixture","object":"chat.completion.chunk","created":1,"model":"deepseek-chat","choices":[{"index":0,"delta":{},"finish_reason":"stop"}],"usage":{"prompt_tokens":5,"completion_tokens":2,"total_tokens":7}});
        (
            [("content-type", "text/event-stream")],
            format!("data: {chunk}\n\ndata: {end}\n\ndata: [DONE]\n\n"),
        )
            .into_response()
    } else {
        Json(json!({"id":"fixture","object":"chat.completion","created":1,"model":"deepseek-chat","choices":[{"index":0,"message":{"role":"assistant","content":"worker response"},"finish_reason":"stop"}],"usage":{"prompt_tokens":5,"completion_tokens":2,"total_tokens":7}})).into_response()
    }
}
fn port() -> std::net::SocketAddr {
    let socket = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    socket.local_addr().unwrap()
}
fn persisted_config(config: &Config) -> Vec<u8> {
    let mut value = serde_json::to_value(config).unwrap();
    value["logging"].as_object_mut().unwrap().remove("file");
    value["storage"]
        .as_object_mut()
        .unwrap()
        .remove("connection_string");
    serde_json::to_vec_pretty(&value).unwrap()
}
fn config(home: &Path, api: &str) -> Config {
    let paths = TalosPaths::from_home(home.into());
    paths.ensure_base_dirs().unwrap();
    let mut config = Config::default().with_runtime_paths(paths);
    config.apply_derived_paths();
    // This topology uses controlled local Sync/Auth/model services, not external MCP servers.
    config.agent.kernel.tools.enable_mcp_tools = false;
    config.cloud.api_base_url = api.into();
    config.gateway.host = "127.0.0.1".into();
    config.gateway.port = port().port();
    config.gateway.auth = true;
    config.llm.provider = "deepseek".into();
    config.llm.model = "deepseek-chat".into();
    config.llm.deepseek.base_url = Some(api.into());
    config.llm.deepseek.api_key = Some("controlled-fixture-key".into());
    config.llm.deepseek.models = vec![talos_config::ProviderModelConfig {
        reasoning_replay: Default::default(),
        reasoning_efforts: None,
        request_parameters: Default::default(),
        id: "deepseek-chat".into(),
        name: "Controlled timing".into(),
        context_window: 65536,
        reasoning: false,
        reasoning_effort: Default::default(),
        speed: Default::default(),
        input: vec![],
        cost: Default::default(),
    }];
    config.sync.enabled = Some(true);
    // Runtime upgrades still read the original host config for resource policies.
    std::fs::write(config.source_path(), persisted_config(&config)).unwrap();
    config
}
fn start_request(session: &str, text: &str) -> StartTurnRequest {
    StartTurnRequest {
        node_id: None,
        session_id: session.into(),
        input: AgentUserInput {
            content: vec![AgentUserContent::Text { text: text.into() }],
        },
        model_selection: None,
        user_permission_mode: None,
        project_id: None,
        current_directory: None,
        client_request_id: None,
    }
}
async fn wait_for(label: &str, mut condition: impl AsyncFnMut() -> bool) {
    tokio::time::timeout(Duration::from_secs(30), async {
        while !condition().await {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap_or_else(|_| panic!("Product flow did not converge: {label}"));
}
async fn worker_state(
    runtime: &TalosApplicationRuntime,
    session: &str,
) -> agent_protocol::SessionExecutionState {
    let record = runtime
        .session_store
        .get_session_record(session)
        .await
        .unwrap()
        .unwrap();
    let context = agent_protocol::AgentSessionContext {
        agent_id: record.agent_id,
        session_id: record.id,
        workspace: runtime
            .config
            .resolve_workspace(&record.workspace_id, ACCOUNT)
            .unwrap(),
    };
    talos_coordinator::AgentCoordinator::read_session_execution_state(
        &*runtime.coordinator,
        context,
    )
    .await
    .unwrap()
    .unwrap()
}
async fn close(runtime: TalosApplicationRuntime) {
    runtime.shutdown_host().await.unwrap();
    if let Some(worker) = runtime.worker_registration.lock().await.take() {
        worker.shutdown().await.unwrap();
    }
    runtime
        .worker_rpc
        .as_ref()
        .unwrap()
        .shutdown()
        .await
        .unwrap();
    runtime.resource_stop.cancel();
    runtime.automation.shutdown().await.unwrap();
    runtime.worker.as_ref().unwrap().shutdown().await.unwrap();
}

#[path = "application_sync_scenarios_tests.rs"]
mod scenarios;
use scenarios::run_topology;
