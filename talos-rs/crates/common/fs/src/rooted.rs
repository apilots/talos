//! Read-only directory capabilities. Path resolution is delegated to cap-std.
use cap_std::fs::{Dir, OpenOptions};
use std::{
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
};
use talos_error::{ErrorCode, TalosError, TalosResult};

pub struct RootedDirectory {
    directory: Dir,
    original_root: PathBuf,
    canonical_root: PathBuf,
}

#[derive(Debug)]
pub struct RootedFileRead {
    pub path: PathBuf,
    pub size_bytes: u64,
    pub prefix: Vec<u8>,
    pub bytes: Vec<u8>,
}

impl RootedDirectory {
    /// The caller owns authorization of this root. Keep this object alive for
    /// the grant lifetime; replacing the original path cannot rebind the grant.
    pub fn open(root: &Path) -> TalosResult<Self> {
        let canonical_root = std::fs::canonicalize(root).map_err(read_error)?;
        let directory = Dir::open_ambient_dir(&canonical_root, cap_std::ambient_authority())
            .map_err(read_error)?;
        Ok(Self {
            directory,
            original_root: root.to_owned(),
            canonical_root,
        })
    }

    /// Resolve paths beneath the pinned directory, including relative symlinks.
    /// Absolute entry paths are accepted only beneath the original root.
    pub fn read(&self, path: &Path, range: std::ops::Range<u64>) -> TalosResult<RootedFileRead> {
        let relative = if path.is_absolute() {
            path.strip_prefix(&self.canonical_root)
                .or_else(|_| path.strip_prefix(&self.original_root))
                .map_err(|_| TalosError::authorization_denied())?
        } else {
            path
        };
        let path = relative.to_owned();
        if !self
            .directory
            .metadata(&path)
            .map_err(read_error)?
            .is_file()
        {
            return Err(TalosError::new(
                ErrorCode::InvalidRequest,
                "The preview resource must be a regular file.",
            ));
        }
        let mut options = OpenOptions::new();
        options.read(true);
        // Do not block if another process swaps the inspected file for a FIFO.
        #[cfg(unix)]
        {
            use cap_std::fs::OpenOptionsExt;
            options.custom_flags(libc::O_NONBLOCK);
        }
        let mut file = self
            .directory
            .open_with(&path, &options)
            .map_err(read_error)?;
        let metadata = file.metadata().map_err(read_error)?;
        if !metadata.is_file() {
            return Err(TalosError::new(
                ErrorCode::InvalidRequest,
                "The preview resource must be a regular file.",
            ));
        }
        let mut prefix = Vec::new();
        (&mut file)
            .take(64 * 1024)
            .read_to_end(&mut prefix)
            .map_err(read_error)?;
        file.seek(SeekFrom::Start(range.start))
            .map_err(read_error)?;
        let mut bytes = Vec::new();
        file.take(range.end.saturating_sub(range.start))
            .read_to_end(&mut bytes)
            .map_err(read_error)?;
        Ok(RootedFileRead {
            path,
            size_bytes: metadata.len(),
            prefix,
            bytes,
        })
    }
}
fn read_error(error: std::io::Error) -> TalosError {
    match error.kind() {
        std::io::ErrorKind::NotFound => TalosError::new(
            ErrorCode::ResourceNotFound,
            "The preview file no longer exists.",
        ),
        std::io::ErrorKind::PermissionDenied => TalosError::authorization_denied(),
        _ => TalosError::internal_with_source(
            "Read the preview resource; technical details were withheld.",
            error,
        ),
    }
}
