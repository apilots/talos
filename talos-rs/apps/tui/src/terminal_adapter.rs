//! Product terminal adapter built on the single-owner inline surface.

use std::io;

use crossterm::{execute, terminal::EnterAlternateScreen, terminal::LeaveAlternateScreen};
use ratatui::{
    backend::{Backend, CrosstermBackend},
    layout::Rect,
    text::Line,
    widgets::{Paragraph, Widget},
};

use crate::{
    render::frame::BufferFrame,
    terminal_surface::{suppress_wide_symbol_placeholder_cells, InlineResizeMode, InlineTerminal},
    utils::wrap_line_visual,
};

/// Coordinates inline and alternate-screen geometry without changing backend ownership.
pub(crate) struct TerminalAdapter<B>
where
    B: Backend,
{
    surface: InlineTerminal<B>,
    saved_inline_viewport: Option<Rect>,
}

pub(crate) type PrimaryTerminal = TerminalAdapter<CrosstermBackend<io::Stdout>>;

impl PrimaryTerminal {
    pub(crate) fn new(height: u16) -> io::Result<Self> {
        Self::from_backend(CrosstermBackend::new(io::stdout()), height)
    }

    pub(crate) fn enter_transient_fullscreen(&mut self) -> io::Result<()> {
        if self.saved_inline_viewport.is_some() {
            if self.surface.sync_fullscreen_area()? {
                self.surface.clear_screen()?;
            }
            return Ok(());
        }
        execute!(self.surface.backend_mut(), EnterAlternateScreen)?;
        self.enter_fullscreen_geometry()?;
        self.surface.clear_screen()
    }

    pub(crate) fn exit_transient_fullscreen(
        &mut self,
        inline_height: u16,
        resize_mode: InlineResizeMode,
    ) -> io::Result<()> {
        self.leave_transient_fullscreen()?;
        self.surface.sync_inline_height(inline_height, resize_mode)
    }

    pub(crate) fn leave_transient_fullscreen(&mut self) -> io::Result<()> {
        if self.saved_inline_viewport.is_none() {
            return Ok(());
        }
        execute!(self.surface.backend_mut(), LeaveAlternateScreen)?;
        self.restore_inline_surface()
    }
}

impl<B> TerminalAdapter<B>
where
    B: Backend,
{
    pub(crate) fn from_backend(backend: B, height: u16) -> io::Result<Self> {
        Ok(Self {
            surface: InlineTerminal::new(backend, height)?,
            saved_inline_viewport: None,
        })
    }

    pub(crate) fn backend_mut(&mut self) -> &mut B {
        self.surface.backend_mut()
    }

    pub(crate) fn size(&self) -> io::Result<ratatui::layout::Size> {
        self.surface.size()
    }

    pub(crate) fn is_transient_fullscreen_active(&self) -> bool {
        self.saved_inline_viewport.is_some()
    }

    pub(crate) fn draw<F>(&mut self, render: F) -> io::Result<()>
    where
        F: FnOnce(&mut BufferFrame<'_>),
    {
        self.surface.draw(render)
    }

    pub(crate) fn clear_viewport(&mut self) -> io::Result<()> {
        self.surface.clear_viewport()
    }

    pub(crate) fn insert_history_lines(&mut self, lines: &[Line<'static>]) -> io::Result<()> {
        if lines.is_empty() {
            return Ok(());
        }
        let width = self.surface.viewport_area().width.max(1) as usize;
        let rows = lines
            .iter()
            .cloned()
            .flat_map(|line| wrap_line_visual(line, width))
            .collect::<Vec<_>>();
        let height = rows.len().min(u16::MAX as usize) as u16;
        self.surface.insert_before(height, |buffer| {
            Paragraph::new(rows).render(buffer.area, buffer);
            suppress_wide_symbol_placeholder_cells(buffer);
        })
    }

    fn enter_fullscreen_geometry(&mut self) -> io::Result<()> {
        if self.saved_inline_viewport.is_some() {
            return Ok(());
        }
        self.saved_inline_viewport = Some(self.surface.viewport_area());
        self.surface.sync_fullscreen_area()?;
        Ok(())
    }

    fn restore_inline_surface(&mut self) -> io::Result<()> {
        if let Some(viewport) = self.saved_inline_viewport.take() {
            self.surface.set_viewport_area(viewport);
            self.surface.clear_viewport()?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use ratatui::{backend::TestBackend, widgets::Paragraph};

    use super::TerminalAdapter;
    use crate::render::frame::RenderFrame;
    use crate::terminal_surface::InlineResizeMode;

    #[test]
    fn fullscreen_geometry_restores_the_exact_inline_viewport() {
        let backend = TestBackend::new(80, 24);
        let mut terminal = TerminalAdapter::from_backend(backend, 5).expect("terminal adapter");
        let inline = terminal.surface.viewport_area();

        terminal
            .enter_fullscreen_geometry()
            .expect("enter fullscreen geometry");
        assert_eq!(
            terminal.surface.viewport_area(),
            ratatui::layout::Rect::new(0, 0, 80, 24)
        );

        terminal
            .restore_inline_surface()
            .expect("restore inline surface");
        assert_eq!(terminal.surface.viewport_area(), inline);
        assert!(!terminal.is_transient_fullscreen_active());
    }

    #[test]
    fn repeated_fullscreen_cancel_cycles_do_not_change_inline_geometry() {
        let backend = TestBackend::new(80, 24);
        let mut terminal = TerminalAdapter::from_backend(backend, 5).expect("terminal adapter");
        let inline = terminal.surface.viewport_area();

        for _ in 0..3 {
            terminal
                .enter_fullscreen_geometry()
                .expect("enter fullscreen geometry");
            terminal
                .restore_inline_surface()
                .expect("restore inline surface");
        }

        assert_eq!(terminal.surface.viewport_area(), inline);
    }

    #[test]
    fn fullscreen_geometry_tracks_terminal_resize_and_restores_inline_mode() {
        let backend = TestBackend::new(80, 24);
        let mut terminal = TerminalAdapter::from_backend(backend, 5).expect("terminal adapter");

        terminal
            .enter_fullscreen_geometry()
            .expect("enter fullscreen geometry");
        terminal.surface.backend_mut().resize(100, 30);
        terminal
            .surface
            .sync_fullscreen_area()
            .expect("resize fullscreen geometry");
        assert_eq!(
            terminal.surface.viewport_area(),
            ratatui::layout::Rect::new(0, 0, 100, 30)
        );

        terminal
            .restore_inline_surface()
            .expect("restore inline surface");
        terminal
            .surface
            .sync_inline_height(5, InlineResizeMode::PreserveBottom)
            .expect("restore resized inline geometry");
        assert_eq!(terminal.surface.viewport_area().width, 100);
        assert_eq!(terminal.surface.viewport_area().height, 5);
    }

    #[test]
    fn restored_inline_surface_clears_stale_cells_before_redraw() {
        let height = 5;
        let backend = TestBackend::new(80, 24);
        let mut terminal =
            TerminalAdapter::from_backend(backend, height).expect("terminal adapter");

        terminal
            .draw(|frame| frame.render_widget(Paragraph::new("short-stale-tail"), frame.area()))
            .expect("initial draw");
        terminal
            .enter_fullscreen_geometry()
            .expect("enter fullscreen geometry");
        terminal
            .restore_inline_surface()
            .expect("restore inline surface");
        terminal
            .draw(|frame| frame.render_widget(Paragraph::new("short"), frame.area()))
            .expect("restored draw");

        let output = format!("{}", terminal.surface.backend_mut());
        assert!(!output.contains("stale-tail"));
        assert_eq!(output.matches("short").count(), 1);
    }
}
