use std::io::Cursor;

use arboard::Clipboard;
use bytes::Bytes;
use image::{DynamicImage, ImageFormat, RgbaImage};
use talos_error::{TalosError, TalosResult};
use talos_sdk::ModelInputKind;

use crate::types::{PastedImageAttachment, PastedImageState};
use crate::TuiApp;

impl TuiApp {
    pub(crate) fn paste_clipboard_image(&mut self) {
        match read_clipboard_png().and_then(|png_bytes| self.handle_pasted_image(png_bytes)) {
            Ok(()) => {}
            Err(error) => {
                self.push_system_notice(format!("Failed to paste clipboard image: {error}"));
                self.show_status_hint(
                    "Clipboard image paste failed.",
                    std::time::Duration::from_secs(3),
                );
            }
        }
    }

    pub(crate) fn handle_pasted_image(&mut self, png_bytes: Bytes) -> TalosResult<()> {
        let supports_images = self
            .current_model_info()
            .is_some_and(|model| model.model.input.contains(&ModelInputKind::Image));
        if !supports_images {
            return Err(TalosError::public(
                talos_error::ErrorCode::InvalidRequest,
                "the active model does not support image attachments".to_string(),
            ));
        }
        if png_bytes.is_empty() {
            return Err(TalosError::public(
                talos_error::ErrorCode::InvalidRequest,
                "clipboard image data is empty".to_string(),
            ));
        }

        self.pasted_image_sequence = self.pasted_image_sequence.saturating_add(1);
        let image_id = self.pasted_image_sequence;
        let marker = format!("[Image #{image_id}]");
        let filename = format!("pasted-image-{image_id}.png");
        self.insert_text(&marker);
        self.pasted_image_attachments.push(PastedImageAttachment {
            marker,
            filename,
            state: PastedImageState::Pending { png_bytes },
        });
        Ok(())
    }
}

fn read_clipboard_png() -> TalosResult<Bytes> {
    let mut clipboard = Clipboard::new()
        .map_err(|error| TalosError::internal(format!("clipboard unavailable: {error}")))?;
    let image = clipboard
        .get_image()
        .map_err(|error| TalosError::internal(format!("clipboard has no image: {error}")))?;
    let rgba = RgbaImage::from_raw(
        image.width as u32,
        image.height as u32,
        image.bytes.into_owned(),
    )
    .ok_or_else(|| TalosError::internal("clipboard image dimensions are invalid".to_string()))?;
    let mut encoded = Cursor::new(Vec::new());
    DynamicImage::ImageRgba8(rgba)
        .write_to(&mut encoded, ImageFormat::Png)
        .map_err(|error| TalosError::internal(format!("clipboard PNG encoding failed: {error}")))?;
    Ok(Bytes::from(encoded.into_inner()))
}
