//! Read/manage existing callbacks through their authenticated execution-instance API.
use crate::TuiApp;
use talos_error::{ErrorCode, TalosError, TalosResult};
use talos_sdk::*;
impl TuiApp {
    pub(crate) async fn show_automation_management(
        &mut self,
        computer: Option<&str>,
    ) -> TalosResult<()> {
        self.show_automation_scope(AutomationListScope::Unfinished, computer)
            .await
    }
    pub(crate) async fn show_automation_scope(
        &mut self,
        scope: AutomationListScope,
        computer: Option<&str>,
    ) -> TalosResult<()> {
        let client = self
            .gateway_client
            .clone()
            .ok_or_else(|| TalosError::new(ErrorCode::ServiceUnavailable, "Gateway unavailable"))?;
        let entries = client
            .list_automation(AutomationListRequest {
                session_id: None,
                scope,
                cursor: None,
                limit: Some(50),
            })
            .await?;
        let mut lines = vec!["Automation · existing Session tasks".to_string()];
        for entry in entries.data {
            lines.push(match entry {
                AutomationListEntry::Goal { goal } => format!(
                    "Goal {} · Session {} · {:?} · {}{}",
                    goal.id,
                    goal.session_id,
                    goal.status,
                    goal.objective,
                    goal.pause_reason
                        .map(|reason| format!(" · {reason:?}"))
                        .unwrap_or_default()
                ),
                AutomationListEntry::Loop { loop_task: l } => {
                    format!(
                        "Loop {} · Session {} · {:?} · {}{}",
                        l.id,
                        l.session_id,
                        l.status,
                        l.prompt,
                        l.stop_reason
                            .map(|reason| format!(" · {reason:?}"))
                            .unwrap_or_default()
                    )
                }
            });
        }
        if entries.next_cursor.is_some() {
            lines.push("More tasks are available through talos automation list --cursor.".into());
        }
        if let Some(computer) = computer {
            let jobs = client
                .list_builtin_jobs(BuiltinJobListRequest {
                    computer_id: computer.into(),
                })
                .await?;
            for job in jobs.data {
                lines.push(format!("{} · {} · instance {} · {} · paused={} running={} · /jobs {} pause|resume|run-once {}",job.id.as_str(),job.name,job.execution_instance_id,job.schedule,job.paused,job.running,job.computer_id,job.id.as_str()));
            }
        } else {
            lines.push("Native Jobs: /jobs <computer> list. Commands are scoped to the actual owner instance.".into());
        }
        self.push_system_notice(lines.join("\n"));
        Ok(())
    }
    pub(crate) async fn manage_builtin_job(&mut self, args: &str) -> TalosResult<()> {
        let mut parts = args.split_whitespace();
        let computer = parts.next().ok_or_else(|| {
            TalosError::new(
                ErrorCode::InvalidRequest,
                "Usage: /jobs <computer> list|pause|resume|run-once [job-id]",
            )
        })?;
        let operation = parts.next().unwrap_or("list");
        if operation == "list" {
            return self.show_automation_management(Some(computer)).await;
        }
        let id = parts.next().ok_or_else(|| {
            TalosError::new(
                ErrorCode::InvalidRequest,
                "Specify a Job ID from /jobs <computer> list",
            )
        })?;
        if parts.next().is_some() {
            return Err(TalosError::new(
                ErrorCode::InvalidRequest,
                "Unexpected Job command arguments",
            ));
        }
        let action = match operation {
            "pause" => BuiltinJobAction::Pause,
            "resume" => BuiltinJobAction::Resume,
            "run-once" => BuiltinJobAction::RunOnce,
            _ => {
                return Err(TalosError::new(
                    ErrorCode::InvalidRequest,
                    "Unknown Job action",
                ))
            }
        };
        let client = self
            .gateway_client
            .clone()
            .ok_or_else(|| TalosError::new(ErrorCode::ServiceUnavailable, "Gateway unavailable"))?;
        let job = client
            .list_builtin_jobs(BuiltinJobListRequest {
                computer_id: computer.into(),
            })
            .await?
            .data
            .into_iter()
            .find(|job| job.id.as_str() == id)
            .ok_or_else(|| {
                TalosError::new(
                    ErrorCode::ResourceNotFound,
                    "Job was not returned for this Computer",
                )
            })?;
        let result = client
            .control_builtin_job(BuiltinJobControlRequest {
                computer_id: job.computer_id,
                execution_instance_id: job.execution_instance_id,
                job_id: job.id,
                action,
            })
            .await?;
        self.push_system_notice(format!(
            "{} · paused={} running={}",
            result.name, result.paused, result.running
        ));
        Ok(())
    }
}
