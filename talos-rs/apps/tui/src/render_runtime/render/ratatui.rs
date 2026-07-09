//! Ratatui projection boundary for render runtime.

use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
};

use crate::render_runtime::common::style::{StyledLine, TextColor, TextStyle};

/// Converts a renderer-neutral line to a Ratatui line.
///
/// Phase 1 intentionally keeps styling minimal; style projection will expand as
/// existing helpers migrate into render runtime.
pub(crate) fn to_ratatui_line(line: &StyledLine) -> Line<'static> {
    Line::from(
        line.segments
            .iter()
            .map(|segment| Span::styled(segment.text.clone(), to_ratatui_style(segment.style)))
            .collect::<Vec<_>>(),
    )
}

fn to_ratatui_style(style: TextStyle) -> Style {
    let mut out = Style::default();
    if let Some(color) = to_ratatui_color(style.foreground) {
        out = out.fg(color);
    }
    if style.bold {
        out = out.add_modifier(Modifier::BOLD);
    }
    out
}

fn to_ratatui_color(color: TextColor) -> Option<Color> {
    match color {
        TextColor::Default => None,
        TextColor::Green => Some(Color::Green),
    }
}

#[cfg(test)]
mod tests {
    use super::to_ratatui_line;
    use crate::render_runtime::common::style::{StyledLine, StyledSegment, TextColor, TextStyle};
    use crate::utils::line_plain_text;

    #[test]
    fn preserves_plain_text_for_ratatui_projection() {
        let line = StyledLine::from_segments(vec![StyledSegment::new(
            "ok",
            TextStyle::foreground(TextColor::Green).bold(),
        )]);

        assert_eq!(line_plain_text(&to_ratatui_line(&line)), "ok");
    }
}
