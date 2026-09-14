use serde_json::{json, Value};
use std::{collections::BTreeMap, path::Path};
use talos_agent_api::{AgentEventPayload, AgentMessageBlockContent, AgentToolCallArguments};
use talos_agent_kernel::{KernelSessionSummary, KernelTranscriptEvent};
use talos_error::TalosResult;

use super::{
    database::DatabaseRepair,
    decode,
    events::{integer, text},
    files::{self, FileRepair},
    invalid, source_error,
};
use crate::runner::UpgradeContext;

pub(super) async fn prepare(
    context: &UpgradeContext<'_>,
    repair: &mut DatabaseRepair,
    db: &mut sqlx::SqliteConnection,
) -> TalosResult<Vec<FileRepair>> {
    let sessions: Vec<(String, String, String)> =
        sqlx::query_as("SELECT session_key,user_id,workspace_id FROM sessions")
            .fetch_all(&mut *db)
            .await
            .map_err(source_error)?;
    let live_actions: Vec<String> =
        sqlx::query_scalar("SELECT action_id FROM agent_continuation_bindings")
            .fetch_all(&mut *db)
            .await
            .map_err(source_error)?;
    let mut files = Vec::new();
    let mut inputs = Vec::new();
    let backup = context
        .paths
        .talos_home
        .join("upgrades")
        .join(super::UPGRADE_ID)
        .join("source");
    for workspace in &context.config.workspaces.definitions {
        let prefix = context
            .config
            .workspace_root_prefix(&workspace.workspace_id)?;
        for entry in entries(&prefix)? {
            if !entry.metadata.is_dir || entry.metadata.is_symlink {
                continue;
            }
            let user = entry
                .path
                .file_name()
                .and_then(|v| v.to_str())
                .ok_or_else(|| invalid("invalid workspace user path"))?;
            talos_config::validate_user_workspace_segment(user)?;
            let path = entry.path.join("MEMORY.md");
            if path.is_symlink() {
                return Err(invalid("MEMORY.md must not be a symlink"));
            }
            if path.is_file() {
                let before = std::fs::read(&path).map_err(source_error)?;
                let content = std::str::from_utf8(&before).map_err(source_error)?;
                let after = files::memory(
                    content,
                    &workspace.workspace_id,
                    chrono::Utc::now().timestamp(),
                )
                .map_err(|e| e.context(format!("repair {}", path.display())))?
                .into_bytes();
                if before != after {
                    files.push(FileRepair {
                        path,
                        backup: backup
                            .join(&workspace.workspace_id)
                            .join(user)
                            .join("MEMORY.md"),
                        before,
                        after,
                    });
                }
            }
        }
    }
    for (session, user, workspace) in sessions {
        let root = context.config.resolve_workspace(&workspace, &user)?.root;
        let directory = root.join("sessions").join(&session);
        let mut session_files = Vec::new();
        for entry in entries(&directory)? {
            let name = entry
                .path
                .file_name()
                .and_then(|v| v.to_str())
                .unwrap_or("");
            if !name.ends_with(".jsonl") {
                continue;
            }
            if entry.metadata.is_symlink || !entry.metadata.is_file {
                return Err(invalid("session artifact is not a regular file"));
            }
            let saved = backup
                .join(&workspace)
                .join(&user)
                .join("sessions")
                .join(&session)
                .join(name);
            let before = if repair.legacy_exists {
                files::source(&entry.path, &saved)?
            } else {
                std::fs::read(&entry.path).map_err(source_error)?
            };
            session_files.push(FileRepair {
                path: entry.path,
                backup: saved,
                before,
                after: Vec::new(),
            });
        }
        let mut records = Vec::new();
        for (index, file) in session_files.iter().enumerate() {
            for (line_index, line) in std::str::from_utf8(&file.before)
                .map_err(source_error)?
                .lines()
                .enumerate()
            {
                if line.trim().is_empty() {
                    continue;
                }
                let row: Value = serde_json::from_str(line).map_err(|e| {
                    source_error(e).context(format!(
                        "{} line {}",
                        file.path.display(),
                        line_index + 1
                    ))
                })?;
                records.push((index, line.to_owned(), row));
            }
        }
        inputs_0_1_8::supplement(&session, &records, repair)
            .map_err(|error| error.context(format!("reconcile session {}", directory.display())))?;
        inputs.push((session, session_files, records));
    }
    repair.finalize(db).await?;
    for (session, mut session_files, records) in inputs {
        convert_session(&session, &mut session_files, records, repair, &live_actions)
            .map_err(|error| error.context(format!("repair session {session}")))?;
        files.extend(
            session_files
                .into_iter()
                .filter(|file| file.before != file.after),
        );
    }
    Ok(files)
}

fn entries(path: &Path) -> TalosResult<Vec<talos_fs::DirectoryEntry>> {
    let mut entries = talos_fs::read_directory(path)?.unwrap_or_default();
    entries.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(entries)
}

fn convert_session(
    session: &str,
    files: &mut [FileRepair],
    records: Vec<(usize, String, Value)>,
    repair: &DatabaseRepair,
    live_actions: &[String],
) -> TalosResult<()> {
    let mut dirty = std::collections::BTreeSet::new();
    let mut boundaries = Vec::new();
    let mut last_fragment = BTreeMap::<String, usize>::new();
    let mut atoms = repair
        .turns
        .iter()
        .filter(|((s, _), _)| s == session)
        .map(|((_, turn), events)| Ok((turn.clone(), history_atoms(events)?)))
        .collect::<TalosResult<BTreeMap<_, _>>>()?;
    for (ordinal, (_, _, row)) in records.iter().enumerate() {
        if row["kind"] != "turn" {
            continue;
        }
        let turn = &row["turn"];
        if text(turn, "session_id")? != session {
            return Err(invalid("artifact session identity differs"));
        }
        let items = turn["items"]
            .as_array()
            .ok_or_else(|| invalid("legacy artifact items missing"))?
            .iter()
            .cloned()
            .map(normalize)
            .collect::<TalosResult<Vec<_>>>()?;
        let identified = repair
            .message_turns
            .get(&(session.to_owned(), text(turn, "message_id")?.to_owned()));
        let candidates = atoms
            .iter()
            .filter(|(key, available)| {
                identified.is_none_or(|id| id == *key)
                    && items.iter().all(|item| available.contains(item))
            })
            .map(|(key, _)| key.clone())
            .collect::<Vec<_>>();
        if candidates.len() != 1 {
            return Err(invalid(format!(
                "legacy artifact seq {} has {} canonical matches",
                integer(turn, "seq")?,
                candidates.len()
            )));
        }
        let id = &candidates[0];
        let available = atoms
            .get_mut(id)
            .ok_or_else(|| invalid("matched turn missing"))?;
        for item in &items {
            // A continuation's private artifact repeats the original user prompt.
            if item["type"] != "user_input" {
                let index = available
                    .iter()
                    .position(|value| value == item)
                    .ok_or_else(|| invalid("legacy artifact content multiplicity differs"))?;
                available.remove(index);
            }
        }
        let seq = integer(turn, "seq")?;
        boundaries.push((seq, ordinal));
        last_fragment.insert(id.clone(), ordinal);
    }
    for (ordinal, (index, line, original)) in records.into_iter().enumerate() {
        let mut row = original.clone();
        if row["kind"] == "turn" {
            dirty.insert(index);
            continue;
        }
        let legacy = row.get("schema_version").is_none_or(|v| v == 1);
        if !legacy && row["schema_version"] != 2 {
            return Err(invalid("unknown artifact schema version"));
        }
        if row["kind"] == "transcript" && legacy {
            migrate_transcript(&mut row["record"], live_actions)?;
        }
        let record = row
            .get_mut("record")
            .ok_or_else(|| invalid("artifact record missing"))?;
        let cursor_path = if record.get("covers_until_seq").is_some() {
            "/covers_until_seq"
        } else {
            "/payload/update/data/report/covers_until_seq"
        };
        if let Some(cursor) = record.pointer_mut(cursor_path) {
            let old = cursor
                .as_i64()
                .ok_or_else(|| invalid("invalid artifact coverage"))?;
            let mapped = if legacy && old > 0 {
                let boundary = boundaries
                    .iter()
                    .rev()
                    .find(|(seq, position)| *seq == old && *position < ordinal)
                    .map(|(_, position)| *position)
                    .ok_or_else(|| invalid("legacy summary boundary has no preceding artifact"))?;
                covered_prefix(session, boundary, &last_fragment, &repair.turn_ends)
            } else {
                repair.cursors.get(&old).copied().unwrap_or(old)
            };
            *cursor = json!(mapped);
        }
        let kind = text(&row, "kind")?;
        match kind {
            "transcript" => {
                let event: KernelTranscriptEvent = decode(row["record"].clone())?;
                if event.session_id != session {
                    return Err(invalid("transcript session differs"));
                }
            }
            "summary" => {
                let summary: KernelSessionSummary = decode(row["record"].clone())?;
                if summary.header.session_id != session {
                    return Err(invalid("summary session differs"));
                }
            }
            _ => return Err(invalid("unknown artifact kind")),
        }
        row["schema_version"] = json!(2);
        if row != original {
            dirty.insert(index);
        }
        let encoded = if row == original {
            line
        } else {
            serde_json::to_string(&row).map_err(source_error)?
        };
        files[index].after.extend_from_slice(encoded.as_bytes());
        files[index].after.push(b'\n');
    }
    for (index, file) in files.iter_mut().enumerate() {
        if !dirty.contains(&index) {
            file.after = file.before.clone();
        }
    }
    Ok(())
}

fn migrate_transcript(record: &mut Value, live_actions: &[String]) -> TalosResult<()> {
    let object = record
        .as_object_mut()
        .ok_or_else(|| invalid("transcript is not an object"))?;
    if let Some(header) = object.remove("header") {
        for (key, value) in header
            .as_object()
            .ok_or_else(|| invalid("invalid transcript header"))?
        {
            if object.insert(key.clone(), value.clone()).is_some() {
                return Err(invalid("duplicate transcript header field"));
            }
        }
    }
    object.remove("kind");
    if let Some(actions) = object.remove("actions") {
        let mut updates = Vec::new();
        for (key, value) in actions
            .as_object()
            .ok_or_else(|| invalid("invalid transcript actions"))?
        {
            match key.as_str() {
                "state_delta" => {
                    for delta in value
                        .as_array()
                        .ok_or_else(|| invalid("invalid state deltas"))?
                    {
                        if delta["namespace"] != "session" {
                            return Err(invalid("non-session state delta"));
                        }
                        let kind = match text(delta, "key")? {
                            "session_compaction_state" => "compaction",
                            "session_continuity" => "continuity",
                            "prompt_warning_state" => "prompt_warning",
                            _ => return Err(invalid("unknown session state")),
                        };
                        let data = delta["value"].get("value").unwrap_or(&delta["value"]);
                        updates.push(json!({"type":kind,"data":data}));
                    }
                }
                "session_state" if !value.is_null() => updates.push(value.clone()),
                "end_turn" if value == false => {}
                "transfer_to"
                | "branch_override"
                | "rewind_before_invocation_id"
                | "compaction"
                | "session_state"
                    if value.is_null() => {}
                _ => return Err(invalid("unsupported transcript action")),
            }
        }
        if object["payload"]["type"] == "state_only" {
            if updates.len() != 1 {
                return Err(invalid("state-only transcript needs exactly one update"));
            }
            object.insert(
                "payload".into(),
                json!({"type":"session_state","update":updates.remove(0)}),
            );
        } else if !updates.is_empty() {
            return Err(invalid("state updates on non-state transcript"));
        }
    }
    if record["payload"]["type"] == "user_action_request" {
        let payload = record["payload"]["payload"]
            .as_object_mut()
            .ok_or_else(|| invalid("invalid action request"))?;
        for key in ["tool_calls", "tool_messages", "deferred_tool_calls"] {
            if !payload.contains_key(key)
                && live_actions.iter().any(|id| payload["action_id"] == *id)
            {
                return Err(invalid("live legacy action lacks its original tool batch"));
            }
            payload.entry(key).or_insert(json!([]));
        }
    }
    Ok(())
}

fn normalize(mut value: Value) -> TalosResult<Value> {
    if value["type"] == "reasoning" {
        value["text"] = json!(text(&value, "text")?.trim_end());
    }
    if value["type"] == "user_input" && value.get("fileIds").is_none() {
        value["fileIds"] = json!([]);
    }
    Ok(value)
}

fn history_atoms(events: &[talos_agent_api::AgentEvent]) -> TalosResult<Vec<Value>> {
    let mut result = Vec::new();
    for event in events {
        let AgentEventPayload::MessageCompleted(message) = &event.payload else {
            continue;
        };
        let mut user = None;
        let mut files = Vec::new();
        for block in &message.blocks {
            let value = match &block.content {
                AgentMessageBlockContent::UserContent(talos_types::AgentUserContent::Text {
                    text,
                }) => {
                    user = Some(text.clone());
                    continue;
                }
                AgentMessageBlockContent::UserContent(talos_types::AgentUserContent::File {
                    file_id,
                }) => {
                    files.push(file_id);
                    continue;
                }
                AgentMessageBlockContent::Reasoning(data) => {
                    json!({"type":"reasoning","text":data.text})
                }
                AgentMessageBlockContent::AssistantText(data)
                    if data.phase == talos_agent_api::AgentAssistantTextPhase::FinalAnswer =>
                {
                    json!({"type":"output_text","text":data.text})
                }
                AgentMessageBlockContent::ToolCall(data) => match &data.arguments {
                    AgentToolCallArguments::Complete { value } => {
                        json!({"type":"tool_call","call_id":data.call_id,"name":data.name,"arguments":{"value":value}})
                    }
                    AgentToolCallArguments::Streaming { .. } => continue,
                },
                AgentMessageBlockContent::ToolResult(data) => {
                    json!({"type":"tool_result","call_id":data.call_id,"tool_name":data.tool_name,"content":data.content,"is_error":data.is_error})
                }
                AgentMessageBlockContent::ExecutionSummary(data) => {
                    json!({"type":"execution_summary","text":data.text})
                }
                _ => continue,
            };
            result.push(normalize(value)?);
        }
        if let Some(text) = user {
            result.push(json!({"type":"user_input","text":text,"fileIds":files}));
        }
    }
    Ok(result)
}

#[path = "inputs_0_1_8.rs"]
mod inputs_0_1_8;

fn covered_prefix(
    session: &str,
    boundary: usize,
    fragments: &BTreeMap<String, usize>,
    ends: &BTreeMap<(String, String), i64>,
) -> i64 {
    let mut ordered = ends
        .iter()
        .filter(|((s, _), _)| s == session)
        .collect::<Vec<_>>();
    ordered.sort_by_key(|(_, end)| **end);
    let mut cursor = 0;
    for ((_, turn), end) in ordered {
        if fragments
            .get(turn)
            .is_none_or(|position| *position > boundary)
        {
            break;
        }
        cursor = *end;
    }
    cursor
}
