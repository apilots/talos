use ratatui::{
    style::{Color, Style},
    text::{Line, Span},
};

use crate::types::{CopyProjection, RenderedChatLine};
use crate::utils::line_plain_text;

/// Indent a rendered line and apply a body style patch to all spans.
/// This is a pure function — does not access any TuiApp fields.
pub(crate) fn decorate_chat_line(
    line: Line<'static>,
    body_style: Style,
    indent: usize,
) -> RenderedChatLine {
    let indent_str = " ".repeat(indent);
    let mut spans: Vec<Span<'static>> = vec![];

    if !indent_str.is_empty() {
        spans.push(Span::raw(indent_str.clone()));
    }

    if line.spans.is_empty() {
        return RenderedChatLine {
            display: Line::from(spans),
            copy_projection: Some(CopyProjection {
                text: String::new(),
                display_start_col: indent,
            }),
        };
    }

    let copy_text = line_plain_text(&line);
    for span in line.spans {
        spans.push(Span::styled(
            span.content.to_string(),
            span.style.patch(body_style),
        ));
    }
    RenderedChatLine {
        display: Line::from(spans),
        copy_projection: Some(CopyProjection {
            text: copy_text,
            display_start_col: indent,
        }),
    }
}

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
