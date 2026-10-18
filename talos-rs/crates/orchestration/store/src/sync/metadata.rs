use super::{session, transcode, StoreSyncAdapter};
use talos_error::{ErrorCode, TalosError, TalosResult};
use talos_storage::StorageTransaction;
use talos_sync::{
    SessionExecutionBinding, SyncImportBatch, SyncResourceKey, SyncResourceSnapshot,
    SyncSessionSnapshot,
};
use talos_types::{
    NodeHealth, NodeInfo, NodeMode, NodeStatus, ProjectInfo, ProjectStatus, ProjectStatusState,
    ProjectUnavailableReason,
};

pub(super) async fn read(
    adapter: &StoreSyncAdapter,
    tx: &mut dyn StorageTransaction,
    resource: &SyncResourceKey,
) -> TalosResult<Option<SyncResourceSnapshot>> {
    Ok(match resource {
        SyncResourceKey::Computer { computer_id } => {
            let rows = tx
                .query(
                    "SELECT * FROM agent_nodes WHERE node_id = ?",
                    &[computer_id],
                )
                .await?;
            let Some(row) = rows.first() else {
                return Ok(None);
            };
            let owner = row.get_string("owner_user_id");
            // G owns Computer directory facts. A host alias only maps immutable Session bindings.
            if computer_id == "embedded"
                || owner.as_deref() != Some(&adapter.binding.scope.account_id)
            {
                return Ok(None);
            }
            Some(SyncResourceSnapshot::Computer(NodeInfo {
                system_info: row
                    .get_string("system_info")
                    .map(|value| serde_json::from_str(&value))
                    .transpose()
                    .map_err(|error| {
                        TalosError::internal_with_source(
                            "Decode Worker system report for synchronization.",
                            error,
                        )
                    })?,
                node_id: computer_id.clone(),
                display_name: text(row, "display_name")?,
                mode: match text(row, "mode")?.as_str() {
                    "embedded" => NodeMode::Embedded,
                    "remote" => NodeMode::Remote,
                    _ => return Err(TalosError::internal("Invalid persisted Computer mode.")),
                },
                endpoint: row.get_string("endpoint"),
                status: match text(row, "status")?.as_str() {
                    "enabled" => NodeStatus::Enabled,
                    "disabled" => NodeStatus::Disabled,
                    _ => return Err(TalosError::internal("Invalid persisted Computer status.")),
                },
                owner_user_id: owner,
                health: NodeHealth::Unknown,
                lease_expires_at: None,
                last_heartbeat_at: None,
                credential_expires_at: None,
                available: false,
                created_at: number(row, "created_at")?,
                updated_at: number(row, "updated_at")?,
            }))
        }
        SyncResourceKey::Project { project_id } => {
            let rows = tx.query("SELECT p.*, (SELECT COUNT(*) FROM sessions s WHERE s.project_id = p.project_id) AS session_count FROM projects p WHERE p.project_id = ? AND p.user_id IN (?, ?)", &[project_id, &adapter.binding.source_user_id, &adapter.binding.scope.account_id]).await?;
            if rows.first().is_some_and(|row| {
                row.get_string("execution_node_id").as_deref() == Some("embedded")
            }) && !capture_local_resource(adapter, tx, "project", project_id).await?
            {
                return Ok(None);
            }
            rows.first()
                .map(|row| {
                    Ok::<_, TalosError>(SyncResourceSnapshot::Project(ProjectInfo {
                        node_id: text(row, "execution_node_id")?,
                        project_id: project_id.clone(),
                        name: text(row, "name")?,
                        directory: text(row, "directory")?.into(),
                        status: ProjectStatus {
                            state: ProjectStatusState::NodeUnavailable,
                            reason: Some(ProjectUnavailableReason::NodeUnavailable),
                        },
                        created_at: number(row, "created_at")?,
                        updated_at: number(row, "updated_at")?,
                        session_count: number(row, "session_count")?,
                    }))
                })
                .transpose()?
        }
        SyncResourceKey::Session { session_id } => {
            let rows = tx
                .query(
                    "SELECT * FROM sessions WHERE session_key = ? AND user_id IN (?, ?)",
                    &[
                        session_id,
                        &adapter.binding.source_user_id,
                        &adapter.binding.scope.account_id,
                    ],
                )
                .await?;
            let Some(row) = rows.first() else {
                return Ok(None);
            };
            let record = crate::session::DatabaseSessionStore::parse_session_record(row)?;
            if record.execution_node_id == "embedded"
                && !capture_local_resource(adapter, tx, "session", session_id).await?
            {
                return Ok(None);
            }
            let memory_processed_through = session::key_at(
                adapter,
                tx,
                session_id,
                record.memory_processed_until_event_seq as i64,
            )
            .await?;
            // The Service supplies the original projection for the bound Worker.
            // A receiver preserves these fields and never uses its own defaults.
            let shared = adapter
                .sync
                .session_snapshot(tx, &adapter.binding, session_id)
                .await?;
            let mut info = match shared {
                Some(snapshot) => snapshot.session,
                None => (adapter.session_projection)(&record)?,
            };
            info.title = record.title.clone();
            info.last_activity = record.last_activity;
            info.message_count = record.message_count;
            info.usage = record.usage.clone();
            info.current_directory = record.current_directory.clone();
            if let Some(selection) = &record.conf.model_selection {
                info.model_selection = selection.clone();
            }
            info.effective_mode = record
                .conf
                .user_permission_mode
                .unwrap_or(info.configured_default_mode);
            info.user_id = adapter.binding.scope.account_id.clone();
            Some(SyncResourceSnapshot::Session(Box::new(
                SyncSessionSnapshot {
                    session: info,
                    conf: record.conf,
                    environment: record.environment,
                    channel_id: record.channel_id,
                    thread_id: record.thread_id,
                    binding: SessionExecutionBinding {
                        computer_id: record.execution_node_id,
                        agent_id: record.agent_id,
                        workspace_id: record.workspace_id,
                        workspace_user_id: row
                            .get_string("workspace_user_id")
                            .unwrap_or(record.user_id),
                    },
                    tree_identity: record.tree_identity,
                    memory_processed_through,
                },
            )))
        }
        SyncResourceKey::File { file_id } => super::file::read(adapter, tx, file_id).await?,
    })
}

pub(super) async fn apply(
    adapter: &StoreSyncAdapter,
    tx: &mut dyn StorageTransaction,
    batch: &SyncImportBatch,
) -> TalosResult<()> {
    for change in &batch.changes {
        let talos_sync_types::SyncChange::Metadata { after, .. } = change else {
            continue;
        };
        match after.as_ref() {
            talos_sync_types::SyncResourceSnapshot::Computer(node) => {
                if node.owner_user_id.as_deref() != Some(&batch.scope.account_id) {
                    return Err(TalosError::new(
                        ErrorCode::PermissionDenied,
                        "Computer belongs to a different account.",
                    ));
                }
                let local = node.node_id.clone();
                if !tx
                    .query(
                        "SELECT 1 FROM computer_deletions WHERE node_id = ? AND owner_user_id = ?",
                        &[&local, &batch.scope.account_id],
                    )
                    .await?
                    .is_empty()
                {
                    continue;
                }
                let mode = "remote";
                let status = transcode::<_, NodeStatus>(&node.status)?;
                let status = match status {
                    NodeStatus::Enabled => "enabled",
                    NodeStatus::Disabled => "disabled",
                };
                let existing = tx
                    .query(
                        "SELECT owner_user_id FROM agent_nodes WHERE node_id = ?",
                        &[&local],
                    )
                    .await?;
                if existing.first().is_some_and(|r| {
                    r.get_string("owner_user_id")
                        .is_some_and(|owner| owner != batch.scope.account_id)
                }) {
                    return Err(TalosError::new(
                        ErrorCode::Conflict,
                        "Computer identity belongs to another account.",
                    ));
                }
                let system_info = node
                    .system_info
                    .as_ref()
                    .map(serde_json::to_string)
                    .transpose()
                    .map_err(|error| {
                        TalosError::internal_with_source(
                            "Encode imported Worker system report.",
                            error,
                        )
                    })?;
                tx.execute("INSERT INTO agent_nodes(node_id, owner_user_id, display_name, mode, endpoint, status, created_at, updated_at, system_info) VALUES (?, NULLIF(?, ''), ?, ?, NULLIF(?, ''), ?, ?, ?, NULLIF(?, '')) ON CONFLICT(node_id) DO UPDATE SET display_name = excluded.display_name, endpoint = excluded.endpoint, status = excluded.status, updated_at = excluded.updated_at, system_info = excluded.system_info", &[&local, &batch.scope.account_id, &node.display_name, mode, node.endpoint.as_deref().unwrap_or_default(), status, &node.created_at.to_string(), &node.updated_at.to_string(), system_info.as_deref().unwrap_or_default()]).await?;
            }
            talos_sync_types::SyncResourceSnapshot::Project(project) => {
                let local = adapter.local_node(&project.node_id);
                let existing = tx.query("SELECT execution_node_id, user_id, directory FROM projects WHERE project_id = ?", &[&project.project_id]).await?;
                if existing.first().is_some_and(|row| {
                    row.get_string("execution_node_id").as_deref() != Some(&local)
                        || row.get_string("directory").as_deref() != Some(&project.directory)
                        || ![&adapter.binding.source_user_id, &batch.scope.account_id]
                            .contains(&&row.get_string("user_id").unwrap_or_default())
                }) {
                    return Err(TalosError::new(
                        ErrorCode::Conflict,
                        "Project identity has a different immutable binding.",
                    ));
                }
                tx.execute("INSERT INTO projects(project_id, execution_node_id, user_id, name, directory, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, ?) ON CONFLICT(project_id) DO UPDATE SET name = excluded.name, updated_at = excluded.updated_at", &[&project.project_id, &local, &batch.scope.account_id, &project.name, &project.directory, &project.created_at.to_string(), &project.updated_at.to_string()]).await?;
            }
            talos_sync_types::SyncResourceSnapshot::Session(session) => {
                session::apply_metadata(adapter, tx, batch, session).await?
            }
            talos_sync_types::SyncResourceSnapshot::File(file) => {
                super::file::apply(adapter, tx, batch, file).await?
            }
        }
    }
    Ok(())
}

pub(super) async fn delete(
    adapter: &StoreSyncAdapter,
    tx: &mut dyn StorageTransaction,
    resource: &SyncResourceKey,
) -> TalosResult<()> {
    if let SyncResourceKey::Computer { computer_id } = resource {
        if adapter
            .binding
            .computers
            .iter()
            .any(|alias| alias.source_node_id == "embedded" && alias.computer_id == *computer_id)
        {
            crate::session::capture_removal_boundary(
                tx,
                &adapter.binding.scope.account_id,
                computer_id,
            )
            .await?;
        }
        tx.execute("INSERT INTO computer_deletions(node_id, owner_user_id, deleted_at) VALUES (?, ?, ?) ON CONFLICT(node_id) DO NOTHING", &[computer_id, &adapter.binding.scope.account_id, &chrono::Utc::now().timestamp().to_string()]).await?;
    }
    let (table, key, owner) = match resource {
        SyncResourceKey::Computer { .. } => ("agent_nodes", "node_id", "owner_user_id"),
        SyncResourceKey::Project { .. } => ("projects", "project_id", "user_id"),
        SyncResourceKey::Session { .. } => ("sessions", "session_key", "user_id"),
        SyncResourceKey::File { .. } => ("files", "file_id", "owner_user_id"),
    };
    // A synchronized deletion only removes the local metadata record. No disk,
    // source FileService, Worker directory or imported execution is touched.
    tx.execute(
        &format!("DELETE FROM {table} WHERE {key} = ? AND {owner} IN (?, ?)"),
        &[
            resource.id(),
            &adapter.binding.source_user_id,
            &adapter.binding.scope.account_id,
        ],
    )
    .await?;
    Ok(())
}

pub(super) fn text(row: &talos_storage::Row, key: &str) -> TalosResult<String> {
    row.get_string(key)
        .ok_or_else(|| TalosError::internal("Missing persisted synchronization field."))
}

async fn capture_local_resource(
    adapter: &StoreSyncAdapter,
    tx: &mut dyn StorageTransaction,
    kind: &str,
    id: &str,
) -> TalosResult<bool> {
    let Some(alias) = adapter
        .binding
        .computers
        .iter()
        .find(|alias| alias.source_node_id == "embedded")
    else {
        return Ok(false);
    };
    if tx
        .query(
            "SELECT 1 FROM local_computer_retirements WHERE node_id = ? AND owner_user_id = ?",
            &[&alias.computer_id, &adapter.binding.scope.account_id],
        )
        .await?
        .is_empty()
    {
        return Ok(true);
    }
    Ok(!tx.query("SELECT 1 FROM local_computer_retirement_resources WHERE node_id = ? AND kind = ? AND resource_id = ?", &[&alias.computer_id, kind, id]).await?.is_empty())
}
pub(super) fn number(row: &talos_storage::Row, key: &str) -> TalosResult<i64> {
    row.get_i64(key)
        .ok_or_else(|| TalosError::internal("Missing persisted synchronization field."))
}
