//! Fixed metrics row above the input panel.

use ratatui::{layout::Rect, widgets::Paragraph};

use crate::{render::frame::RenderFrame, TuiApp};

/// Renders turn metrics without changing the composer geometry when metrics are absent.
pub(crate) fn render(app: &TuiApp, frame: &mut impl RenderFrame, area: Rect) {
    if area.is_empty() {
        return;
    }
    let Some(metrics) = app.turn_metrics_status_line(area.width as usize) else {
        return;
    };
    frame.render_widget(Paragraph::new(metrics), area);
}
