//! Stable composer surface layout and component assembly.

use ratatui::layout::Rect;

use crate::TuiApp;

use super::{frame::RenderFrame, input_panel, metrics_bar, status_bar};

pub(crate) const METRICS_HEIGHT: u16 = 1;
pub(crate) const MIN_INPUT_HEIGHT: u16 = 3;
pub(crate) const MAX_INPUT_HEIGHT: u16 = 8;
pub(crate) const STATUS_HEIGHT: u16 = 1;

/// Stable geometry for the bottom composer surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ComposerLayout {
    pub(crate) metrics: Rect,
    pub(crate) input: Rect,
    pub(crate) status: Rect,
}

impl ComposerLayout {
    /// Splits the composer surface into metrics, bounded input, and status regions.
    pub(crate) fn new(area: Rect, requested_input_height: u16) -> Self {
        let metrics_height = area.height.min(METRICS_HEIGHT);
        let input_height = area
            .height
            .saturating_sub(metrics_height)
            .min(requested_input_height.clamp(MIN_INPUT_HEIGHT, MAX_INPUT_HEIGHT));
        let status_height = area
            .height
            .saturating_sub(metrics_height)
            .saturating_sub(input_height)
            .min(STATUS_HEIGHT);
        Self {
            metrics: Rect::new(area.x, area.y, area.width, metrics_height),
            input: Rect::new(
                area.x,
                area.y.saturating_add(metrics_height),
                area.width,
                input_height,
            ),
            status: Rect::new(
                area.x,
                area.y.saturating_add(metrics_height + input_height),
                area.width,
                status_height,
            ),
        }
    }
}

pub(crate) fn input_height(app: &TuiApp, width: u16) -> u16 {
    (app.draft_input_lines(width.max(1) as usize)
        .len()
        .min(u16::MAX as usize) as u16)
        .clamp(MIN_INPUT_HEIGHT, MAX_INPUT_HEIGHT)
}

pub(crate) fn height(app: &TuiApp, width: u16) -> u16 {
    METRICS_HEIGHT + input_height(app, width) + STATUS_HEIGHT
}

/// Renders the stable composer surface from its independent components.
pub(crate) fn render(app: &mut TuiApp, frame: &mut impl RenderFrame, area: Rect) {
    let layout = ComposerLayout::new(area, input_height(app, area.width));
    metrics_bar::render(app, frame, layout.metrics);
    input_panel::render(app, frame, layout.input);
    status_bar::render(app, frame, layout.status);
}
