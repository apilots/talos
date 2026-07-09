//! Primary terminal adapter for fixed viewport and scrollback writes.
//!
//! This module owns the terminal boundary for the TUI runtime. It keeps the
//! mutable live viewport separate from finalized transcript rows, which are
//! inserted into the primary terminal scrollback above the viewport.

use std::io;
use std::io::Write;

use crossterm::{
    cursor::MoveTo,
    queue,
    style::{
        Attribute as CrosstermAttribute, Color as CrosstermColor, Colors, Print, SetAttribute,
        SetColors,
    },
    terminal::{Clear, ClearType},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::Rect,
    style::{Color, Modifier},
    text::{Line, Span},
    Terminal, TerminalOptions, Viewport,
};

use crate::utils::wrap_line_visual;

/// Owns the fixed TUI viewport and all writes into primary terminal scrollback.
///
/// Callers draw transient live/input UI through [`draw`](Self::draw). Finalized
/// transcript rows must go through [`insert_history_lines`](Self::insert_history_lines)
/// so the viewport is moved by terminal scroll-region operations instead of
/// being copied into the terminal history.
pub(crate) struct PrimaryTerminal {
    terminal: Terminal<CrosstermBackend<io::Stdout>>,
    viewport: Rect,
    screen_height: u16,
    history_bottom: u16,
}

impl PrimaryTerminal {
    pub(crate) fn new(width: u16, height: u16, screen_height: u16) -> io::Result<Self> {
        let viewport_height = height.clamp(1, screen_height.max(1));
        let viewport = Rect::new(/*x*/ 0, /*y*/ 0, width, viewport_height);
        let backend = CrosstermBackend::new(io::stdout());
        let terminal = create_fixed_terminal(backend, viewport)?;
        Ok(Self {
            terminal,
            viewport,
            screen_height,
            history_bottom: 0,
        })
    }

    pub(crate) fn backend_mut(&mut self) -> &mut CrosstermBackend<io::Stdout> {
        self.terminal.backend_mut()
    }

    pub(crate) fn size(&self) -> io::Result<ratatui::layout::Size> {
        self.terminal.size()
    }

    pub(crate) fn resize_viewport(
        &mut self,
        width: u16,
        height: u16,
        screen_height: u16,
    ) -> io::Result<()> {
        let next_height = height.clamp(1, screen_height.max(1));
        if self.viewport.width == width
            && self.viewport.height == next_height
            && self.screen_height == screen_height
        {
            return Ok(());
        }

        self.clear_current_viewport()?;
        self.screen_height = screen_height.max(1);
        self.viewport.width = width.max(1);
        self.viewport.height = next_height;
        self.fit_history_above_viewport()?;
        self.viewport.y = self.viewport_top_after_history();
        self.recreate_fixed_terminal()
    }

    pub(crate) fn draw<F>(&mut self, render_callback: F) -> io::Result<()>
    where
        F: FnOnce(&mut ratatui::Frame),
    {
        self.terminal.draw(render_callback).map(|_| ())
    }

    pub(crate) fn clear_viewport(&mut self) -> io::Result<()> {
        self.clear_current_viewport()?;
        queue!(
            self.terminal.backend_mut(),
            MoveTo(/*x*/ 0, self.history_bottom),
            Clear(ClearType::FromCursorDown)
        )?;
        self.terminal.backend_mut().flush()
    }

    pub(crate) fn insert_history_lines(&mut self, lines: &[Line<'static>]) -> io::Result<()> {
        if lines.is_empty() {
            return Ok(());
        }

        let rows = wrap_history_lines(lines, self.viewport.width as usize);
        self.clear_current_viewport()?;
        for row in &rows {
            self.append_history_row(row)?;
        }
        self.viewport.y = self.viewport_top_after_history();
        self.recreate_fixed_terminal()
    }

    fn fit_history_above_viewport(&mut self) -> io::Result<()> {
        let history_limit = self.history_bottom_limit();
        while self.history_bottom > history_limit {
            self.scroll_history_region_up(history_limit)?;
            self.history_bottom = self.history_bottom.saturating_sub(1);
        }
        Ok(())
    }

    fn append_history_row(&mut self, row: &Line<'static>) -> io::Result<()> {
        let history_limit = self.history_bottom_limit();
        if self.history_bottom >= history_limit {
            self.scroll_history_region_up(history_limit)?;
            self.history_bottom = history_limit.saturating_sub(1);
        }
        let writer = self.terminal.backend_mut();
        queue!(
            writer,
            MoveTo(/*x*/ 0, self.history_bottom),
            Clear(ClearType::CurrentLine)
        )?;
        write_history_line(writer, row)?;
        self.history_bottom = self.history_bottom.saturating_add(1).min(history_limit);
        writer.flush()
    }

    fn scroll_history_region_up(&mut self, history_limit: u16) -> io::Result<()> {
        if history_limit <= 1 || self.screen_height == 0 {
            return Ok(());
        }
        let writer = self.terminal.backend_mut();
        queue!(
            writer,
            MoveTo(/*x*/ 0, self.screen_height.saturating_sub(1)),
            Print("\r\n"),
        )?;
        writer.flush()
    }

    fn clear_current_viewport(&mut self) -> io::Result<()> {
        let bottom = self.viewport.bottom().min(self.screen_height);
        let writer = self.terminal.backend_mut();
        for row in self.viewport.top()..bottom {
            queue!(writer, MoveTo(/*x*/ 0, row), Clear(ClearType::CurrentLine))?;
        }
        writer.flush()
    }

    fn history_bottom_limit(&self) -> u16 {
        self.screen_height
            .saturating_sub(self.viewport.height)
            .max(1)
    }

    fn viewport_top_after_history(&self) -> u16 {
        viewport_top_after_history(
            self.history_bottom,
            self.viewport.height,
            self.screen_height,
        )
    }

    fn recreate_fixed_terminal(&mut self) -> io::Result<()> {
        let backend = CrosstermBackend::new(io::stdout());
        self.terminal = create_fixed_terminal(backend, self.viewport)?;
        Ok(())
    }
}

fn viewport_top_after_history(
    history_bottom: u16,
    viewport_height: u16,
    screen_height: u16,
) -> u16 {
    history_bottom.min(screen_height.saturating_sub(viewport_height))
}

fn wrap_history_lines(lines: &[Line<'static>], terminal_width: usize) -> Vec<Line<'static>> {
    let width = terminal_width.max(1);
    lines
        .iter()
        .cloned()
        .flat_map(|line| wrap_line_visual(line, width))
        .collect()
}

fn create_fixed_terminal(
    backend: CrosstermBackend<io::Stdout>,
    viewport: Rect,
) -> io::Result<Terminal<CrosstermBackend<io::Stdout>>> {
    Terminal::with_options(
        backend,
        TerminalOptions {
            viewport: Viewport::Fixed(viewport),
        },
    )
}

fn write_history_line<W: Write>(writer: &mut W, line: &Line<'_>) -> io::Result<()> {
    queue!(writer, Clear(ClearType::UntilNewLine))?;
    let spans = line
        .spans
        .iter()
        .map(|span| Span {
            style: span.style.patch(line.style),
            content: span.content.clone(),
        })
        .collect::<Vec<_>>();
    write_spans(writer, spans.iter())
}

fn write_spans<'a, I, W>(writer: &mut W, spans: I) -> io::Result<()>
where
    I: IntoIterator<Item = &'a Span<'a>>,
    W: Write,
{
    let mut fg = Color::Reset;
    let mut bg = Color::Reset;
    let mut last_modifier = Modifier::empty();
    for span in spans {
        let mut modifier = Modifier::empty();
        modifier.insert(span.style.add_modifier);
        modifier.remove(span.style.sub_modifier);
        if modifier != last_modifier {
            ModifierDiff {
                from: last_modifier,
                to: modifier,
            }
            .queue(writer)?;
            last_modifier = modifier;
        }

        let next_fg = span.style.fg.unwrap_or(Color::Reset);
        let next_bg = span.style.bg.unwrap_or(Color::Reset);
        if next_fg != fg || next_bg != bg {
            queue!(
                writer,
                SetColors(Colors::new(
                    ratatui_color_to_crossterm(next_fg),
                    ratatui_color_to_crossterm(next_bg)
                ))
            )?;
            fg = next_fg;
            bg = next_bg;
        }
        queue!(writer, Print(span.content.as_ref()))?;
    }
    queue!(
        writer,
        SetColors(Colors::new(CrosstermColor::Reset, CrosstermColor::Reset)),
        SetAttribute(CrosstermAttribute::Reset)
    )
}

fn ratatui_color_to_crossterm(color: Color) -> CrosstermColor {
    match color {
        Color::Reset => CrosstermColor::Reset,
        Color::Black => CrosstermColor::Black,
        Color::Red => CrosstermColor::DarkRed,
        Color::Green => CrosstermColor::DarkGreen,
        Color::Yellow => CrosstermColor::DarkYellow,
        Color::Blue => CrosstermColor::DarkBlue,
        Color::Magenta => CrosstermColor::DarkMagenta,
        Color::Cyan => CrosstermColor::DarkCyan,
        Color::Gray => CrosstermColor::Grey,
        Color::DarkGray => CrosstermColor::DarkGrey,
        Color::LightRed => CrosstermColor::Red,
        Color::LightGreen => CrosstermColor::Green,
        Color::LightYellow => CrosstermColor::Yellow,
        Color::LightBlue => CrosstermColor::Blue,
        Color::LightMagenta => CrosstermColor::Magenta,
        Color::LightCyan => CrosstermColor::Cyan,
        Color::White => CrosstermColor::White,
        Color::Rgb(r, g, b) => CrosstermColor::Rgb { r, g, b },
        Color::Indexed(index) => CrosstermColor::AnsiValue(index),
    }
}

struct ModifierDiff {
    from: Modifier,
    to: Modifier,
}

impl ModifierDiff {
    fn queue<W: Write>(self, writer: &mut W) -> io::Result<()> {
        let removed = self.from - self.to;
        if removed.contains(Modifier::REVERSED) {
            queue!(writer, SetAttribute(CrosstermAttribute::NoReverse))?;
        }
        if removed.contains(Modifier::BOLD) {
            queue!(writer, SetAttribute(CrosstermAttribute::NormalIntensity))?;
            if self.to.contains(Modifier::DIM) {
                queue!(writer, SetAttribute(CrosstermAttribute::Dim))?;
            }
        }
        if removed.contains(Modifier::ITALIC) {
            queue!(writer, SetAttribute(CrosstermAttribute::NoItalic))?;
        }
        if removed.contains(Modifier::UNDERLINED) {
            queue!(writer, SetAttribute(CrosstermAttribute::NoUnderline))?;
        }
        if removed.contains(Modifier::DIM) {
            queue!(writer, SetAttribute(CrosstermAttribute::NormalIntensity))?;
        }
        if removed.contains(Modifier::CROSSED_OUT) {
            queue!(writer, SetAttribute(CrosstermAttribute::NotCrossedOut))?;
        }

        let added = self.to - self.from;
        if added.contains(Modifier::REVERSED) {
            queue!(writer, SetAttribute(CrosstermAttribute::Reverse))?;
        }
        if added.contains(Modifier::BOLD) {
            queue!(writer, SetAttribute(CrosstermAttribute::Bold))?;
        }
        if added.contains(Modifier::ITALIC) {
            queue!(writer, SetAttribute(CrosstermAttribute::Italic))?;
        }
        if added.contains(Modifier::UNDERLINED) {
            queue!(writer, SetAttribute(CrosstermAttribute::Underlined))?;
        }
        if added.contains(Modifier::DIM) {
            queue!(writer, SetAttribute(CrosstermAttribute::Dim))?;
        }
        if added.contains(Modifier::CROSSED_OUT) {
            queue!(writer, SetAttribute(CrosstermAttribute::CrossedOut))?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn history_line_writer_preserves_plain_text() {
        let mut output = Vec::new();
        let line = Line::from(vec![Span::raw("hello "), Span::raw("world")]);

        write_history_line(&mut output, &line).unwrap();
        let output = String::from_utf8(output).unwrap();

        assert!(output.contains("hello world"));
    }

    #[test]
    fn history_lines_wrap_to_terminal_width_before_insert() {
        let lines = vec![Line::from("abcdefghij")];

        let wrapped = wrap_history_lines(&lines, 4)
            .into_iter()
            .map(|line| crate::utils::line_plain_text(&line))
            .collect::<Vec<_>>();

        assert_eq!(
            wrapped,
            vec!["abcd".to_string(), "efgh".to_string(), "ij".to_string()]
        );
    }

    #[test]
    fn viewport_top_follows_short_history_and_clamps_to_bottom() {
        assert_eq!(
            viewport_top_after_history(
                /*history_bottom*/ 8, /*viewport_height*/ 6, /*screen_height*/ 30
            ),
            8
        );
        assert_eq!(
            viewport_top_after_history(
                /*history_bottom*/ 28, /*viewport_height*/ 6, /*screen_height*/ 30,
            ),
            24
        );
    }
}
