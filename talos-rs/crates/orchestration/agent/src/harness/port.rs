use std::time::Duration;

use talos_agent_api::AgentInterruptOutcome;

use super::Harness;
use crate::{
    AgentAdmission, AgentContext, AgentFuture, AgentHarness, AgentInputParams,
    AgentInterruptParams, AgentProvider, AgentResult, AgentRunExecution, AgentRunParams,
    AgentSessionContext,
};

impl<P> AgentHarness for Harness<P>
where
    P: AgentProvider,
{
    fn start(&self, params: AgentRunParams) -> AgentFuture<'_, AgentResult<AgentRunExecution>> {
        Box::pin(Harness::start(self, params))
    }

    fn submit_input(
        &self,
        params: AgentInputParams,
    ) -> AgentFuture<'_, AgentResult<AgentAdmission>> {
        Box::pin(Harness::submit_input(self, params))
    }

    fn interrupt(
        &self,
        params: AgentInterruptParams,
    ) -> AgentFuture<'_, AgentResult<AgentInterruptOutcome>> {
        Box::pin(Harness::interrupt(self, params))
    }

    fn compact_session(
        &self,
        context: AgentContext,
    ) -> AgentFuture<'_, AgentResult<AgentRunExecution>> {
        Box::pin(Harness::compact_session(self, context))
    }

    fn clear_session(&self, params: AgentSessionContext) -> AgentFuture<'_, AgentResult<()>> {
        Box::pin(Harness::clear_session(self, params))
    }

    fn close_session(&self, params: AgentSessionContext) -> AgentFuture<'_, AgentResult<()>> {
        Box::pin(Harness::close_session(self, params))
    }

    fn shutdown(&self, timeout: Duration) -> AgentFuture<'_, AgentResult<()>> {
        Box::pin(Harness::shutdown(self, timeout))
    }
}
