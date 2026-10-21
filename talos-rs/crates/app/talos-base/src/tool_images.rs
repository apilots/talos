use std::sync::Arc;

use futures_util::future::BoxFuture;
use talos_agent::{AttachmentReader, TurnFileScope};
use talos_error::{TalosError, TalosResult};
use talos_llm_provider::{Attachment, AttachmentKind};
use talos_service::{Actor, FileService};
use talos_tools::ToolImageStore;
use talos_types::{FileInfo, FileScope, FileUploadRequest};

pub(crate) struct ToolImages(pub Arc<FileService>);

impl ToolImageStore for ToolImages {
    fn save_files<'a>(
        &'a self,
        user_id: &'a str,
        files: Vec<talos_types::ToolOutputFile>,
    ) -> BoxFuture<'a, TalosResult<Vec<FileInfo>>> {
        Box::pin(async move {
            let actor = Actor::regular(user_id);
            let mut saved = Vec::new();
            for file in files {
                saved.push(
                    self.0
                        .upload(
                            &actor,
                            FileUploadRequest {
                                filename: file.filename,
                                scope: FileScope::Personal,
                                tags: vec!["browser".into()],
                                metadata: Default::default(),
                            },
                            Box::pin(futures_util::stream::once(async move { Ok(file.bytes) })),
                        )
                        .await?
                        .file,
                );
            }
            Ok(saved)
        })
    }

    fn save<'a>(
        &'a self,
        user_id: &'a str,
        images: Vec<Attachment>,
    ) -> BoxFuture<'a, TalosResult<Vec<FileInfo>>> {
        Box::pin(async move {
            let actor = Actor::regular(user_id);
            let mut files = Vec::<FileInfo>::new();
            for image in images {
                let expected_mime_type = image.media_type;
                let response = self
                    .0
                    .upload(
                        &actor,
                        FileUploadRequest {
                            filename: image.filename,
                            scope: FileScope::Personal,
                            tags: vec!["mcp".into()],
                            metadata: Default::default(),
                        },
                        Box::pin(futures_util::stream::once(async move { Ok(image.bytes) })),
                    )
                    .await?;
                if response.file.mime_type != expected_mime_type {
                    return Err(TalosError::new(
                        talos_error::ErrorCode::UnsupportedMediaType,
                        "The MCP image bytes do not match its declared format.",
                    ));
                }
                files.push(response.file);
            }
            Ok(files)
        })
    }

    fn read<'a>(
        &'a self,
        user_id: &'a str,
        file_ids: &'a [String],
    ) -> BoxFuture<'a, TalosResult<Vec<Attachment>>> {
        Box::pin(async move {
            AttachmentReader::read(
                self.0.as_ref(),
                &TurnFileScope {
                    owner_user_id: user_id.into(),
                    project_id: None,
                },
                file_ids,
            )
            .await?
            .into_iter()
            .map(|file| {
                if !file.media_type.starts_with("image/") {
                    return Err(TalosError::new(
                        talos_error::ErrorCode::UnsupportedMediaType,
                        "The tool file is not an image.",
                    ));
                }
                Ok(Attachment::new(
                    AttachmentKind::Image,
                    file.filename,
                    file.media_type,
                    file.bytes,
                ))
            })
            .collect()
        })
    }
}

#[cfg(test)]
#[path = "tool_images_tests.rs"]
mod tests;
