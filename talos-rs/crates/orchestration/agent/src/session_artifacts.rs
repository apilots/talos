use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use talos_agent_kernel::{
    EventActions, EventAuthor, EventCompaction, ExecutionBranch, InvocationSession,
    KernelTranscriptAuthPayload, KernelTranscriptCompactionPayload,
    KernelTranscriptDynamicToolRequestPayload, KernelTranscriptDynamicToolResponsePayload,
    KernelTranscriptEvent, KernelTranscriptEventKind, KernelTranscriptFilter,
    KernelTranscriptPayload, KernelTranscriptService, KernelTranscriptTransferPayload,
    SessionState, StateDelta, StateNamespace,
};
use talos_error::TalosResult;
use talos_store::session::{
    SessionArtifactStore, SessionTranscriptActions, SessionTranscriptAuthPayload,
    SessionTranscriptAuthor, SessionTranscriptBranch, SessionTranscriptCompaction,
    SessionTranscriptCompactionPayload, SessionTranscriptDynamicToolRequestPayload,
    SessionTranscriptDynamicToolResponsePayload, SessionTranscriptEventKind,
    SessionTranscriptHeader, SessionTranscriptPayload, SessionTranscriptRecord,
    SessionTranscriptStateDelta, SessionTranscriptStateNamespace, SessionTranscriptTransferPayload,
};

pub(crate) struct SessionStoreKernelTranscriptService {
    store: Arc<dyn SessionArtifactStore>,
    workspace_root: PathBuf,
}

impl SessionStoreKernelTranscriptService {
    pub(crate) fn new(store: Arc<dyn SessionArtifactStore>, workspace_root: PathBuf) -> Self {
        Self {
            store,
            workspace_root,
        }
    }
}

struct KernelSessionTranscriptMapper;

impl KernelSessionTranscriptMapper {
    fn map_branch_to_transcript(branch: ExecutionBranch) -> SessionTranscriptBranch {
        match branch {
            ExecutionBranch::DirectAnswer => SessionTranscriptBranch::DirectAnswer,
            ExecutionBranch::ReactiveLoop => SessionTranscriptBranch::ReactiveLoop,
            ExecutionBranch::ModelThinking => SessionTranscriptBranch::ModelThinking,
        }
    }

    fn map_branch_from_transcript(branch: SessionTranscriptBranch) -> ExecutionBranch {
        match branch {
            SessionTranscriptBranch::DirectAnswer => ExecutionBranch::DirectAnswer,
            SessionTranscriptBranch::ReactiveLoop => ExecutionBranch::ReactiveLoop,
            SessionTranscriptBranch::ModelThinking => ExecutionBranch::ModelThinking,
        }
    }

    fn map_author_to_transcript(author: EventAuthor) -> SessionTranscriptAuthor {
        match author {
            EventAuthor::User => SessionTranscriptAuthor::User,
            EventAuthor::Agent => SessionTranscriptAuthor::Agent,
            EventAuthor::Tool => SessionTranscriptAuthor::Tool,
            EventAuthor::System => SessionTranscriptAuthor::System,
        }
    }

    fn map_author_from_transcript(author: SessionTranscriptAuthor) -> EventAuthor {
        match author {
            SessionTranscriptAuthor::User => EventAuthor::User,
            SessionTranscriptAuthor::Agent => EventAuthor::Agent,
            SessionTranscriptAuthor::Tool => EventAuthor::Tool,
            SessionTranscriptAuthor::System => EventAuthor::System,
        }
    }

    fn map_kind_to_transcript(kind: KernelTranscriptEventKind) -> SessionTranscriptEventKind {
        match kind {
            KernelTranscriptEventKind::UserMessage => SessionTranscriptEventKind::UserMessage,
            KernelTranscriptEventKind::AgentMessage => SessionTranscriptEventKind::AgentMessage,
            KernelTranscriptEventKind::ToolCall => SessionTranscriptEventKind::ToolCall,
            KernelTranscriptEventKind::ToolResult => SessionTranscriptEventKind::ToolResult,
            KernelTranscriptEventKind::SystemMessage => SessionTranscriptEventKind::SystemMessage,
            KernelTranscriptEventKind::Planning => SessionTranscriptEventKind::Planning,
            KernelTranscriptEventKind::StateUpdate => SessionTranscriptEventKind::StateUpdate,
        }
    }

    fn map_kind_from_transcript(kind: SessionTranscriptEventKind) -> KernelTranscriptEventKind {
        match kind {
            SessionTranscriptEventKind::UserMessage => KernelTranscriptEventKind::UserMessage,
            SessionTranscriptEventKind::AgentMessage => KernelTranscriptEventKind::AgentMessage,
            SessionTranscriptEventKind::ToolCall => KernelTranscriptEventKind::ToolCall,
            SessionTranscriptEventKind::ToolResult => KernelTranscriptEventKind::ToolResult,
            SessionTranscriptEventKind::SystemMessage => KernelTranscriptEventKind::SystemMessage,
            SessionTranscriptEventKind::Planning => KernelTranscriptEventKind::Planning,
            SessionTranscriptEventKind::StateUpdate => KernelTranscriptEventKind::StateUpdate,
        }
    }

    fn map_actions_to_transcript(actions: &EventActions) -> SessionTranscriptActions {
        SessionTranscriptActions {
            state_delta: actions
                .state_delta
                .iter()
                .map(|delta| SessionTranscriptStateDelta {
                    namespace: match delta.namespace {
                        StateNamespace::App => SessionTranscriptStateNamespace::App,
                        StateNamespace::User => SessionTranscriptStateNamespace::User,
                        StateNamespace::Temp => SessionTranscriptStateNamespace::Temp,
                        StateNamespace::Session => SessionTranscriptStateNamespace::Session,
                        StateNamespace::Workspace => SessionTranscriptStateNamespace::Workspace,
                    },
                    key: delta.key.clone(),
                    value: delta.value.clone().into(),
                })
                .collect(),
            end_turn: actions.end_turn,
            transfer_to: actions.transfer_to.clone(),
            branch_override: actions.branch_override.map(Self::map_branch_to_transcript),
            rewind_before_invocation_id: actions.rewind_before_invocation_id.clone(),
            compaction: actions
                .compaction
                .as_ref()
                .map(|compaction| SessionTranscriptCompaction {
                    start_event_id: compaction.start_event_id.clone(),
                    end_event_id: compaction.end_event_id.clone(),
                }),
        }
    }

    fn map_actions_from_transcript(actions: SessionTranscriptActions) -> EventActions {
        EventActions {
            state_delta: actions
                .state_delta
                .into_iter()
                .map(|delta| StateDelta {
                    namespace: match delta.namespace {
                        SessionTranscriptStateNamespace::App => StateNamespace::App,
                        SessionTranscriptStateNamespace::User => StateNamespace::User,
                        SessionTranscriptStateNamespace::Temp => StateNamespace::Temp,
                        SessionTranscriptStateNamespace::Session => StateNamespace::Session,
                        SessionTranscriptStateNamespace::Workspace => StateNamespace::Workspace,
                    },
                    key: delta.key,
                    value: delta.value.into(),
                })
                .collect(),
            end_turn: actions.end_turn,
            transfer_to: actions.transfer_to,
            branch_override: actions
                .branch_override
                .map(Self::map_branch_from_transcript),
            rewind_before_invocation_id: actions.rewind_before_invocation_id,
            compaction: actions.compaction.map(|compaction| EventCompaction {
                start_event_id: compaction.start_event_id,
                end_event_id: compaction.end_event_id,
            }),
        }
    }

    fn apply_state_delta(session: &mut InvocationSession, event: &KernelTranscriptEvent) {
        for delta in &event.actions.state_delta {
            session.state.apply_delta(delta);
        }
    }

    fn map_payload_to_transcript(payload: &KernelTranscriptPayload) -> SessionTranscriptPayload {
        match payload {
            KernelTranscriptPayload::Message { message } => SessionTranscriptPayload::Message {
                message: message.clone(),
            },
            KernelTranscriptPayload::StateOnly => SessionTranscriptPayload::StateOnly,
            KernelTranscriptPayload::Transfer { payload } => SessionTranscriptPayload::Transfer {
                payload: SessionTranscriptTransferPayload {
                    target_agent: payload.target_agent.clone(),
                },
            },
            KernelTranscriptPayload::Auth { payload } => SessionTranscriptPayload::Auth {
                payload: SessionTranscriptAuthPayload {
                    provider: payload.provider.clone(),
                    prompt: payload.prompt.clone(),
                },
            },
            KernelTranscriptPayload::DynamicToolRequest { payload } => {
                SessionTranscriptPayload::DynamicToolRequest {
                    payload: SessionTranscriptDynamicToolRequestPayload {
                        call_id: payload.call_id.clone(),
                        tool_name: payload.tool_name.clone(),
                        arguments: payload.arguments.clone(),
                        reasoning_continuation: payload.reasoning_continuation.clone(),
                    },
                }
            }
            KernelTranscriptPayload::DynamicToolResponse { payload } => {
                SessionTranscriptPayload::DynamicToolResponse {
                    payload: SessionTranscriptDynamicToolResponsePayload {
                        call_id: payload.call_id.clone(),
                        tool_name: payload.tool_name.clone(),
                        output: payload.output.clone(),
                        success: payload.success,
                    },
                }
            }
            KernelTranscriptPayload::Compaction { payload } => {
                SessionTranscriptPayload::Compaction {
                    payload: SessionTranscriptCompactionPayload {
                        summary_message: payload.summary_message.clone(),
                    },
                }
            }
        }
    }

    fn map_payload_from_transcript(payload: SessionTranscriptPayload) -> KernelTranscriptPayload {
        match payload {
            SessionTranscriptPayload::Message { message } => {
                KernelTranscriptPayload::Message { message }
            }
            SessionTranscriptPayload::StateOnly => KernelTranscriptPayload::StateOnly,
            SessionTranscriptPayload::Transfer { payload } => KernelTranscriptPayload::Transfer {
                payload: KernelTranscriptTransferPayload {
                    target_agent: payload.target_agent,
                },
            },
            SessionTranscriptPayload::Auth { payload } => KernelTranscriptPayload::Auth {
                payload: KernelTranscriptAuthPayload {
                    provider: payload.provider,
                    prompt: payload.prompt,
                },
            },
            SessionTranscriptPayload::DynamicToolRequest { payload } => {
                KernelTranscriptPayload::DynamicToolRequest {
                    payload: KernelTranscriptDynamicToolRequestPayload {
                        call_id: payload.call_id,
                        tool_name: payload.tool_name,
                        arguments: payload.arguments,
                        reasoning_continuation: payload.reasoning_continuation,
                    },
                }
            }
            SessionTranscriptPayload::DynamicToolResponse { payload } => {
                KernelTranscriptPayload::DynamicToolResponse {
                    payload: KernelTranscriptDynamicToolResponsePayload {
                        call_id: payload.call_id,
                        tool_name: payload.tool_name,
                        output: payload.output,
                        success: payload.success,
                    },
                }
            }
            SessionTranscriptPayload::Compaction { payload } => {
                KernelTranscriptPayload::Compaction {
                    payload: KernelTranscriptCompactionPayload {
                        summary_message: payload.summary_message,
                    },
                }
            }
        }
    }
}

impl KernelSessionTranscriptMapper {
    fn to_record(event: &KernelTranscriptEvent) -> SessionTranscriptRecord {
        SessionTranscriptRecord {
            header: SessionTranscriptHeader {
                id: event.id.clone(),
                session_id: event.session_id.clone(),
                invocation_id: event.invocation_id.clone(),
                branch: Self::map_branch_to_transcript(event.branch),
                timestamp: event.timestamp,
            },
            author: Self::map_author_to_transcript(event.author.clone()),
            kind: Self::map_kind_to_transcript(event.kind.clone()),
            payload: Self::map_payload_to_transcript(&event.payload),
            actions: Self::map_actions_to_transcript(&event.actions),
            partial: event.partial,
        }
    }

    fn from_record(record: SessionTranscriptRecord) -> KernelTranscriptEvent {
        KernelTranscriptEvent {
            id: record.header.id,
            session_id: record.header.session_id,
            invocation_id: record.header.invocation_id,
            branch: Self::map_branch_from_transcript(record.header.branch),
            author: Self::map_author_from_transcript(record.author),
            kind: Self::map_kind_from_transcript(record.kind),
            payload: Self::map_payload_from_transcript(record.payload),
            actions: Self::map_actions_from_transcript(record.actions),
            partial: record.partial,
            timestamp: record.header.timestamp,
        }
    }
}

#[async_trait]
impl KernelTranscriptService for SessionStoreKernelTranscriptService {
    async fn append_event(
        &self,
        session: &mut InvocationSession,
        event: KernelTranscriptEvent,
    ) -> TalosResult<KernelTranscriptEvent> {
        if event.partial {
            return Ok(event);
        }

        KernelSessionTranscriptMapper::apply_state_delta(session, &event);
        self.store
            .append_transcript_record(
                &self.workspace_root,
                &KernelSessionTranscriptMapper::to_record(&event),
            )
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
        let events = if let (Some(invocation_id), Some(branch)) = (invocation_id, branch) {
            self.store
                .load_transcript_records(
                    &self.workspace_root,
                    session_id,
                    Some(invocation_id),
                    Some(KernelSessionTranscriptMapper::map_branch_to_transcript(
                        branch,
                    )),
                )
                .await?
                .into_iter()
                .map(KernelSessionTranscriptMapper::from_record)
                .collect()
        } else if let Some(branch) = branch {
            self.store
                .load_transcript_records(
                    &self.workspace_root,
                    session_id,
                    invocation_id,
                    Some(KernelSessionTranscriptMapper::map_branch_to_transcript(
                        branch,
                    )),
                )
                .await?
                .into_iter()
                .map(KernelSessionTranscriptMapper::from_record)
                .collect()
        } else {
            self.store
                .load_transcript_records(&self.workspace_root, session_id, invocation_id, None)
                .await?
                .into_iter()
                .map(KernelSessionTranscriptMapper::from_record)
                .collect()
        };

        let mut session = InvocationSession {
            session_id: session_id.to_string(),
            events,
            state: SessionState::default(),
        };
        for event in session.events.clone() {
            KernelSessionTranscriptMapper::apply_state_delta(&mut session, &event);
        }
        Ok(session)
    }

    async fn list_events(
        &self,
        session_id: &str,
        filter: &KernelTranscriptFilter,
    ) -> TalosResult<Vec<KernelTranscriptEvent>> {
        let session = self
            .load_session(session_id, filter.invocation_id.as_deref(), filter.branch)
            .await?;
        Ok(session.filter_events(filter.invocation_id.as_deref(), filter.branch))
    }
}
