use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use talos_error::{TalosError, TalosResult};
use talos_memory::{
    OptionalField, UserWorkspaceMemoryEntry, UserWorkspaceMemoryEntryUpdate,
    UserWorkspaceMemorySection, UserWorkspaceMemorySnapshot, UserWorkspaceMemoryUpdate,
};
use talos_types::{ExpectedFileVersion, MemoryKind};

use crate::spec::{ToolBehaviorFlags, ToolResultPolicy, ToolVisibility};
use crate::{Tool, ToolContext, ToolOutputPayload};

/// Model-facing mutation boundary for durable User Workspace Memory.
pub struct UserWorkspaceMemoryUpdateTool;

#[async_trait]
impl Tool for UserWorkspaceMemoryUpdateTool {
    fn name(&self) -> &str {
        "UserWorkspaceMemoryUpdate"
    }

    fn internal_name(&self) -> &str {
        "user_workspace_memory_update"
    }

    fn aliases(&self) -> &'static [&'static str] {
        &["user_workspace_memory_update"]
    }

    fn description(&self) -> &str {
        "Create, update, replace, or remove durable cross-session memory for the current agent and user. For update_entry, provide changed fields directly and use clear_fields only to clear optional fields."
    }

    fn visibility(&self) -> ToolVisibility {
        ToolVisibility::AlwaysLoad
    }

    fn behavior_flags(&self) -> ToolBehaviorFlags {
        ToolBehaviorFlags {
            read_only: false,
            destructive: true,
            concurrency_safe: false,
            requires_user_interaction: false,
        }
    }

    fn result_policy(&self) -> ToolResultPolicy {
        ToolResultPolicy {
            max_inline_bytes: 4 * 1024,
            persist_large_output: false,
            preview_lines: 20,
        }
    }

    fn parameters_schema(&self) -> Option<Value> {
        let entry = entry_schema();
        Some(serde_json::json!({
            "type": "object",
            "oneOf": [
                {
                    "type": "object",
                    "required": ["operation", "entry"],
                    "properties": {
                        "operation": {"const": "create_entry"},
                        "entry": entry.clone()
                    },
                    "additionalProperties": false
                },
                {
                    "type": "object",
                    "required": ["operation", "key", "update"],
                    "properties": {
                        "operation": {"const": "update_entry"},
                        "key": key_schema(),
                        "update": entry_update_schema()
                    },
                    "additionalProperties": false
                },
                {
                    "type": "object",
                    "required": ["operation", "entry"],
                    "properties": {
                        "operation": {"const": "upsert_entry"},
                        "entry": entry.clone()
                    },
                    "additionalProperties": false
                },
                {
                    "type": "object",
                    "required": ["operation", "entry"],
                    "properties": {
                        "operation": {"const": "replace_entry"},
                        "entry": entry
                    },
                    "additionalProperties": false
                },
                {
                    "type": "object",
                    "required": ["operation", "key"],
                    "properties": {
                        "operation": {"const": "remove_entry"},
                        "key": key_schema()
                    },
                    "additionalProperties": false
                },
                {
                    "type": "object",
                    "required": ["operation", "content", "base_version"],
                    "properties": {
                        "operation": {"const": "replace_document"},
                        "content": {"type": "string", "maxLength": 8000},
                        "base_version": expected_version_schema()
                    },
                    "additionalProperties": false
                }
            ]
        }))
    }

    fn validate_input(&self, input: &Value) -> TalosResult<()> {
        serde_json::from_value::<ToolUserWorkspaceMemoryUpdate>(input.clone())
            .and_then(ToolUserWorkspaceMemoryUpdate::into_domain)
            .map(|_| ())
            .map_err(|error| {
                TalosError::new(talos_error::ErrorCode::InvalidRequest, error.to_string())
            })
    }

    async fn execute_payload(
        &self,
        input: Value,
        context: ToolContext,
    ) -> TalosResult<ToolOutputPayload> {
        let writer = context.user_workspace_memory_writer().ok_or_else(|| {
            TalosError::new(
                talos_error::ErrorCode::ToolExecutionFailed,
                "UserWorkspaceMemoryUpdate requires memory write access".to_string(),
            )
        })?;
        let scope = context.memory_scope().ok_or_else(|| {
            TalosError::new(
                talos_error::ErrorCode::ToolExecutionFailed,
                "UserWorkspaceMemoryUpdate requires a trusted memory scope".to_string(),
            )
        })?;
        let update = serde_json::from_value::<ToolUserWorkspaceMemoryUpdate>(input)
            .and_then(ToolUserWorkspaceMemoryUpdate::into_domain)
            .map_err(|error| {
                TalosError::new(talos_error::ErrorCode::InvalidRequest, error.to_string())
            })?;
        let snapshot = match update {
            UserWorkspaceMemoryUpdate::CreateEntry { entry } => {
                writer.create_entry(scope, entry).await?
            }
            UserWorkspaceMemoryUpdate::UpdateEntry { key, update } => {
                writer.update_entry(scope, &key, update).await?
            }
            UserWorkspaceMemoryUpdate::UpsertEntry { entry } => {
                writer.upsert_entry(scope, entry).await?
            }
            UserWorkspaceMemoryUpdate::ReplaceEntry { entry } => {
                writer.replace_entry(scope, entry).await?
            }
            UserWorkspaceMemoryUpdate::RemoveEntry { key } => {
                writer.remove_entry(scope, &key).await?
            }
            UserWorkspaceMemoryUpdate::ReplaceDocument {
                content,
                base_version,
            } => {
                writer
                    .replace_document(scope, content, base_version)
                    .await?
            }
        };
        let state = match &snapshot {
            UserWorkspaceMemorySnapshot::Missing { .. } => ToolMemoryFileState::Missing,
            UserWorkspaceMemorySnapshot::Valid { .. } => ToolMemoryFileState::Valid,
            UserWorkspaceMemorySnapshot::Invalid { .. } => ToolMemoryFileState::Invalid,
        };
        ToolOutputPayload::success_json(
            &ToolUserWorkspaceMemoryUpdateResult {
                state,
                version_token: snapshot.version_token().map(str::to_string),
                character_count: snapshot.raw_content().chars().count(),
            },
            "serialize User Workspace Memory update result",
        )
    }
}

fn key_schema() -> Value {
    serde_json::json!({
        "type": "string",
        "pattern": "^[a-z0-9][a-z0-9_-]{0,63}$"
    })
}

fn entry_schema() -> Value {
    serde_json::json!({
        "type": "object",
        "required": ["key", "section", "title", "kind", "summary"],
        "properties": {
            "key": key_schema(),
            "section": {"enum": ["interaction_guidance", "key_knowledge", "active_context"]},
            "title": {"type": "string", "maxLength": 160},
            "kind": {"enum": ["preference", "fact", "decision", "task", "episode"]},
            "summary": {"type": "string", "maxLength": 2000},
            "why": {"type": ["string", "null"], "maxLength": 2000},
            "apply": {"type": ["string", "null"], "maxLength": 2000},
            "status": {"type": ["string", "null"], "maxLength": 512},
            "next": {"type": ["string", "null"], "maxLength": 2000}
        },
        "additionalProperties": false
    })
}

fn entry_update_schema() -> Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "section": {"enum": ["interaction_guidance", "key_knowledge", "active_context"]},
            "title": {"type": "string", "maxLength": 160},
            "kind": {"enum": ["preference", "fact", "decision", "task", "episode"]},
            "summary": {"type": "string", "maxLength": 2000},
            "why": {"type": "string", "maxLength": 2000},
            "apply": {"type": "string", "maxLength": 2000},
            "status": {"type": "string", "maxLength": 512},
            "next": {"type": "string", "maxLength": 2000},
            "clear_fields": {
                "type": "array",
                "items": {"enum": ["why", "apply", "status", "next"]},
                "uniqueItems": true,
                "description": "Optional fields to clear. Omitted fields otherwise remain unchanged."
            }
        },
        "additionalProperties": false
    })
}

fn expected_version_schema() -> Value {
    serde_json::json!({
        "oneOf": [
            {
                "type": "object",
                "required": ["mode"],
                "properties": {"mode": {"const": "must_not_exist"}},
                "additionalProperties": false
            },
            {
                "type": "object",
                "required": ["mode", "version_token"],
                "properties": {
                    "mode": {"const": "match"},
                    "version_token": {"type": "string"}
                },
                "additionalProperties": false
            }
        ]
    })
}

#[derive(Debug, Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
enum ToolUserWorkspaceMemoryUpdate {
    CreateEntry {
        entry: UserWorkspaceMemoryEntry,
    },
    UpdateEntry {
        key: String,
        update: ToolUserWorkspaceMemoryEntryUpdate,
    },
    UpsertEntry {
        entry: UserWorkspaceMemoryEntry,
    },
    ReplaceEntry {
        entry: UserWorkspaceMemoryEntry,
    },
    RemoveEntry {
        key: String,
    },
    ReplaceDocument {
        content: String,
        base_version: ExpectedFileVersion,
    },
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct ToolUserWorkspaceMemoryEntryUpdate {
    section: Option<UserWorkspaceMemorySection>,
    title: Option<String>,
    kind: Option<MemoryKind>,
    summary: Option<String>,
    why: Option<String>,
    apply: Option<String>,
    status: Option<String>,
    next: Option<String>,
    #[serde(default)]
    clear_fields: Vec<ToolOptionalField>,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum ToolOptionalField {
    Why,
    Apply,
    Status,
    Next,
}

impl From<ToolUserWorkspaceMemoryUpdate> for UserWorkspaceMemoryUpdate {
    fn from(value: ToolUserWorkspaceMemoryUpdate) -> Self {
        match value {
            ToolUserWorkspaceMemoryUpdate::CreateEntry { entry } => Self::CreateEntry { entry },
            ToolUserWorkspaceMemoryUpdate::UpdateEntry { key, update } => Self::UpdateEntry {
                key,
                update: update.into(),
            },
            ToolUserWorkspaceMemoryUpdate::UpsertEntry { entry } => Self::UpsertEntry { entry },
            ToolUserWorkspaceMemoryUpdate::ReplaceEntry { entry } => Self::ReplaceEntry { entry },
            ToolUserWorkspaceMemoryUpdate::RemoveEntry { key } => Self::RemoveEntry { key },
            ToolUserWorkspaceMemoryUpdate::ReplaceDocument {
                content,
                base_version,
            } => Self::ReplaceDocument {
                content,
                base_version,
            },
        }
    }
}

impl ToolUserWorkspaceMemoryUpdate {
    fn into_domain(self) -> Result<UserWorkspaceMemoryUpdate, serde_json::Error> {
        if let Self::UpdateEntry { update, .. } = &self {
            for (value_is_set, field) in [
                (update.why.is_some(), ToolOptionalField::Why),
                (update.apply.is_some(), ToolOptionalField::Apply),
                (update.status.is_some(), ToolOptionalField::Status),
                (update.next.is_some(), ToolOptionalField::Next),
            ] {
                if value_is_set && update.clear_fields.contains(&field) {
                    return Err(<serde_json::Error as serde::de::Error>::custom(
                        "an optional field cannot be set and cleared in the same update",
                    ));
                }
            }
        }
        let update = UserWorkspaceMemoryUpdate::from(self);
        update
            .validate()
            .map_err(|error| <serde_json::Error as serde::de::Error>::custom(error.to_string()))?;
        Ok(update)
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ToolUserWorkspaceMemoryUpdateResult {
    state: ToolMemoryFileState,
    version_token: Option<String>,
    character_count: usize,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "snake_case")]
enum ToolMemoryFileState {
    Missing,
    Valid,
    Invalid,
}

impl From<ToolUserWorkspaceMemoryEntryUpdate> for UserWorkspaceMemoryEntryUpdate {
    fn from(value: ToolUserWorkspaceMemoryEntryUpdate) -> Self {
        Self {
            section: value.section,
            title: value.title,
            kind: value.kind,
            summary: value.summary,
            why: optional_update(value.why, &value.clear_fields, ToolOptionalField::Why),
            apply: optional_update(value.apply, &value.clear_fields, ToolOptionalField::Apply),
            status: optional_update(value.status, &value.clear_fields, ToolOptionalField::Status),
            next: optional_update(value.next, &value.clear_fields, ToolOptionalField::Next),
        }
    }
}

fn optional_update(
    value: Option<String>,
    clear_fields: &[ToolOptionalField],
    field: ToolOptionalField,
) -> OptionalField<String> {
    match value {
        Some(value) => OptionalField::Set(value),
        None if clear_fields.contains(&field) => OptionalField::Clear,
        None => OptionalField::Unchanged,
    }
}

#[cfg(test)]
#[path = "user_workspace_memory_update_tests.rs"]
mod tests;
