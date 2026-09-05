use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use talos_agent_kernel::{
    ExecutionBranch, InvocationSession, KernelSessionArtifactStore, KernelTranscriptEvent,
    KernelTranscriptService, SessionState,
};
use talos_error::TalosResult;

/// Kernel transcript service backed by the Provider-private artifact store.
pub struct KernelTranscriptStore {
    artifacts: Arc<dyn KernelSessionArtifactStore>,
    workspace_root: PathBuf,
}

impl KernelTranscriptStore {
    pub fn new(artifacts: Arc<dyn KernelSessionArtifactStore>, workspace_root: PathBuf) -> Self {
        Self {
            artifacts,
            workspace_root,
        }
    }

    fn apply_state_update(session: &mut InvocationSession, event: &KernelTranscriptEvent) {
        if let talos_agent_kernel::KernelTranscriptPayload::SessionState { update } = &event.payload
        {
            session.state.apply(update);
        }
    }
}

#[async_trait]
impl KernelTranscriptService for KernelTranscriptStore {
    async fn append_event(
        &self,
        session: &mut InvocationSession,
        event: KernelTranscriptEvent,
    ) -> TalosResult<KernelTranscriptEvent> {
        if event.partial {
            return Ok(event);
        }

        Self::apply_state_update(session, &event);
        self.artifacts
            .append_transcript_event(&self.workspace_root, &event)
            .await?;
        session.events.push(event.clone());
        Ok(event)
    }

    async fn load_session(
        &self,
        session_id: &str,
        invocation_id: Option<&str>,
        branch: Option<ExecutionBranch>,
    ) -> TalosResult<InvocationSession> {
        let events = self
            .artifacts
            .load_transcript_events(&self.workspace_root, session_id, invocation_id, branch)
            .await?;
        let mut session = InvocationSession {
            session_id: session_id.to_string(),
            events,
            state: SessionState::default(),
        };
        for event in session.events.clone() {
            Self::apply_state_update(&mut session, &event);
        }
        Ok(session)
    }
}
