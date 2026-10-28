//! Document preparation behind the common preview capability. No format endpoint,
//! workspace extraction, or client-specific source reader is introduced here.
use bytes::Bytes;
use process_wrap::tokio::{CommandWrap, KillOnDrop};
use std::{
    collections::BTreeMap,
    io::{Cursor, Read},
    path::{Component, Path},
    process::Stdio,
    time::Duration,
};
use talos_error::{ErrorCode, TalosError, TalosResult};
use talos_types::{
    FilePreviewBytes, FilePreviewDocument, FilePreviewDocumentUnavailable as Unavailable,
    FilePreviewEntry, FilePreviewMetadata,
};

const MEMBER_PREFIX: &str = "__preview__/entries/";
const PDF_PATH: &str = "__preview__/document.pdf";
const MAX_ARCHIVE_ENTRIES: usize = 10_000;

#[derive(Clone, Copy)]
pub(crate) enum ArchiveKind {
    Zip,
    Tar,
    TarGzip,
}
pub(crate) enum PreparedDocument {
    Pdf(Bytes),
    Archive {
        bytes: Bytes,
        kind: ArchiveKind,
        limit: u64,
        members: BTreeMap<String, String>,
    },
}

pub(crate) fn needs_document(metadata: &FilePreviewMetadata) -> bool {
    office(metadata) || archive(metadata).is_some()
}
pub(crate) async fn prepare(
    metadata: &FilePreviewMetadata,
    bytes: Bytes,
    limit: u64,
) -> (FilePreviewDocument, Option<PreparedDocument>) {
    if office(metadata) {
        match convert_office(metadata, &bytes, limit).await {
            Ok(pdf) => (
                FilePreviewDocument::Pdf {
                    resource_path: PDF_PATH.into(),
                },
                Some(PreparedDocument::Pdf(pdf)),
            ),
            Err(reason) => (FilePreviewDocument::Unavailable { reason }, None),
        }
    } else if let Some(kind) = archive(metadata) {
        let content = bytes.clone();
        match tokio::task::spawn_blocking(move || archive_entries(&content, kind, limit)).await {
            Ok(Ok(entries)) => {
                let members = entries
                    .iter()
                    .map(|entry| (entry.resource_path.clone(), entry.path.clone()))
                    .collect();
                (
                    FilePreviewDocument::Archive { entries },
                    Some(PreparedDocument::Archive {
                        bytes,
                        kind,
                        limit,
                        members,
                    }),
                )
            }
            _ => (
                FilePreviewDocument::Unavailable {
                    reason: Unavailable::InvalidArchive,
                },
                None,
            ),
        }
    } else {
        (
            FilePreviewDocument::Unavailable {
                reason: Unavailable::ConversionFailed,
            },
            None,
        )
    }
}
impl PreparedDocument {
    pub(crate) fn handles(&self, path: &str) -> bool {
        match self {
            Self::Pdf(_) => path == PDF_PATH,
            Self::Archive { .. } => path.starts_with(MEMBER_PREFIX),
        }
    }
    pub(crate) fn read(
        &self,
        path: &str,
        offset: u64,
        limit: u32,
    ) -> TalosResult<FilePreviewBytes> {
        let (metadata, bytes) = match self {
            Self::Pdf(bytes) => (
                FilePreviewMetadata {
                    name: "preview.pdf".into(),
                    mime_type: "application/pdf".into(),
                    size_bytes: bytes.len() as u64,
                },
                bytes.clone(),
            ),
            Self::Archive {
                bytes,
                kind,
                limit: byte_limit,
                members,
            } => {
                let member = members.get(path).ok_or_else(invalid_archive)?;
                return read_member(bytes, *kind, member, offset, limit, *byte_limit);
            }
        };
        let start = offset.min(bytes.len() as u64) as usize;
        let end = offset
            .saturating_add(u64::from(limit))
            .min(bytes.len() as u64) as usize;
        Ok(FilePreviewBytes {
            metadata,
            offset,
            data: bytes.slice(start..end),
        })
    }
}
fn office(metadata: &FilePreviewMetadata) -> bool {
    metadata
        .mime_type
        .starts_with("application/vnd.openxmlformats-officedocument.")
        || metadata
            .mime_type
            .starts_with("application/vnd.oasis.opendocument.")
        || [
            "application/msword",
            "application/vnd.ms-excel",
            "application/vnd.ms-powerpoint",
        ]
        .contains(&metadata.mime_type.as_str())
}
fn archive(metadata: &FilePreviewMetadata) -> Option<ArchiveKind> {
    match metadata.mime_type.as_str() {
        "application/zip" | "application/x-zip-compressed" => Some(ArchiveKind::Zip),
        "application/x-tar" => Some(ArchiveKind::Tar),
        "application/gzip" | "application/x-gzip"
            if metadata.name.ends_with(".tar.gz") || metadata.name.ends_with(".tgz") =>
        {
            Some(ArchiveKind::TarGzip)
        }
        _ => None,
    }
}
pub(crate) fn mime_type(path: &Path, prefix: &[u8]) -> String {
    let guessed = mime_guess::from_path(path).first_or_octet_stream();
    infer::get(prefix)
        .filter(|kind| match (kind.mime_type(), guessed.as_ref()) {
            ("application/zip", mime) => {
                !(mime.starts_with("application/vnd.openxmlformats-officedocument.")
                    || mime.starts_with("application/vnd.oasis.opendocument."))
            }
            // A prefix may identify the compound container before its Office
            // directory. Keep metadata reads bounded, as for OOXML ZIP above.
            (
                "application/x-ole-storage",
                "application/msword" | "application/vnd.ms-excel" | "application/vnd.ms-powerpoint",
            ) => false,
            _ => true,
        })
        .map(|kind| kind.mime_type().to_owned())
        .unwrap_or_else(|| {
            // A .ts path can mean TypeScript or an MPEG transport stream. For
            // unrecognized bytes, prefer actual text over ambiguous binary hints.
            // The metadata prefix may stop inside a UTF-8 character; upload
            // admission still validates the complete body in FileService.
            let prefix = match std::str::from_utf8(prefix) {
                Err(error) if error.error_len().is_none() => &prefix[..error.valid_up_to()],
                _ => prefix,
            };
            if (guessed.type_() == "video" || guessed.as_ref() == "application/octet-stream")
                && crate::file::is_utf8_text(prefix)
            {
                "text/plain".into()
            } else {
                guessed.to_string()
            }
        })
}
fn member_metadata(path: &str, size: u64) -> FilePreviewMetadata {
    FilePreviewMetadata {
        name: path.rsplit('/').next().unwrap_or(path).into(),
        mime_type: mime_guess::from_path(path)
            .first_or_octet_stream()
            .to_string(),
        size_bytes: size,
    }
}
fn archive_entries(
    bytes: &Bytes,
    kind: ArchiveKind,
    limit: u64,
) -> TalosResult<Vec<FilePreviewEntry>> {
    let mut entries = BTreeMap::new();
    match kind {
        ArchiveKind::Zip => {
            let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).map_err(archive_error)?;
            if zip.len() > MAX_ARCHIVE_ENTRIES {
                return Err(invalid_archive());
            }
            let names = zip.file_names().map(str::to_owned).collect::<Vec<_>>();
            for name in names {
                let file = zip.by_name(&name).map_err(archive_error)?;
                if !file.is_file() || file.enclosed_name().is_none() {
                    continue;
                }
                if let Some(path) = member_path(Path::new(&name)) {
                    entries
                        .entry(path.clone())
                        .or_insert_with(|| entry(&name, &path, file.size()));
                }
            }
        }
        ArchiveKind::Tar | ArchiveKind::TarGzip => {
            let mut tar = tar_reader(bytes, kind, limit);
            for file in tar.entries().map_err(archive_error)? {
                let file = file.map_err(archive_error)?;
                if !file.header().entry_type().is_file() {
                    continue;
                }
                let path = file.path().map_err(archive_error)?;
                let Some(normalized) = member_path(&path) else {
                    continue;
                };
                let Some(name) = path.to_str() else {
                    continue;
                };
                entries
                    .entry(normalized.clone())
                    .or_insert_with(|| entry(name, &normalized, file.size()));
                if entries.len() > MAX_ARCHIVE_ENTRIES {
                    return Err(invalid_archive());
                }
            }
        }
    }
    Ok(entries.into_values().collect())
}
fn member_path(path: &Path) -> Option<String> {
    let mut parts = Vec::new();
    for component in path.components() {
        match component {
            Component::Normal(part) => parts.push(part.to_str()?),
            Component::CurDir => {}
            _ => return None,
        }
    }
    Some(parts.join("/"))
}
fn entry(name: &str, path: &str, size: u64) -> FilePreviewEntry {
    FilePreviewEntry {
        path: name.to_owned(),
        resource_path: format!("{MEMBER_PREFIX}{path}"),
        metadata: member_metadata(name, size),
    }
}
fn tar_reader(bytes: &Bytes, kind: ArchiveKind, limit: u64) -> tar::Archive<Box<dyn Read + '_>> {
    let reader: Box<dyn Read> = match kind {
        ArchiveKind::TarGzip => {
            Box::new(flate2::read::GzDecoder::new(Cursor::new(bytes)).take(limit.saturating_add(1)))
        }
        _ => Box::new(Cursor::new(bytes)),
    };
    tar::Archive::new(reader)
}
fn read_member(
    bytes: &Bytes,
    kind: ArchiveKind,
    name: &str,
    offset: u64,
    limit: u32,
    byte_limit: u64,
) -> TalosResult<FilePreviewBytes> {
    match kind {
        ArchiveKind::Zip => {
            let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).map_err(archive_error)?;
            let mut file = zip.by_name(name).map_err(archive_error)?;
            if !file.is_file() || file.enclosed_name().is_none() {
                return Err(invalid_archive());
            }
            let metadata = member_metadata(name, file.size());
            member_range(&mut file, metadata, offset, limit, byte_limit)
        }
        ArchiveKind::Tar | ArchiveKind::TarGzip => {
            let mut tar = tar_reader(bytes, kind, byte_limit);
            for file in tar.entries().map_err(archive_error)? {
                let mut file = file.map_err(archive_error)?;
                if file.header().entry_type().is_file()
                    && file.path().map_err(archive_error)?.to_str() == Some(name)
                {
                    let metadata = member_metadata(name, file.size());
                    return member_range(&mut file, metadata, offset, limit, byte_limit);
                }
            }
            Err(invalid_archive())
        }
    }
}
fn member_range(
    reader: &mut impl Read,
    mut metadata: FilePreviewMetadata,
    offset: u64,
    limit: u32,
    byte_limit: u64,
) -> TalosResult<FilePreviewBytes> {
    // Match Worker metadata reads: inspect only a bounded prefix, including for
    // metadata-only requests. Listing an archive never decompresses every member.
    let mut prefix = Vec::new();
    (&mut *reader)
        .take(64 * 1024)
        .read_to_end(&mut prefix)
        .map_err(archive_error)?;
    metadata.mime_type = mime_type(Path::new(&metadata.name), &prefix);
    let mut reader = Cursor::new(prefix).chain(reader);
    let mut bytes = Vec::new();
    if limit > 0 {
        if metadata.size_bytes > byte_limit {
            return Err(TalosError::new(
                ErrorCode::AttachmentLimitExceeded,
                "The archive member exceeds the preview byte limit.",
            ));
        }
        std::io::copy(
            &mut (&mut reader).take(offset.min(metadata.size_bytes)),
            &mut std::io::sink(),
        )
        .map_err(archive_error)?;
        reader
            .take(u64::from(limit))
            .read_to_end(&mut bytes)
            .map_err(archive_error)?;
    }
    Ok(FilePreviewBytes {
        metadata,
        offset,
        data: bytes.into(),
    })
}
fn invalid_archive() -> TalosError {
    TalosError::new(
        ErrorCode::InvalidRequest,
        "The archive member cannot be previewed.",
    )
}
fn archive_error(error: impl std::error::Error + Send + Sync + 'static) -> TalosError {
    TalosError::from_source(
        ErrorCode::InvalidRequest,
        "The archive could not be previewed.",
        error,
    )
}
async fn convert_office(
    metadata: &FilePreviewMetadata,
    bytes: &Bytes,
    limit: u64,
) -> Result<Bytes, Unavailable> {
    let temp = tempfile::tempdir().map_err(|_| Unavailable::ConversionFailed)?;
    let suffix = Path::new(&metadata.name)
        .extension()
        .and_then(|v| v.to_str())
        .unwrap_or("bin");
    let input = temp.path().join(format!("input.{suffix}"));
    tokio::fs::write(&input, bytes)
        .await
        .map_err(|_| Unavailable::ConversionFailed)?;
    let profile = url::Url::from_directory_path(temp.path().join("profile"))
        .map_err(|_| Unavailable::ConversionFailed)?;
    let mut command = tokio::process::Command::new("soffice");
    command
        .arg(format!("-env:UserInstallation={profile}"))
        .args([
            "--headless",
            "--nologo",
            "--nodefault",
            "--nofirststartwizard",
            "--convert-to",
            "pdf",
            "--outdir",
        ])
        .arg(temp.path())
        .arg(&input)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let mut command = CommandWrap::from(command);
    command.wrap(KillOnDrop);
    #[cfg(unix)]
    command.wrap(process_wrap::tokio::ProcessGroup::leader());
    #[cfg(windows)]
    command.wrap(process_wrap::tokio::JobObject);
    let mut child = command.spawn().map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            Unavailable::ConverterUnavailable
        } else {
            Unavailable::ConversionFailed
        }
    })?;
    let status = tokio::time::timeout(Duration::from_secs(90), child.wait())
        .await
        .map_err(|_| Unavailable::ConversionFailed)?
        .map_err(|_| Unavailable::ConversionFailed)?;
    if !status.success() {
        return Err(Unavailable::ConversionFailed);
    }
    let output = temp.path().join("input.pdf");
    if tokio::fs::metadata(&output)
        .await
        .map_err(|_| Unavailable::ConversionFailed)?
        .len()
        > limit
    {
        return Err(Unavailable::TooLarge);
    }
    let bytes = tokio::fs::read(output)
        .await
        .map_err(|_| Unavailable::ConversionFailed)?;
    Ok(bytes.into())
}

#[cfg(test)]
#[path = "preview_tests.rs"]
mod tests;
