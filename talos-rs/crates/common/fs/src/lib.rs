use std::fs::{self, File, OpenOptions};
use std::future::Future;
use std::io::Write;
use std::path::{Path, PathBuf};

use fs4::{FileExt, TryLockError};
use serde::de::DeserializeOwned;
use serde::Serialize;
use talos_error::{TalosError, TalosResult};

#[cfg(unix)]
use atomic_write_file::unix::OpenOptionsExt as AtomicUnixOpenOptionsExt;
#[cfg(unix)]
use std::os::unix::fs::{OpenOptionsExt as StdUnixOpenOptionsExt, PermissionsExt};

/// Selects how a write operation handles an existing target file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WriteMode {
    /// Replace the target file using atomic write semantics where supported.
    Replace,
    /// Create a new file and fail if the target already exists.
    CreateNew,
}

/// Controls how much durability a write operation requests from the OS.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WriteDurability {
    /// Leave flushing to the OS page cache.
    Buffered,
    /// Flush the written file content.
    File,
    /// Flush both the written file content and its parent directory.
    FileAndDirectory,
}

/// Platform-specific file permissions applied when a file is created or replaced.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FilePermissions {
    /// Unix permission bits such as `0o600`.
    #[cfg(unix)]
    pub unix_mode: u32,
}

#[cfg(unix)]
impl FilePermissions {
    /// Build Unix permissions from raw permission bits.
    pub const fn unix(unix_mode: u32) -> Self {
        Self { unix_mode }
    }
}

#[cfg(not(unix))]
impl FilePermissions {
    /// Accept Unix permission bits on non-Unix platforms as a no-op.
    pub const fn unix(_unix_mode: u32) -> Self {
        Self {}
    }
}

/// Options used by replace and create-new write operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WriteOptions {
    /// Controls whether writes replace existing files or require a new path.
    pub mode: WriteMode,
    /// Controls file and directory fsync behavior after writing.
    pub durability: WriteDurability,
    /// Creates the parent directory before writing when true.
    pub create_parent: bool,
    /// Applies explicit permissions to newly written files when provided.
    pub permissions: Option<FilePermissions>,
}

impl Default for WriteOptions {
    fn default() -> Self {
        Self {
            mode: WriteMode::Replace,
            durability: WriteDurability::FileAndDirectory,
            create_parent: true,
            permissions: None,
        }
    }
}

impl WriteOptions {
    /// Build options for creating a new file that must not already exist.
    pub const fn create_new() -> Self {
        Self {
            mode: WriteMode::CreateNew,
            durability: WriteDurability::FileAndDirectory,
            create_parent: true,
            permissions: None,
        }
    }

    /// Return options with explicit file permissions attached.
    pub const fn with_permissions(mut self, permissions: FilePermissions) -> Self {
        self.permissions = Some(permissions);
        self
    }
}

/// Options used by append operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AppendOptions {
    /// Controls file and directory fsync behavior after appending.
    pub durability: WriteDurability,
    /// Creates the parent directory before appending when true.
    pub create_parent: bool,
    /// Applies explicit permissions when the append creates a new file.
    pub permissions: Option<FilePermissions>,
}

impl Default for AppendOptions {
    fn default() -> Self {
        Self {
            durability: WriteDurability::File,
            create_parent: true,
            permissions: None,
        }
    }
}

/// Metadata for a file-system path without following a final symlink.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FileMetadata {
    /// Last modification time in Unix epoch milliseconds when available.
    pub modified_at_ms: Option<i64>,
    /// File size in bytes, or platform metadata size for non-files.
    pub size_bytes: u64,
    /// True when the path points to a regular file.
    pub is_file: bool,
    /// True when the path points to a directory.
    pub is_dir: bool,
    /// True when the path itself is a symlink.
    pub is_symlink: bool,
    /// True when the path permissions are read-only for the current metadata view.
    pub readonly: bool,
}

/// One direct child returned by directory listing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirectoryEntry {
    /// Absolute or relative child path as returned by the directory iterator.
    pub path: PathBuf,
    /// Metadata for the child path without following a final symlink.
    pub metadata: FileMetadata,
}

/// Synchronous filesystem abstraction used by infrastructure code and tests.
pub trait FileSystem {
    /// Read an optional UTF-8 text file, returning `None` when it is missing.
    fn read_file(&self, path: &Path) -> TalosResult<Option<String>>;
    /// Read and parse a required JSON file.
    fn read_json<T: DeserializeOwned>(&self, path: &Path) -> TalosResult<T>;
    /// Read and parse a JSON file, returning `T::default()` when it is missing.
    fn read_json_or_default<T: DeserializeOwned + Default>(&self, path: &Path) -> TalosResult<T>;

    /// Write bytes using default safe replace options.
    fn write_bytes(&self, path: &Path, bytes: &[u8]) -> TalosResult<()>;
    /// Write bytes using explicit write options.
    fn write_bytes_with(&self, path: &Path, bytes: &[u8], options: WriteOptions)
        -> TalosResult<()>;
    /// Write UTF-8 text using default safe replace options.
    fn write_file(&self, path: &Path, content: &str) -> TalosResult<()>;
    /// Write UTF-8 text using explicit write options.
    fn write_file_with(&self, path: &Path, content: &str, options: WriteOptions)
        -> TalosResult<()>;
    /// Serialize a value as pretty JSON using default safe replace options.
    fn write_json_pretty<T: Serialize>(&self, path: &Path, value: &T) -> TalosResult<()>;
    /// Serialize a value as pretty JSON using explicit write options.
    fn write_json_pretty_with<T: Serialize>(
        &self,
        path: &Path,
        value: &T,
        options: WriteOptions,
    ) -> TalosResult<()>;
    /// Copy a regular file, creating the target parent directory when needed.
    fn copy_file(&self, source: &Path, target: &Path) -> TalosResult<u64>;

    /// Append bytes using default append options.
    fn append_bytes(&self, path: &Path, bytes: &[u8]) -> TalosResult<()>;
    /// Append bytes using explicit append options.
    fn append_bytes_with(
        &self,
        path: &Path,
        bytes: &[u8],
        options: AppendOptions,
    ) -> TalosResult<()>;
    /// Append UTF-8 text using default append options.
    fn append_file(&self, path: &Path, content: &str) -> TalosResult<()>;
    /// Append UTF-8 text using explicit append options.
    fn append_file_with(
        &self,
        path: &Path,
        content: &str,
        options: AppendOptions,
    ) -> TalosResult<()>;

    /// Create a directory, optionally creating every missing parent directory.
    ///
    /// Use this before materializing a directory-owned resource such as a cache
    /// root, install root, or workspace directory. When `recursive` is true,
    /// an existing directory is treated as success, while a non-directory at
    /// the same path remains an error. When `recursive` is false, only the
    /// final directory segment is created.
    fn create_directory(&self, path: &Path, recursive: bool) -> TalosResult<()>;

    /// Remove a file, treating a missing file as success.
    fn remove_file(&self, path: &Path) -> TalosResult<()>;

    /// Remove a directory tree when the whole subtree is owned by the caller.
    ///
    /// Use this for cache cleanup, temp directories, trash directories, or
    /// other paths whose contents are not shared with another subsystem. The
    /// operation is idempotent for missing paths. Do not use it to replace a
    /// directory that also has a state file without designing rollback or
    /// recovery at the business layer first.
    fn remove_directory(&self, path: &Path) -> TalosResult<()>;

    /// Read metadata for a path, returning `None` when it is missing.
    fn get_file_metadata(&self, path: &Path) -> TalosResult<Option<FileMetadata>>;
    /// Rename or move a path.
    fn rename(&self, source: &Path, target: &Path) -> TalosResult<()>;
    /// Acquire an exclusive blocking lock for a lock file path.
    fn with_file_lock(&self, path: &Path) -> TalosResult<FileLock>;
    /// Try to acquire an exclusive lock without blocking.
    fn try_file_lock(&self, path: &Path) -> TalosResult<Option<FileLock>>;
}

/// Standard-library backed filesystem implementation.
#[derive(Debug, Clone, Copy, Default)]
pub struct StdFileSystem;

static STD_FS: StdFileSystem = StdFileSystem;

/// Return the process-wide standard filesystem implementation.
pub fn std_fs() -> &'static StdFileSystem {
    &STD_FS
}

impl FileSystem for StdFileSystem {
    fn read_file(&self, path: &Path) -> TalosResult<Option<String>> {
        match fs::read_to_string(path) {
            Ok(content) => Ok(Some(content)),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(err) => Err(storage_error(format!(
                "read file `{}`: {err}",
                path.display()
            ))),
        }
    }

    fn read_json<T: DeserializeOwned>(&self, path: &Path) -> TalosResult<T> {
        let content = self
            .read_file(path)?
            .ok_or_else(|| storage_error(format!("read file `{}`: not found", path.display())))?;
        serde_json::from_str(&content).map_err(|err| {
            TalosError::SerializationError(format!("parse json file `{}`: {err}", path.display()))
        })
    }

    fn read_json_or_default<T: DeserializeOwned + Default>(&self, path: &Path) -> TalosResult<T> {
        let Some(content) = self.read_file(path)? else {
            return Ok(T::default());
        };
        serde_json::from_str(&content).map_err(|err| {
            TalosError::SerializationError(format!("parse json file `{}`: {err}", path.display()))
        })
    }

    fn write_bytes(&self, path: &Path, bytes: &[u8]) -> TalosResult<()> {
        self.write_bytes_with(path, bytes, WriteOptions::default())
    }

    fn write_bytes_with(
        &self,
        path: &Path,
        bytes: &[u8],
        options: WriteOptions,
    ) -> TalosResult<()> {
        if options.create_parent {
            create_parent_dir(path)?;
        }

        match options.mode {
            WriteMode::Replace => write_replace(path, bytes, options)?,
            WriteMode::CreateNew => write_create_new(path, bytes, options)?,
        }

        Ok(())
    }

    fn write_file(&self, path: &Path, content: &str) -> TalosResult<()> {
        self.write_bytes(path, content.as_bytes())
    }

    fn write_file_with(
        &self,
        path: &Path,
        content: &str,
        options: WriteOptions,
    ) -> TalosResult<()> {
        self.write_bytes_with(path, content.as_bytes(), options)
    }

    fn write_json_pretty<T: Serialize>(&self, path: &Path, value: &T) -> TalosResult<()> {
        self.write_json_pretty_with(path, value, WriteOptions::default())
    }

    fn write_json_pretty_with<T: Serialize>(
        &self,
        path: &Path,
        value: &T,
        options: WriteOptions,
    ) -> TalosResult<()> {
        let payload = serde_json::to_vec_pretty(value).map_err(|err| {
            TalosError::SerializationError(format!(
                "serialize json file `{}`: {err}",
                path.display()
            ))
        })?;
        self.write_bytes_with(path, &payload, options)
    }

    fn copy_file(&self, source: &Path, target: &Path) -> TalosResult<u64> {
        create_parent_dir(target)?;
        fs::copy(source, target).map_err(|err| {
            storage_error(format!(
                "copy file `{}` to `{}`: {err}",
                source.display(),
                target.display()
            ))
        })
    }

    fn append_bytes(&self, path: &Path, bytes: &[u8]) -> TalosResult<()> {
        self.append_bytes_with(path, bytes, AppendOptions::default())
    }

    fn append_bytes_with(
        &self,
        path: &Path,
        bytes: &[u8],
        options: AppendOptions,
    ) -> TalosResult<()> {
        if options.create_parent {
            create_parent_dir(path)?;
        }
        let existed = path.exists();
        let mut open_options = OpenOptions::new();
        open_options.create(true).append(true);
        #[cfg(unix)]
        if let Some(permissions) = options.permissions {
            open_options.mode(permissions.unix_mode);
        }
        let mut file = open_options.open(path).map_err(|err| {
            storage_error(format!("open file `{}` for append: {err}", path.display()))
        })?;
        file.write_all(bytes)
            .map_err(|err| storage_error(format!("append file `{}`: {err}", path.display())))?;
        sync_file_if_needed(&file, options.durability, path)?;
        if !existed {
            apply_permissions(path, options.permissions)?;
            sync_parent_if_needed(path, options.durability)?;
        }
        Ok(())
    }

    fn append_file(&self, path: &Path, content: &str) -> TalosResult<()> {
        self.append_bytes(path, content.as_bytes())
    }

    fn append_file_with(
        &self,
        path: &Path,
        content: &str,
        options: AppendOptions,
    ) -> TalosResult<()> {
        self.append_bytes_with(path, content.as_bytes(), options)
    }

    fn create_directory(&self, path: &Path, recursive: bool) -> TalosResult<()> {
        let result = if recursive {
            fs::create_dir_all(path)
        } else {
            fs::create_dir(path)
        };
        result.map_err(|err| storage_error(format!("create directory `{}`: {err}", path.display())))
    }

    fn remove_file(&self, path: &Path) -> TalosResult<()> {
        match fs::remove_file(path) {
            Ok(()) => Ok(()),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(err) => Err(storage_error(format!(
                "remove file `{}`: {err}",
                path.display()
            ))),
        }
    }

    fn remove_directory(&self, path: &Path) -> TalosResult<()> {
        match fs::remove_dir_all(path) {
            Ok(()) => Ok(()),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(err) => Err(storage_error(format!(
                "remove directory `{}`: {err}",
                path.display()
            ))),
        }
    }

    fn get_file_metadata(&self, path: &Path) -> TalosResult<Option<FileMetadata>> {
        match fs::symlink_metadata(path) {
            Ok(metadata) => Ok(Some(file_metadata_from_std(metadata))),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(err) => Err(storage_error(format!(
                "read file metadata `{}`: {err}",
                path.display()
            ))),
        }
    }

    fn rename(&self, source: &Path, target: &Path) -> TalosResult<()> {
        fs::rename(source, target).map_err(|err| {
            storage_error(format!(
                "rename `{}` to `{}`: {err}",
                source.display(),
                target.display()
            ))
        })
    }

    fn with_file_lock(&self, path: &Path) -> TalosResult<FileLock> {
        create_parent_dir(path)?;
        let file = open_lock_file(path)?;
        FileExt::lock(&file).map_err(|err| {
            storage_error(format!("lock file `{}` exclusively: {err}", path.display()))
        })?;
        Ok(FileLock {
            path: path.to_path_buf(),
            file: Some(file),
        })
    }

    fn try_file_lock(&self, path: &Path) -> TalosResult<Option<FileLock>> {
        create_parent_dir(path)?;
        let file = open_lock_file(path)?;
        match FileExt::try_lock(&file) {
            Ok(()) => Ok(Some(FileLock {
                path: path.to_path_buf(),
                file: Some(file),
            })),
            Err(TryLockError::WouldBlock) => Ok(None),
            Err(TryLockError::Error(err)) => Err(storage_error(format!(
                "try lock file `{}` exclusively: {err}",
                path.display()
            ))),
        }
    }
}

/// Exclusive file lock released on drop or explicit unlock.
#[derive(Debug)]
pub struct FileLock {
    path: PathBuf,
    file: Option<File>,
}

impl FileLock {
    /// Release the lock before the guard is dropped.
    pub fn unlock(mut self) -> TalosResult<()> {
        let Some(file) = self.file.take() else {
            return Ok(());
        };
        FileExt::unlock(&file)
            .map_err(|err| storage_error(format!("unlock file `{}`: {err}", self.path.display())))
    }
}

impl Drop for FileLock {
    fn drop(&mut self) {
        if let Some(file) = self.file.take() {
            let _ = FileExt::unlock(&file);
        }
    }
}

/// Read a UTF-8 text file, returning `None` when the file does not exist.
pub async fn read_file(path: &Path) -> TalosResult<Option<String>> {
    let path = path.to_path_buf();
    tokio::task::spawn_blocking(move || std_fs().read_file(&path))
        .await
        .map_err(|err| storage_error(format!("join file read task: {err}")))?
}

/// Read and parse a required JSON file synchronously.
pub fn read_json<T: DeserializeOwned>(path: &Path) -> TalosResult<T> {
    std_fs().read_json(path)
}

/// Read and parse a JSON file synchronously, returning `T::default()` when it is missing.
pub fn read_json_or_default<T: DeserializeOwned + Default>(path: &Path) -> TalosResult<T> {
    std_fs().read_json_or_default(path)
}

/// Write bytes synchronously using default safe replace options.
pub fn write_bytes(path: &Path, bytes: &[u8]) -> TalosResult<()> {
    std_fs().write_bytes(path, bytes)
}

/// Write bytes synchronously using explicit write options.
pub fn write_bytes_with(path: &Path, bytes: &[u8], options: WriteOptions) -> TalosResult<()> {
    std_fs().write_bytes_with(path, bytes, options)
}

/// Write a UTF-8 text file using the default safe replace semantics.
pub async fn write_file(path: &Path, content: &str) -> TalosResult<()> {
    let path = path.to_path_buf();
    let content = content.to_string();
    tokio::task::spawn_blocking(move || std_fs().write_file(&path, &content))
        .await
        .map_err(|err| storage_error(format!("join file write task: {err}")))?
}

/// Serialize a value as pretty JSON synchronously using default safe replace options.
pub fn write_json_pretty<T: Serialize>(path: &Path, value: &T) -> TalosResult<()> {
    std_fs().write_json_pretty(path, value)
}

/// Serialize a value as pretty JSON synchronously using explicit write options.
pub fn write_json_pretty_with<T: Serialize>(
    path: &Path,
    value: &T,
    options: WriteOptions,
) -> TalosResult<()> {
    std_fs().write_json_pretty_with(path, value, options)
}

/// Copy a regular file synchronously, creating the target parent directory when needed.
pub fn copy_file(source: &Path, target: &Path) -> TalosResult<u64> {
    std_fs().copy_file(source, target)
}

/// Append UTF-8 text to a file, creating the file and parent directories when needed.
pub async fn append_file(path: &Path, content: &str) -> TalosResult<()> {
    let path = path.to_path_buf();
    let content = content.to_string();
    tokio::task::spawn_blocking(move || std_fs().append_file(&path, &content))
        .await
        .map_err(|err| storage_error(format!("join file append task: {err}")))?
}

/// Create a directory, optionally creating every missing parent directory.
pub fn create_directory(path: &Path, recursive: bool) -> TalosResult<()> {
    std_fs().create_directory(path, recursive)
}

/// Remove a file synchronously, treating a missing file as success.
pub fn remove_file(path: &Path) -> TalosResult<()> {
    std_fs().remove_file(path)
}

/// Remove an owned directory tree, treating a missing path as success.
pub async fn remove_directory(path: &Path) -> TalosResult<()> {
    let path = path.to_path_buf();
    tokio::task::spawn_blocking(move || std_fs().remove_directory(&path))
        .await
        .map_err(|err| storage_error(format!("join directory remove task: {err}")))?
}

/// Read generic file metadata, returning `None` when the file does not exist.
pub async fn get_file_metadata(path: &Path) -> TalosResult<Option<FileMetadata>> {
    let path = path.to_path_buf();
    tokio::task::spawn_blocking(move || std_fs().get_file_metadata(&path))
        .await
        .map_err(|err| storage_error(format!("join file metadata task: {err}")))?
}

/// Rename or move a path synchronously.
pub fn rename(source: &Path, target: &Path) -> TalosResult<()> {
    std_fs().rename(source, target)
}

/// Resolve a path to its canonical filesystem location.
pub fn canonicalize_path(path: &Path) -> TalosResult<PathBuf> {
    fs::canonicalize(path)
        .map_err(|err| storage_error(format!("canonicalize path `{}`: {err}", path.display())))
}

/// List direct children of a directory, returning `None` when it does not exist.
pub fn read_directory(path: &Path) -> TalosResult<Option<Vec<DirectoryEntry>>> {
    let dir = match fs::read_dir(path) {
        Ok(dir) => dir,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => {
            return Err(storage_error(format!(
                "read directory `{}`: {err}",
                path.display()
            )));
        }
    };
    let mut entries = Vec::new();
    for entry in dir {
        let entry = entry.map_err(|err| {
            storage_error(format!("read directory entry `{}`: {err}", path.display()))
        })?;
        let entry_path = entry.path();
        let metadata = fs::symlink_metadata(&entry_path).map_err(|err| {
            storage_error(format!(
                "read path metadata `{}`: {err}",
                entry_path.display()
            ))
        })?;
        entries.push(DirectoryEntry {
            path: entry_path,
            metadata: file_metadata_from_std(metadata),
        });
    }
    Ok(Some(entries))
}

/// Run an async operation while holding an exclusive sidecar lock for a target file.
pub async fn with_file_lock<T, F, Fut>(path: &Path, operation: F) -> TalosResult<T>
where
    F: FnOnce() -> Fut,
    Fut: Future<Output = TalosResult<T>>,
{
    let lock_path = sidecar_lock_path(path);
    let file_lock = tokio::task::spawn_blocking(move || std_fs().with_file_lock(&lock_path))
        .await
        .map_err(|err| storage_error(format!("join file lock task: {err}")))??;
    let result = operation().await;
    let unlock_result = file_lock.unlock();
    match (result, unlock_result) {
        (Ok(value), Ok(())) => Ok(value),
        (Err(err), _) => Err(err),
        (Ok(_), Err(err)) => Err(err),
    }
}

fn write_replace(path: &Path, bytes: &[u8], options: WriteOptions) -> TalosResult<()> {
    let mut open_options = atomic_write_file::OpenOptions::new();
    #[cfg(unix)]
    if let Some(permissions) = options.permissions {
        open_options.preserve_mode(false);
        open_options.mode(permissions.unix_mode);
    }
    let mut file = open_options.open(path).map_err(|err| {
        storage_error(format!("open file `{}` for replace: {err}", path.display()))
    })?;
    file.write_all(bytes)
        .map_err(|err| storage_error(format!("write file `{}`: {err}", path.display())))?;
    file.commit()
        .map_err(|err| storage_error(format!("commit file `{}`: {err}", path.display())))?;
    apply_permissions(path, options.permissions)?;
    sync_parent_if_needed(path, options.durability)?;
    Ok(())
}

fn write_create_new(path: &Path, bytes: &[u8], options: WriteOptions) -> TalosResult<()> {
    let mut open_options = OpenOptions::new();
    open_options.write(true).create_new(true);
    #[cfg(unix)]
    if let Some(permissions) = options.permissions {
        open_options.mode(permissions.unix_mode);
    }
    let mut file = open_options.open(path).map_err(|err| {
        if err.kind() == std::io::ErrorKind::AlreadyExists {
            TalosError::Conflict(format!("file already exists `{}`", path.display()))
        } else {
            storage_error(format!("create file `{}`: {err}", path.display()))
        }
    })?;
    file.write_all(bytes)
        .map_err(|err| storage_error(format!("write file `{}`: {err}", path.display())))?;
    sync_file_if_needed(&file, options.durability, path)?;
    apply_permissions(path, options.permissions)?;
    sync_parent_if_needed(path, options.durability)?;
    Ok(())
}

fn create_parent_dir(path: &Path) -> TalosResult<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|err| {
            storage_error(format!(
                "create parent directory `{}`: {err}",
                parent.display()
            ))
        })?;
    }
    Ok(())
}

fn open_lock_file(path: &Path) -> TalosResult<File> {
    OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .open(path)
        .map_err(|err| storage_error(format!("open lock file `{}`: {err}", path.display())))
}

fn sync_file_if_needed(file: &File, durability: WriteDurability, path: &Path) -> TalosResult<()> {
    match durability {
        WriteDurability::Buffered => Ok(()),
        WriteDurability::File | WriteDurability::FileAndDirectory => file
            .sync_all()
            .map_err(|err| storage_error(format!("sync file `{}`: {err}", path.display()))),
    }
}

fn sync_parent_if_needed(path: &Path, durability: WriteDurability) -> TalosResult<()> {
    if durability != WriteDurability::FileAndDirectory {
        return Ok(());
    }
    let Some(parent) = path.parent() else {
        return Ok(());
    };
    File::open(parent)
        .and_then(|file| file.sync_all())
        .map_err(|err| storage_error(format!("sync directory `{}`: {err}", parent.display())))
}

fn apply_permissions(path: &Path, permissions: Option<FilePermissions>) -> TalosResult<()> {
    #[cfg(unix)]
    if let Some(permissions) = permissions {
        fs::set_permissions(path, fs::Permissions::from_mode(permissions.unix_mode)).map_err(
            |err| storage_error(format!("set file permissions `{}`: {err}", path.display())),
        )?;
    }
    #[cfg(not(unix))]
    let _ = (path, permissions);
    Ok(())
}

fn file_metadata_from_std(metadata: fs::Metadata) -> FileMetadata {
    let file_type = metadata.file_type();
    FileMetadata {
        modified_at_ms: metadata
            .modified()
            .ok()
            .and_then(|value| value.duration_since(std::time::UNIX_EPOCH).ok())
            .and_then(|duration| i64::try_from(duration.as_millis()).ok()),
        size_bytes: metadata.len(),
        is_file: file_type.is_file(),
        is_dir: file_type.is_dir(),
        is_symlink: file_type.is_symlink(),
        readonly: metadata.permissions().readonly(),
    }
}

fn sidecar_lock_path(path: &Path) -> PathBuf {
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .map(|value| format!("{value}.lock"))
        .unwrap_or_else(|| "lock".to_string());
    path.with_extension(extension)
}

fn storage_error(message: String) -> TalosError {
    TalosError::StorageError(message)
}

#[cfg(test)]
#[path = "lib_tests.rs"]
mod tests;
