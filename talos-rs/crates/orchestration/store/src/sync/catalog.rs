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
            r#"WITH resources(kind, id) AS (
        SELECT 'computer', node_id FROM agent_nodes WHERE owner_user_id = ?1 AND node_id <> 'embedded'
        UNION ALL SELECT 'computer', node_id FROM computer_deletions WHERE owner_user_id = ?1
        UNION ALL SELECT 'project', project_id FROM projects WHERE user_id = ?2
        UNION ALL SELECT 'session', session_key FROM sessions WHERE user_id = ?2
        UNION ALL SELECT 'file', file_id FROM files WHERE owner_user_id = ?2 AND status = 'ready'
    )
    SELECT r.kind, r.id FROM resources r
    WHERE r.kind || ':' || r.id > ?4
      AND NOT EXISTS (SELECT 1 FROM sync_documents d
                      WHERE d.hub_id=?3 AND d.account_id=?1 AND d.kind=r.kind AND d.resource_id=r.id)
      AND NOT EXISTS (SELECT 1 FROM visible_sync_outbox o
                      WHERE o.hub_id=?3 AND o.account_id=?1 AND o.kind=r.kind AND o.resource_id=r.id)
    ORDER BY r.kind, r.id LIMIT ?5"#,
            &[
                &params.binding.scope.account_id,
                &params.binding.source_user_id,
                &params.binding.scope.hub_id,
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
        resources.push(resource(&row.get_string("kind").unwrap_or_default(), id)?);
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

pub(super) fn resource(kind: &str, id: String) -> TalosResult<SyncResourceKey> {
    Ok(match kind {
        "computer" => SyncResourceKey::Computer { computer_id: id },
        "project" => SyncResourceKey::Project { project_id: id },
        "session" => SyncResourceKey::Session { session_id: id },
        "file" => SyncResourceKey::File { file_id: id },
        _ => {
            return Err(TalosError::internal(
                "Invalid synchronization resource kind.",
            ))
        }
    })
}
