//! Single-owner inline terminal surface.
//!
//! Ratatui 0.28 fixes inline viewport height at terminal construction time.
//! Talos needs a bounded composer and growing live projection, so this module
//! owns the backend and diff buffers while allowing the viewport rectangle to
//! change without recreating the terminal or introducing another stdout owner.

use std::io;

use ratatui::{
    backend::Backend,
    buffer::{Buffer, Cell},
    layout::{Position, Rect, Size},
};

use crate::render::frame::BufferFrame;

/// Selects the terminal edge preserved while resizing the inline surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum InlineResizeMode {
    PreserveBottom,
    PrepareHistoryInsertion,
}

/// Owns one backend, one viewport, and the two buffers used for frame diffs.
pub(crate) struct InlineTerminal<B>
where
    B: Backend,
{
    backend: B,
    buffers: [Buffer; 2],
    current: usize,
    viewport_area: Rect,
    screen_size: Size,
}

impl<B> InlineTerminal<B>
where
    B: Backend,
{
    pub(crate) fn new(mut backend: B, height: u16) -> io::Result<Self> {
        let screen_size = normalized_size(backend.size()?);
        let viewport_area = reserve_inline_viewport(&mut backend, screen_size, height)?;
        Ok(Self {
            backend,
            buffers: [Buffer::empty(viewport_area), Buffer::empty(viewport_area)],
            current: 0,
            viewport_area,
            screen_size,
        })
    }

    pub(crate) fn backend_mut(&mut self) -> &mut B {
        &mut self.backend
    }

    pub(crate) fn size(&self) -> io::Result<Size> {
        self.backend.size()
    }

    pub(crate) const fn viewport_area(&self) -> Rect {
        self.viewport_area
    }

    pub(crate) fn set_viewport_area(&mut self, area: Rect) {
        self.viewport_area = area;
        self.buffers = [Buffer::empty(area), Buffer::empty(area)];
        self.current = 0;
    }

    pub(crate) fn sync_fullscreen_area(&mut self) -> io::Result<bool> {
        let size = normalized_size(self.backend.size()?);
        let area = Rect::new(
            /*x*/ 0,
            /*y*/ 0,
            size.width.max(1),
            size.height.max(1),
        );
        self.screen_size = size;
        if area == self.viewport_area {
            return Ok(false);
        }
        self.set_viewport_area(area);
        Ok(true)
    }

    /// Changes inline height in place while keeping history immediately above it.
    pub(crate) fn sync_inline_height(
        &mut self,
        requested_height: u16,
        resize_mode: InlineResizeMode,
    ) -> io::Result<()> {
        let screen_size = normalized_size(self.backend.size()?);
        let height = requested_height.clamp(1, screen_size.height.max(1));
        let previous_area = self.viewport_area;
        let next_y =
            if height < previous_area.height && resize_mode == InlineResizeMode::PreserveBottom {
                previous_area
                    .bottom()
                    .min(screen_size.height)
                    .saturating_sub(height)
            } else {
                previous_area.y
            };
        let mut next_area = Rect::new(/*x*/ 0, next_y, screen_size.width.max(1), height);

        if next_area.bottom() > screen_size.height {
            let overflow = next_area.bottom().saturating_sub(screen_size.height);
            self.scroll_screen_up(overflow, screen_size)?;
            next_area.y = screen_size.height.saturating_sub(height);
        }

        if next_area != previous_area {
            let clear_top = previous_area.top().min(next_area.top());
            self.clear_after(Position::new(/*x*/ 0, clear_top))?;
            self.set_viewport_area(next_area);
        }
        self.screen_size = screen_size;
        Ok(())
    }

    pub(crate) fn draw<F>(&mut self, render: F) -> io::Result<()>
    where
        F: FnOnce(&mut BufferFrame<'_>),
    {
        let area = self.viewport_area;
        let mut frame = BufferFrame::new(area, &mut self.buffers[self.current]);
        render(&mut frame);

        let previous = 1 - self.current;
        let updates = self.buffers[previous].diff(&self.buffers[self.current]);
        self.backend.draw(updates.into_iter())?;
        self.backend.flush()?;

        self.buffers[previous].reset();
        self.current = previous;
        Ok(())
    }

    /// Inserts rendered rows above the inline viewport and into terminal scrollback.
    pub(crate) fn insert_before<F>(&mut self, height: u16, render: F) -> io::Result<()>
    where
        F: FnOnce(&mut Buffer),
    {
        if height == 0 {
            return Ok(());
        }

        let area = Rect::new(/*x*/ 0, /*y*/ 0, self.viewport_area.width, height);
        let mut history = Buffer::empty(area);
        render(&mut history);
        let mut cells = history.content.as_slice();

        let mut drawn_height = i32::from(self.viewport_area.top());
        let mut remaining_height = i32::from(height);
        let viewport_height = i32::from(self.viewport_area.height);
        let screen_height = i32::from(self.screen_size.height);

        while remaining_height + viewport_height > screen_height {
            let to_draw = remaining_height.min(screen_height);
            let scroll_up = 0.max(drawn_height + to_draw - screen_height);
            self.scroll_screen_up(scroll_up as u16, self.screen_size)?;
            cells = self.draw_rows((drawn_height - scroll_up) as u16, to_draw as u16, cells)?;
            drawn_height += to_draw - scroll_up;
            remaining_height -= to_draw;
        }

        let scroll_up = 0.max(drawn_height + remaining_height + viewport_height - screen_height);
        self.scroll_screen_up(scroll_up as u16, self.screen_size)?;
        self.draw_rows(
            (drawn_height - scroll_up) as u16,
            remaining_height as u16,
            cells,
        )?;
        drawn_height += remaining_height - scroll_up;

        self.set_viewport_area(Rect {
            y: drawn_height as u16,
            ..self.viewport_area
        });
        self.clear_viewport()
    }

    pub(crate) fn clear_viewport(&mut self) -> io::Result<()> {
        self.clear_after(self.viewport_area.as_position())?;
        self.backend
            .set_cursor_position(self.viewport_area.as_position())?;
        self.backend.flush()
    }

    pub(crate) fn clear_screen(&mut self) -> io::Result<()> {
        self.backend
            .clear_region(ratatui::backend::ClearType::All)?;
        self.buffers[0].reset();
        self.buffers[1].reset();
        self.backend.flush()
    }

    fn clear_after(&mut self, position: Position) -> io::Result<()> {
        self.backend.set_cursor_position(position)?;
        self.backend
            .clear_region(ratatui::backend::ClearType::AfterCursor)?;
        self.buffers[1 - self.current].reset();
        Ok(())
    }

    fn scroll_screen_up(&mut self, rows: u16, screen_size: Size) -> io::Result<()> {
        if rows == 0 {
            return Ok(());
        }
        self.backend.set_cursor_position(Position::new(
            /*x*/ 0,
            screen_size.height.saturating_sub(1),
        ))?;
        self.backend.append_lines(rows)
    }

    fn draw_rows<'a>(
        &mut self,
        y_offset: u16,
        row_count: u16,
        cells: &'a [Cell],
    ) -> io::Result<&'a [Cell]> {
        let width = self.viewport_area.width as usize;
        let cell_count = width.saturating_mul(row_count as usize);
        let (to_draw, remainder) = cells.split_at(cell_count);
        if row_count > 0 {
            let updates = to_draw.iter().enumerate().map(|(index, cell)| {
                (
                    (index % width) as u16,
                    y_offset + (index / width) as u16,
                    cell,
                )
            });
            self.backend.draw(updates)?;
            self.backend.flush()?;
        }
        Ok(remainder)
    }
}

fn reserve_inline_viewport<B>(
    backend: &mut B,
    screen_size: Size,
    requested_height: u16,
) -> io::Result<Rect>
where
    B: Backend,
{
    let cursor = backend.get_cursor_position()?;
    let height = requested_height.clamp(1, screen_size.height.max(1));
    let rows_after_cursor = height.saturating_sub(1);
    backend.append_lines(rows_after_cursor)?;

    let available_rows = screen_size
        .height
        .saturating_sub(cursor.y)
        .saturating_sub(1);
    let scrolled_rows = rows_after_cursor.saturating_sub(available_rows);
    Ok(Rect::new(
        /*x*/ 0,
        cursor.y.saturating_sub(scrolled_rows),
        screen_size.width.max(1),
        height,
    ))
}

fn normalized_size(size: Size) -> Size {
    Size::new(size.width.max(1), size.height.max(1))
}

/// Clears placeholder cells emitted after wide symbols before terminal insertion.
pub(crate) fn suppress_wide_symbol_placeholder_cells(buffer: &mut Buffer) {
    for y in buffer.area.top()..buffer.area.bottom() {
        let mut cells_to_skip = 0usize;
        for x in buffer.area.left()..buffer.area.right() {
            let cell = &mut buffer[(x, y)];
            if cells_to_skip > 0 {
                cell.set_symbol("");
                cells_to_skip -= 1;
                continue;
            }
            cells_to_skip = crate::utils::text_display_width(cell.symbol()).saturating_sub(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use ratatui::{backend::TestBackend, text::Line, widgets::Paragraph, widgets::Widget};

    use super::{InlineResizeMode, InlineTerminal};

    #[test]
    fn inline_viewport_changes_height_without_replacing_backend() {
        let backend = TestBackend::new(40, 12);
        let mut terminal = InlineTerminal::new(backend, 3).expect("inline terminal");

        terminal
            .sync_inline_height(7, InlineResizeMode::PreserveBottom)
            .expect("grow viewport");
        assert_eq!(terminal.viewport_area().height, 7);

        terminal
            .sync_inline_height(3, InlineResizeMode::PreserveBottom)
            .expect("shrink viewport");
        assert_eq!(terminal.viewport_area().height, 3);
        assert_eq!(
            terminal.size().expect("size"),
            ratatui::layout::Size::new(40, 12)
        );
    }

    #[test]
    fn ordinary_inline_shrink_keeps_the_bottom_edge_stable() {
        let backend = TestBackend::new(40, 12);
        let mut terminal = InlineTerminal::new(backend, 5).expect("inline terminal");
        terminal
            .insert_before(/*height*/ 1, |buffer| {
                Paragraph::new("history").render(buffer.area, buffer);
            })
            .expect("insert history");
        let previous_bottom = terminal.viewport_area().bottom();

        terminal
            .sync_inline_height(3, InlineResizeMode::PreserveBottom)
            .expect("shrink viewport");

        assert_eq!(terminal.viewport_area().bottom(), previous_bottom);
    }

    #[test]
    fn terminal_handoff_replaces_released_rows_without_a_gap() {
        let backend = TestBackend::new(40, 12);
        let mut terminal = InlineTerminal::new(backend, 5).expect("inline terminal");
        terminal
            .insert_before(/*height*/ 1, |buffer| {
                Paragraph::new("previous").render(buffer.area, buffer);
            })
            .expect("insert previous history");
        let previous_bottom = terminal.viewport_area().bottom();

        terminal
            .sync_inline_height(3, InlineResizeMode::PrepareHistoryInsertion)
            .expect("shrink viewport");
        terminal
            .insert_before(/*height*/ 2, |buffer| {
                Paragraph::new(vec![Line::from("first"), Line::from("second")])
                    .render(buffer.area, buffer);
            })
            .expect("insert finalized history");

        assert_eq!(terminal.viewport_area().height, 3);
        assert_eq!(terminal.viewport_area().bottom(), previous_bottom);
        let output = format!("{}", terminal.backend_mut());
        let rows = output.lines().collect::<Vec<_>>();
        let previous = rows
            .iter()
            .position(|row| row.contains("previous"))
            .expect("previous history row");
        let first = rows
            .iter()
            .position(|row| row.contains("first"))
            .expect("first finalized row");
        let second = rows
            .iter()
            .position(|row| row.contains("second"))
            .expect("second finalized row");
        assert_eq!(first, previous + 1);
        assert_eq!(second, first + 1);
    }

    #[test]
    fn inserted_history_remains_above_the_inline_viewport() {
        let backend = TestBackend::new(30, 8);
        let mut terminal = InlineTerminal::new(backend, 3).expect("inline terminal");

        terminal
            .insert_before(/*height*/ 2, |buffer| {
                Paragraph::new(vec![Line::from("first"), Line::from("second")])
                    .render(buffer.area, buffer);
            })
            .expect("insert history");
        terminal
            .draw(|frame| {
                use crate::render::frame::RenderFrame;
                frame.render_widget(Paragraph::new("composer"), frame.area());
            })
            .expect("draw composer");

        let output = format!("{}", terminal.backend_mut());
        assert!(output.contains("first"));
        assert!(output.contains("second"));
        assert!(output.contains("composer"));
    }
}
