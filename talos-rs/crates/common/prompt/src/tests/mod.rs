use std::process::Command;
use std::sync::Arc;

use async_trait::async_trait;
use chrono::Utc;
use serde_json::Value;
use talos_llm_provider::Role;
use talos_memory::{Memory, MemoryStore, TalosTurn};
use talos_tools::{GrepTool, Tool, ToolContext, ToolOutputPayload, ToolRegistry};
use talos_types::talos::{RunItem, StopReason, TokenUsage};
use talos_types::ModelInfo;
use uuid::Uuid;

use super::*;

struct EchoTool;

#[async_trait]
impl Tool for EchoTool {
    fn name(&self) -> &str {
        "echo"
    }

    fn description(&self) -> &str {
        "Echo input for testing"
    }

    async fn execute_payload(
        &self,
        _input: Value,
        _context: ToolContext,
    ) -> talos_error::TalosResult<ToolOutputPayload> {
        Ok(ToolOutputPayload::success_text("ok"))
    }
}

struct VerboseTool {
    name: &'static str,
    description: &'static str,
}

#[async_trait]
impl Tool for VerboseTool {
    fn name(&self) -> &str {
        self.name
    }

    fn description(&self) -> &str {
        self.description
    }

    async fn execute_payload(
        &self,
        _input: Value,
        _context: ToolContext,
    ) -> talos_error::TalosResult<ToolOutputPayload> {
        Ok(ToolOutputPayload::success_text("ok"))
    }
}

mod bootstrap_tests;
mod builder_tests;
mod continuity_tests;
mod render_cache_tests;
mod session_history_tests;
mod template_tests;
