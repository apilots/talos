//! Projection renderers for the render runtime.

pub(crate) mod history;
pub(crate) mod live;
pub(crate) mod ratatui;
pub(crate) mod timeline;
pub(crate) mod tool_copy;
pub(crate) mod tool_projection;

pub(crate) fn user_input_display(text: &str, file_ids: &[String]) -> String {
    match (text.trim().is_empty(), file_ids.len()) {
        (_, 0) => text.to_string(),
        (true, count) => format!("[{count} attachment(s)]"),
        (false, count) => format!("{text}\n[{count} attachment(s)]"),
    }
}
