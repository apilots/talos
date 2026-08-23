use std::sync::{Arc, RwLock};

use async_trait::async_trait;
use chrono::Utc;
use talos_error::{TalosError, TalosResult};
use talos_llm_provider::{ChatRequest, ChatResponse, ChatStream, LlmProvider, LlmProviderRouter};
use talos_store::profile::{ModelCallUsageUpdate, ProfileStore, ProfileUsageUpdate};
use talos_types::{ModelInfo, ModelRef, ModelUsageKind, TokenUsage};
use uuid::Uuid;

use super::{resolve_inner_model_ref, InnerModelChatParams};

/// Executes Agent-owned auxiliary model calls through the shared Provider and Profile paths.
///
/// The client owns the currently validated model selection, but it does not own Provider
/// configuration, business prompts, output parsing, or business persistence.
#[derive(Clone)]
pub struct InnerModelClient {
    model_ref: Arc<RwLock<ModelRef>>,
    provider_router: Arc<dyn LlmProviderRouter>,
    profile_store: Arc<dyn ProfileStore>,
    profile_timezone: String,
}

impl InnerModelClient {
    /// Starts explicit dependency assembly for one Inner Model client.
    pub fn builder() -> InnerModelClientBuilder {
        InnerModelClientBuilder::default()
    }

    /// Returns a snapshot of the model selected for subsequent Inner Model calls.
    pub fn model_ref(&self) -> ModelRef {
        self.model_ref
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    /// Installs a model ref that the composition root already validated.
    ///
    /// Callers must validate the model catalog and Provider construction before invoking this
    /// method so a failed reload cannot replace the active selection.
    pub fn reload(&self, model_ref: ModelRef) {
        *self
            .model_ref
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = model_ref;
    }

    /// Executes one auxiliary chat call and records its Provider usage in Profile storage.
    ///
    /// Only Inner usage kinds are accepted. Provider and usage-recording failures are returned to
    /// the business owner so it can preserve its write and cursor invariants.
    pub async fn chat(&self, params: InnerModelChatParams) -> TalosResult<ChatResponse> {
        if !params.usage_kind.is_inner() {
            return Err(TalosError::public(
                talos_error::ErrorCode::InvalidRequest,
                format!(
                    "usage kind '{}' is not an Inner Model workload",
                    params.usage_kind.as_str()
                ),
            ));
        }
        let model_ref = self.model_ref();
        let provider = self.provider_router.provider_for(&model_ref)?;
        let response = provider
            .chat(
                ChatRequest::builder()
                    .model(model_ref.id.clone())
                    .messages(params.messages)
                    .build(),
            )
            .await?;
        let usage = TokenUsage {
            input_tokens: response.usage.prompt_tokens,
            output_tokens: response.usage.completion_tokens,
            total_tokens: response.usage.total_tokens,
            cache_creation_input_tokens: response.usage.cache_creation_input_tokens,
            cache_read_input_tokens: response.usage.cache_read_input_tokens,
        };
        let completed_at = Utc::now();
        let timezone = talos_config::parse_timezone(&self.profile_timezone)?;
        let completed_at_ms = completed_at.timestamp_millis();
        let usage_date = completed_at
            .with_timezone(&timezone)
            .date_naive()
            .format("%Y-%m-%d")
            .to_string();
        let usage_available = usage.is_available();
        self.profile_store
            .record_usage(ProfileUsageUpdate::ModelCall(ModelCallUsageUpdate {
                user_id: params.user_id,
                session_id: params.session_id,
                request_id: Uuid::new_v4().to_string(),
                kind: params.usage_kind,
                model_ref,
                usage,
                usage_available,
                timezone: self.profile_timezone.clone(),
                completed_at_ms,
                usage_date,
            }))
            .await?;
        Ok(response)
    }

    /// Adapts this client to the plain `LlmProvider` boundary required by Daily Memory formation.
    ///
    /// The Agent Kernel memory API predates `InnerModelClient` and accepts only an
    /// `Arc<dyn LlmProvider>`. This narrow adapter preserves that stable boundary while attaching
    /// the user, session, and usage kind required for Inner Model routing and accounting. It
    /// delegates chat calls back to [`InnerModelClient::chat`], rejects streaming, and must not
    /// acquire prompts, parsing, persistence, fallback, or lifecycle behavior.
    ///
    /// Remove this adapter when the Kernel memory boundary gains a model-call abstraction that
    /// can carry the same scope without depending on the Agent layer.
    pub fn scoped_provider(
        self: &Arc<Self>,
        user_id: String,
        session_id: Option<String>,
        usage_kind: ModelUsageKind,
    ) -> TalosResult<Arc<dyn LlmProvider>> {
        if !usage_kind.is_inner() {
            return Err(TalosError::public(
                talos_error::ErrorCode::InvalidRequest,
                format!(
                    "usage kind '{}' is not an Inner Model workload",
                    usage_kind.as_str()
                ),
            ));
        }
        Ok(Arc::new(ScopedInnerModelProvider {
            client: self.clone(),
            user_id,
            session_id,
            usage_kind,
        }))
    }
}

/// Plain-chat Provider view of one fixed user/session/usage scope.
///
/// Instances are created only by [`InnerModelClient::scoped_provider`] and exist solely to bridge
/// the current Agent Kernel memory interface.
struct ScopedInnerModelProvider {
    client: Arc<InnerModelClient>,
    user_id: String,
    session_id: Option<String>,
    usage_kind: ModelUsageKind,
}

#[async_trait]
impl LlmProvider for ScopedInnerModelProvider {
    /// Returns the adapter identity used for Provider diagnostics.
    fn name(&self) -> &str {
        "inner-model"
    }

    /// Delegates the request messages to the scoped Inner Model call and usage path.
    async fn chat(&self, request: ChatRequest) -> TalosResult<ChatResponse> {
        self.client
            .chat(InnerModelChatParams {
                user_id: self.user_id.clone(),
                session_id: self.session_id.clone(),
                usage_kind: self.usage_kind,
                messages: request.messages,
            })
            .await
    }

    /// Rejects streaming because the current Daily Memory caller only supports plain chat.
    async fn stream(&self, _request: ChatRequest) -> TalosResult<ChatStream> {
        Err(TalosError::public(
            talos_error::ErrorCode::InvalidRequest,
            "Inner Model does not support streaming calls".to_string(),
        ))
    }

    /// Returns the currently active Inner Model id for the legacy Provider contract.
    fn default_model(&self) -> String {
        self.client.model_ref().id
    }
}

/// Collects the existing routing and Profile dependencies required by `InnerModelClient`.
///
/// It validates model resolution and Provider construction during `build`; it does not create a
/// second Provider registry or persist configuration.
#[derive(Default)]
pub struct InnerModelClientBuilder {
    configured_model: Option<Option<ModelRef>>,
    agent_default_model: Option<ModelRef>,
    model_catalog: Option<Vec<ModelInfo>>,
    provider_router: Option<Arc<dyn LlmProviderRouter>>,
    profile_store: Option<Arc<dyn ProfileStore>>,
    profile_timezone: Option<String>,
}

impl InnerModelClientBuilder {
    /// Sets the optional explicit Agent Inner Model selection.
    pub fn configured_model(mut self, model_ref: Option<ModelRef>) -> Self {
        self.configured_model = Some(model_ref);
        self
    }

    /// Sets the Agent default used when no explicit Inner Model is configured.
    pub fn agent_default_model(mut self, model_ref: ModelRef) -> Self {
        self.agent_default_model = Some(model_ref);
        self
    }

    /// Sets the shared model catalog used to validate the effective selection.
    pub fn model_catalog(mut self, model_catalog: Vec<ModelInfo>) -> Self {
        self.model_catalog = Some(model_catalog);
        self
    }

    /// Sets the existing Provider Router used for model-specific Provider resolution.
    pub fn provider_router(mut self, provider_router: Arc<dyn LlmProviderRouter>) -> Self {
        self.provider_router = Some(provider_router);
        self
    }

    /// Sets the existing Profile Store used for per-user model-call accounting.
    pub fn profile_store(mut self, profile_store: Arc<dyn ProfileStore>) -> Self {
        self.profile_store = Some(profile_store);
        self
    }

    /// Sets the timezone used to derive Profile daily-usage dates.
    pub fn profile_timezone(mut self, profile_timezone: String) -> Self {
        self.profile_timezone = Some(profile_timezone);
        self
    }

    /// Resolves and validates the effective model, then builds the shared client.
    pub fn build(self) -> TalosResult<InnerModelClient> {
        let agent_default_model = required(self.agent_default_model, "agent_default_model")?;
        let model_catalog = required(self.model_catalog, "model_catalog")?;
        let model_ref = resolve_inner_model_ref(
            required(self.configured_model, "configured_model")?.as_ref(),
            &agent_default_model,
            &model_catalog,
        )?;
        let provider_router = required(self.provider_router, "provider_router")?;
        provider_router.provider_for(&model_ref)?;
        Ok(InnerModelClient {
            model_ref: Arc::new(RwLock::new(model_ref)),
            provider_router,
            profile_store: required(self.profile_store, "profile_store")?,
            profile_timezone: required(self.profile_timezone, "profile_timezone")?,
        })
    }
}

/// Converts a missing Builder dependency into one contextual configuration error.
fn required<T>(value: Option<T>, field: &str) -> TalosResult<T> {
    value.ok_or_else(|| {
        TalosError::public(
            talos_error::ErrorCode::InvalidRequest,
            format!("InnerModelClientBuilder.{field} is required"),
        )
    })
}
