use crate::{AgentRunResult, InvocationContext, Plugin, TaskPlan, TaskResult, TaskSpec};
use std::sync::Arc;
use talos_error::TalosResult;

#[derive(Default, Clone)]
pub struct PluginRegistry {
    plugins: Vec<Arc<dyn Plugin>>,
}

impl PluginRegistry {
    pub fn new() -> Self {
        Self {
            plugins: Vec::new(),
        }
    }

    pub fn register(&mut self, plugin: Arc<dyn Plugin>) {
        self.plugins.push(plugin);
    }

    pub fn is_empty(&self) -> bool {
        self.plugins.is_empty()
    }

    pub async fn before_plan(&self, ctx: &mut InvocationContext) -> TalosResult<()> {
        for plugin in &self.plugins {
            plugin.before_plan(ctx).await?;
        }
        Ok(())
    }

    pub async fn after_plan(
        &self,
        ctx: &mut InvocationContext,
        plan: &TaskPlan,
    ) -> TalosResult<()> {
        for plugin in &self.plugins {
            plugin.after_plan(ctx, plan).await?;
        }
        Ok(())
    }

    pub async fn before_task(
        &self,
        ctx: &mut InvocationContext,
        task: &TaskSpec,
    ) -> TalosResult<()> {
        for plugin in &self.plugins {
            plugin.before_task(ctx, task).await?;
        }
        Ok(())
    }

    pub async fn after_task(
        &self,
        ctx: &mut InvocationContext,
        result: &TaskResult,
    ) -> TalosResult<()> {
        for plugin in &self.plugins {
            plugin.after_task(ctx, result).await?;
        }
        Ok(())
    }

    pub async fn after_finalize(
        &self,
        ctx: &mut InvocationContext,
        result: &mut AgentRunResult,
    ) -> TalosResult<()> {
        for plugin in &self.plugins {
            plugin.after_finalize(ctx, result).await?;
        }
        Ok(())
    }
}
