//! Roll back a failed, still-empty log setup while the Workspace lock is held.
use super::*;

pub(super) fn rollback(directory: Option<&Path>) -> TalosResult<()> {
    let Some(directory) = directory else {
        return Ok(());
    };
    for entry in fs::read_dir(directory).map_err(io_error)? {
        let entry = entry.map_err(io_error)?;
        let metadata = entry.metadata().map_err(io_error)?;
        let known = matches!(
            entry.file_name().to_str(),
            Some("output.log" | "metadata.json" | "capture.lock")
        );
        if !known
            || !metadata.is_file()
            || (entry.file_name() == "output.log" && metadata.len() != 0)
        {
            return Err(TalosError::new(
                ErrorCode::InvalidRequest,
                "Bash log setup rollback found unexpected data; quota remains reserved.",
            ));
        }
    }
    for name in ["output.log", "metadata.json", "capture.lock"] {
        match fs::remove_file(directory.join(name)) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(io_error(error)),
        }
    }
    fs::remove_dir(directory).map_err(io_error)
}
