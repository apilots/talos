use chrono::{DateTime, Local, Utc};
use std::path::{Component, Path, PathBuf};
use talos_config::{resolve_runtime_path, TalosPaths};
use talos_error::{TalosError, TalosResult};
use tokio::fs;

use crate::store::MemoryStore;

const DAY_FILE_PATTERN_LEN: usize = 10; // YYYY-MM-DD

pub(crate) fn default_workspace_dir() -> PathBuf {
    TalosPaths::resolve().workspace_dir_for_agent("talos")
}

pub(crate) fn resolve_workspace_dir(path: &str) -> PathBuf {
    resolve_runtime_path(path)
}

pub(crate) fn session_dir(store: &MemoryStore, session_id: &str) -> PathBuf {
    store
        .workspace_dir()
        .join("sessions")
        .join(sanitize_segment(session_id))
}

pub(crate) fn workspace_memory_dir(store: &MemoryStore) -> PathBuf {
    store.workspace_dir().join("memory")
}

pub(crate) fn workspace_memory_file_for(store: &MemoryStore, timestamp: DateTime<Utc>) -> PathBuf {
    workspace_memory_dir(store).join(format!(
        "{}.md",
        timestamp.with_timezone(&Local).format("%Y-%m-%d")
    ))
}

pub(crate) async fn list_daily_files(
    store: &MemoryStore,
    session_id: &str,
) -> TalosResult<Vec<PathBuf>> {
    let mut files = Vec::new();
    let session_dir = session_dir(store, session_id);

    let mut dir = match fs::read_dir(&session_dir).await {
        Ok(dir) => dir,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(files),
        Err(err) => {
            return Err(TalosError::StorageError(format!(
                "read session memory dir: {}",
                err
            )));
        }
    };

    while let Some(entry) = dir
        .next_entry()
        .await
        .map_err(|e| TalosError::StorageError(format!("read session memory entry: {}", e)))?
    {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let Some(name) = path.file_name().and_then(|value| value.to_str()) else {
            continue;
        };
        if !is_daily_file_name(name) {
            continue;
        }
        files.push(path);
    }

    Ok(files)
}

pub(crate) async fn list_workspace_memory_files(store: &MemoryStore) -> TalosResult<Vec<PathBuf>> {
    let mut files = Vec::new();
    let dir_path = workspace_memory_dir(store);
    let mut dir = match fs::read_dir(&dir_path).await {
        Ok(dir) => dir,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(files),
        Err(err) => {
            return Err(TalosError::StorageError(format!(
                "read workspace memory dir: {}",
                err
            )));
        }
    };

    while let Some(entry) = dir
        .next_entry()
        .await
        .map_err(|e| TalosError::StorageError(format!("read workspace memory entry: {}", e)))?
    {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let Some(name) = path.file_name().and_then(|value| value.to_str()) else {
            continue;
        };
        if !is_daily_file_name(name) {
            continue;
        }
        files.push(path);
    }

    files.sort();
    Ok(files)
}

pub(crate) fn is_daily_file_name(file_name: &str) -> bool {
    if !file_name.ends_with(".md") {
        return false;
    }

    let date = &file_name[..file_name.len() - 3];
    if date.len() != DAY_FILE_PATTERN_LEN {
        return false;
    }

    let chars: Vec<char> = date.chars().collect();
    for (idx, ch) in chars.iter().enumerate() {
        let is_dash = idx == 4 || idx == 7;
        if is_dash {
            if *ch != '-' {
                return false;
            }
        } else if !ch.is_ascii_digit() {
            return false;
        }
    }

    true
}

pub(crate) fn sanitize_segment(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for ch in input.chars() {
        if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
            out.push(ch);
        } else {
            out.push('_');
        }
    }
    if out.trim_matches('_').is_empty() {
        "session".to_string()
    } else {
        out
    }
}

pub(crate) fn normalize_memory_path(path: &str) -> TalosResult<PathBuf> {
    let normalized = path.trim().trim_start_matches("./").replace('\\', "/");
    if normalized.is_empty() {
        return Err(TalosError::ValidationError(
            "memory path cannot be empty".to_string(),
        ));
    }

    let mut clean = PathBuf::new();
    for component in Path::new(&normalized).components() {
        match component {
            Component::CurDir => continue,
            Component::Normal(segment) => clean.push(segment),
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(TalosError::ValidationError(format!(
                    "memory path is outside workspace: {}",
                    path.trim()
                )));
            }
        }
    }

    let mut components = clean.components();
    match (components.next(), components.next(), components.next()) {
        (Some(Component::Normal(name)), None, None)
            if name.to_string_lossy().eq_ignore_ascii_case("MEMORY.md") =>
        {
            Ok(clean)
        }
        (Some(Component::Normal(dir)), Some(Component::Normal(file_name)), None)
            if dir == "memory" && file_name.to_str().map(is_daily_file_name).unwrap_or(false) =>
        {
            Ok(clean)
        }
        _ => Err(TalosError::ValidationError(format!(
            "memory path is not an allowed memory source: {}",
            path.trim()
        ))),
    }
}
