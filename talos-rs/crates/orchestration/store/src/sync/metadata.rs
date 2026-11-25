use super::{session, StoreSyncAdapter};
use talos_error::{ErrorCode, TalosError, TalosResult};
use talos_storage::StorageTransaction;
use talos_sync::{
    ResourceKind, SessionExecutionBinding, SyncChange, SyncImportBatch, SyncResourceKey,
    SyncResourceSnapshot, SyncSessionSnapshot, TalosSync,
};
use talos_types::{
    NodeHealth, NodeInfo, NodeMode, NodeStatus, ProjectInfo, ProjectStatus, ProjectStatusState,
    ProjectUnavailableReason,
};

#[derive(Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub(super) struct MetadataSource {
    fields: super::source::SourceFields,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    tags: Vec<String>,
}

pub(super) async fn read_source(
    adapter: &StoreSyncAdapter,
    tx: &mut dyn StorageTransaction,
    resource: &SyncResourceKey,
) -> TalosResult<Option<MetadataSource>> {
    let id = resource.id();
    let user = &adapter.binding.source_user_id;
    let account = &adapter.binding.scope.account_id;
    let mut fields = match resource {
        SyncResourceKey::Computer { .. } => tx.query(
            "SELECT owner_user_id, display_name, mode, endpoint, status, created_at, updated_at, system_info
             FROM agent_nodes WHERE node_id=? AND owner_user_id=? AND node_id<>'embedded'",
            &[id, account],
        ).await?,
        SyncResourceKey::Project { .. } => tx.query(
            "SELECT p.execution_node_id, p.name, p.directory, p.created_at, p.updated_at,
                    (SELECT COUNT(*) FROM sessions s WHERE s.project_id=p.project_id) AS session_count
             FROM projects p WHERE p.project_id=? AND p.user_id IN (?,?)",
            &[id, user, account],
        ).await?,
        SyncResourceKey::File { .. } => tx.query(
            "SELECT project_id, storage_peer_id, original_filename, mime_type, size_bytes, metadata_json,
                    created_at, updated_at FROM files WHERE file_id=? AND owner_user_id IN (?,?) AND status='ready'",
            &[id, user, account],
        ).await?,
        SyncResourceKey::Session { .. } => return Err(TalosError::internal("Session capture uses a history cut.")),
    };
    let Some(row) = fields.pop() else {
        return Ok(None);
    };
    let embedded_project = matches!(resource, SyncResourceKey::Project { .. })
        && row.get_string("execution_node_id").as_deref() == Some("embedded");
    let mut fields = super::source::SourceFields::from(row);
    if embedded_project {
        if !TalosSync::captures_embedded_resource(tx, &adapter.binding, ResourceKind::Project, id)
            .await?
        {
            return Ok(None);
        }
        // Freeze this registered association before the local alias can change.
        fields.set_text("execution_node_id", adapter.computer("embedded"));
    }
    let tags = if matches!(resource, SyncResourceKey::File { .. }) {
        tx.query(
            "SELECT tag FROM file_tags WHERE file_id=? ORDER BY tag",
            &[id],
        )
        .await?
        .iter()
        .map(|row| text(row, "tag"))
        .collect::<TalosResult<Vec<_>>>()?
    } else {
        Vec::new()
    };
    Ok(Some(MetadataSource { fields, tags }))
}

impl MetadataSource {
    pub(super) fn project(
        self,
        adapter: &StoreSyncAdapter,
        resource: &SyncResourceKey,
    ) -> TalosResult<SyncResourceSnapshot> {
        let row = self.fields.row();
        let id = resource.id();
        Ok(match resource {
            SyncResourceKey::Computer { .. } => SyncResourceSnapshot::Computer(NodeInfo {
                managed_worker_phase: None,
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
                node_id: id.into(),
                display_name: text(&row, "display_name")?,
                mode: match text(&row, "mode")?.as_str() {
                    "embedded" => NodeMode::Embedded,
                    "remote" => NodeMode::Remote,
                    _ => return Err(TalosError::internal("Invalid persisted Computer mode.")),
                },
                endpoint: row.get_string("endpoint"),
                status: match text(&row, "status")?.as_str() {
                    "enabled" => NodeStatus::Enabled,
                    "disabled" => NodeStatus::Disabled,
                    _ => return Err(TalosError::internal("Invalid persisted Computer status.")),
                },
                owner_user_id: row.get_string("owner_user_id"),
                health: NodeHealth::Unknown,
                lease_expires_at: None,
                last_heartbeat_at: None,
                credential_expires_at: None,
                available: false,
                created_at: number(&row, "created_at")?,
                updated_at: number(&row, "updated_at")?,
            }),
            SyncResourceKey::Project { .. } => SyncResourceSnapshot::Project(ProjectInfo {
                node_id: text(&row, "execution_node_id")?,
                project_id: id.into(),
                name: text(&row, "name")?,
                directory: text(&row, "directory")?.into(),
                status: ProjectStatus {
                    state: ProjectStatusState::NodeUnavailable,
                    reason: Some(ProjectUnavailableReason::NodeUnavailable),
                },
                created_at: number(&row, "created_at")?,
                updated_at: number(&row, "updated_at")?,
                session_count: number(&row, "session_count")?,
            }),
            SyncResourceKey::File { .. } => super::file::project(adapter, id, &row, self.tags)?,
            SyncResourceKey::Session { .. } => {
                return Err(TalosError::internal("Session capture uses a history cut."))
            }
        })
    }
}

pub(super) async fn project_session(
    adapter: &StoreSyncAdapter,
    tx: &mut dyn StorageTransaction,
    session_id: &str,
    generation: u64,
    row: &talos_storage::Row,
) -> TalosResult<SyncResourceSnapshot> {
    let record = crate::session::DatabaseSessionStore::parse_session_record(row)?;
    let memory_processed_through = session::key_at_generation(
        adapter,
        tx,
        session_id,
        generation,
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
    info.session_kind = record.session_kind;
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
    Ok(SyncResourceSnapshot::Session(Box::new(
        SyncSessionSnapshot {
            session: info,
            worktree_binding: record.worktree_binding,
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

pub(super) async fn apply(
    adapter: &StoreSyncAdapter,
    tx: &mut dyn StorageTransaction,
    batch: &SyncImportBatch,
) -> TalosResult<()> {
    for change in &batch.changes {
        let SyncChange::Metadata { after, .. } = change else {
            continue;
        };
        match after.as_ref() {
            // Historical Computer payloads remain relayable checkpoints, but only
            // the authenticated Server directory may change selectable Computers.
            SyncResourceSnapshot::Computer(_) => {}
            SyncResourceSnapshot::Project(project) => {
                let directory = project.directory.to_str().ok_or_else(|| {
                    TalosError::internal("Synchronized Project directory must be UTF-8.")
                })?;
                let local = adapter.local_node(&project.node_id);
                let existing = tx.query("SELECT execution_node_id, user_id, directory FROM projects WHERE project_id = ?", &[&project.project_id]).await?;
                if existing.first().is_some_and(|row| {
                    row.get_string("execution_node_id").as_deref() != Some(&local)
                        || row.get_string("directory").as_deref() != Some(directory)
                        || ![&adapter.binding.source_user_id, &batch.scope.account_id]
                            .contains(&&row.get_string("user_id").unwrap_or_default())
                }) {
                    return Err(TalosError::new(
                        ErrorCode::Conflict,
                        "Project identity has a different immutable binding.",
                    ));
                }
                tx.execute("INSERT INTO projects(project_id, execution_node_id, user_id, name, directory, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, ?) ON CONFLICT(project_id) DO UPDATE SET name = excluded.name, updated_at = excluded.updated_at", &[&project.project_id, &local, &batch.scope.account_id, &project.name, directory, &project.created_at.to_string(), &project.updated_at.to_string()]).await?;
            }
            SyncResourceSnapshot::Session(session) => {
                session::apply_metadata(adapter, tx, batch, session).await?
            }
            SyncResourceSnapshot::File(file) => {
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
    if let SyncResourceKey::Session { session_id } = resource {
        let owned = tx
            .query(
                "SELECT 1 FROM sessions WHERE session_key=? AND user_id IN (?,?)",
                &[
                    session_id,
                    &adapter.binding.source_user_id,
                    &adapter.binding.scope.account_id,
                ],
            )
            .await?;
        if !owned.is_empty() {
            super::source::retire_history(tx, session_id, /*deleted*/ true).await?;
        }
    }
    let (table, key, owner) = match resource {
        SyncResourceKey::Computer { .. } => return Ok(()),
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

pub(super) async fn session_row(
    adapter: &StoreSyncAdapter,
    tx: &mut dyn StorageTransaction,
    session_id: &str,
) -> TalosResult<Option<talos_storage::Row>> {
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
    let Some(row) = rows.into_iter().next() else {
        return Ok(None);
    };
    if text(&row, "execution_node_id")? == "embedded"
        && !TalosSync::captures_embedded_resource(
            tx,
            &adapter.binding,
            ResourceKind::Session,
            session_id,
        )
        .await?
    {
        return Ok(None);
    }
    Ok(Some(row))
}

pub(super) fn number(row: &talos_storage::Row, key: &str) -> TalosResult<i64> {
    row.get_i64(key)
        .ok_or_else(|| TalosError::internal("Missing persisted synchronization field."))
}
