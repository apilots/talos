//! Decode persisted Session records without exposing storage diagnostics to callers.

use super::DatabaseSessionStore;
use crate::session::{SessionRecord, SessionTreeIdentity};
use serde_json::Value;
use talos_error::{TalosError, TalosResult};
use talos_storage::Row;
use talos_types::talos::{SessionSourceProvider, SessionSourceType, TalosSessionEnvironment};
use talos_types::{AgentPath, SessionConf, SessionUsage, DEFAULT_SESSION_TITLE};

impl DatabaseSessionStore {
    pub(super) fn parse_session_record(row: &Row) -> TalosResult<SessionRecord> {
        let message_count = row
            .get_i64("message_count")
            .unwrap_or_default()
            .try_into()
            .unwrap_or_default();
        let id = row.get_string("session_key").unwrap_or_default();
        let usage = Self::parse_required_json::<SessionUsage>(
            row.get_string("usage_json"),
            &id,
            "usage_json",
        )?;
        let source_type = Self::parse_required_enum::<SessionSourceType>(
            row.get_string("source_type"),
            &id,
            "source_type",
        )?;
        let source_provider = Self::parse_required_enum::<SessionSourceProvider>(
            row.get_string("source_provider"),
            &id,
            "source_provider",
        )?;
        let environment = Self::parse_required_json::<Option<TalosSessionEnvironment>>(
            row.get_string("environment_json"),
            &id,
            "environment_json",
        )?;

        let parent_session_id =
            Self::normalized_optional_string(row.get_string("parent_session_id"));
        let parent_turn_id = Self::normalized_optional_string(row.get_string("parent_turn_id"));
        let tree_root_session_id =
            Self::normalized_optional_string(row.get_string("tree_root_session_id"));
        let agent_path = Self::normalized_optional_string(row.get_string("agent_path"))
            .map(|path| {
                path.parse::<AgentPath>().map_err(|error| {
                    let diagnostic = format!("session '{id}' has invalid agent path: {error}");
                    TalosError::internal_with_source(
                        "Session has invalid agent path; technical details were withheld.",
                        error,
                    )
                    .context(diagnostic)
                })
            })
            .transpose()?;
        let tree_identity = match (
            parent_session_id,
            parent_turn_id,
            tree_root_session_id,
            agent_path,
        ) {
            (None, None, None, None) => SessionTreeIdentity::Primary,
            (Some(session_id), Some(turn_id), Some(tree_root_session_id), Some(path)) => {
                SessionTreeIdentity::Subagent {
                    parent: talos_types::AgentTurnRef {
                        session_id,
                        turn_id,
                    },
                    tree_root_session_id,
                    path,
                }
            }
            _ => {
                return Err(
                    TalosError::internal("Session has incomplete subagent tree identity.").context(
                        format!("session '{id}' has incomplete subagent tree identity"),
                    ),
                );
            }
        };
        let conf = Self::parse_required_json::<SessionConf>(
            row.get_string("conf_json"),
            &id,
            "conf_json",
        )?;
        let agent_id =
            Self::normalized_optional_string(row.get_string("agent_id")).ok_or_else(|| {
                TalosError::internal("Session is missing its agent binding.")
                    .context(format!("session '{id}' is missing its agent binding"))
            })?;
        let execution_node_id = Self::normalized_optional_string(
            row.get_string("execution_node_id"),
        )
        .ok_or_else(|| {
            TalosError::internal("Session is missing its execution node binding.").context(format!(
                "session '{id}' is missing its execution node binding"
            ))
        })?;

        let workspace_id = Self::normalized_optional_string(row.get_string("workspace_id"))
            .ok_or_else(|| {
                TalosError::internal("Session is missing its workspace binding.")
                    .context(format!("session '{id}' is missing its workspace binding"))
            })?;

        Ok(SessionRecord {
            id,
            agent_id,
            execution_node_id,
            tree_identity,
            workspace_id,
            title: row
                .get_string("title")
                .filter(|title| !title.trim().is_empty())
                .unwrap_or_else(|| DEFAULT_SESSION_TITLE.to_string()),
            user_id: row.get_string("user_id").unwrap_or_default(),
            source_type,
            source_provider,
            channel_id: Self::normalized_optional_string(row.get_string("channel_id")),
            thread_id: Self::normalized_optional_string(row.get_string("thread_id")),
            environment,
            created_at: row
                .get_i64("created_at")
                .unwrap_or_default()
                .try_into()
                .unwrap_or_default(),
            last_activity: row
                .get_i64("last_activity")
                .unwrap_or_default()
                .try_into()
                .unwrap_or_default(),
            message_count,
            latest_turn_seq: row
                .get_i64("latest_turn_seq")
                .unwrap_or_default()
                .try_into()
                .unwrap_or_default(),
            memory_processed_until_event_seq: row
                .get_i64("memory_processed_until_event_seq")
                .unwrap_or_default()
                .try_into()
                .unwrap_or_default(),
            usage,
            conf,
            project_id: Self::normalized_optional_string(row.get_string("project_id")),
            current_directory: Self::normalized_optional_string(
                row.get_string("current_directory"),
            )
            .map(std::path::PathBuf::from),
        })
    }

    fn parse_required_json<T: serde::de::DeserializeOwned>(
        raw: Option<String>,
        session_id: &str,
        column: &str,
    ) -> TalosResult<T> {
        let raw = raw.ok_or_else(|| {
            TalosError::internal(
                "The stored session is missing a required field; technical details were withheld.",
            )
            .context(format!("session '{session_id}' is missing {column}"))
        })?;
        serde_json::from_str(&raw).map_err(|error| {
            let diagnostic = format!("parse session '{session_id}' {column}: {error}");
            TalosError::internal_with_source(
                "Parse session; technical details were withheld.",
                error,
            )
            .context(diagnostic)
        })
    }

    fn parse_required_enum<T: serde::de::DeserializeOwned>(
        raw: Option<String>,
        session_id: &str,
        column: &str,
    ) -> TalosResult<T> {
        let raw = raw.ok_or_else(|| {
            TalosError::internal(
                "The stored session is missing a required field; technical details were withheld.",
            )
            .context(format!("session '{session_id}' is missing {column}"))
        })?;
        serde_json::from_value(Value::String(raw)).map_err(|error| {
            let diagnostic = format!("parse session '{session_id}' {column}: {error}");
            TalosError::internal_with_source(
                "Parse session; technical details were withheld.",
                error,
            )
            .context(diagnostic)
        })
    }
}
