use std::path::PathBuf;

use talos_config::resolve_runtime_path;
use talos_error::{TalosError, TalosResult};
use talos_types::{DocUpdateRequest, ExpectedFileVersion, TalosConfig};

/// Resolve the configured workspace root into a runtime filesystem path.
pub(crate) fn resolve_workspace_root(config: &TalosConfig) -> PathBuf {
    let workspace_dir = config.agent.kernel.workspace_dir.trim();
    resolve_runtime_path(workspace_dir)
}

/// Compute the revision token used for workspace-managed text content.
pub(crate) fn workspace_revision(content: &str) -> String {
    let mut hash: u64 = 0xcbf29ce484222325;
    for byte in content.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{hash:016x}")
}

/// Ensure the requested workspace write matches the current revision state.
pub(crate) fn ensure_workspace_write_version(
    existing_content: Option<&str>,
    request: &DocUpdateRequest,
) -> TalosResult<()> {
    match (&request.base_version, existing_content) {
        (ExpectedFileVersion::MustNotExist, Some(_)) => Err(TalosError::public(
            talos_error::ErrorCode::Conflict,
            "target file already exists".to_string(),
        )),
        (ExpectedFileVersion::MustNotExist, None) => Ok(()),
        (ExpectedFileVersion::Match { .. }, None) => Err(TalosError::public(
            talos_error::ErrorCode::ResourceNotFound,
            "target file does not exist".to_string(),
        )),
        (ExpectedFileVersion::Match { version_token }, Some(content)) => {
            let current = workspace_revision(content);
            if current == *version_token {
                Ok(())
            } else {
                Err(TalosError::public(
                    talos_error::ErrorCode::Conflict,
                    "file version token mismatch".to_string(),
                ))
            }
        }
    }
}
