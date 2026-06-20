use std::fs::{self, File, OpenOptions};
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WriteMode {
    Replace,
    CreateNew,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WriteDurability {
    Buffered,
    File,
    FileAndDirectory,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FilePermissions {
    #[cfg(unix)]
    pub unix_mode: u32,
}

#[cfg(unix)]
impl FilePermissions {
    pub const fn unix(unix_mode: u32) -> Self {
        Self { unix_mode }
    }
}

#[cfg(not(unix))]
impl FilePermissions {
    pub const fn unix(_unix_mode: u32) -> Self {
        Self {}
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WriteOptions {
    pub mode: WriteMode,
    pub durability: WriteDurability,
    pub create_parent: bool,
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
    pub const fn create_new() -> Self {
        Self {
            mode: WriteMode::CreateNew,
            durability: WriteDurability::FileAndDirectory,
            create_parent: true,
            permissions: None,
        }
    }

    pub const fn with_permissions(mut self, permissions: FilePermissions) -> Self {
        self.permissions = Some(permissions);
        self
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AppendOptions {
    pub durability: WriteDurability,
    pub create_parent: bool,
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

pub trait FileSystem {
    fn read_to_string(&self, path: &Path) -> TalosResult<String>;
    fn read_optional_to_string(&self, path: &Path) -> TalosResult<Option<String>>;
    fn read_json<T: DeserializeOwned>(&self, path: &Path) -> TalosResult<T>;
    fn read_json_or_default<T: DeserializeOwned + Default>(&self, path: &Path) -> TalosResult<T>;

    fn write_bytes(&self, path: &Path, bytes: &[u8]) -> TalosResult<()>;
    fn write_bytes_with(&self, path: &Path, bytes: &[u8], options: WriteOptions)
        -> TalosResult<()>;
    fn write_string(&self, path: &Path, content: &str) -> TalosResult<()>;
    fn write_string_with(
        &self,
        path: &Path,
        content: &str,
        options: WriteOptions,
    ) -> TalosResult<()>;
    fn write_json_pretty<T: Serialize>(&self, path: &Path, value: &T) -> TalosResult<()>;
    fn write_json_pretty_with<T: Serialize>(
        &self,
        path: &Path,
        value: &T,
        options: WriteOptions,
    ) -> TalosResult<()>;

    fn append_bytes(&self, path: &Path, bytes: &[u8]) -> TalosResult<()>;
    fn append_bytes_with(
        &self,
        path: &Path,
        bytes: &[u8],
        options: AppendOptions,
    ) -> TalosResult<()>;
    fn append_string(&self, path: &Path, content: &str) -> TalosResult<()>;
    fn append_string_with(
        &self,
        path: &Path,
        content: &str,
        options: AppendOptions,
    ) -> TalosResult<()>;

    /// Create this directory and every missing parent directory.
    ///
    /// Use this before materializing a directory-owned resource such as a cache
    /// root, install root, or workspace directory. The operation is idempotent:
    /// an existing directory is treated as success, while a non-directory at
    /// the same path remains an error.
    fn create_dir_all(&self, path: &Path) -> TalosResult<()>;

    fn remove_file(&self, path: &Path) -> TalosResult<()>;

    /// Remove a directory tree when the whole subtree is owned by the caller.
    ///
    /// Use this for cache cleanup, temp directories, trash directories, or
    /// other paths whose contents are not shared with another subsystem. The
    /// operation is idempotent for missing paths. Do not use it to replace a
    /// directory that also has a state file without designing rollback or
    /// recovery at the business layer first.
    fn remove_dir_all(&self, path: &Path) -> TalosResult<()>;

    fn rename(&self, source: &Path, target: &Path) -> TalosResult<()>;
    fn lock_exclusive(&self, path: &Path) -> TalosResult<FileLock>;
    fn try_lock_exclusive(&self, path: &Path) -> TalosResult<Option<FileLock>>;
}

#[derive(Debug, Clone, Copy, Default)]
pub struct StdFileSystem;

static STD_FS: StdFileSystem = StdFileSystem;

pub fn std_fs() -> &'static StdFileSystem {
    &STD_FS
}

impl FileSystem for StdFileSystem {
    fn read_to_string(&self, path: &Path) -> TalosResult<String> {
        fs::read_to_string(path)
            .map_err(|err| storage_error(format!("read file `{}`: {err}", path.display())))
    }

    fn read_optional_to_string(&self, path: &Path) -> TalosResult<Option<String>> {
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
        let content = self.read_to_string(path)?;
        serde_json::from_str(&content).map_err(|err| {
            TalosError::SerializationError(format!("parse json file `{}`: {err}", path.display()))
        })
    }

    fn read_json_or_default<T: DeserializeOwned + Default>(&self, path: &Path) -> TalosResult<T> {
        let Some(content) = self.read_optional_to_string(path)? else {
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

    fn write_string(&self, path: &Path, content: &str) -> TalosResult<()> {
        self.write_bytes(path, content.as_bytes())
    }

    fn write_string_with(
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

    fn append_string(&self, path: &Path, content: &str) -> TalosResult<()> {
        self.append_bytes(path, content.as_bytes())
    }

    fn append_string_with(
        &self,
        path: &Path,
        content: &str,
        options: AppendOptions,
    ) -> TalosResult<()> {
        self.append_bytes_with(path, content.as_bytes(), options)
    }

    fn create_dir_all(&self, path: &Path) -> TalosResult<()> {
        fs::create_dir_all(path)
            .map_err(|err| storage_error(format!("create directory `{}`: {err}", path.display())))
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

    fn remove_dir_all(&self, path: &Path) -> TalosResult<()> {
        match fs::remove_dir_all(path) {
            Ok(()) => Ok(()),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(err) => Err(storage_error(format!(
                "remove directory `{}`: {err}",
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

    fn lock_exclusive(&self, path: &Path) -> TalosResult<FileLock> {
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

    fn try_lock_exclusive(&self, path: &Path) -> TalosResult<Option<FileLock>> {
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

#[derive(Debug)]
pub struct FileLock {
    path: PathBuf,
    file: Option<File>,
}

impl FileLock {
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

pub fn read_to_string(path: &Path) -> TalosResult<String> {
    std_fs().read_to_string(path)
}

pub fn read_optional_to_string(path: &Path) -> TalosResult<Option<String>> {
    std_fs().read_optional_to_string(path)
}

pub fn read_json<T: DeserializeOwned>(path: &Path) -> TalosResult<T> {
    std_fs().read_json(path)
}

pub fn read_json_or_default<T: DeserializeOwned + Default>(path: &Path) -> TalosResult<T> {
    std_fs().read_json_or_default(path)
}

pub fn write_bytes(path: &Path, bytes: &[u8]) -> TalosResult<()> {
    std_fs().write_bytes(path, bytes)
}

pub fn write_bytes_with(path: &Path, bytes: &[u8], options: WriteOptions) -> TalosResult<()> {
    std_fs().write_bytes_with(path, bytes, options)
}

pub fn write_string(path: &Path, content: &str) -> TalosResult<()> {
    std_fs().write_string(path, content)
}

pub fn write_string_with(path: &Path, content: &str, options: WriteOptions) -> TalosResult<()> {
    std_fs().write_string_with(path, content, options)
}

pub fn write_json_pretty<T: Serialize>(path: &Path, value: &T) -> TalosResult<()> {
    std_fs().write_json_pretty(path, value)
}

pub fn write_json_pretty_with<T: Serialize>(
    path: &Path,
    value: &T,
    options: WriteOptions,
) -> TalosResult<()> {
    std_fs().write_json_pretty_with(path, value, options)
}

pub fn append_string(path: &Path, content: &str) -> TalosResult<()> {
    std_fs().append_string(path, content)
}

pub fn create_dir_all(path: &Path) -> TalosResult<()> {
    std_fs().create_dir_all(path)
}

pub fn remove_file(path: &Path) -> TalosResult<()> {
    std_fs().remove_file(path)
}

pub fn remove_dir_all(path: &Path) -> TalosResult<()> {
    std_fs().remove_dir_all(path)
}

pub fn rename(source: &Path, target: &Path) -> TalosResult<()> {
    std_fs().rename(source, target)
}

pub fn lock_exclusive(path: &Path) -> TalosResult<FileLock> {
    std_fs().lock_exclusive(path)
}

pub fn try_lock_exclusive(path: &Path) -> TalosResult<Option<FileLock>> {
    std_fs().try_lock_exclusive(path)
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

fn storage_error(message: String) -> TalosError {
    TalosError::StorageError(message)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Serialize, serde::Deserialize, PartialEq, Eq)]
    struct Sample {
        name: String,
    }

    #[test]
    fn write_string_replaces_file_contents() {
        let dir = tempfile::tempdir().expect("temp dir should be created");
        let path = dir.path().join("state.txt");

        write_string(&path, "old").expect("initial write should succeed");
        write_string(&path, "new").expect("replace write should succeed");

        assert_eq!(std::fs::read_to_string(path).unwrap(), "new");
    }

    #[test]
    fn create_new_rejects_existing_file() {
        let dir = tempfile::tempdir().expect("temp dir should be created");
        let path = dir.path().join("state.txt");
        write_string(&path, "old").expect("initial write should succeed");

        let err = write_string_with(&path, "new", WriteOptions::create_new())
            .expect_err("create-new should reject existing files");

        assert!(matches!(err, TalosError::Conflict(_)));
        assert_eq!(std::fs::read_to_string(path).unwrap(), "old");
    }

    #[test]
    fn json_helpers_round_trip_pretty_json() {
        let dir = tempfile::tempdir().expect("temp dir should be created");
        let path = dir.path().join("state.json");

        write_json_pretty(
            &path,
            &Sample {
                name: "demo".to_string(),
            },
        )
        .expect("json write should succeed");

        let value: Sample = read_json(&path).expect("json read should succeed");
        assert_eq!(
            value,
            Sample {
                name: "demo".to_string()
            }
        );
        assert!(std::fs::read_to_string(path)
            .expect("json file should be readable")
            .contains('\n'));
    }
}
