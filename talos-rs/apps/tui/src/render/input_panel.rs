//! Bounded composer input panel.

use ratatui::{layout::Rect, style::Style, widgets::Block, widgets::Paragraph};

use crate::{render::frame::RenderFrame, TuiApp};

const INPUT_BACKGROUND: ratatui::style::Color = ratatui::style::Color::Rgb(72, 76, 88);

/// Renders the draft inside the bounded input area and clips wrapped lines internally.
pub(crate) fn render(app: &mut TuiApp, frame: &mut impl RenderFrame, area: Rect) {
    app.input_area = Some(area);
    if area.is_empty() {
        return;
    }
    let style = Style::default().bg(INPUT_BACKGROUND);
    frame.render_widget(Block::default().style(style), area);
    let lines = app.input_lines(area.width as usize, area.height as usize);
    frame.render_widget(Paragraph::new(lines).style(style), area);
}
