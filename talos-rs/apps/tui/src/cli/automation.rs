//! Named terminal commands are adapters of the shared SDK contract.
use super::types::*;
use std::sync::Arc;
use talos_error::{ErrorCode, TalosError, TalosResult};
use talos_sdk::*;
use talos_tui::GatewayClient;

pub(super) struct CommandOutput {
    pub(super) value: serde_json::Value,
    pub(super) foreground: Option<(String, AutomationObjectRef)>,
}
impl CommandOutput {
    fn read(value: serde_json::Value) -> Self {
        Self {
            value,
            foreground: None,
        }
    }
}
fn encode(value: impl serde::Serialize) -> TalosResult<serde_json::Value> {
    serde_json::to_value(value)
        .map_err(|error| TalosError::internal_with_source("Encode automation response", error))
}
pub(super) fn created_target(result: &AutomationControlResult) -> TalosResult<AutomationObjectRef> {
    result.created_object.clone().ok_or_else(|| {
        TalosError::new(
            ErrorCode::ServiceUnavailable,
            "The creation receipt is unavailable; inspect this Session before retrying.",
        )
    })
}
fn invalid(message: &str) -> TalosError {
    TalosError::new(ErrorCode::InvalidRequest, message)
}
pub(super) fn loop_schedule(every: Option<u32>, dynamic: bool) -> TalosResult<LoopSchedule> {
    if dynamic && every.is_some() {
        return Err(invalid("Choose either --every or --dynamic."));
    }
    let schedule = match every {
        Some(minutes) => LoopSchedule::FixedInterval { minutes },
        None => LoopSchedule::ModelDecided {},
    };
    schedule.validate()?;
    Ok(schedule)
}
pub(super) fn loop_create(
    prompt: String,
    every: Option<u32>,
    dynamic: bool,
) -> TalosResult<LoopAction> {
    Ok(LoopAction::Create {
        prompt,
        schedule: loop_schedule(every, dynamic)?,
    })
}
async fn snapshot(client: &dyn GatewayClient, id: &str) -> TalosResult<SessionAutomationSnapshot> {
    client
        .session_automation(SessionAutomationReadRequest {
            session_id: id.into(),
        })
        .await
}
async fn apply(
    client: &dyn GatewayClient,
    session_id: String,
    action: AutomationAction,
) -> TalosResult<AutomationControlResult> {
    client
        .execute_automation(AutomationRequest { session_id, action })
        .await
}
pub(super) async fn goal(
    client: Arc<dyn GatewayClient>,
    command: GoalCmd,
) -> TalosResult<CommandOutput> {
    let session = command.session;
    let foreground = matches!(&command.command, Some(GoalCommands::Start { .. }));
    client.get_session(&session).await?;
    let action = match command.command.unwrap_or(GoalCommands::Show) {
        GoalCommands::Show => {
            return Ok(CommandOutput::read(encode(
                snapshot(client.as_ref(), &session).await?,
            )?));
        }
        GoalCommands::Start {
            prompt,
            token_budget,
        } => GoalAction::Create {
            objective: prompt.join(" "),
            token_budget,
        },
        GoalCommands::Edit { prompt } => {
            let g = snapshot(client.as_ref(), &session)
                .await?
                .goal
                .ok_or_else(|| invalid("No current Goal"))?;
            GoalAction::Edit {
                goal_id: g.id,
                expected_revision: g.objective_revision,
                objective: prompt.join(" "),
            }
        }
        operation => {
            let explicit = match &operation {
                GoalCommands::Clear { id } => id.clone(),
                _ => None,
            };
            let goal_id = match explicit {
                Some(id) => id,
                None => {
                    snapshot(client.as_ref(), &session)
                        .await?
                        .goal
                        .ok_or_else(|| invalid("No current Goal"))?
                        .id
                }
            };
            match operation {
                GoalCommands::Pause => GoalAction::Pause { goal_id },
                GoalCommands::Resume => GoalAction::Resume { goal_id },
                GoalCommands::Clear { .. } => GoalAction::Clear { goal_id },
                _ => unreachable!(),
            }
        }
    };
    let resume_reference = match &action {
        GoalAction::Resume { goal_id } => Some(AutomationObjectRef::Goal {
            goal_id: goal_id.clone(),
        }),
        _ => None,
    };
    let result = apply(
        client.as_ref(),
        session.clone(),
        AutomationAction::Goal { action },
    )
    .await?;
    let reference = if foreground {
        Some(created_target(&result)?)
    } else {
        resume_reference
    };
    let foreground = reference.map(|object| (session, object));
    Ok(CommandOutput {
        value: encode(result)?,
        foreground,
    })
}
pub(super) async fn loop_task(
    client: Arc<dyn GatewayClient>,
    command: LoopCmd,
) -> TalosResult<CommandOutput> {
    let session = command.session;
    let foreground = matches!(&command.command, Some(LoopCommands::Start { .. }));
    client.get_session(&session).await?;
    let action = match command.command.unwrap_or(LoopCommands::List) {
        LoopCommands::List => {
            return Ok(CommandOutput::read(encode(
                snapshot(client.as_ref(), &session).await?,
            )?));
        }
        LoopCommands::Start {
            prompt,
            every,
            dynamic,
        } => loop_create(prompt.join(" "), every, dynamic)?,
        LoopCommands::Edit {
            id,
            prompt,
            every,
            dynamic,
        } => {
            let task = snapshot(client.as_ref(), &session)
                .await?
                .loops
                .into_iter()
                .find(|task| task.id == id)
                .ok_or_else(|| invalid("Active Loop not found"))?;
            let prompt = if prompt.is_empty() {
                task.prompt.clone()
            } else {
                prompt.join(" ")
            };
            let schedule = if dynamic || every.is_some() {
                loop_schedule(every, dynamic)?
            } else {
                task.schedule
            };
            LoopAction::Edit {
                loop_id: id,
                expected_revision: task.config_revision,
                prompt,
                schedule,
            }
        }
        LoopCommands::Stop { id } => LoopAction::Stop { loop_id: id },
        LoopCommands::Clear { id } => LoopAction::Clear { loop_id: id },
    };
    let result = apply(
        client.as_ref(),
        session.clone(),
        AutomationAction::Loop { action },
    )
    .await?;
    let reference = if foreground {
        Some(created_target(&result)?)
    } else {
        None
    };
    let foreground = reference.map(|object| (session, object));
    Ok(CommandOutput {
        value: encode(result)?,
        foreground,
    })
}
pub(super) async fn management(
    client: Arc<dyn GatewayClient>,
    command: AutomationCmd,
) -> TalosResult<serde_json::Value> {
    match command.command {
        AutomationCommands::List {
            session,
            ended,
            cursor,
        } => serde_json::to_value(
            client
                .list_automation(AutomationListRequest {
                    session_id: session,
                    scope: if ended {
                        AutomationListScope::Ended
                    } else {
                        AutomationListScope::Unfinished
                    },
                    cursor,
                    limit: Some(50),
                })
                .await?,
        )
        .map_err(|e| TalosError::internal_with_source("Encode Automation", e)),
        AutomationCommands::Jobs { computer, command } => {
            let jobs = client
                .list_builtin_jobs(BuiltinJobListRequest {
                    computer_id: computer,
                })
                .await?;
            let (id, action) = match command.unwrap_or(JobCommands::List) {
                JobCommands::List => {
                    return serde_json::to_value(jobs)
                        .map_err(|e| TalosError::internal_with_source("Encode Jobs", e))
                }
                JobCommands::Pause { id } => (id, BuiltinJobAction::Pause),
                JobCommands::Resume { id } => (id, BuiltinJobAction::Resume),
                JobCommands::RunOnce { id } => (id, BuiltinJobAction::RunOnce),
            };
            let job = jobs
                .data
                .into_iter()
                .find(|job| job.id.as_str() == id)
                .ok_or_else(|| invalid("Job was not returned for this Computer"))?;
            let result = client
                .control_builtin_job(BuiltinJobControlRequest {
                    computer_id: job.computer_id,
                    execution_instance_id: job.execution_instance_id,
                    job_id: job.id,
                    action,
                })
                .await?;
            serde_json::to_value(result)
                .map_err(|e| TalosError::internal_with_source("Encode Job", e))
        }
    }
}
