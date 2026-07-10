//! Rendering target shared by the runtime terminal and deterministic tests.

use ratatui::{
    buffer::Buffer,
    layout::Rect,
    widgets::{StatefulWidget, Widget},
};

/// Minimal frame contract required by Talos TUI components.
///
/// Production rendering uses [`BufferFrame`], while existing visual tests use
/// Ratatui's native frame. Components must only render widgets and must not own
/// terminal lifecycle or viewport transitions.
pub(crate) trait RenderFrame {
    fn area(&self) -> Rect;

    fn render_widget<W>(&mut self, widget: W, area: Rect)
    where
        W: Widget;

    fn render_stateful_widget<W>(&mut self, widget: W, area: Rect, state: &mut W::State)
    where
        W: StatefulWidget;
}

impl RenderFrame for ratatui::Frame<'_> {
    fn area(&self) -> Rect {
        ratatui::Frame::area(self)
    }

    fn render_widget<W>(&mut self, widget: W, area: Rect)
    where
        W: Widget,
    {
        ratatui::Frame::render_widget(self, widget, area);
    }

    fn render_stateful_widget<W>(&mut self, widget: W, area: Rect, state: &mut W::State)
    where
        W: StatefulWidget,
    {
        ratatui::Frame::render_stateful_widget(self, widget, area, state);
    }
}

/// Frame backed by the terminal surface's current diff buffer.
pub(crate) struct BufferFrame<'a> {
    area: Rect,
    buffer: &'a mut Buffer,
}

impl<'a> BufferFrame<'a> {
    pub(crate) const fn new(area: Rect, buffer: &'a mut Buffer) -> Self {
        Self { area, buffer }
    }
}

impl RenderFrame for BufferFrame<'_> {
    fn area(&self) -> Rect {
        self.area
    }

    fn render_widget<W>(&mut self, widget: W, area: Rect)
    where
        W: Widget,
    {
        widget.render(area, self.buffer);
    }

    fn render_stateful_widget<W>(&mut self, widget: W, area: Rect, state: &mut W::State)
    where
        W: StatefulWidget,
    {
        widget.render(area, self.buffer, state);
    }
}
