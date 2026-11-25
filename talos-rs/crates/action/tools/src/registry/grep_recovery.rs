use talos_error::TalosResult;
use talos_tools_sandbox::ToolSandboxRequest;
use talos_types::{SandboxApprovalDecision, UserActionResponse};

use super::ToolRegistry;
use crate::invocation::ToolInvocation;
use crate::spec::{RegisteredToolSpec, ToolSource};
use crate::worker_environment::RipgrepRecovery;
use crate::{ToolContext, ToolDispatchOutcome};

pub(super) struct RipgrepDispatch<'a> {
    recovery: &'a RipgrepRecovery,
    request: ToolSandboxRequest,
    approval: Option<SandboxApprovalDecision>,
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
        if response.is_none()
            && (context.is_parallel_execution()
                || context.tool_search_path().resolve("rg").is_some())
        {
            return None;
        }

        let mut request = recovery.request(invocation, context)?;
        let approval = match response {
            None if recovery.installer_available(&self.sandbox) => None,
            None => return None,
            Some(UserActionResponse::SandboxApproval(decision)) => {
                let fingerprint = context.get_sandbox_approval_fingerprint(call_id)?;
                let approval = self.sandbox.approval_request(&request);
                if decision == SandboxApprovalDecision::AllowOnce {
                    request.approval_grant = Some(approval.allow_once(&fingerprint).ok()?);
                } else if fingerprint != approval.fingerprint() {
                    return None;
                }
                // Installation approval has its own scope. Do not replay it as
                // permission for the original search.
                context.clear_sandbox_approval(call_id);
                Some(decision)
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
                &registry.sandbox,
            )
            .await
    }
}
