use crate::{
    AgentRunResult, CallbackControl, CallbackRegistry, ExecutionEvent, InvocationContext,
    KernelExecutionSettings, LlmRequestEnvelope, LlmResponseEnvelope, Plugin, TaskResult,
    ToolCallbackPayload,
};
use std::sync::Arc;
use talos_error::TalosResult;

#[derive(Default, Clone)]
pub struct InvocationPluginManager {
    plugins: Vec<Arc<dyn Plugin>>,
    callbacks: Option<Arc<CallbackRegistry>>,
}

impl std::fmt::Debug for InvocationPluginManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("InvocationPluginManager")
            .field("plugins_len", &self.plugins.len())
            .field("has_callbacks", &self.callbacks.is_some())
            .finish()
    }
}

impl InvocationPluginManager {
    pub fn new() -> Self {
        Self {
            plugins: Vec::new(),
            callbacks: None,
        }
    }

    pub fn register(&mut self, plugin: Arc<dyn Plugin>) {
        self.plugins.push(plugin);
    }

    pub fn is_empty(&self) -> bool {
        self.plugins.is_empty() && self.callbacks.is_none()
    }

    pub fn with_callbacks(mut self, callbacks: Arc<CallbackRegistry>) -> Self {
        self.callbacks = Some(callbacks);
        self
    }

    pub fn merged_with(&self, other: &Self) -> Self {
        let mut plugins = self.plugins.clone();
        plugins.extend(other.plugins.iter().cloned());
        Self {
            plugins,
            callbacks: other.callbacks.clone().or_else(|| self.callbacks.clone()),
        }
    }

    pub async fn before_agent_run(&self, ctx: &mut InvocationContext) -> TalosResult<()> {
        for plugin in &self.plugins {
            plugin.before_agent_run(ctx).await?;
        }
        Ok(())
    }

    pub async fn after_agent_run(
        &self,
        ctx: &mut InvocationContext,
        result: &mut AgentRunResult,
    ) -> TalosResult<()> {
        for plugin in &self.plugins {
            plugin.after_agent_run(ctx, result).await?;
        }
        Ok(())
    }

    pub async fn before_run(&self, ctx: &mut InvocationContext) -> TalosResult<()> {
        for plugin in &self.plugins {
            plugin.before_run(ctx).await?;
        }
        Ok(())
    }

    pub async fn on_event(
        &self,
        ctx: &mut InvocationContext,
        event: &mut ExecutionEvent,
    ) -> TalosResult<bool> {
        for plugin in &self.plugins {
            if !plugin.on_event(ctx, event).await? {
                return Ok(false);
            }
        }
        Ok(true)
    }

    pub async fn after_run(&self, ctx: &mut InvocationContext) -> TalosResult<()> {
        for plugin in &self.plugins {
            plugin.after_run(ctx).await?;
        }
        Ok(())
    }

    pub async fn before_flow(
        &self,
        ctx: &mut InvocationContext,
        settings: &KernelExecutionSettings,
    ) -> TalosResult<()> {
        for plugin in &self.plugins {
            plugin.before_flow(ctx, settings).await?;
        }
        Ok(())
    }

    pub async fn after_flow(
        &self,
        ctx: &mut InvocationContext,
        result: &TaskResult,
    ) -> TalosResult<()> {
        for plugin in &self.plugins {
            plugin.after_flow(ctx, result).await?;
        }
        Ok(())
    }

    pub async fn before_model(
        &self,
        ctx: &mut InvocationContext,
        request: &mut LlmRequestEnvelope,
    ) -> TalosResult<CallbackControl> {
        for plugin in &self.plugins {
            if matches!(
                plugin.before_model(ctx, request).await?,
                CallbackControl::Stop
            ) {
                return Ok(CallbackControl::Stop);
            }
        }
        if let Some(callbacks) = &self.callbacks {
            if matches!(
                callbacks.run_before_model(ctx, request).await?,
                CallbackControl::Stop
            ) {
                return Ok(CallbackControl::Stop);
            }
        }
        Ok(CallbackControl::Continue)
    }

    pub async fn after_model(
        &self,
        ctx: &mut InvocationContext,
        response: &mut LlmResponseEnvelope,
    ) -> TalosResult<CallbackControl> {
        for plugin in &self.plugins {
            if matches!(
                plugin.after_model(ctx, response).await?,
                CallbackControl::Stop
            ) {
                return Ok(CallbackControl::Stop);
            }
        }
        if let Some(callbacks) = &self.callbacks {
            if matches!(
                callbacks.run_after_model(ctx, response).await?,
                CallbackControl::Stop
            ) {
                return Ok(CallbackControl::Stop);
            }
        }
        Ok(CallbackControl::Continue)
    }

    pub async fn before_tool(
        &self,
        ctx: &mut InvocationContext,
        payload: &ToolCallbackPayload,
    ) -> TalosResult<CallbackControl> {
        for plugin in &self.plugins {
            if matches!(
                plugin.before_tool(ctx, payload).await?,
                CallbackControl::Stop
            ) {
                return Ok(CallbackControl::Stop);
            }
        }
        if let Some(callbacks) = &self.callbacks {
            if matches!(
                callbacks.run_before_tool(ctx, payload).await?,
                CallbackControl::Stop
            ) {
                return Ok(CallbackControl::Stop);
            }
        }
        Ok(CallbackControl::Continue)
    }

    pub async fn after_tool(
        &self,
        ctx: &mut InvocationContext,
        payload: &ToolCallbackPayload,
    ) -> TalosResult<CallbackControl> {
        for plugin in &self.plugins {
            if matches!(
                plugin.after_tool(ctx, payload).await?,
                CallbackControl::Stop
            ) {
                return Ok(CallbackControl::Stop);
            }
        }
        if let Some(callbacks) = &self.callbacks {
            if matches!(
                callbacks.run_after_tool(ctx, payload).await?,
                CallbackControl::Stop
            ) {
                return Ok(CallbackControl::Stop);
            }
        }
        Ok(CallbackControl::Continue)
    }
}

pub type PluginRegistry = InvocationPluginManager;
