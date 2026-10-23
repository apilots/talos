use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use tokio::fs;

use crate::facade::ToolSandboxExecution;
use crate::manager::ToolSandboxContext;
use crate::process::ToolExecutionFuture;
use crate::{
    ToolSandboxAuditEvent, ToolSandboxAuditOutcome, ToolSandboxAuditStage, ToolSandboxEnforcement,
};

mod authorization;

use authorization::{
    authorize_read, authorize_write, file_system_error, metadata, read_directory,
    require_direct_access,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ToolFileMetadata {
    pub is_file: bool,
    pub is_directory: bool,
    pub is_symlink: bool,
    pub len: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ToolDirectoryEntry {
    pub path: PathBuf,
    pub file_name: OsString,
}

/// Invocation-bound filesystem access for built-in Talos tools.
///
/// The handle owns its sandbox context and does not allow callers to replace
/// policy data or select the underlying filesystem implementation.
#[derive(Clone)]
pub struct ToolFileSystemHandle {
    context: Arc<ToolSandboxContext>,
    file_system: Arc<dyn ToolFileSystem>,
    execution: Arc<ToolSandboxExecution>,
}

impl std::fmt::Debug for ToolFileSystemHandle {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("ToolFileSystemHandle")
    }
}

impl ToolFileSystemHandle {
    pub(crate) fn new(
        context: Arc<ToolSandboxContext>,
        execution: Arc<ToolSandboxExecution>,
    ) -> Self {
        let file_system: Arc<dyn ToolFileSystem> = if context.is_noop() {
            Arc::new(DirectToolFileSystem)
        } else {
            Arc::new(AuthorizingToolFileSystem)
        };
        Self {
            context,
            file_system,
            execution,
        }
    }

    pub fn enforcement(&self) -> ToolSandboxEnforcement {
        if self.context.is_noop() {
            ToolSandboxEnforcement::None
        } else {
            ToolSandboxEnforcement::InProcessAuthorizer
        }
    }

    pub fn metadata<'a>(&'a self, path: &'a Path) -> ToolExecutionFuture<'a, ToolFileMetadata> {
        self.audited(self.file_system.metadata(path, &self.context))
    }

    pub fn read<'a>(&'a self, path: &'a Path) -> ToolExecutionFuture<'a, Vec<u8>> {
        self.audited(self.file_system.read(path, &self.context))
    }

    /// Opens a read-only descriptor after the same authorization and audit as
    /// read(). Callers may stream or seek without re-opening an unchecked path.
    pub fn open_read<'a>(&'a self, path: &'a Path) -> ToolExecutionFuture<'a, fs::File> {
        self.audited(self.file_system.open_read(path, &self.context))
    }

    pub fn read_to_string<'a>(&'a self, path: &'a Path) -> ToolExecutionFuture<'a, String> {
        self.audited(self.file_system.read_to_string(path, &self.context))
    }

    pub fn try_exists<'a>(&'a self, path: &'a Path) -> ToolExecutionFuture<'a, bool> {
        self.audited(self.file_system.try_exists(path, &self.context))
    }

    pub fn read_directory<'a>(
        &'a self,
        path: &'a Path,
    ) -> ToolExecutionFuture<'a, Vec<ToolDirectoryEntry>> {
        self.audited(self.file_system.read_directory(path, &self.context))
    }

    pub fn create_directory_all<'a>(&'a self, path: &'a Path) -> ToolExecutionFuture<'a, ()> {
        self.audited(self.file_system.create_directory_all(path, &self.context))
    }

    pub fn write<'a>(&'a self, path: &'a Path, contents: &'a [u8]) -> ToolExecutionFuture<'a, ()> {
        self.audited(self.file_system.write(path, contents, &self.context))
    }

    pub fn remove_file<'a>(&'a self, path: &'a Path) -> ToolExecutionFuture<'a, ()> {
        self.audited(self.file_system.remove_file(path, &self.context))
    }

    pub fn rename<'a>(&'a self, source: &'a Path, target: &'a Path) -> ToolExecutionFuture<'a, ()> {
        self.audited(self.file_system.rename(source, target, &self.context))
    }

    fn audited<'a, T>(&'a self, future: ToolExecutionFuture<'a, T>) -> ToolExecutionFuture<'a, T>
    where
        T: Send + 'a,
    {
        Box::pin(async move {
            let result = future.await;
            let (outcome, error_category) = match &result {
                Ok(_) => (ToolSandboxAuditOutcome::Succeeded, None),
                Err(error) => (ToolSandboxAuditOutcome::Failed, Some(error.category())),
            };
            self.execution.record(&ToolSandboxAuditEvent {
                invocation: self.context.invocation.clone(),
                stage: ToolSandboxAuditStage::FileSystem,
                outcome,
                user_mode: self.context.user_mode,
                effective_policy: Some(self.context.effective_policy.clone()),
                enforcement: Some(self.enforcement()),
                error_category,
            });
            result
        })
    }
}

pub(crate) trait ToolFileSystem: Send + Sync {
    fn open_read<'a>(
        &'a self,
        path: &'a Path,
        context: &'a ToolSandboxContext,
    ) -> ToolExecutionFuture<'a, fs::File>;
    fn metadata<'a>(
        &'a self,
        path: &'a Path,
        context: &'a ToolSandboxContext,
    ) -> ToolExecutionFuture<'a, ToolFileMetadata>;

    fn read<'a>(
        &'a self,
        path: &'a Path,
        context: &'a ToolSandboxContext,
    ) -> ToolExecutionFuture<'a, Vec<u8>>;

    fn read_to_string<'a>(
        &'a self,
        path: &'a Path,
        context: &'a ToolSandboxContext,
    ) -> ToolExecutionFuture<'a, String>;

    fn try_exists<'a>(
        &'a self,
        path: &'a Path,
        context: &'a ToolSandboxContext,
    ) -> ToolExecutionFuture<'a, bool>;

    fn read_directory<'a>(
        &'a self,
        path: &'a Path,
        context: &'a ToolSandboxContext,
    ) -> ToolExecutionFuture<'a, Vec<ToolDirectoryEntry>>;

    fn create_directory_all<'a>(
        &'a self,
        path: &'a Path,
        context: &'a ToolSandboxContext,
    ) -> ToolExecutionFuture<'a, ()>;

    fn write<'a>(
        &'a self,
        path: &'a Path,
        contents: &'a [u8],
        context: &'a ToolSandboxContext,
    ) -> ToolExecutionFuture<'a, ()>;

    fn remove_file<'a>(
        &'a self,
        path: &'a Path,
        context: &'a ToolSandboxContext,
    ) -> ToolExecutionFuture<'a, ()>;

    fn rename<'a>(
        &'a self,
        source: &'a Path,
        target: &'a Path,
        context: &'a ToolSandboxContext,
    ) -> ToolExecutionFuture<'a, ()>;
}

struct DirectToolFileSystem;

impl ToolFileSystem for DirectToolFileSystem {
    fn open_read<'a>(
        &'a self,
        path: &'a Path,
        context: &'a ToolSandboxContext,
    ) -> ToolExecutionFuture<'a, fs::File> {
        Box::pin(async move {
            require_direct_access(context)?;
            fs::File::open(path).await.map_err(file_system_error)
        })
    }
    fn metadata<'a>(
        &'a self,
        path: &'a Path,
        context: &'a ToolSandboxContext,
    ) -> ToolExecutionFuture<'a, ToolFileMetadata> {
        Box::pin(async move {
            require_direct_access(context)?;
            metadata(path).await
        })
    }

    fn read<'a>(
        &'a self,
        path: &'a Path,
        context: &'a ToolSandboxContext,
    ) -> ToolExecutionFuture<'a, Vec<u8>> {
        Box::pin(async move {
            require_direct_access(context)?;
            fs::read(path).await.map_err(file_system_error)
        })
    }

    fn read_to_string<'a>(
        &'a self,
        path: &'a Path,
        context: &'a ToolSandboxContext,
    ) -> ToolExecutionFuture<'a, String> {
        Box::pin(async move {
            require_direct_access(context)?;
            fs::read_to_string(path).await.map_err(file_system_error)
        })
    }

    fn try_exists<'a>(
        &'a self,
        path: &'a Path,
        context: &'a ToolSandboxContext,
    ) -> ToolExecutionFuture<'a, bool> {
        Box::pin(async move {
            require_direct_access(context)?;
            fs::try_exists(path).await.map_err(file_system_error)
        })
    }

    fn read_directory<'a>(
        &'a self,
        path: &'a Path,
        context: &'a ToolSandboxContext,
    ) -> ToolExecutionFuture<'a, Vec<ToolDirectoryEntry>> {
        Box::pin(async move {
            require_direct_access(context)?;
            read_directory(path).await
        })
    }

    fn create_directory_all<'a>(
        &'a self,
        path: &'a Path,
        context: &'a ToolSandboxContext,
    ) -> ToolExecutionFuture<'a, ()> {
        Box::pin(async move {
            require_direct_access(context)?;
            fs::create_dir_all(path).await.map_err(file_system_error)
        })
    }

    fn write<'a>(
        &'a self,
        path: &'a Path,
        contents: &'a [u8],
        context: &'a ToolSandboxContext,
    ) -> ToolExecutionFuture<'a, ()> {
        Box::pin(async move {
            require_direct_access(context)?;
            fs::write(path, contents).await.map_err(file_system_error)
        })
    }

    fn remove_file<'a>(
        &'a self,
        path: &'a Path,
        context: &'a ToolSandboxContext,
    ) -> ToolExecutionFuture<'a, ()> {
        Box::pin(async move {
            require_direct_access(context)?;
            fs::remove_file(path).await.map_err(file_system_error)
        })
    }

    fn rename<'a>(
        &'a self,
        source: &'a Path,
        target: &'a Path,
        context: &'a ToolSandboxContext,
    ) -> ToolExecutionFuture<'a, ()> {
        Box::pin(async move {
            require_direct_access(context)?;
            fs::rename(source, target).await.map_err(file_system_error)
        })
    }
}

struct AuthorizingToolFileSystem;

impl ToolFileSystem for AuthorizingToolFileSystem {
    fn open_read<'a>(
        &'a self,
        path: &'a Path,
        context: &'a ToolSandboxContext,
    ) -> ToolExecutionFuture<'a, fs::File> {
        Box::pin(async move {
            let path = authorize_read(path, context)?;
            fs::File::open(path).await.map_err(file_system_error)
        })
    }
    fn metadata<'a>(
        &'a self,
        path: &'a Path,
        context: &'a ToolSandboxContext,
    ) -> ToolExecutionFuture<'a, ToolFileMetadata> {
        Box::pin(async move {
            let path = authorize_read(path, context)?;
            metadata(&path).await
        })
    }

    fn read<'a>(
        &'a self,
        path: &'a Path,
        context: &'a ToolSandboxContext,
    ) -> ToolExecutionFuture<'a, Vec<u8>> {
        Box::pin(async move {
            let path = authorize_read(path, context)?;
            fs::read(path).await.map_err(file_system_error)
        })
    }

    fn read_to_string<'a>(
        &'a self,
        path: &'a Path,
        context: &'a ToolSandboxContext,
    ) -> ToolExecutionFuture<'a, String> {
        Box::pin(async move {
            let path = authorize_read(path, context)?;
            fs::read_to_string(path).await.map_err(file_system_error)
        })
    }

    fn try_exists<'a>(
        &'a self,
        path: &'a Path,
        context: &'a ToolSandboxContext,
    ) -> ToolExecutionFuture<'a, bool> {
        Box::pin(async move {
            let path = authorize_read(path, context)?;
            fs::try_exists(path).await.map_err(file_system_error)
        })
    }

    fn read_directory<'a>(
        &'a self,
        path: &'a Path,
        context: &'a ToolSandboxContext,
    ) -> ToolExecutionFuture<'a, Vec<ToolDirectoryEntry>> {
        Box::pin(async move {
            let path = authorize_read(path, context)?;
            read_directory(&path).await
        })
    }

    fn create_directory_all<'a>(
        &'a self,
        path: &'a Path,
        context: &'a ToolSandboxContext,
    ) -> ToolExecutionFuture<'a, ()> {
        Box::pin(async move {
            let path = authorize_write(path, context)?;
            fs::create_dir_all(path).await.map_err(file_system_error)
        })
    }

    fn write<'a>(
        &'a self,
        path: &'a Path,
        contents: &'a [u8],
        context: &'a ToolSandboxContext,
    ) -> ToolExecutionFuture<'a, ()> {
        Box::pin(async move {
            let path = authorize_write(path, context)?;
            let path = authorize_write(&path, context)?;
            fs::write(path, contents).await.map_err(file_system_error)
        })
    }

    fn remove_file<'a>(
        &'a self,
        path: &'a Path,
        context: &'a ToolSandboxContext,
    ) -> ToolExecutionFuture<'a, ()> {
        Box::pin(async move {
            let path = authorize_write(path, context)?;
            fs::remove_file(path).await.map_err(file_system_error)
        })
    }

    fn rename<'a>(
        &'a self,
        source: &'a Path,
        target: &'a Path,
        context: &'a ToolSandboxContext,
    ) -> ToolExecutionFuture<'a, ()> {
        Box::pin(async move {
            let source = authorize_write(source, context)?;
            let target = authorize_write(target, context)?;
            let source = authorize_write(&source, context)?;
            let target = authorize_write(&target, context)?;
            fs::rename(source, target).await.map_err(file_system_error)
        })
    }
}

#[cfg(test)]
#[path = "file_system_tests.rs"]
mod tests;
