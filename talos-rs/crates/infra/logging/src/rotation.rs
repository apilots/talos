//! Size rotation runs on the existing log worker. Cooperating processes lock the
//! pathname and reopen the active file, so no writer keeps an archived descriptor.
use crate::LogRotation;
use std::{
    fs::{self, File, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
};

pub(crate) fn validate(limits: &LogRotation) -> talos_error::TalosResult<()> {
    if limits.max_bytes == 0 || limits.max_files < 2 {
        return Err(talos_error::TalosError::new(
            talos_error::ErrorCode::InvalidRequest,
            "logging.rotation requires positive max_bytes and max_files of at least two.",
        ));
    }
    Ok(())
}

pub(crate) struct RotatingFile {
    path: PathBuf,
    limits: LogRotation,
}

impl RotatingFile {
    pub(crate) fn open(path: PathBuf, limits: LogRotation) -> io::Result<Self> {
        let writer = Self { path, limits };
        let _lock = writer.lock()?;
        writer.active()?;
        writer.prune()?;
        Ok(writer)
    }

    fn lock(&self) -> io::Result<File> {
        let mut path = self.path.as_os_str().to_owned();
        path.push(".lock");
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path)?;
        lock.lock()?;
        // Closing this independently opened descriptor releases the OS lock on
        // every return/error path, including process exit. No extra worker exists.
        Ok(lock)
    }

    fn active(&self) -> io::Result<File> {
        OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
    }

    fn archives(&self) -> io::Result<Vec<(u64, PathBuf)>> {
        let name = self
            .path
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| io::Error::other("The log filename is invalid."))?;
        let prefix = format!("{name}.");
        let directory = self
            .path
            .parent()
            .filter(|path| !path.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        let mut archives = Vec::new();
        for entry in fs::read_dir(directory)? {
            let entry = entry?;
            if !entry.file_type()?.is_file() {
                continue;
            }
            if let Some(number) = entry
                .file_name()
                .to_str()
                .and_then(|name| name.strip_prefix(&prefix))
                .and_then(|suffix| suffix.parse::<u64>().ok())
            {
                archives.push((number, entry.path()));
            }
        }
        archives.sort_by_key(|(number, _)| *number);
        Ok(archives)
    }

    fn prune(&self) -> io::Result<()> {
        let archives = self.archives()?;
        let remove = archives.len().saturating_sub(self.limits.max_files - 1);
        for (_, path) in archives.into_iter().take(remove) {
            fs::remove_file(path)?;
        }
        Ok(())
    }
}

impl Write for RotatingFile {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.is_empty() {
            return Ok(0);
        }
        let _lock = self.lock()?;
        let file = self.active()?;
        let size = file.metadata()?.len();
        if size > 0 && size.saturating_add(bytes.len() as u64) > self.limits.max_bytes {
            drop(file);
            let next = self
                .archives()?
                .last()
                .map_or(Some(1), |(number, _)| number.checked_add(1))
                .ok_or_else(|| io::Error::other("Log archive sequence exhausted."))?;
            let mut archive = self.path.as_os_str().to_owned();
            archive.push(format!(".{next}"));
            fs::rename(&self.path, archive)?;
            // Keep the old file until the replacement record has been accepted.
            self.active()?.write_all(bytes)?;
            self.prune()?;
        } else {
            let mut file = file;
            file.write_all(bytes)?;
        }
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        let _lock = self.lock()?;
        self.active()?.flush()
    }
}

#[cfg(test)]
#[path = "rotation_tests.rs"]
mod tests;
