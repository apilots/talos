use crate::{Coordinator, CoordinatorResponse, CoordinatorStatus};
use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;
use talos_agent::{AgentOrchestrator, OrchestratorRequest};
use talos_error::TalosResult;
use talos_router::Router;
use talos_session_store::SessionStore;
use talos_types::canonical::{CanonicalContext, CanonicalEvent, CanonicalEventType};
use talos_types::gateway::{GatewayRequest, GatewayResponse};
use talos_types::{RequestHandler, SessionPatch, SessionQuery, SessionRecord, SessionService};
use tokio::sync::{mpsc, RwLock};
use tracing::{debug, error, info};
use uuid::Uuid;

/// Implementation of the Coordinator trait
pub struct CoordinatorImpl {
    /// Router instance for routing messages to appropriate agents
    router: Arc<dyn Router>,

    /// Session store for managing conversation state
    session_store: Arc<dyn SessionStore>,

    /// Registered agents map
    agents: Arc<RwLock<HashMap<String, Arc<dyn AgentOrchestrator>>>>,

    /// Default model name
    default_model: String,

    /// Default workspace id when request metadata does not include one
    default_workspace_id: String,
}

impl CoordinatorImpl {
    /// Create a new instance of CoordinatorImpl
    pub fn new(
        router: Arc<dyn Router>,
        session_store: Arc<dyn SessionStore>,
        default_model: impl Into<String>,
    ) -> Self {
        Self {
            router,
            session_store,
            agents: Arc::new(RwLock::new(HashMap::new())),
            default_model: default_model.into(),
            default_workspace_id: "default".to_string(),
        }
    }

    pub fn with_default_workspace_id(mut self, workspace_id: impl Into<String>) -> Self {
        self.default_workspace_id = workspace_id.into();
        self
    }

    /// Get reference to registered agents
    pub fn agents(&self) -> Arc<RwLock<HashMap<String, Arc<dyn AgentOrchestrator>>>> {
        self.agents.clone()
    }

    fn map_orchestrator_event_type(event_type: &str) -> Option<CanonicalEventType> {
        match event_type {
            "message.started" => Some(CanonicalEventType::MessageStarted),
            "message.reasoning.delta" => Some(CanonicalEventType::MessageReasoningDelta),
            "message.output.delta" => Some(CanonicalEventType::MessageOutputDelta),
            "message.tool_call.started" => Some(CanonicalEventType::MessageToolCallStarted),
            "message.tool_call.completed" => Some(CanonicalEventType::MessageToolCallCompleted),
            "message.completed" => Some(CanonicalEventType::MessageCompleted),
            "message.error" => Some(CanonicalEventType::MessageError),
            _ => None,
        }
    }
}

#[async_trait]
impl Coordinator for CoordinatorImpl {
    async fn handle_message(
        &self,
        channel: &str,
        user_id: &str,
        content: &str,
        model: Option<&str>,
    ) -> TalosResult<CoordinatorResponse> {
        debug!(
            "Handling message from channel: {}, user: {}, model: {:?}",
            channel, user_id, model
        );

        // First, determine which agent should handle this message based on channel and user
        let target_agent = self.router.route(channel, user_id).await?;

        match target_agent {
            Some(agent_id) => {
                let agents = self.agents.read().await;

                if let Some(agent) = agents.get(&agent_id) {
                    let response = agent
                        .run(OrchestratorRequest {
                            agent_id: agent_id.clone(),
                            session_id: format!("legacy:{}:{}", channel, user_id),
                            invocation_id: Some(format!("inv-{}", talos_types::now())),
                            workspace_id: self.default_workspace_id.clone(),
                            input: content.to_string(),
                            model: model.map(ToString::to_string),
                            metadata: None,
                        })
                        .await?;

                    Ok(CoordinatorResponse {
                        result: response.output_text,
                        data: None,
                        status: CoordinatorStatus::Success,
                    })
                } else {
                    error!("No agent found for route: {}", agent_id);
                    Err(talos_error::TalosError::AgentNotFound(agent_id))
                }
            }
            None => {
                error!("No route found for channel: {}, user: {}", channel, user_id);
                Err(talos_error::TalosError::RouteNotFound(format!(
                    "{}:{}",
                    channel, user_id
                )))
            }
        }
    }

    async fn register_agent(
        &self,
        agent_id: &str,
        agent: Arc<dyn AgentOrchestrator>,
    ) -> TalosResult<()> {
        info!("Registering agent: {}", agent_id);

        let mut agents = self.agents.write().await;
        agents.insert(agent_id.to_string(), agent);

        Ok(())
    }
}

#[async_trait]
impl RequestHandler for CoordinatorImpl {
    async fn handle(&self, request: GatewayRequest) -> TalosResult<GatewayResponse> {
        let context = request.context.clone().unwrap_or(CanonicalContext {
            source_type: "gateway".to_string(),
            provider: "gateway".to_string(),
            channel_id: request.channel.clone(),
            user_id: request.user_id.clone(),
            thread_id: None,
            message_id: None,
            trace_id: None,
        });

        let target_agent = self
            .router
            .route(&request.channel, &request.user_id)
            .await?
            .unwrap_or_else(|| "default".to_string());

        let session = if let Some(session_id) = request.session_id.as_deref() {
            self.session_store
                .get_session_record(session_id)
                .await?
                .ok_or_else(|| {
                    talos_error::TalosError::ResourceNotFound(format!("session {}", session_id))
                })?
        } else {
            self.session_store
                .resolve_or_create_session(
                    &context,
                    &target_agent,
                    &self.default_model,
                    request.model.as_deref(),
                    request.metadata.clone(),
                )
                .await?
        };

        let model_for_turn = request
            .model
            .clone()
            .unwrap_or_else(|| session.model.clone());

        let agents = self.agents.read().await;
        let agent = agents
            .get(&target_agent)
            .cloned()
            .ok_or_else(|| talos_error::TalosError::AgentNotFound(target_agent.clone()))?;
        drop(agents);

        let workspace_id = request
            .metadata
            .as_ref()
            .and_then(|value| value.get("workspace_id"))
            .and_then(|value| value.as_str())
            .unwrap_or(self.default_workspace_id.as_str())
            .to_string();

        let agent_response = agent
            .run(OrchestratorRequest {
                agent_id: target_agent.clone(),
                session_id: session.id.clone(),
                invocation_id: Some(format!("inv-{}", talos_types::now())),
                workspace_id,
                input: request.content.clone(),
                model: Some(model_for_turn.clone()),
                metadata: request.metadata.clone(),
            })
            .await?;

        self.session_store
            .append_message_by_session_id(&session.id, "user", &request.content)
            .await?;
        self.session_store
            .append_message_by_session_id(&session.id, "assistant", &agent_response.output_text)
            .await?;

        Ok(GatewayResponse {
            content: agent_response.output_text,
            output_items: agent_response.output_items,
            stop_reason: agent_response.stop_reason,
            usage: agent_response.usage,
            success: true,
            model: model_for_turn,
            session_id: Some(session.id),
        })
    }

    async fn handle_stream(
        &self,
        request: GatewayRequest,
    ) -> TalosResult<mpsc::Receiver<CanonicalEvent>> {
        let context = request.context.clone().unwrap_or(CanonicalContext {
            source_type: "gateway".to_string(),
            provider: "gateway".to_string(),
            channel_id: request.channel.clone(),
            user_id: request.user_id.clone(),
            thread_id: None,
            message_id: None,
            trace_id: None,
        });

        let target_agent = self
            .router
            .route(&request.channel, &request.user_id)
            .await?
            .unwrap_or_else(|| "default".to_string());

        let session = if let Some(session_id) = request.session_id.as_deref() {
            self.session_store
                .get_session_record(session_id)
                .await?
                .ok_or_else(|| {
                    talos_error::TalosError::ResourceNotFound(format!("session {}", session_id))
                })?
        } else {
            self.session_store
                .resolve_or_create_session(
                    &context,
                    &target_agent,
                    &self.default_model,
                    request.model.as_deref(),
                    request.metadata.clone(),
                )
                .await?
        };

        let model_for_turn = request
            .model
            .clone()
            .unwrap_or_else(|| session.model.clone());

        let agents = self.agents.read().await;
        let agent = agents
            .get(&target_agent)
            .cloned()
            .ok_or_else(|| talos_error::TalosError::AgentNotFound(target_agent.clone()))?;
        drop(agents);

        let workspace_id = request
            .metadata
            .as_ref()
            .and_then(|value| value.get("workspace_id"))
            .and_then(|value| value.as_str())
            .unwrap_or(self.default_workspace_id.as_str())
            .to_string();

        let mut orchestrator_stream = agent
            .run_stream(OrchestratorRequest {
                agent_id: target_agent.clone(),
                session_id: session.id.clone(),
                invocation_id: Some(format!("inv-{}", talos_types::now())),
                workspace_id,
                input: request.content.clone(),
                model: Some(model_for_turn.clone()),
                metadata: request.metadata.clone(),
            })
            .await?;

        let session_store = self.session_store.clone();
        let session_id = session.id.clone();
        let user_content = request.content.clone();
        let message_id = format!("msg_{}", Uuid::new_v4().simple());
        let stream_session_id = session.id.clone();
        let (tx, rx) = mpsc::channel(256);
        tokio::spawn(async move {
            let mut sequence: u64 = 0;
            let mut assistant_output = String::new();

            while let Some(event) = orchestrator_stream.recv().await {
                let Some(event_type) =
                    CoordinatorImpl::map_orchestrator_event_type(event.event_type.as_str())
                else {
                    continue;
                };

                if event_type == CanonicalEventType::MessageOutputDelta {
                    if let Some(delta) = event.data.get("delta").and_then(|value| value.as_str()) {
                        assistant_output.push_str(delta);
                    }
                }

                if tx
                    .send(CanonicalEvent {
                        event_type,
                        message_id: message_id.clone(),
                        session_id: stream_session_id.clone(),
                        sequence,
                        data: event.data,
                    })
                    .await
                    .is_err()
                {
                    return;
                }
                sequence = sequence.saturating_add(1);
            }

            let _ = session_store
                .append_message_by_session_id(&session_id, "user", &user_content)
                .await;
            if !assistant_output.trim().is_empty() {
                let _ = session_store
                    .append_message_by_session_id(&session_id, "assistant", &assistant_output)
                    .await;
            }
        });

        Ok(rx)
    }
}

#[async_trait]
impl SessionService for CoordinatorImpl {
    async fn create_session(
        &self,
        context: CanonicalContext,
        model: Option<String>,
        metadata: Option<serde_json::Value>,
    ) -> TalosResult<SessionRecord> {
        let channel = format!("{}:{}", context.provider, context.channel_id);
        let target_agent = self
            .router
            .route(&channel, &context.user_id)
            .await?
            .unwrap_or_else(|| "default".to_string());

        self.session_store
            .create_session(
                &context,
                &target_agent,
                &self.default_model,
                model.as_deref(),
                metadata,
            )
            .await
    }

    async fn list_sessions(&self, query: SessionQuery) -> TalosResult<Vec<SessionRecord>> {
        self.session_store.list_session_records(&query).await
    }

    async fn get_session(&self, session_id: &str) -> TalosResult<Option<SessionRecord>> {
        self.session_store.get_session_record(session_id).await
    }

    async fn patch_session(
        &self,
        session_id: &str,
        patch: SessionPatch,
    ) -> TalosResult<Option<SessionRecord>> {
        self.session_store
            .patch_session_record(session_id, &patch)
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use tokio;

    // Define mock implementations for testing
    struct MockRouter;

    impl MockRouter {
        fn new() -> Self {
            Self
        }
    }

    #[async_trait]
    impl talos_router::Router for MockRouter {
        async fn route(&self, _channel: &str, _user_id: &str) -> TalosResult<Option<String>> {
            Ok(Some("test_agent".to_string()))
        }

        async fn add_binding(&self, _binding: talos_router::Binding) -> TalosResult<()> {
            Ok(())
        }

        async fn remove_binding(&self, _id: &str) -> TalosResult<()> {
            Ok(())
        }

        async fn list_bindings(&self) -> TalosResult<Vec<talos_router::Binding>> {
            Ok(vec![])
        }
    }

    #[tokio::test]
    async fn test_coordinator_creation() {
        let mock_router = Arc::new(MockRouter::new());
        let mock_session_store = Arc::new(talos_session_store::MockSessionStore::new());

        let coordinator = CoordinatorImpl::new(mock_router, mock_session_store, "qwen-max");

        assert_eq!(coordinator.agents.read().await.len(), 0);
    }

    #[tokio::test]
    async fn test_register_agent() {
        let mock_router = Arc::new(MockRouter::new());
        let mock_session_store = Arc::new(talos_session_store::MockSessionStore::new());

        let coordinator = CoordinatorImpl::new(mock_router, mock_session_store, "qwen-max");

        let mock_agent = Arc::new(talos_agent::MockAgent::new());
        let result = coordinator.register_agent("test_agent", mock_agent).await;

        assert!(result.is_ok());
        assert_eq!(coordinator.agents.read().await.len(), 1);
    }

    #[tokio::test]
    async fn test_handle_stream_returns_orchestrator_events() {
        let mock_router = Arc::new(MockRouter::new());
        let mock_session_store = Arc::new(talos_session_store::MockSessionStore::new());
        let coordinator = CoordinatorImpl::new(mock_router, mock_session_store, "qwen-max");
        coordinator
            .register_agent("test_agent", Arc::new(talos_agent::MockAgent::new()))
            .await
            .expect("agent registration should succeed");

        let mut stream = coordinator
            .handle_stream(GatewayRequest {
                channel: "test-channel".to_string(),
                user_id: "user-1".to_string(),
                content: "hello".to_string(),
                model: Some("qwen-max".to_string()),
                session_id: None,
                context: None,
                metadata: None,
            })
            .await
            .expect("stream request should succeed");

        let mut event_types = Vec::new();
        while let Some(event) = stream.recv().await {
            event_types.push(event.event_type.clone());
            if event.event_type == CanonicalEventType::MessageCompleted {
                break;
            }
        }

        assert!(event_types.contains(&CanonicalEventType::MessageStarted));
        assert!(event_types.contains(&CanonicalEventType::MessageOutputDelta));
        assert!(event_types.contains(&CanonicalEventType::MessageCompleted));
    }
}
