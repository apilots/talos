//! Streaming user discovery shares the round's budget and bounded waiting queue.
use std::path::PathBuf;
use std::time::{Duration, Instant};
use talos_config::{Config, EffectiveWorkspace};
use talos_error::{ErrorCode, TalosError, TalosResult};

pub(super) struct Discovery {
    definition: usize,
    entries: Option<std::fs::ReadDir>,
}
impl Discovery {
    pub fn new() -> Self {
        Self {
            definition: 0,
            entries: None,
        }
    }
    pub fn step(
        &mut self,
        config: &Config,
        cancelled: impl Fn() -> bool,
        budget: Duration,
        capacity: usize,
    ) -> (Vec<EffectiveWorkspace>, Vec<TalosError>, bool) {
        let deadline = Instant::now() + budget;
        let mut workspaces = Vec::new();
        let mut errors = Vec::new();
        let mut visited = 0;
        while workspaces.len() < capacity
            && visited < 4096
            && errors.len() < 128
            && (visited == 0 || Instant::now() < deadline)
            && !cancelled()
        {
            let Some(definition) = config.workspaces.definitions.get(self.definition) else {
                return (workspaces, errors, true);
            };
            if self.entries.is_none() {
                match prefix(config, &definition.workspace_id) {
                    Ok(Some(entries)) => self.entries = Some(entries),
                    Ok(None) => {
                        self.definition += 1;
                        continue;
                    }
                    Err(error) => {
                        errors.push(error);
                        self.definition += 1;
                        continue;
                    }
                }
            }
            let Some(entry) = self.entries.as_mut().expect("open Workspace prefix").next() else {
                self.entries = None;
                self.definition += 1;
                continue;
            };
            visited += 1;
            let result = entry.map_err(io_error).and_then(|entry| {
                let kind = entry.file_type().map_err(io_error)?;
                if !kind.is_dir() || kind.is_symlink() {
                    return Ok(None);
                }
                let name = entry.file_name();
                let Some(user) = name.to_str() else {
                    return Ok(None);
                };
                if talos_config::validate_user_workspace_segment(user).is_err() {
                    return Ok(None);
                }
                let workspace = config.resolve_workspace(&definition.workspace_id, user)?;
                match workspace.root.join("tool-output").symlink_metadata() {
                    Ok(_) => Ok(Some(workspace)),
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
                    Err(error) => Err(io_error(error)),
                }
            });
            match result {
                Ok(Some(workspace)) => workspaces.push(workspace),
                Ok(None) => {}
                Err(error) => errors.push(error),
            }
        }
        (workspaces, errors, false)
    }
}
fn io_error(error: std::io::Error) -> TalosError {
    TalosError::internal_with_source("Discover output log Workspaces failed.", error)
}
fn prefix(config: &Config, workspace_id: &str) -> TalosResult<Option<std::fs::ReadDir>> {
    let path: PathBuf = config.workspace_root_prefix(workspace_id)?;
    match std::fs::symlink_metadata(&path) {
        Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {
            std::fs::read_dir(path).map(Some).map_err(io_error)
        }
        Ok(_) => Err(TalosError::new(
            ErrorCode::PermissionDenied,
            "Output maintenance Workspace prefix must be a regular directory.",
        )),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(io_error(error)),
    }
}
