use std::collections::BTreeMap;

use talos_error::{TalosError, TalosResult};
use talos_sdk::{FileScope, FileUploadRequest, ModelInputKind, SkillSuggestion};
use tokio::task::JoinHandle;

use crate::types::{PastedImageAttachment, PastedImageState};
use crate::utils::byte_index_at_char;
use crate::TuiApp;

impl TuiApp {
    pub(crate) fn compose_user_input_text(&self) -> TalosResult<String> {
        self.ordered_pasted_image_attachment_indices()?;
        let mut composed = self.input_buffer.clone();
        for block in &self.pasted_blocks {
            composed = composed.replacen(&block.marker, &block.text, 1);
        }
        for attachment in &self.pasted_image_attachments {
            composed = composed.replacen(&attachment.marker, "", 1);
        }
        Ok(composed.trim().to_string())
    }

    pub(crate) fn ordered_pasted_image_attachment_indices(&self) -> TalosResult<Vec<usize>> {
        let mut ordered = Vec::with_capacity(self.pasted_image_attachments.len());
        for (index, attachment) in self.pasted_image_attachments.iter().enumerate() {
            let positions = self
                .input_buffer
                .match_indices(&attachment.marker)
                .map(|(position, _)| position)
                .collect::<Vec<_>>();
            if positions.len() != 1 {
                return Err(TalosError::new(
                    talos_error::ErrorCode::InvalidRequest,
                    format!(
                        "pasted image marker `{}` is missing or duplicated",
                        attachment.marker
                    ),
                ));
            }
            ordered.push((positions[0], index));
        }
        ordered.sort_by_key(|(position, _)| *position);
        Ok(ordered.into_iter().map(|(_, index)| index).collect())
    }

    pub(crate) fn move_cursor_left_across_pasted_image(&mut self) {
        if let Some((start, _)) = self
            .pasted_image_marker_spans()
            .into_iter()
            .find(|(start, end)| *start < self.cursor_index && self.cursor_index <= *end)
        {
            self.cursor_index = start;
        } else {
            self.cursor_index = self.cursor_index.saturating_sub(1);
        }
    }

    pub(crate) fn move_cursor_right_across_pasted_image(&mut self) {
        let count = self.input_buffer.chars().count();
        if let Some((_, end)) = self
            .pasted_image_marker_spans()
            .into_iter()
            .find(|(start, end)| *start <= self.cursor_index && self.cursor_index < *end)
        {
            self.cursor_index = end;
        } else if self.cursor_index < count {
            self.cursor_index += 1;
        }
    }

    pub(crate) fn remove_pasted_image_before_cursor(&mut self) -> bool {
        let Some((index, start, end)) = self
            .pasted_image_attachment_spans()
            .into_iter()
            .find(|(_, start, end)| *start < self.cursor_index && self.cursor_index <= *end)
        else {
            return false;
        };
        let start_byte = byte_index_at_char(&self.input_buffer, start);
        let end_byte = byte_index_at_char(&self.input_buffer, end);
        self.input_buffer.drain(start_byte..end_byte);
        self.cursor_index = start;
        self.pasted_image_attachments.remove(index);
        true
    }

    pub(crate) fn move_cursor_out_of_pasted_image_marker(&mut self) {
        if let Some((_, end)) = self
            .pasted_image_marker_spans()
            .into_iter()
            .find(|(start, end)| *start < self.cursor_index && self.cursor_index < *end)
        {
            self.cursor_index = end;
        }
    }

    fn pasted_image_marker_spans(&self) -> Vec<(usize, usize)> {
        self.pasted_image_attachment_spans()
            .into_iter()
            .map(|(_, start, end)| (start, end))
            .collect()
    }

    fn pasted_image_attachment_spans(&self) -> Vec<(usize, usize, usize)> {
        self.pasted_image_attachments
            .iter()
            .enumerate()
            .filter_map(|(index, attachment)| {
                self.input_buffer
                    .find(&attachment.marker)
                    .map(|byte_start| {
                        let start = self.input_buffer[..byte_start].chars().count();
                        let end = start + attachment.marker.chars().count();
                        (index, start, end)
                    })
            })
            .collect()
    }

    pub(crate) fn start_image_upload(&mut self, submission: ImageSubmission) -> TalosResult<()> {
        if self.pending_image_upload.is_some() {
            return Ok(());
        }
        let draft_text = self.compose_user_input_text()?;
        let ordered_indices = self.ordered_pasted_image_attachment_indices()?;
        if !self
            .current_model_info()
            .is_some_and(|model| model.model.input.contains(&ModelInputKind::Image))
        {
            return Err(TalosError::new(
                talos_error::ErrorCode::InvalidRequest,
                "the active model does not support image attachments",
            ));
        }

        let scope = self
            .current_project_id
            .clone()
            .map_or(FileScope::Personal, |project_id| FileScope::Project {
                project_id,
            });
        let client = self
            .gateway_client
            .clone()
            .ok_or_else(|| TalosError::internal("gateway client unavailable"))?;
        let attachments = self.pasted_image_attachments.clone();
        let uploads = ordered_indices
            .iter()
            .filter_map(|&index| {
                let attachment = &attachments[index];
                match &attachment.state {
                    PastedImageState::Pending { png_bytes } => {
                        Some((index, attachment.filename.clone(), png_bytes.clone()))
                    }
                    PastedImageState::Uploaded { .. } => None,
                }
            })
            .collect::<Vec<_>>();
        let task = tokio::spawn(async move {
            let mut uploaded = Vec::with_capacity(uploads.len());
            for (index, filename, png_bytes) in uploads {
                match client
                    .upload_file(
                        FileUploadRequest {
                            filename,
                            scope: scope.clone(),
                            tags: Vec::new(),
                            metadata: BTreeMap::new(),
                        },
                        png_bytes,
                    )
                    .await
                {
                    Ok(response) => uploaded.push((index, response.file.file_id)),
                    Err(error) => {
                        return UploadResult {
                            uploaded,
                            result: Err(error),
                        }
                    }
                }
            }
            UploadResult {
                uploaded,
                result: Ok(()),
            }
        });
        self.pending_image_upload = Some(PendingImageUpload {
            task,
            session_id: self.session_id.clone(),
            project_id: self.current_project_id.clone(),
            draft_text,
            submission: Some(submission),
            skills: self.selected_skills.clone(),
            attachments,
            ordered_indices,
        });
        Ok(())
    }

    pub(crate) async fn poll_image_upload(&mut self) {
        if !self
            .pending_image_upload
            .as_ref()
            .is_some_and(|upload| upload.task.is_finished())
        {
            return;
        }
        let mut upload = self
            .pending_image_upload
            .take()
            .expect("finished image upload");
        let result = (&mut upload.task).await;
        if self.session_id != upload.session_id
            || self.current_project_id != upload.project_id
            || self.close_in_progress
        {
            return;
        }

        let UploadResult { uploaded, result } = match result {
            Ok(result) => result,
            Err(error) => {
                self.report_image_upload_failure(TalosError::internal_with_source(
                    "image upload task failed",
                    error,
                ));
                return;
            }
        };
        let draft_matches = self.pasted_image_attachments == upload.attachments
            && self.selected_skills == upload.skills
            && self
                .compose_user_input_text()
                .is_ok_and(|text| text == upload.draft_text)
            && self
                .ordered_pasted_image_attachment_indices()
                .is_ok_and(|indices| indices == upload.ordered_indices);
        for (index, file_id) in uploaded {
            if let Some(attachment) = self
                .pasted_image_attachments
                .iter_mut()
                .find(|attachment| **attachment == upload.attachments[index])
            {
                attachment.state = PastedImageState::Uploaded { file_id };
            }
        }
        if let Err(error) = result {
            self.report_image_upload_failure(error);
            return;
        }
        if !draft_matches {
            self.show_status_hint(
                "Draft changed during upload. Press Enter to send the updated draft.",
                std::time::Duration::from_secs(4),
            );
            return;
        }
        if !self
            .current_model_info()
            .is_some_and(|model| model.model.input.contains(&ModelInputKind::Image))
        {
            self.report_image_upload_failure(TalosError::new(
                talos_error::ErrorCode::InvalidRequest,
                "the active model does not support image attachments",
            ));
            return;
        }

        let file_ids = upload
            .ordered_indices
            .iter()
            .map(|&index| match &self.pasted_image_attachments[index].state {
                PastedImageState::Uploaded { file_id } => Ok(file_id.clone()),
                PastedImageState::Pending { .. } => Err(TalosError::internal(
                    "image upload did not produce a file id",
                )),
            })
            .collect::<TalosResult<Vec<_>>>();
        let file_ids = match file_ids {
            Ok(file_ids) => file_ids,
            Err(error) => {
                self.report_image_upload_failure(error);
                return;
            }
        };
        let result = match upload.submission.take().expect("image submission") {
            ImageSubmission::Message(text) => self.submit_composed_message(text, file_ids).await,
            ImageSubmission::Automation { prompt, skills } => {
                let result = self
                    .send_user_message_with_skills(prompt, skills, file_ids)
                    .await;
                if matches!(result, Ok(true)) {
                    self.reset_input();
                }
                result
            }
        };
        if let Err(error) = result {
            self.report_input_failure(error);
        }
    }

    pub(crate) fn report_image_upload_failure(&mut self, error: TalosError) {
        self.push_system_notice(format!(
            "Failed to upload image attachment: {}",
            error.to_api_error().message
        ));
        self.push_right_event("image attachment upload failed".to_string());
    }
}

pub(crate) enum ImageSubmission {
    Message(String),
    Automation {
        prompt: String,
        skills: Vec<SkillSuggestion>,
    },
}

pub(crate) struct PendingImageUpload {
    task: JoinHandle<UploadResult>,
    session_id: Option<String>,
    project_id: Option<String>,
    draft_text: String,
    submission: Option<ImageSubmission>,
    skills: Vec<SkillSuggestion>,
    attachments: Vec<PastedImageAttachment>,
    ordered_indices: Vec<usize>,
}

impl Drop for PendingImageUpload {
    fn drop(&mut self) {
        self.task.abort();
    }
}

struct UploadResult {
    uploaded: Vec<(usize, String)>,
    result: TalosResult<()>,
}
