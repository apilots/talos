use crate::execution::ContentsMode;
use crate::{
    execution::{PlanningResponseSegmentKind, ReplayDynamicToolResult, ReplayToolInvocation},
    CompactionSummaryPayload, ContentsBuilder, EventActions, EventAuthor, EventPayload,
    ExecutionEvent, ExecutionEventKind, FlowProfile, InvocationContext, KernelTranscriptEvent,
    KernelTranscriptPayload, LlmRequestEnvelope, LlmResponseEnvelope, Planner,
    RequestOutputContract,
};
use async_trait::async_trait;
use chrono::Utc;
use std::sync::Arc;
use talos_error::TalosResult;
use talos_llm_provider::{ChatMessage, Role};
use talos_types::talos::{RunItem, TalosContentItem};

mod compaction;
mod contents;
mod identity;
mod planning;
mod request;
mod request_kinds;
mod response;
mod response_kinds;
mod response_processors;
#[cfg(test)]
#[path = "processor_tests.rs"]
mod tests;

pub use compaction::CompactionRequestProcessor;
pub use contents::ContentsRequestProcessor;
pub use identity::IdentityRequestProcessor;
pub use planning::{PlanningRequestProcessor, PlanningResponseProcessor};
pub use request::{RequestProcessor, RequestProcessorOutput, RequestProcessorPipeline};
pub use request_kinds::{
    AgentTransferRequestProcessor, AuthRequestProcessor, BasicRequestProcessor,
    ContextCacheRequestProcessor, DynamicToolReplayRequestProcessor, InstructionsRequestProcessor,
    InteractionsRequestProcessor, OutputSchemaRequestProcessor, RequestProcessorKind,
    ToolContextRequestProcessor,
};
pub use response::{ResponseProcessor, ResponseProcessorOutput, ResponseProcessorPipeline};
pub use response_kinds::ResponseProcessorKind;
pub use response_processors::{EventProjectionProcessor, StateDeltaResponseProcessor};

pub(crate) use request::latest_branch_event;
