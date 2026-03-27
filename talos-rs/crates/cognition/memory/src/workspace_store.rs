use talos_error::{TalosError, TalosResult};
use tokio::fs::{self, OpenOptions};
use tokio::io::AsyncWriteExt;

use crate::path_policy;
use crate::store::{MemoryStore, WorkspaceMemoryRecord};

fn normalize_body(input: &str) -> String {
    input
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
        .to_ascii_lowercase()
}

async fn contains_duplicate_workspace_memory(store: &MemoryStore, body: &str) -> TalosResult<bool> {
    let normalized_body = normalize_body(body);
    if normalized_body.is_empty() {
        return Ok(true);
    }

    for path in path_policy::list_workspace_memory_files(store).await? {
        let content = fs::read_to_string(path).await.map_err(|e| {
            TalosError::StorageError(format!("read workspace memory file for dedupe: {}", e))
        })?;
        if normalize_body(&content).contains(&normalized_body) {
            return Ok(true);
        }
    }

    Ok(false)
}

pub(crate) async fn append_workspace_memory(
    store: &MemoryStore,
    record: &WorkspaceMemoryRecord,
) -> TalosResult<()> {
    let body = record.content.trim();
    if body.is_empty() {
        return Ok(());
    }
    if contains_duplicate_workspace_memory(store, body).await? {
        return Ok(());
    }
    let dir = store.workspace_memory_dir();
    fs::create_dir_all(&dir)
        .await
        .map_err(|e| TalosError::StorageError(format!("create workspace memory dir: {}", e)))?;
    let file_path = store.workspace_memory_file_for(record.timestamp);
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(file_path)
        .await
        .map_err(|e| TalosError::StorageError(format!("open workspace memory file: {}", e)))?;
    let title = record.title.as_deref().unwrap_or("Memory");
    let payload = format!(
        "## {} {}\n{}\n\n",
        title,
        record.timestamp.format("%H:%M"),
        body
    );
    file.write_all(payload.as_bytes())
        .await
        .map_err(|e| TalosError::StorageError(format!("append workspace memory file: {}", e)))?;
    Ok(())
}
