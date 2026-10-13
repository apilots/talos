use talos_error::{ErrorCode, TalosError, TalosResult};
use talos_storage::Storage;
use talos_sync::{SyncResourceKey, SyncScanPage, SyncScanParams};

pub(super) async fn scan(
    storage: &dyn Storage,
    params: SyncScanParams,
) -> TalosResult<SyncScanPage> {
    if !(1..=1024).contains(&params.limit) {
        return Err(TalosError::new(
            ErrorCode::InvalidRequest,
            "Invalid synchronization scan limit.",
        ));
    }
    let rows = storage
        .query(
            r#"SELECT kind, id FROM (
        SELECT 'computer' AS kind, node_id AS id FROM agent_nodes WHERE owner_user_id = ? AND node_id <> 'embedded'
        UNION ALL SELECT 'project', project_id FROM projects WHERE user_id = ?
        UNION ALL SELECT 'session', session_key FROM sessions WHERE user_id = ?
        UNION ALL SELECT 'file', file_id FROM files WHERE owner_user_id = ? AND status = 'ready'
    ) WHERE kind || ':' || id > ? ORDER BY kind, id LIMIT ?"#,
            &[
                &params.binding.scope.account_id,
                &params.binding.source_user_id,
                &params.binding.source_user_id,
                &params.binding.source_user_id,
                params.cursor.as_deref().unwrap_or_default(),
                &params.limit.to_string(),
            ],
        )
        .await?;
    let mut resources = Vec::new();
    for row in rows {
        let id = row
            .get_string("id")
            .ok_or_else(|| TalosError::internal("Missing synchronization resource ID."))?;
        let resource = match row.get_string("kind").as_deref() {
            Some("computer") => SyncResourceKey::Computer { computer_id: id },
            Some("project") => SyncResourceKey::Project { project_id: id },
            Some("session") => SyncResourceKey::Session { session_id: id },
            Some("file") => SyncResourceKey::File { file_id: id },
            _ => {
                return Err(TalosError::internal(
                    "Invalid synchronization resource kind.",
                ))
            }
        };
        resources.push(resource);
    }
    let next_cursor = (resources.len() == params.limit).then(|| {
        let last = resources.last().expect("nonempty page");
        format!("{}:{}", last.kind().as_str(), last.id())
    });
    Ok(SyncScanPage {
        resources,
        next_cursor,
    })
}
