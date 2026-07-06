use ratatui::{style::Color, text::Line};

use crate::types::{CopyProjection, RenderedChatLine};

pub(crate) fn blank_chat_line() -> RenderedChatLine {
    RenderedChatLine {
        display: Line::from(""),
        copy_projection: Some(CopyProjection {
            text: String::new(),
            display_start_col: 0,
        }),
    }
}

pub(crate) fn push_block_header_gap(lines: &mut Vec<RenderedChatLine>) {
    lines.push(blank_chat_line());
}

pub(crate) fn spinning_color(spinner_index: usize) -> Color {
    match spinner_index % 4 {
        0 => Color::Cyan,
        1 => Color::Blue,
        2 => Color::Magenta,
        _ => Color::Cyan,
    }
}
