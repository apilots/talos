use std::time::Duration;

use talos_agent::{
    AgentAdmission, AgentContext, AgentFuture, AgentHarness, AgentInputParams,
    AgentInterruptParams, AgentResult, AgentRunExecution, AgentRunParams, AgentSessionContext,
};
use talos_agent_api::AgentInterruptOutcome;

pub(crate) struct TestHarness;

impl AgentHarness for TestHarness {
    fn start(&self, _params: AgentRunParams) -> AgentFuture<'_, AgentResult<AgentRunExecution>> {
        Box::pin(async { panic!("registry test Harness cannot start a Turn") })
    }

    fn submit_input(
        &self,
        _params: AgentInputParams,
    ) -> AgentFuture<'_, AgentResult<AgentAdmission>> {
        Box::pin(async { panic!("registry test Harness cannot accept input") })
    }

    fn interrupt(
        &self,
        _params: AgentInterruptParams,
    ) -> AgentFuture<'_, AgentResult<AgentInterruptOutcome>> {
        Box::pin(async { panic!("registry test Harness cannot interrupt a Turn") })
    }

    fn compact_session(
        &self,
        _params: AgentContext,
    ) -> AgentFuture<'_, AgentResult<AgentRunExecution>> {
        Box::pin(async { panic!("registry test Harness cannot compact a Session") })
    }

    fn clear_session(
        &self,
        _params: AgentSessionContext,
        _expected_history_cursor: talos_types::SessionHistoryCursor,
    ) -> AgentFuture<'_, AgentResult<()>> {
        Box::pin(async { panic!("registry test Harness cannot clear a Session") })
    }

    fn close_session(
        &self,
        _params: AgentSessionContext,
        _expected_history_cursor: talos_types::SessionHistoryCursor,
    ) -> AgentFuture<'_, AgentResult<()>> {
        Box::pin(async { panic!("registry test Harness cannot close a Session") })
    }

    fn shutdown(&self, _timeout: Duration) -> AgentFuture<'_, AgentResult<()>> {
        Box::pin(async { Ok(()) })
    }
}
