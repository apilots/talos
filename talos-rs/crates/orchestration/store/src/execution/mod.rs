//! Node-owned execution persistence. Product Session aggregates and Timeline
//! notices remain in their original product Store and database.

use std::path::Path;

use agent_protocol::{AgentSessionContext, AgentSessionEventCursor};
use serde::{de::DeserializeOwned, Serialize};
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::{Sqlite, SqlitePool, Transaction};
use talos_error::{ErrorCode, TalosError, TalosResult};

mod dream;
mod history;
mod read;
pub(crate) mod supplement_merge;
mod write;

static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./src/execution/migrations/sqlite");

/// One Worker's durable facts, shared by all of that Worker's ingress adapters.
/// Opening this Store never opens or initializes the product Session database.
#[derive(Clone)]
pub struct WorkerExecutionStore {
    pool: SqlitePool,
}

impl WorkerExecutionStore {
    pub async fn open(path: &Path) -> TalosResult<Self> {
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await.map_err(|error| {
                TalosError::internal_with_source("Create Worker execution directory.", error)
            })?;
        }
        let pool = SqlitePoolOptions::new()
            .max_connections(4)
            .connect_with(
                SqliteConnectOptions::new()
                    .filename(path)
                    .create_if_missing(true)
                    .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal)
                    .foreign_keys(true)
                    .busy_timeout(std::time::Duration::from_secs(10)),
            )
            .await
            .map_err(database_error)?;
        MIGRATOR.run(&pool).await.map_err(|error| {
            TalosError::internal_with_source("Initialize Worker execution storage.", error)
        })?;
        Ok(Self { pool })
    }

    async fn binding(
        tx: &mut Transaction<'_, Sqlite>,
        session: &AgentSessionContext,
    ) -> TalosResult<(u64, Option<String>)> {
        let row: Option<(String, i64, Option<String>)> = sqlx::query_as(
            "SELECT binding_json, generation, active_turn_id FROM sessions WHERE session_id = ?",
        )
        .bind(&session.session_id)
        .fetch_optional(&mut **tx)
        .await
        .map_err(database_error)?;
        let (binding, generation, active) = row.ok_or_else(|| {
            TalosError::new(
                ErrorCode::SessionNotFound,
                "Worker execution Session does not exist.",
            )
        })?;
        if decode::<AgentSessionContext>(&binding)? != *session {
            return Err(TalosError::new(
                ErrorCode::PermissionDenied,
                "Worker Session binding does not match the authorized scope.",
            ));
        }
        Ok((generation as u64, active))
    }
}

fn encode<T: Serialize>(value: &T) -> TalosResult<String> {
    serde_json::to_string(value)
        .map_err(|error| TalosError::internal_with_source("Encode Worker execution state.", error))
}
fn decode<T: DeserializeOwned>(value: &str) -> TalosResult<T> {
    serde_json::from_str(value)
        .map_err(|error| TalosError::internal_with_source("Decode Worker execution state.", error))
}
fn database_error(error: sqlx::Error) -> TalosError {
    TalosError::internal_with_source("Access Worker execution storage.", error)
}
fn cursor(session: &AgentSessionContext, generation: u64, offset: i64) -> AgentSessionEventCursor {
    AgentSessionEventCursor {
        session_id: session.session_id.clone(),
        generation,
        offset,
    }
}
fn conflict() -> TalosError {
    TalosError::new(
        ErrorCode::Conflict,
        "Worker execution state changed before this operation.",
    )
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;

#[derive(serde::Serialize)]
#[serde(tag = "type")]
enum ExecutionCommandKey<'a> {
    Product {
        command: talos_types::IdempotentCommand,
        key: &'a str,
    },
    Input {
        message_id: &'a str,
    },
    Compaction {
        turn_id: &'a str,
    },
}
fn command_key(command: &agent_protocol::AgentExecutionCommand) -> TalosResult<String> {
    use agent_protocol::AgentExecutionCommand;
    let (identity, input) = match command {
        AgentExecutionCommand::Run { params } => {
            (params.command.as_ref(), &params.input.message_id)
        }
        AgentExecutionCommand::Input { params } => {
            (params.command.as_ref(), &params.input.message_id)
        }
        AgentExecutionCommand::Compaction { context } => {
            return encode(&ExecutionCommandKey::Compaction {
                turn_id: &context.turn_id,
            })
        }
    };
    match identity {
        Some(identity) => encode(&ExecutionCommandKey::Product {
            command: identity.command,
            key: &identity.idempotency_key,
        }),
        None => encode(&ExecutionCommandKey::Input { message_id: input }),
    }
}

fn command_metadata(
    command: &agent_protocol::AgentExecutionCommand,
    event: &talos_agent_api::AgentEvent,
) -> (
    Option<agent_protocol::AgentCommandIdentity>,
    Option<talos_types::AutomationInputOrigin>,
    Option<talos_types::ModelRef>,
) {
    let (message_id, identity, origin, model) = match command {
        agent_protocol::AgentExecutionCommand::Run { params } => (
            &params.input.message_id,
            params.command.clone(),
            params
                .automation_origin
                .clone()
                .map(|origin| talos_types::AutomationInputOrigin {
                    message_id: params.input.message_id.clone(),
                    origin,
                }),
            Some(params.context.model_selection.model_ref.clone()),
        ),
        agent_protocol::AgentExecutionCommand::Input { params } => {
            (&params.input.message_id, params.command.clone(), None, None)
        }
        agent_protocol::AgentExecutionCommand::Compaction { context } => {
            return (
                None,
                None,
                matches!(
                    event.payload,
                    talos_agent_api::AgentEventPayload::TurnStarted(_)
                )
                .then(|| context.model_selection.model_ref.clone()),
            )
        }
    };
    if matches!(&event.payload, talos_agent_api::AgentEventPayload::MessageCompleted(message) if &message.message_id == message_id)
    {
        (identity, origin, model)
    } else {
        (None, None, None)
    }
}

fn command_purpose(
    command: &agent_protocol::AgentExecutionCommand,
) -> agent_protocol::AgentExecutionPurpose {
    match command {
        agent_protocol::AgentExecutionCommand::Compaction { .. } => {
            agent_protocol::AgentExecutionPurpose::Compaction
        }
        _ => agent_protocol::AgentExecutionPurpose::Turn,
    }
}
