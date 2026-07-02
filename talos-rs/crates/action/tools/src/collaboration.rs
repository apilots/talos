use std::sync::Arc;

use async_trait::async_trait;
use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use talos_error::{TalosError, TalosResult};
use talos_types::agent_dispatch::AgentDispatchResult;
use talos_types::talos::{StopReason, TalosContext, TalosProvider, TalosSourceType};
use talos_types::TalosSession;
use tokio::sync::{watch, Mutex};
use tokio::task::JoinHandle;
use tokio::time::{timeout, Duration};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case", tag = "type")]
pub enum CollaborationAgentStatus {
    Idle,
    Running,
    Completed {
        output: String,
        stop_reason: StopReason,
    },
    Failed {
        error: String,
    },
    Closed,
    NotFound,
}

impl CollaborationAgentStatus {
    pub fn is_final(&self) -> bool {
        matches!(
            self,
            Self::Completed { .. } | Self::Failed { .. } | Self::Closed | Self::NotFound
        )
    }
}

#[derive(Debug)]
struct CollaborationTask {
    status_tx: watch::Sender<CollaborationAgentStatus>,
    handle: Mutex<Option<JoinHandle<()>>>,
}

impl CollaborationTask {
    fn new() -> Self {
        let (status_tx, _status_rx) = watch::channel(CollaborationAgentStatus::Idle);
        Self {
            status_tx,
            handle: Mutex::new(None),
        }
    }
}

#[async_trait]
pub trait CollaborationAgentHost: Send + Sync {
    async fn create_session(&self, context: TalosContext) -> TalosResult<TalosSession>;

    async fn get_session(&self, session_id: &str) -> TalosResult<Option<TalosSession>>;

    async fn close_session(&self, session: TalosSession) -> TalosResult<()>;

    async fn submit_input(
        &self,
        session: TalosSession,
        message: String,
    ) -> TalosResult<AgentDispatchResult>;
}

pub struct CollaborationRuntime {
    host: Arc<dyn CollaborationAgentHost>,
    tasks: DashMap<String, Arc<CollaborationTask>>,
}

impl std::fmt::Debug for CollaborationRuntime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CollaborationRuntime")
            .finish_non_exhaustive()
    }
}

impl CollaborationRuntime {
    pub fn new(host: Arc<dyn CollaborationAgentHost>) -> Self {
        Self {
            host,
            tasks: DashMap::new(),
        }
    }

    pub async fn spawn_agent(
        &self,
        user_id: &str,
        initial_message: Option<String>,
    ) -> TalosResult<String> {
        let session = self
            .host
            .create_session(TalosContext {
                source_type: TalosSourceType::System,
                provider: TalosProvider::System,
                user_id: user_id.to_string(),
                message_id: Uuid::new_v4().to_string(),
                trace_id: Uuid::new_v4().to_string(),
                channel_id: None,
                thread_id: None,
                project_root: None,
                environment: None,
            })
            .await?;
        let session_id = session.context.session_id.clone();
        self.tasks
            .entry(session_id.clone())
            .or_insert_with(|| Arc::new(CollaborationTask::new()));
        if let Some(message) = initial_message.filter(|value| !value.trim().is_empty()) {
            self.send_input(session_id.as_str(), &message, false)
                .await?;
        }
        Ok(session_id)
    }

    pub async fn send_input(
        &self,
        session_id: &str,
        message: &str,
        interrupt: bool,
    ) -> TalosResult<String> {
        if message.trim().is_empty() {
            return Err(TalosError::ValidationError(
                "message cannot be empty".to_string(),
            ));
        }
        let session = self
            .host
            .get_session(session_id)
            .await?
            .ok_or_else(|| TalosError::ResourceNotFound(format!("session {}", session_id)))?;
        let task = self
            .tasks
            .entry(session_id.to_string())
            .or_insert_with(|| Arc::new(CollaborationTask::new()))
            .clone();

        {
            let current = task.status_tx.borrow().clone();
            if matches!(current, CollaborationAgentStatus::Running) && !interrupt {
                return Err(TalosError::ToolError(format!(
                    "collaboration agent `{session_id}` is already running"
                )));
            }
            if interrupt {
                if let Some(handle) = task.handle.lock().await.take() {
                    handle.abort();
                }
            }
        }

        let host = Arc::clone(&self.host);
        let status_tx = task.status_tx.clone();
        let message = message.to_string();

        let _ = status_tx.send(CollaborationAgentStatus::Running);
        let submission_id = Uuid::new_v4().to_string();
        let handle = tokio::spawn(async move {
            let next_status = match host.submit_input(session, message).await {
                Ok(response) => CollaborationAgentStatus::Completed {
                    output: response.output_text,
                    stop_reason: response.stop_reason,
                },
                Err(error) => CollaborationAgentStatus::Failed {
                    error: error.to_string(),
                },
            };
            let _ = status_tx.send(next_status);
        });
        *task.handle.lock().await = Some(handle);

        Ok(submission_id)
    }

    pub async fn wait(
        &self,
        targets: &[String],
        timeout_ms: u64,
    ) -> TalosResult<Vec<(String, CollaborationAgentStatus)>> {
        let mut receivers = Vec::new();
        let mut initial = Vec::new();
        for target in targets {
            let Some(task) = self.tasks.get(target).map(|entry| entry.clone()) else {
                initial.push((target.clone(), CollaborationAgentStatus::NotFound));
                continue;
            };
            let status = task.status_tx.borrow().clone();
            if status.is_final() {
                initial.push((target.clone(), status));
            } else {
                receivers.push((target.clone(), task.status_tx.subscribe()));
            }
        }

        if !initial.is_empty() {
            return Ok(initial);
        }

        let timeout_ms = timeout_ms.max(10_000);
        let deadline = Duration::from_millis(timeout_ms);
        let mut completed = Vec::new();
        for (target, mut rx) in receivers {
            let result = timeout(deadline, async {
                loop {
                    if rx.borrow().is_final() {
                        return rx.borrow().clone();
                    }
                    if rx.changed().await.is_err() {
                        return CollaborationAgentStatus::NotFound;
                    }
                }
            })
            .await;
            if let Ok(status) = result {
                completed.push((target, status));
                break;
            }
        }
        Ok(completed)
    }

    pub async fn close_agent(&self, target: &str) -> TalosResult<CollaborationAgentStatus> {
        if let Some(task) = self.tasks.get(target).map(|entry| entry.clone()) {
            if let Some(handle) = task.handle.lock().await.take() {
                handle.abort();
            }
            let _ = task.status_tx.send(CollaborationAgentStatus::Closed);
        }
        if let Some(session) = self.host.get_session(target).await? {
            self.host.close_session(session).await?;
        }
        Ok(CollaborationAgentStatus::Closed)
    }

    pub async fn resume_agent(&self, target: &str) -> TalosResult<CollaborationAgentStatus> {
        let Some(_session) = self.host.get_session(target).await? else {
            return Ok(CollaborationAgentStatus::NotFound);
        };
        let task = self
            .tasks
            .entry(target.to_string())
            .or_insert_with(|| Arc::new(CollaborationTask::new()))
            .clone();
        let _ = task.status_tx.send(CollaborationAgentStatus::Idle);
        Ok(CollaborationAgentStatus::Idle)
    }
}
