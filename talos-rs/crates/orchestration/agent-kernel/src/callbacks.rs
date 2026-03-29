use crate::{InvocationContext, LlmRequestEnvelope, LlmResponseEnvelope};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use talos_error::TalosResult;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CallbackKind {
    BeforeModel,
    AfterModel,
    BeforeTool,
    AfterTool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CallbackControl {
    Continue,
    Stop,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ToolCallbackPayload {
    pub tool_name: String,
    pub call_id: String,
}

#[async_trait]
pub trait BeforeModelCallback: Send + Sync {
    async fn run(
        &self,
        _ctx: &mut InvocationContext,
        _request: &mut LlmRequestEnvelope,
    ) -> TalosResult<CallbackControl> {
        Ok(CallbackControl::Continue)
    }
}

#[async_trait]
pub trait AfterModelCallback: Send + Sync {
    async fn run(
        &self,
        _ctx: &mut InvocationContext,
        _response: &mut LlmResponseEnvelope,
    ) -> TalosResult<CallbackControl> {
        Ok(CallbackControl::Continue)
    }
}

#[async_trait]
pub trait BeforeToolCallback: Send + Sync {
    async fn run(
        &self,
        _ctx: &mut InvocationContext,
        _payload: &ToolCallbackPayload,
    ) -> TalosResult<CallbackControl> {
        Ok(CallbackControl::Continue)
    }
}

#[async_trait]
pub trait AfterToolCallback: Send + Sync {
    async fn run(
        &self,
        _ctx: &mut InvocationContext,
        _payload: &ToolCallbackPayload,
    ) -> TalosResult<CallbackControl> {
        Ok(CallbackControl::Continue)
    }
}

#[derive(Default)]
pub struct CallbackRegistry {
    before_model: Vec<Arc<dyn BeforeModelCallback>>,
    after_model: Vec<Arc<dyn AfterModelCallback>>,
    before_tool: Vec<Arc<dyn BeforeToolCallback>>,
    after_tool: Vec<Arc<dyn AfterToolCallback>>,
}

impl CallbackRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_before_model(mut self, callback: Arc<dyn BeforeModelCallback>) -> Self {
        self.before_model.push(callback);
        self
    }

    pub fn with_after_model(mut self, callback: Arc<dyn AfterModelCallback>) -> Self {
        self.after_model.push(callback);
        self
    }

    pub fn with_before_tool(mut self, callback: Arc<dyn BeforeToolCallback>) -> Self {
        self.before_tool.push(callback);
        self
    }

    pub fn with_after_tool(mut self, callback: Arc<dyn AfterToolCallback>) -> Self {
        self.after_tool.push(callback);
        self
    }

    pub async fn run_before_model(
        &self,
        ctx: &mut InvocationContext,
        request: &mut LlmRequestEnvelope,
    ) -> TalosResult<CallbackControl> {
        for callback in &self.before_model {
            if matches!(callback.run(ctx, request).await?, CallbackControl::Stop) {
                return Ok(CallbackControl::Stop);
            }
        }
        Ok(CallbackControl::Continue)
    }

    pub async fn run_after_model(
        &self,
        ctx: &mut InvocationContext,
        response: &mut LlmResponseEnvelope,
    ) -> TalosResult<CallbackControl> {
        for callback in &self.after_model {
            if matches!(callback.run(ctx, response).await?, CallbackControl::Stop) {
                return Ok(CallbackControl::Stop);
            }
        }
        Ok(CallbackControl::Continue)
    }

    pub async fn run_before_tool(
        &self,
        ctx: &mut InvocationContext,
        payload: &ToolCallbackPayload,
    ) -> TalosResult<CallbackControl> {
        for callback in &self.before_tool {
            if matches!(callback.run(ctx, payload).await?, CallbackControl::Stop) {
                return Ok(CallbackControl::Stop);
            }
        }
        Ok(CallbackControl::Continue)
    }

    pub async fn run_after_tool(
        &self,
        ctx: &mut InvocationContext,
        payload: &ToolCallbackPayload,
    ) -> TalosResult<CallbackControl> {
        for callback in &self.after_tool {
            if matches!(callback.run(ctx, payload).await?, CallbackControl::Stop) {
                return Ok(CallbackControl::Stop);
            }
        }
        Ok(CallbackControl::Continue)
    }
}
