//! Optional observation ports. Callbacks must be bounded, nonblocking and IO-free.
#[derive(Debug, Clone, Copy)]
pub enum KernelObservedPurpose {
    Working,
    FinalAnswer,
}
use std::{fmt::Debug, sync::Arc};
use talos_error::TalosError;
use talos_llm_provider::{
    ChatMessage, ChatRequest, ChatResponse, FunctionCall, ModelBlockStreamEvent,
};

#[derive(Debug, Clone, Copy)]
pub enum KernelToolEnd {
    Completed,
    Waiting,
}

pub trait KernelTrajectoryRecorder: Send + Sync + Debug {
    fn tool_queued(&self, _call_id: &str) {}
    fn tool_started(&self, _call_id: &str) {}
    fn tool_finished(&self, _call_id: &str, _end: KernelToolEnd) {}
    fn prepared_tool_result(&self, _message: &ChatMessage) {}
    fn start_model(
        &self,
        mode: KernelObservedPurpose,
        connection_id: &str,
        request: &ChatRequest,
    ) -> Option<Arc<dyn KernelModelObservation>>;
}
pub trait KernelModelObservation: Send + Sync + Debug {
    fn tool_calls(&self, _calls: &[FunctionCall]) {}
    fn model_event(&self, event: &ModelBlockStreamEvent);
    fn response(&self, response: &ChatResponse);
    fn finished(&self, error: Option<&TalosError>);
    fn interrupted(&self);
}
pub(crate) struct ModelObservationGuard {
    observer: Option<Arc<dyn KernelModelObservation>>,
}
impl ModelObservationGuard {
    pub fn new(observer: Option<Arc<dyn KernelModelObservation>>) -> Self {
        Self { observer }
    }
    pub fn observer(&self) -> Option<Arc<dyn KernelModelObservation>> {
        self.observer.clone()
    }
    pub fn finish(&mut self, error: Option<&TalosError>) {
        if let Some(observer) = self.observer.take() {
            observer.finished(error);
        }
    }
}
impl Drop for ModelObservationGuard {
    fn drop(&mut self) {
        if let Some(observer) = self.observer.take() {
            observer.interrupted();
        }
    }
}
