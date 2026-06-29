use talos_error::TalosResult;

use crate::retrieval;
use crate::store::{MemoryStore, WorkspaceMemoryRecord};

#[derive(Debug, Clone, PartialEq, Eq)]
struct WorkspaceMemoryBlock {
    title: String,
    session_id: String,
}

fn parse_block(lines: &[&str]) -> Option<WorkspaceMemoryBlock> {
    let title = lines.first().and_then(|line| {
        let trimmed = line.trim();
        if !trimmed.starts_with("## [") {
            return None;
        }
        let closing = trimmed.find(']')?;
        Some(trimmed[closing + 1..].trim().to_string())
    })?;

    let session_id = lines.iter().find_map(|line| {
        line.trim()
            .strip_prefix("session: ")
            .map(|value| value.trim().to_string())
    })?;

    Some(WorkspaceMemoryBlock { title, session_id })
}

fn parse_workspace_memory_blocks(content: &str) -> Vec<WorkspaceMemoryBlock> {
    let lines: Vec<&str> = content.lines().collect();
    let mut blocks = Vec::new();
    let mut start = None;

    for (idx, line) in lines.iter().enumerate() {
        if line.trim().starts_with("## [") {
            if let Some(existing_start) = start.take() {
                if let Some(block) = parse_block(&lines[existing_start..idx]) {
                    blocks.push(block);
                }
            }
            start = Some(idx);
        }
    }

    if let Some(existing_start) = start {
        if let Some(block) = parse_block(&lines[existing_start..]) {
            blocks.push(block);
        }
    }

    blocks
}

async fn is_duplicate_in_today(
    store: &MemoryStore,
    record: &WorkspaceMemoryRecord,
) -> TalosResult<bool> {
    let file_path = store.workspace_memory_file_for(record.timestamp);
    let Some(content) = talos_fs::read_file(&file_path).await? else {
        return Ok(false);
    };
    Ok(parse_workspace_memory_blocks(&content)
        .into_iter()
        .any(|block| block.session_id == record.session_id && block.title == record.title))
}

pub(crate) async fn append_workspace_memory(
    store: &MemoryStore,
    record: &WorkspaceMemoryRecord,
) -> TalosResult<()> {
    let body = record.content.trim();
    if body.is_empty() {
        return Ok(());
    }
    if is_duplicate_in_today(store, record).await? {
        return Ok(());
    }

    let file_path = store.workspace_memory_file_for(record.timestamp);
    let payload = format!(
        "## [{}] {}\nsession: {}\n\n{}\n\n",
        record
            .timestamp
            .with_timezone(&store.timezone())
            .format("%H:%M"),
        record.title,
        record.session_id,
        body
    );
    talos_fs::append_file(&file_path, &payload).await?;
    retrieval::rebuild_file_index_for_timestamp(store, record.timestamp).await?;
    Ok(())
}

#[cfg(test)]
#[path = "workspace_store_tests.rs"]
mod tests;
