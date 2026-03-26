use talos_error::{TalosError, TalosResult};
use tokio::fs::{self, OpenOptions};
use tokio::io::AsyncWriteExt;

use crate::path_policy;
use crate::store::{MemoryStore, RootMemoryContext, WorkspaceMemoryRecord};

const DEFAULT_ROOT_MEMORY_TEMPLATE: &str = r#"# MEMORY

Use this file for durable user-written memory.

## Preferences
- Add long-term user preferences here.

## Constraints
- Add important constraints and rules here.

## Ongoing Goals
- Track current long-running goals here.

## Notes
- Keep stable facts and project context here.
"#;

async fn ensure_root_memory_file(store: &MemoryStore) -> TalosResult<()> {
    let path = store.root_memory_path();
    if fs::try_exists(&path)
        .await
        .map_err(|err| TalosError::StorageError(format!("check root memory file: {}", err)))?
    {
        return Ok(());
    }

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).await.map_err(|err| {
            TalosError::StorageError(format!("create root memory parent dir: {}", err))
        })?;
    }

    fs::write(&path, DEFAULT_ROOT_MEMORY_TEMPLATE)
        .await
        .map_err(|err| TalosError::StorageError(format!("write default root memory file: {}", err)))
}

pub(crate) async fn load_root_memory(store: &MemoryStore) -> TalosResult<RootMemoryContext> {
    let path = store.root_memory_path();
    ensure_root_memory_file(store).await?;
    let content = match fs::read_to_string(&path).await {
        Ok(content) => Some(content),
        Err(err) => {
            return Err(TalosError::StorageError(format!(
                "read root memory file: {}",
                err
            )));
        }
    };
    Ok(RootMemoryContext { path, content })
}

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
