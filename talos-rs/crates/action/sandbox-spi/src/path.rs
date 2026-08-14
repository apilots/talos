use std::ffi::OsString;
use std::path::{Component, Path, PathBuf};

use crate::SandboxProviderError;

/// An absolute, lexically normalized path accepted by the sandbox SPI.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct AbsolutePathBuf(PathBuf);

impl AbsolutePathBuf {
    pub fn new(path: PathBuf) -> Result<Self, SandboxProviderError> {
        if !path.is_absolute() {
            return Err(SandboxProviderError::InvalidIsolationSpec {
                reason: format!("path must be absolute: {}", path.display()),
            });
        }

        Ok(Self(normalize_absolute_path(&path)?))
    }

    pub fn as_path(&self) -> &Path {
        &self.0
    }

    pub fn into_path_buf(self) -> PathBuf {
        self.0
    }

    pub(crate) fn contains(&self, candidate: &Self) -> bool {
        candidate.as_path().starts_with(self.as_path())
    }
}

fn normalize_absolute_path(path: &Path) -> Result<PathBuf, SandboxProviderError> {
    let mut prefix = None;
    let mut has_root = false;
    let mut segments = Vec::<OsString>::new();

    for component in path.components() {
        match component {
            Component::Prefix(value) => prefix = Some(value.as_os_str().to_owned()),
            Component::RootDir => has_root = true,
            Component::CurDir => {}
            Component::ParentDir => {
                if segments.pop().is_none() {
                    return Err(SandboxProviderError::InvalidIsolationSpec {
                        reason: format!("path escapes its root: {}", path.display()),
                    });
                }
            }
            Component::Normal(value) => segments.push(value.to_owned()),
        }
    }

    let mut normalized = PathBuf::new();
    if let Some(prefix) = prefix {
        normalized.push(prefix);
    }
    if has_root {
        normalized.push(std::path::MAIN_SEPARATOR.to_string());
    }
    for segment in segments {
        normalized.push(segment);
    }
    Ok(normalized)
}
