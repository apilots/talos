use talos_types::{ToolInputFile, ToolOutputFile};
pub(crate) fn completed_downloads(
    root: &std::path::Path,
) -> talos_error::TalosResult<Vec<ToolOutputFile>> {
    use std::io::Read;
    use talos_error::{ErrorCode, TalosError};
    let mut result = Vec::new();
    let mut total = 0usize;
    for entry in std::fs::read_dir(root)
        .map_err(|e| TalosError::internal_with_source("Read browser download directory.", e))?
    {
        let entry = entry
            .map_err(|e| TalosError::internal_with_source("Read browser download entry.", e))?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') || name.ends_with(".crdownload") || name.ends_with(".tmp") {
            continue;
        }
        let mut options = std::fs::OpenOptions::new();
        options.read(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.custom_flags(libc::O_NOFOLLOW);
        }
        if !entry
            .file_type()
            .map_err(|e| TalosError::internal_with_source("Inspect browser download.", e))?
            .is_file()
        {
            continue;
        }
        let mut file = options
            .open(entry.path())
            .map_err(|e| TalosError::internal_with_source("Open completed browser download.", e))?;
        if !file
            .metadata()
            .map_err(|e| TalosError::internal_with_source("Inspect completed download.", e))?
            .is_file()
        {
            continue;
        }
        let mut bytes = Vec::new();
        file.by_ref()
            .take((20 * 1024 * 1024 - total + 1) as u64)
            .read_to_end(&mut bytes)
            .map_err(|e| TalosError::internal_with_source("Read completed browser download.", e))?;
        total += bytes.len();
        if total > 20 * 1024 * 1024 || result.len() >= 5 {
            return Err(TalosError::new(
                ErrorCode::UploadTooLarge,
                "Collect at most five browser downloads totalling 20 MiB.",
            ));
        }
        result.push(ToolOutputFile {
            filename: name,
            bytes: bytes.into(),
        });
    }
    Ok(result)
}

pub(crate) fn prepare_upload(
    root: &std::path::Path,
    file: &ToolInputFile,
) -> talos_error::TalosResult<(tempfile::TempDir, std::path::PathBuf)> {
    use talos_error::TalosError;
    let directory = tempfile::Builder::new()
        .prefix(".upload-")
        .tempdir_in(root)
        .map_err(|e| TalosError::internal_with_source("Create private browser upload.", e))?;
    let name = std::path::Path::new(&file.filename)
        .file_name()
        .and_then(|n| n.to_str())
        .filter(|n| !n.is_empty() && *n != "." && *n != "..")
        .ok_or_else(|| {
            TalosError::new(
                talos_error::ErrorCode::InvalidRequest,
                "Invalid attachment filename.",
            )
        })?;
    let path = directory.path().join(name);
    use std::io::Write;
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut output = options
        .open(&path)
        .map_err(|e| TalosError::internal_with_source("Create authorized browser upload.", e))?;
    output
        .write_all(&file.bytes)
        .map_err(|e| TalosError::internal_with_source("Write authorized browser upload.", e))?;
    Ok((directory, path))
}

pub(crate) fn acknowledge_downloads(
    root: &std::path::Path,
    files: &[ToolOutputFile],
) -> talos_error::TalosResult<()> {
    use talos_error::TalosError;
    for file in files {
        if std::path::Path::new(&file.filename)
            .file_name()
            .and_then(|n| n.to_str())
            != Some(file.filename.as_str())
        {
            return Err(TalosError::new(
                talos_error::ErrorCode::InvalidRequest,
                "Invalid browser file receipt.",
            ));
        }
        if completed_downloads(root)?
            .iter()
            .any(|candidate| candidate.filename == file.filename && candidate.bytes == file.bytes)
        {
            std::fs::remove_file(root.join(&file.filename)).map_err(|e| {
                TalosError::internal_with_source("Remove persisted browser download.", e)
            })?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn completed_downloads_exclude_partial_directories_and_links() {
        let root = tempfile::tempdir().expect("root");
        let secret = tempfile::NamedTempFile::new().expect("secret");
        std::fs::write(root.path().join("report.csv"), "name,value\na,1").expect("download");
        std::fs::write(root.path().join("pending.crdownload"), "partial").expect("partial");
        std::fs::create_dir(root.path().join("folder")).expect("directory");
        #[cfg(unix)]
        std::os::unix::fs::symlink(secret.path(), root.path().join("link.txt")).expect("link");
        let files = completed_downloads(root.path()).expect("downloads");
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].filename, "report.csv");
    }
    #[test]
    fn upload_path_is_private_and_cannot_escape_its_temporary_directory() {
        let root = tempfile::tempdir().expect("root");
        let upload = ToolInputFile {
            file_id: "file".into(),
            filename: "../../report.csv".into(),
            bytes: bytes::Bytes::from_static(b"a,1"),
        };
        let (guard, path) = prepare_upload(root.path(), &upload).expect("upload");
        assert_eq!(path.parent(), Some(guard.path()));
        assert_eq!(std::fs::read(&path).expect("read"), b"a,1");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(&path).expect("stat").permissions().mode() & 0o777,
                0o600
            );
        }
        drop(guard);
        assert!(!path.exists());
    }
}
