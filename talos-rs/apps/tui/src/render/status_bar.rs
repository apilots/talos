//! Fixed model, context, and directory status row.

use ratatui::{layout::Rect, style::Color, style::Style, widgets::Paragraph, widgets::Wrap};

use crate::{render::frame::RenderFrame, TuiApp};

/// Renders the status row without changing its geometry as runtime values change.
pub(crate) fn render(app: &TuiApp, frame: &mut impl RenderFrame, area: Rect) {
    if area.is_empty() {
        return;
    }
    let style = if app.in_flight {
        Style::default().fg(Color::Cyan)
    } else {
        Style::default()
    };
    let line = app.input_status_line(area.width as usize);
    frame.render_widget(
        Paragraph::new(line).style(style).wrap(Wrap { trim: false }),
        area,
    );
}
