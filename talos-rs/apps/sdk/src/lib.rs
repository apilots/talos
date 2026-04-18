//! Talos Client Library
//!
//! Strongly typed SDK for Talos Session-first HTTP APIs.

use futures_util::StreamExt;
use reqwest::{Client, RequestBuilder, Response};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::path::PathBuf;
use tokio::sync::mpsc;

use talos_error::{TalosError, TalosResult};
pub use talos_types::talos::{
    StopReason, TalosContentItem, TalosContext, TalosEvent, TalosEventPayload, TalosEventType,
    TalosInputItem, TalosMessageCompletedEvent, TalosMessageErrorEvent,
    TalosMessageExecutionSummaryEvent, TalosMessageOutputDeltaEvent,
    TalosMessageReasoningDeltaEvent, TalosMessageRequest, TalosMessageResponse,
    TalosMessageStartedEvent, TalosMessageToolCallCompletedEvent,
    TalosMessageToolCallPendingApprovalEvent, TalosMessageToolCallStartedEvent, TalosOutputItem,
    TalosProvider, TalosSourceType, TalosThinkingSource, TalosThinkingStage,
};
pub use talos_types::{
    DocUpdateRequest, ExpectedFileVersion, MemoryFileDetail, MemoryFileSummary, ModelInfo,
    OptionalCurrentDirectoryPatch, OptionalProjectIdPatch, ProjectPatch, ProjectRecord,
    SessionAgentConf, SessionConf, SessionQuery, TalosSession, WorkspaceDocDetail,
    WorkspaceDocKind, WorkspaceDocSummary,
};

#[derive(Clone, Debug)]
pub struct ClientConfig {
    pub gateway_url: String,
    pub auth_token: Option<String>,
    pub timeout: u64,
}

impl Default for ClientConfig {
    fn default() -> Self {
        Self {
            gateway_url: "http://localhost:8080".to_string(),
            auth_token: None,
            timeout: 30,
        }
    }
}

#[derive(Debug, Clone)]
pub struct SessionCreateParams {
    pub context: Option<TalosContext>,
    pub model: Option<String>,
    pub conf: Option<SessionConf>,
    pub project_id: Option<String>,
    pub current_directory: Option<PathBuf>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionUpdate {
    pub conf: Option<SessionConf>,
    pub project_id: Option<OptionalProjectIdPatch>,
    pub current_directory: Option<OptionalCurrentDirectoryPatch>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CreateSessionRequest {
    context: TalosContext,
    model: Option<String>,
    conf: Option<SessionConf>,
    project_id: Option<String>,
    current_directory: Option<PathBuf>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ListModelsResponse {
    data: Vec<ModelInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ListSessionsResponse {
    data: Vec<TalosSession>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ListProjectsResponse {
    data: Vec<ProjectRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ListMemoryFilesResponse {
    data: Vec<MemoryFileSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ListWorkspaceDocsResponse {
    data: Vec<WorkspaceDocSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SearchMemoryFilesResponse {
    data: Vec<MemorySearchResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ListSessionTurnsResponse {
    data: Vec<TalosMessageResponse>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SessionPatchResponse {
    success: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MemoryFileListParams {
    pub limit: Option<usize>,
    pub before_date: Option<String>,
    pub date_from: Option<String>,
    pub date_to: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemorySearchParams {
    pub q: String,
    pub limit: Option<usize>,
    pub date_from: Option<String>,
    pub date_to: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MemorySearchResult {
    pub path: String,
    pub snippet: String,
    pub start_line: usize,
    pub end_line: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CreateProjectRequest {
    name: String,
    directory: PathBuf,
}

pub struct TalosClient {
    http_client: Client,
    config: ClientConfig,
}

impl TalosClient {
    pub fn new(config: ClientConfig) -> TalosResult<Self> {
        let mut builder =
            Client::builder().connect_timeout(std::time::Duration::from_secs(config.timeout));

        // For local gateway endpoints, bypass system proxy settings to avoid
        // loopback requests being intercepted by desktop proxy tools.
        if is_local_gateway_url(&config.gateway_url) {
            builder = builder.no_proxy();
        }

        let http_client = builder
            .build()
            .map_err(|e| TalosError::ClientError(e.to_string()))?;

        Ok(Self {
            http_client,
            config,
        })
    }

    pub fn new_default() -> TalosResult<Self> {
        Self::new(ClientConfig::default())
    }

    pub async fn list_models(&self) -> TalosResult<Vec<ModelInfo>> {
        let url = format!("{}/v1/models", self.config.gateway_url);
        let response = self
            .authorized(self.http_client.get(&url))
            .send()
            .await
            .map_err(|e| Self::request_error("GET", &url, e))?;
        let payload: ListModelsResponse = Self::decode_json(response, "GET", &url).await?;
        Ok(payload.data)
    }

    /// Create a session and return the full `TalosSession` snapshot from the gateway.
    pub async fn create_session(&self, params: &SessionCreateParams) -> TalosResult<TalosSession> {
        let request = CreateSessionRequest {
            context: params
                .context
                .clone()
                .unwrap_or_else(|| default_context("anonymous")),
            model: params.model.clone(),
            conf: params.conf.clone(),
            project_id: params.project_id.clone(),
            current_directory: params.current_directory.clone(),
        };

        let url = format!("{}/v1/sessions", self.config.gateway_url);
        let response = self
            .authorized(self.http_client.post(&url).json(&request))
            .send()
            .await
            .map_err(|e| Self::request_error("POST", &url, e))?;
        Self::decode_json(response, "POST", &url).await
    }

    pub async fn get_session(&self, session_id: &str) -> TalosResult<TalosSession> {
        let url = format!("{}/v1/sessions/{}", self.config.gateway_url, session_id);
        let response = self
            .authorized(self.http_client.get(&url))
            .send()
            .await
            .map_err(|e| Self::request_error("GET", &url, e))?;
        Self::decode_json(response, "GET", &url).await
    }

    pub async fn list_sessions(&self) -> TalosResult<Vec<TalosSession>> {
        self.list_sessions_with_query(&SessionQuery::default())
            .await
    }

    pub async fn list_sessions_by_project(
        &self,
        project_id: &str,
    ) -> TalosResult<Vec<TalosSession>> {
        self.list_sessions_with_query(&SessionQuery {
            user_id: None,
            channel_id: None,
            provider: None,
            project_id: Some(project_id.to_string()),
        })
        .await
    }

    pub async fn list_sessions_with_query(
        &self,
        query: &SessionQuery,
    ) -> TalosResult<Vec<TalosSession>> {
        let url = format!("{}/v1/sessions", self.config.gateway_url);
        let response = self
            .authorized(self.http_client.get(&url).query(query))
            .send()
            .await
            .map_err(|e| Self::request_error("GET", &url, e))?;
        let payload: ListSessionsResponse = Self::decode_json(response, "GET", &url).await?;
        Ok(payload.data)
    }

    pub async fn list_projects(&self) -> TalosResult<Vec<ProjectRecord>> {
        let url = format!("{}/v1/projects", self.config.gateway_url);
        let response = self
            .authorized(self.http_client.get(&url))
            .send()
            .await
            .map_err(|e| Self::request_error("GET", &url, e))?;
        let payload: ListProjectsResponse = Self::decode_json(response, "GET", &url).await?;
        Ok(payload.data)
    }

    pub async fn get_project(&self, project_id: &str) -> TalosResult<ProjectRecord> {
        let url = format!("{}/v1/projects/{}", self.config.gateway_url, project_id);
        let response = self
            .authorized(self.http_client.get(&url))
            .send()
            .await
            .map_err(|e| Self::request_error("GET", &url, e))?;
        Self::decode_json(response, "GET", &url).await
    }

    pub async fn create_project(&self, name: &str, directory: &Path) -> TalosResult<ProjectRecord> {
        let url = format!("{}/v1/projects", self.config.gateway_url);
        let request = CreateProjectRequest {
            name: name.to_string(),
            directory: directory.to_path_buf(),
        };
        let response = self
            .authorized(self.http_client.post(&url).json(&request))
            .send()
            .await
            .map_err(|e| Self::request_error("POST", &url, e))?;
        Self::decode_json(response, "POST", &url).await
    }

    pub async fn patch_project(
        &self,
        project_id: &str,
        patch: &ProjectPatch,
    ) -> TalosResult<ProjectRecord> {
        let url = format!("{}/v1/projects/{}", self.config.gateway_url, project_id);
        let response = self
            .authorized(self.http_client.patch(&url).json(patch))
            .send()
            .await
            .map_err(|e| Self::request_error("PATCH", &url, e))?;
        Self::decode_json(response, "PATCH", &url).await
    }

    pub async fn update_project_name(
        &self,
        project_id: &str,
        name: &str,
    ) -> TalosResult<ProjectRecord> {
        self.patch_project(
            project_id,
            &ProjectPatch {
                name: Some(name.to_string()),
            },
        )
        .await
    }

    pub async fn delete_project(&self, project_id: &str) -> TalosResult<()> {
        let url = format!("{}/v1/projects/{}", self.config.gateway_url, project_id);
        let response = self
            .authorized(self.http_client.delete(&url))
            .send()
            .await
            .map_err(|e| Self::request_error("DELETE", &url, e))?;
        if response.status().is_success() {
            return Ok(());
        }
        Err(Self::http_error(response, "DELETE", &url).await)
    }

    pub async fn update_session(
        &self,
        session_id: &str,
        update: &SessionUpdate,
    ) -> TalosResult<()> {
        let url = format!("{}/v1/sessions/{}", self.config.gateway_url, session_id);
        let response = self
            .authorized(self.http_client.patch(&url).json(update))
            .send()
            .await
            .map_err(|e| Self::request_error("PATCH", &url, e))?;
        let payload: SessionPatchResponse = Self::decode_json(response, "PATCH", &url).await?;
        if payload.success {
            return Ok(());
        }
        Err(TalosError::ClientError(
            "session patch did not succeed".to_string(),
        ))
    }

    pub async fn update_session_model(&self, session_id: &str, model: &str) -> TalosResult<()> {
        let update = SessionUpdate {
            conf: Some(SessionConf {
                agent: Some(SessionAgentConf {
                    model: Some(model.to_string()),
                }),
                tools: None,
            }),
            project_id: None,
            current_directory: None,
        };
        self.update_session(session_id, &update).await
    }

    pub async fn update_session_project(
        &self,
        session_id: &str,
        project_id: &str,
    ) -> TalosResult<()> {
        let update = SessionUpdate {
            conf: None,
            project_id: Some(OptionalProjectIdPatch::Set {
                project_id: project_id.to_string(),
            }),
            current_directory: None,
        };
        self.update_session(session_id, &update).await
    }

    pub async fn clear_session_project(&self, session_id: &str) -> TalosResult<()> {
        let update = SessionUpdate {
            conf: None,
            project_id: Some(OptionalProjectIdPatch::Clear),
            current_directory: None,
        };
        self.update_session(session_id, &update).await
    }

    pub async fn update_session_current_directory(
        &self,
        session_id: &str,
        path: &Path,
    ) -> TalosResult<()> {
        let update = SessionUpdate {
            conf: None,
            project_id: None,
            current_directory: Some(OptionalCurrentDirectoryPatch::Set {
                path: path.to_path_buf(),
            }),
        };
        self.update_session(session_id, &update).await
    }

    pub async fn clear_session_current_directory(&self, session_id: &str) -> TalosResult<()> {
        let update = SessionUpdate {
            conf: None,
            project_id: None,
            current_directory: Some(OptionalCurrentDirectoryPatch::Clear),
        };
        self.update_session(session_id, &update).await
    }

    pub async fn close_session(&self, session_id: &str) -> TalosResult<()> {
        let url = format!(
            "{}/v1/sessions/{}/close",
            self.config.gateway_url, session_id
        );
        let response = self
            .authorized(self.http_client.post(&url))
            .send()
            .await
            .map_err(|e| Self::request_error("POST", &url, e))?;
        let payload: SessionPatchResponse = Self::decode_json(response, "POST", &url).await?;
        if payload.success {
            return Ok(());
        }
        Err(TalosError::ClientError(
            "session close did not succeed".to_string(),
        ))
    }

    pub async fn delete_session(&self, session_id: &str) -> TalosResult<()> {
        let url = format!("{}/v1/sessions/{}", self.config.gateway_url, session_id);
        let response = self
            .authorized(self.http_client.delete(&url))
            .send()
            .await
            .map_err(|e| Self::request_error("DELETE", &url, e))?;
        let payload: SessionPatchResponse = Self::decode_json(response, "DELETE", &url).await?;
        if payload.success {
            return Ok(());
        }
        Err(TalosError::ClientError(
            "session delete did not succeed".to_string(),
        ))
    }

    pub async fn compact_session(&self, session_id: &str) -> TalosResult<TalosSession> {
        let url = format!(
            "{}/v1/sessions/{}/compact",
            self.config.gateway_url, session_id
        );
        let response = self
            .authorized(self.http_client.post(&url))
            .send()
            .await
            .map_err(|e| Self::request_error("POST", &url, e))?;
        Self::decode_json(response, "POST", &url).await
    }

    pub async fn clear_session(&self, session_id: &str) -> TalosResult<TalosSession> {
        let url = format!(
            "{}/v1/sessions/{}/clear",
            self.config.gateway_url, session_id
        );
        let response = self
            .authorized(self.http_client.post(&url))
            .send()
            .await
            .map_err(|e| Self::request_error("POST", &url, e))?;
        Self::decode_json(response, "POST", &url).await
    }

    pub async fn get_session_turns(
        &self,
        session_id: &str,
    ) -> TalosResult<Vec<TalosMessageResponse>> {
        let url = format!(
            "{}/v1/sessions/{}/turns",
            self.config.gateway_url, session_id
        );
        let response = self
            .authorized(self.http_client.get(&url))
            .send()
            .await
            .map_err(|e| Self::request_error("GET", &url, e))?;
        let payload: ListSessionTurnsResponse = Self::decode_json(response, "GET", &url).await?;
        Ok(payload.data)
    }

    pub async fn list_memory_files(
        &self,
        params: &MemoryFileListParams,
    ) -> TalosResult<Vec<MemoryFileSummary>> {
        let url = format!("{}/v1/memory/files", self.config.gateway_url);
        let response = self
            .authorized(self.http_client.get(&url).query(params))
            .send()
            .await
            .map_err(|e| Self::request_error("GET", &url, e))?;
        let payload: ListMemoryFilesResponse = Self::decode_json(response, "GET", &url).await?;
        Ok(payload.data)
    }

    pub async fn get_memory_file(&self, date: &str) -> TalosResult<MemoryFileDetail> {
        let url = format!("{}/v1/memory/files/{}", self.config.gateway_url, date);
        let response = self
            .authorized(self.http_client.get(&url))
            .send()
            .await
            .map_err(|e| Self::request_error("GET", &url, e))?;
        Self::decode_json(response, "GET", &url).await
    }

    pub async fn put_memory_file(
        &self,
        date: &str,
        request: &DocUpdateRequest,
    ) -> TalosResult<MemoryFileDetail> {
        let url = format!("{}/v1/memory/files/{}", self.config.gateway_url, date);
        let response = self
            .authorized(self.http_client.put(&url).json(request))
            .send()
            .await
            .map_err(|e| Self::request_error("PUT", &url, e))?;
        Self::decode_json(response, "PUT", &url).await
    }

    pub async fn search_memory(
        &self,
        params: &MemorySearchParams,
    ) -> TalosResult<Vec<MemorySearchResult>> {
        let url = format!("{}/v1/memory/search", self.config.gateway_url);
        let response = self
            .authorized(self.http_client.get(&url).query(params))
            .send()
            .await
            .map_err(|e| Self::request_error("GET", &url, e))?;
        let payload: SearchMemoryFilesResponse = Self::decode_json(response, "GET", &url).await?;
        Ok(payload.data)
    }

    pub async fn list_workspace_docs(&self) -> TalosResult<Vec<WorkspaceDocSummary>> {
        let url = format!("{}/v1/workspace/docs", self.config.gateway_url);
        let response = self
            .authorized(self.http_client.get(&url))
            .send()
            .await
            .map_err(|e| Self::request_error("GET", &url, e))?;
        let payload: ListWorkspaceDocsResponse = Self::decode_json(response, "GET", &url).await?;
        Ok(payload.data)
    }

    pub async fn get_workspace_doc(
        &self,
        kind: WorkspaceDocKind,
    ) -> TalosResult<WorkspaceDocDetail> {
        let url = format!(
            "{}/v1/workspace/docs/{}",
            self.config.gateway_url,
            workspace_doc_kind_path(kind)
        );
        let response = self
            .authorized(self.http_client.get(&url))
            .send()
            .await
            .map_err(|e| Self::request_error("GET", &url, e))?;
        Self::decode_json(response, "GET", &url).await
    }

    pub async fn put_workspace_doc(
        &self,
        kind: WorkspaceDocKind,
        request: &DocUpdateRequest,
    ) -> TalosResult<WorkspaceDocDetail> {
        let url = format!(
            "{}/v1/workspace/docs/{}",
            self.config.gateway_url,
            workspace_doc_kind_path(kind)
        );
        let response = self
            .authorized(self.http_client.put(&url).json(request))
            .send()
            .await
            .map_err(|e| Self::request_error("PUT", &url, e))?;
        Self::decode_json(response, "PUT", &url).await
    }

    pub async fn cancel_active_turn(&self, session_id: &str) -> TalosResult<()> {
        let url = format!(
            "{}/v1/sessions/{}/cancel",
            self.config.gateway_url, session_id
        );
        let response = self
            .authorized(self.http_client.post(&url))
            .send()
            .await
            .map_err(|e| Self::request_error("POST", &url, e))?;
        let payload: SessionPatchResponse = Self::decode_json(response, "POST", &url).await?;
        if payload.success {
            return Ok(());
        }
        Err(TalosError::ClientError(
            "session turn cancel did not succeed".to_string(),
        ))
    }

    pub async fn submit_tool_approval(
        &self,
        session_id: &str,
        call_id: &str,
        tool_name: &str,
        approved: bool,
        reason: Option<String>,
    ) -> TalosResult<()> {
        let url = format!(
            "{}/v1/sessions/{}/tool-approvals",
            self.config.gateway_url, session_id
        );
        #[derive(Serialize)]
        struct ToolApprovalBody<'a> {
            call_id: &'a str,
            tool_name: &'a str,
            approved: bool,
            reason: Option<String>,
        }
        let body = ToolApprovalBody {
            call_id,
            tool_name,
            approved,
            reason,
        };
        let response = self
            .authorized(self.http_client.post(&url).json(&body))
            .send()
            .await
            .map_err(|e| Self::request_error("POST", &url, e))?;
        if response.status().is_success() {
            return Ok(());
        }
        Err(Self::http_error(response, "POST", &url).await)
    }

    pub async fn send_message(
        &self,
        session_id: &str,
        request: &TalosMessageRequest,
    ) -> TalosResult<TalosMessageResponse> {
        let url = format!(
            "{}/v1/sessions/{}/messages",
            self.config.gateway_url, session_id
        );
        let response = self
            .authorized(self.http_client.post(&url).json(request))
            .send()
            .await
            .map_err(|e| Self::request_error("POST", &url, e))?;
        Self::decode_json(response, "POST", &url).await
    }

    pub async fn send_message_stream(
        &self,
        session_id: &str,
        request: &TalosMessageRequest,
    ) -> TalosResult<mpsc::Receiver<TalosEvent>> {
        let mut stream_request = request.clone();
        stream_request.stream = Some(true);

        let url = format!(
            "{}/v1/sessions/{}/messages",
            self.config.gateway_url, session_id
        );
        let response = self
            .authorized(self.http_client.post(&url).json(&stream_request))
            .send()
            .await
            .map_err(|e| Self::request_error("POST", &url, e))?;

        if !response.status().is_success() {
            return Err(Self::http_error(response, "POST", &url).await);
        }

        let (tx, rx) = mpsc::channel(256);
        let mut bytes_stream = response.bytes_stream();

        tokio::spawn(async move {
            let mut buffer = String::new();
            while let Some(chunk) = bytes_stream.next().await {
                let Ok(bytes) = chunk else { break };
                buffer.push_str(&String::from_utf8_lossy(&bytes));

                while let Some(newline_pos) = buffer.find('\n') {
                    let line = buffer[..newline_pos].trim_end_matches('\r').to_string();
                    buffer = buffer[newline_pos + 1..].to_string();

                    if let Some(data) = line.strip_prefix("data:") {
                        let payload = data.trim();
                        if payload.is_empty() || payload == "[DONE]" {
                            continue;
                        }

                        let Ok(event) = serde_json::from_str::<TalosEvent>(payload) else {
                            continue;
                        };

                        if tx.send(event).await.is_err() {
                            return;
                        }
                    }
                }
            }
        });

        Ok(rx)
    }

    fn authorized(&self, builder: RequestBuilder) -> RequestBuilder {
        if let Some(token) = &self.config.auth_token {
            return builder.bearer_auth(token);
        }
        builder
    }

    fn request_error(method: &str, url: &str, error: impl std::fmt::Display) -> TalosError {
        TalosError::ClientError(format!("{} {} request failed: {}", method, url, error))
    }

    async fn http_error(response: Response, method: &str, url: &str) -> TalosError {
        let status = response.status();
        let body = response
            .text()
            .await
            .unwrap_or_else(|_| "Unknown error".to_string());
        TalosError::ClientError(format!(
            "{} {} failed with status {}: {}",
            method, url, status, body
        ))
    }

    async fn decode_json<T: DeserializeOwned>(
        response: Response,
        method: &str,
        url: &str,
    ) -> TalosResult<T> {
        if !response.status().is_success() {
            return Err(Self::http_error(response, method, url).await);
        }

        response.json::<T>().await.map_err(|e| {
            TalosError::ClientError(format!("{} {} returned invalid JSON: {}", method, url, e))
        })
    }
}

fn default_context(user_id: &str) -> TalosContext {
    TalosContext::sdk(user_id).with_channel_id("default")
}

fn workspace_doc_kind_path(kind: WorkspaceDocKind) -> &'static str {
    match kind {
        WorkspaceDocKind::Soul => "soul",
        WorkspaceDocKind::Identity => "identity",
        WorkspaceDocKind::User => "user",
        WorkspaceDocKind::Heartbeat => "heartbeat",
        WorkspaceDocKind::Memory => "memory",
    }
}

fn is_local_gateway_url(gateway_url: &str) -> bool {
    let Ok(url) = reqwest::Url::parse(gateway_url) else {
        return false;
    };

    let Some(host) = url.host_str() else {
        return false;
    };

    matches!(host, "localhost" | "127.0.0.1" | "::1")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_client_creation() {
        let config = ClientConfig::default();
        let client = TalosClient::new(config);
        assert!(client.is_ok());
    }

    #[tokio::test]
    async fn test_client_config_defaults() {
        let config = ClientConfig::default();
        assert_eq!(config.gateway_url, "http://localhost:8080");
        assert_eq!(config.timeout, 30);
        assert!(config.auth_token.is_none());
    }

    #[test]
    fn test_default_context_values() {
        let ctx = default_context("user-1");
        assert_eq!(ctx.source_type, TalosSourceType::Http);
        assert_eq!(ctx.provider, TalosProvider::Sdk);
        assert_eq!(ctx.channel_id.as_deref(), Some("default"));
        assert_eq!(ctx.user_id, "user-1");
    }

    #[test]
    fn test_is_local_gateway_url() {
        assert!(is_local_gateway_url("http://localhost:8080"));
        assert!(is_local_gateway_url("http://127.0.0.1:8080"));
        assert!(!is_local_gateway_url("http://example.com:8080"));
    }

    #[test]
    fn test_workspace_doc_kind_path() {
        assert_eq!(workspace_doc_kind_path(WorkspaceDocKind::Soul), "soul");
        assert_eq!(workspace_doc_kind_path(WorkspaceDocKind::Memory), "memory");
    }

    #[test]
    fn test_memory_file_list_params_query_serialization() {
        let params = MemoryFileListParams {
            limit: Some(10),
            before_date: Some("2026-04-05".to_string()),
            date_from: Some("2026-04-01".to_string()),
            date_to: Some("2026-04-30".to_string()),
        };
        let url = reqwest::Client::new()
            .get("http://localhost:8080/v1/memory/files")
            .query(&params)
            .build()
            .expect("request should build")
            .url()
            .to_string();
        assert!(url.contains("limit=10"));
        assert!(url.contains("before_date=2026-04-05"));
        assert!(url.contains("date_from=2026-04-01"));
        assert!(url.contains("date_to=2026-04-30"));
    }

    #[test]
    fn test_doc_update_request_shape_uses_expected_file_version() {
        let request = DocUpdateRequest {
            content: "body".to_string(),
            base_version: ExpectedFileVersion::MustNotExist,
        };
        let value = serde_json::to_value(&request).expect("request should serialize");
        assert_eq!(value["base_version"]["mode"], "must_not_exist");
    }

    #[test]
    fn test_talos_session_roundtrip_shape() {
        let mut config = talos_types::TalosConfig::default();
        config.agent.model = "gpt-4o-mini".to_string();
        config.llm.model = "gpt-4o-mini".to_string();

        let session = TalosSession {
            title: "Untitled Session".to_string(),
            context: talos_types::SessionContext {
                session_id: "sess_1".to_string(),
                workspace_id: None,
                source_type: TalosSourceType::Http,
                provider: TalosProvider::Sdk,
                user_id: "u".to_string(),
                message_id: "m1".to_string(),
                trace_id: "t1".to_string(),
                channel_id: Some("default".to_string()),
                thread_id: None,
                project_id: None,
                current_directory: None,
                project_root: None,
                environment: None,
            },
            project_id: None,
            current_directory: None,
            config,
            state: talos_types::SessionState {
                created_at: 1,
                last_activity: 123,
                message_count: 7,
                usage: talos_types::talos::TalosUsage::default(),
            },
        };

        assert_eq!(session.context.session_id, "sess_1");
        assert_eq!(session.config.llm.model, "gpt-4o-mini");
        assert_eq!(session.state.message_count, 7);
        assert_eq!(session.state.last_activity, 123);
    }

    #[test]
    fn test_request_error_includes_method_and_url() {
        let error = TalosClient::request_error("GET", "http://localhost:8080/v1/models", "boom");

        let TalosError::ClientError(message) = error else {
            panic!("expected client error");
        };
        assert!(message.contains("GET http://localhost:8080/v1/models request failed"));
    }
}
