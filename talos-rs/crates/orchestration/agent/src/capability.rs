use crate::{
    AgentCapabilities, AgentDescriptor, AgentError, AgentInput, AgentInputParams,
    AgentInterruptParams, AgentResult, AgentResumeParams, AgentRunOptions, AgentRunParams,
};

/// Standard Agent capability names enforced by Harness and Coordinator.
///
/// Add a variant only when the capability can be validated through the shared
/// Agent API. Runtime-private features must stay in runtime_config.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentCapability {
    /// The Agent can stream standardized AgentEvent values.
    Streaming,
    /// The Agent can accept interrupt requests for active runs.
    Interrupt,
    /// The Agent can accept structured dynamic input for an existing run.
    DynamicInput,
}

impl AgentCapability {
    fn name(self) -> &'static str {
        match self {
            Self::Streaming => "streaming",
            Self::Interrupt => "interrupt",
            Self::DynamicInput => "dynamic_input",
        }
    }
}

impl AgentCapabilities {
    /// Return true when the advertised capability is supported.
    pub fn supports(&self, capability: AgentCapability) -> bool {
        match capability {
            AgentCapability::Streaming => self.streaming,
            AgentCapability::Interrupt => self.interrupt,
            AgentCapability::DynamicInput => self.dynamic_input,
        }
    }
}

/// Validates shared Agent requests against the selected Agent descriptor.
///
/// This validator is the single place for capability fail-fast rules. It must
/// stay independent from concrete Runtime implementations and product stores.
#[derive(Debug)]
pub struct AgentCapabilityValidator<'a> {
    agent_id: &'a str,
    capabilities: &'a AgentCapabilities,
}

impl<'a> AgentCapabilityValidator<'a> {
    /// Create a validator for a registered Agent descriptor.
    pub fn for_descriptor(descriptor: &'a AgentDescriptor) -> Self {
        Self {
            agent_id: &descriptor.agent_id,
            capabilities: &descriptor.capabilities,
        }
    }

    /// Validate a non-streaming run request.
    pub fn validate_run(&self, request: &AgentRunParams) -> AgentResult<()> {
        self.validate_agent_identity(request)?;
        self.validate_run_options(&request.options)?;
        self.validate_model_option_matches_session(request)?;
        if request.resume_context.is_some() {
            return Err(AgentError::invalid_execution_context(format!(
                "agent `{}` run request for turn `{}` contains continuation context; use resume API instead",
                self.agent_id, request.turn_id()
            )));
        }
        Ok(())
    }

    /// Validate a streaming run request.
    pub fn validate_streaming_run(&self, request: &AgentRunParams) -> AgentResult<()> {
        self.validate_run(request)?;
        self.require(AgentCapability::Streaming)
    }

    /// Validate a same-turn continuation request.
    pub fn validate_resume(&self, params: &AgentResumeParams) -> AgentResult<()> {
        self.validate_agent_identity(&params.run)?;
        self.validate_run_options(&params.run.options)?;
        self.validate_model_option_matches_session(&params.run)?;
        if params.run.resume_context.is_none() {
            return Err(AgentError::invalid_execution_context(format!(
                "agent `{}` resume requires a continuation context for turn `{}`",
                self.agent_id,
                params.run.turn_id()
            )));
        }
        self.require(AgentCapability::DynamicInput)
    }

    /// Validate a same-turn streaming continuation request.
    pub fn validate_streaming_resume(&self, params: &AgentResumeParams) -> AgentResult<()> {
        self.validate_resume(params)?;
        self.require(AgentCapability::Streaming)
    }

    /// Validate an interrupt request before it reaches a runtime handle.
    pub fn validate_interrupt(&self, _params: &AgentInterruptParams) -> AgentResult<()> {
        self.require(AgentCapability::Interrupt)
    }

    /// Validate input bridge payloads before they reach Harness lifecycle code.
    pub fn validate_input(&self, params: &AgentInputParams) -> AgentResult<()> {
        match &params.input {
            AgentInput::DynamicToolResult { .. } => self.require(AgentCapability::DynamicInput),
            AgentInput::Message { .. } => Ok(()),
        }
    }

    /// Validate cross-runtime run options.
    ///
    /// The current option set only contains model selection, which is supported
    /// by the Kernel-backed runtime. Future options must add capability checks
    /// here instead of scattering validation into runtime adapters.
    pub fn validate_run_options(&self, options: &AgentRunOptions) -> AgentResult<()> {
        let _ = options;
        Ok(())
    }

    fn validate_model_option_matches_session(&self, request: &AgentRunParams) -> AgentResult<()> {
        let Some(model) = &request.options.current_model else {
            return Ok(());
        };
        let selected_model = model.trim();
        if selected_model.is_empty() {
            return Ok(());
        }
        let session_model = request.config.model_selection.model_ref.id.trim();
        if session_model == selected_model {
            return Ok(());
        }
        Err(AgentError::invalid_execution_context(format!(
            "agent `{}` run option model `{}` does not match session model `{}`",
            self.agent_id, selected_model, session_model
        )))
    }

    fn validate_agent_identity(&self, request: &AgentRunParams) -> AgentResult<()> {
        if request.context.agent_id == self.agent_id {
            return Ok(());
        }
        Err(AgentError::invalid_execution_context(format!(
            "run targets agent `{}` but reached agent `{}`",
            request.context.agent_id, self.agent_id
        )))
    }

    fn require(&self, capability: AgentCapability) -> AgentResult<()> {
        if self.capabilities.supports(capability) {
            return Ok(());
        }
        Err(AgentError::capability_unsupported(
            self.agent_id,
            capability.name(),
        ))
    }
}
