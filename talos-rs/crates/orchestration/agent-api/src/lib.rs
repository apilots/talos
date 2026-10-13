//! Canonical Agent product objects shared by Talos transports and services.
//!
//! This crate owns messages, events, Timeline/History DTOs, and public Turn API
//! objects. Execution lifecycle, Provider contracts, errors, and event-state
//! reduction belong to `talos-agent`.

mod continuation;
mod event;
mod history;
mod lifecycle;
mod message;
mod run;
mod turn_api;

pub use continuation::AgentContinuationRef;
pub use event::{
    AgentContextCompactionDelta, AgentEvent, AgentEventPayload, AgentMessageBlockCompleted,
    AgentMessageBlockDelta, AgentMessageBlockDeltaContent, AgentMessageBlockStarted,
    AgentMessageStarted, AgentPendingInput, AgentRunMetrics, AgentStopReason,
    AgentToolCallArgumentsDelta, AgentTurnCompleted, AgentTurnFailed, AgentTurnStarted,
    RecoverTimelineResponse, SessionActiveTurnChangedNotification, SessionActiveTurnResponse,
    TimelineActiveTurn, TimelineCursorStatus, TimelineEvent, TimelineEventPayload, TimelineReplay,
    TimelineSystemNotice,
};
pub use history::{SessionHistoryResponse, SessionHistoryTurn};
pub use lifecycle::ActiveTurnState;
pub use message::{
    AgentAssistantText, AgentAssistantTextPhase, AgentContextCompaction, AgentExecutionSummary,
    AgentMessage, AgentMessageBlock, AgentMessageBlockContent, AgentMessageBlockStatus,
    AgentMessageRole, AgentMessageStatus, AgentReasoning, AgentSystemNotice, AgentToolCall,
    AgentToolCallArguments, AgentToolProgress, AgentToolResult, PromptWarning, PromptWarningFile,
    PromptWarningNotice, SystemNoticeContent,
};
pub use run::{AgentRunFailure, AgentRunOutput, AgentRunResult};
pub use talos_types::{AgentMediaSource, AgentTurnRef, AgentUserContent, AgentUserInput};
pub use turn_api::{
    AgentInterruptOutcome, InterruptTurnRequest, InterruptTurnResponse, SubmitTurnInput,
    SubmitTurnInputRequest, SubmitTurnInputResponse,
};
