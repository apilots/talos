use sha2::{Digest, Sha256};
use std::fs::{self, File, OpenOptions};
use std::io::{self, Cursor, Read};
use std::path::{Component, Path};
use talos_error::{ErrorCode, TalosError, TalosResult};
use zip::ZipArchive;

const MAX_PACKAGE_BYTES: u64 = 64 * 1024 * 1024;
const MAX_UNPACKED_BYTES: u64 = 256 * 1024 * 1024;
const MAX_ENTRIES: usize = 4096;

pub(crate) fn package_sha256(path: &Path) -> TalosResult<String> {
    Ok(digest(&read_package(path)?))
}

pub(super) fn materialize(path: &Path, sha256: &str, root: &Path) -> TalosResult<()> {
    let bytes = read_package(path)?;
    if digest(&bytes) != sha256 {
        return Err(TalosError::new(
            ErrorCode::Conflict,
            "The local plugin package changed while it was being imported.",
        ));
    }
    fs::create_dir_all(root).map_err(io_error)?;
    match path.extension().and_then(|extension| extension.to_str()) {
        Some("json") => fs::write(root.join("mcp.json"), bytes).map_err(io_error),
        Some("zip") => extract_zip(bytes, root),
        _ => Err(invalid(
            "A local plugin package must be a ZIP or an MCP JSON file.",
        )),
    }
}

fn read_package(path: &Path) -> TalosResult<Vec<u8>> {
    if !fs::metadata(path).map_err(io_error)?.is_file() {
        return Err(invalid("The local plugin package must be a regular file."));
    }
    let mut bytes = Vec::new();
    File::open(path)
        .map_err(io_error)?
        .take(MAX_PACKAGE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(io_error)?;
    if bytes.len() as u64 > MAX_PACKAGE_BYTES {
        return Err(invalid("The local plugin package exceeds 64 MiB."));
    }
    Ok(bytes)
}

fn extract_zip(bytes: Vec<u8>, root: &Path) -> TalosResult<()> {
    let mut archive = ZipArchive::new(Cursor::new(bytes))
        .map_err(|error| invalid("The plugin ZIP is invalid.").with_source(error))?;
    if archive.len() > MAX_ENTRIES {
        return Err(invalid("The plugin ZIP has too many entries."));
    }
    let mut top_level = None;
    let mut total = 0;
    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .map_err(|error| invalid("The plugin ZIP entry is invalid.").with_source(error))?;
        let path = entry
            .enclosed_name()
            .filter(|path| {
                path.components()
                    .all(|part| matches!(part, Component::Normal(_)))
            })
            .ok_or_else(|| invalid("The plugin ZIP contains an unsafe path."))?;
        let mut components = path.components();
        let first = components
            .next()
            .ok_or_else(|| invalid("The plugin ZIP contains an empty path."))?;
        if top_level.get_or_insert_with(|| first.as_os_str().to_owned()) != first.as_os_str()
            || (components.next().is_none() && !entry.is_dir())
        {
            return Err(invalid(
                "The plugin ZIP must contain one top-level directory.",
            ));
        }
        if entry.is_symlink() {
            return Err(invalid("The plugin ZIP must not contain symbolic links."));
        }
        let target = root.join(path);
        if entry.is_dir() {
            fs::create_dir_all(&target).map_err(io_error)?;
            continue;
        }
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent).map_err(io_error)?;
        }
        let size = entry.size();
        total += size.min(MAX_UNPACKED_BYTES + 1);
        if total > MAX_UNPACKED_BYTES {
            return Err(invalid("The unpacked plugin ZIP exceeds 256 MiB."));
        }
        let mut output = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&target)
            .map_err(io_error)?;
        let written = io::copy(&mut (&mut entry).take(size + 1), &mut output).map_err(io_error)?;
        if written != size {
            return Err(invalid("The plugin ZIP entry has an invalid size."));
        }
        #[cfg(unix)]
        if let Some(mode) = entry.unix_mode() {
            use std::os::unix::fs::PermissionsExt;
            output
                .set_permissions(fs::Permissions::from_mode(0o644 | (mode & 0o111)))
                .map_err(io_error)?;
        }
    }
    Ok(())
}

fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn invalid(message: &'static str) -> TalosError {
    TalosError::new(ErrorCode::InvalidRequest, message)
}

fn io_error(error: io::Error) -> TalosError {
    invalid("The local plugin package could not be read or prepared.").with_source(error)
}
