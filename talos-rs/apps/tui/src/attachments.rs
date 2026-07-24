use std::collections::BTreeMap;

use talos_error::{TalosError, TalosResult};
use talos_sdk::{FileScope, FileUploadRequest, ModelInputKind};

use crate::types::PastedImageState;
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
                return Err(TalosError::ValidationError(format!(
                    "pasted image marker `{}` is missing or duplicated",
                    attachment.marker
                )));
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

    pub(crate) async fn upload_pending_image_attachments(&mut self) -> TalosResult<Vec<String>> {
        if self.pasted_image_attachments.is_empty() {
            return Ok(Vec::new());
        }
        let ordered_indices = self.ordered_pasted_image_attachment_indices()?;
        if !self
            .current_model_info()
            .is_some_and(|model| model.model.input.contains(&ModelInputKind::Image))
        {
            return Err(TalosError::ValidationError(
                "the active model does not support image attachments".to_string(),
            ));
        }

        let scope = self
            .current_project_id
            .clone()
            .map_or(FileScope::Personal, |project_id| FileScope::Project {
                project_id,
            });
        for &index in &ordered_indices {
            let pending = match &self.pasted_image_attachments[index].state {
                PastedImageState::Pending { png_bytes } => Some((
                    self.pasted_image_attachments[index].filename.clone(),
                    png_bytes.clone(),
                )),
                PastedImageState::Uploaded { .. } => None,
            };
            let Some((filename, png_bytes)) = pending else {
                continue;
            };
            let client = self
                .gateway_client
                .clone()
                .ok_or_else(|| TalosError::ClientError("gateway client unavailable".to_string()))?;
            let response = client
                .upload_file(
                    FileUploadRequest {
                        filename,
                        scope: scope.clone(),
                        tags: Vec::new(),
                        metadata: BTreeMap::new(),
                    },
                    png_bytes,
                )
                .await?;
            self.pasted_image_attachments[index].state = PastedImageState::Uploaded {
                file_id: response.file.file_id,
            };
        }

        ordered_indices
            .into_iter()
            .map(|index| match &self.pasted_image_attachments[index].state {
                PastedImageState::Uploaded { file_id } => Ok(file_id.clone()),
                PastedImageState::Pending { .. } => Err(TalosError::ValidationError(
                    "pasted image upload did not produce a file id".to_string(),
                )),
            })
            .collect()
    }
}
