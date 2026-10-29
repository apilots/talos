//! Office document preparation behind the common preview capability.
//! Archives use the original metadata/download path without indexing or extraction.
use bytes::Bytes;
use process_wrap::tokio::{CommandWrap, KillOnDrop};
use std::{path::Path, process::Stdio, time::Duration};
use talos_types::{
    FilePreviewBytes, FilePreviewDocument, FilePreviewDocumentUnavailable as Unavailable,
    FilePreviewMetadata,
};

const PDF_PATH: &str = "__preview__/document.pdf";

pub(crate) struct PreparedDocument {
    pdf: Bytes,
}

pub(crate) fn needs_document(metadata: &FilePreviewMetadata) -> bool {
    office(metadata)
}
pub(crate) async fn prepare(
    metadata: &FilePreviewMetadata,
    bytes: Bytes,
    limit: u64,
) -> (FilePreviewDocument, Option<PreparedDocument>) {
    match convert_office(metadata, &bytes, limit).await {
        Ok(pdf) => (
            FilePreviewDocument::Pdf {
                resource_path: PDF_PATH.into(),
            },
            Some(PreparedDocument { pdf }),
        ),
        Err(reason) => (FilePreviewDocument::Unavailable { reason }, None),
    }
}
impl PreparedDocument {
    pub(crate) fn handles(&self, path: &str) -> bool {
        path == PDF_PATH
    }
    pub(crate) fn read(&self, offset: u64, limit: u32) -> FilePreviewBytes {
        let bytes = &self.pdf;
        let start = offset.min(bytes.len() as u64) as usize;
        let end = offset
            .saturating_add(u64::from(limit))
            .min(bytes.len() as u64) as usize;
        FilePreviewBytes {
            metadata: FilePreviewMetadata {
                name: "preview.pdf".into(),
                mime_type: "application/pdf".into(),
                size_bytes: bytes.len() as u64,
            },
            offset,
            data: bytes.slice(start..end),
        }
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
