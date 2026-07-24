pub mod chat;
pub mod messages;
pub mod responses;

use base64::Engine;

pub(super) fn base64_data(bytes: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

pub(super) fn data_url(media_type: &str, bytes: &[u8]) -> String {
    format!("data:{media_type};base64,{}", base64_data(bytes))
}
