use talos_error::TalosResult;
use talos_tools_sandbox::ToolSandboxRequest;
use talos_types::UserActionResponse;

use super::ToolRegistry;
use crate::invocation::ToolInvocation;
use crate::spec::{RegisteredToolSpec, ToolSource};
use crate::worker_environment::{InstallApproval, RipgrepRecovery};
use crate::{ToolContext, ToolDispatchOutcome};

pub(super) struct RipgrepDispatch<'a> {
    recovery: &'a RipgrepRecovery,
    request: ToolSandboxRequest,
    approval: Option<InstallApproval>,
}

impl ToolRegistry {
    pub(super) fn prepare_ripgrep_recovery(
        &self,
        invocation: &ToolInvocation,
        context: &ToolContext,
        spec: Option<&RegisteredToolSpec>,
    ) -> Option<RipgrepDispatch<'_>> {
        let spec = spec?;
        if spec.source != ToolSource::Local || spec.internal_name != "grep" {
            return None;
        }
        let recovery = self.worker_environment.ripgrep_recovery()?;
        let call_id = &invocation.call_id;
        let response = context.get_user_action_response(call_id);
        if response.is_none() && self.worker_environment.resolve("rg").is_some() {
            return None;
        }

        let request = recovery.request(invocation, context)?;
        let approval = match response {
            None => None,
            Some(UserActionResponse::SandboxApproval(decision)) => {
                let fingerprint = context.get_sandbox_approval_fingerprint(call_id)?;
                if fingerprint != self.sandbox.approval_request(&request).fingerprint() {
                    return None;
                }
                // Installation approval has its own scope. Do not replay it as
                // permission for the original search.
                context.clear_sandbox_approval(call_id);
                Some(InstallApproval {
                    decision,
                    fingerprint,
                })
            }
            Some(_) => return None,
        };
        Some(RipgrepDispatch {
            recovery,
            request,
            approval,
        })
    }
}

impl RipgrepDispatch<'_> {
    pub(super) async fn recover(
        self,
        invocation: &ToolInvocation,
        context: &ToolContext,
        registry: &ToolRegistry,
    ) -> TalosResult<Option<ToolDispatchOutcome>> {
        self.recovery
            .recover(
                self.request,
                self.approval,
                invocation,
                context,
                &registry.worker_environment,
                &registry.sandbox,
            )
            .await
    }
}
