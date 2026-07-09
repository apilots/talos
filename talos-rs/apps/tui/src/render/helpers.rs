use ratatui::{style::Color, text::Line};

use crate::types::RenderedChatLine;

pub(crate) fn blank_chat_line() -> RenderedChatLine {
    RenderedChatLine {
        display: Line::from(""),
    }
}

pub(crate) fn spinning_color(spinner_index: usize) -> Color {
    match spinner_index % 4 {
        0 => Color::Cyan,
        1 => Color::Blue,
        2 => Color::Magenta,
        _ => Color::Cyan,
    }
}
