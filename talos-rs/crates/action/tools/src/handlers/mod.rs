pub mod dynamic;
mod local;
mod mcp;

use async_trait::async_trait;
use talos_error::TalosResult;
use talos_tools_sandbox::RequestedFileSystemAccess;

use crate::invocation::{ToolDispatchContext, ToolInvocation};
use crate::output::ToolDispatchOutcome;
use crate::spec::ToolSource;

pub use dynamic::DynamicToolHandler;
pub use local::LocalToolHandler;
pub use mcp::McpToolHandler;

#[async_trait]
pub trait ToolHandler: Send + Sync {
    fn kind(&self) -> ToolSource;

    fn matches(&self, invocation: &ToolInvocation) -> bool;

    async fn pre_execute(
        &self,
        _invocation: &ToolInvocation,
        _context: &ToolDispatchContext,
    ) -> TalosResult<()> {
        Ok(())
    }

    async fn is_mutating(&self, _invocation: &ToolInvocation) -> bool {
        false
    }

    fn requested_file_system_access(
        &self,
        _invocation: &ToolInvocation,
        _context: &ToolDispatchContext,
    ) -> TalosResult<Vec<RequestedFileSystemAccess>> {
        Ok(Vec::new())
    }

    async fn handle(
        &self,
        invocation: ToolInvocation,
        context: ToolDispatchContext,
    ) -> TalosResult<ToolDispatchOutcome>;
}
