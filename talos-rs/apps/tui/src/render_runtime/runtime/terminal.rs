//! Primary terminal mode driver for the render runtime.

use std::io::{self, Write};

use crate::render_runtime::transcript::writer::InteractionLayer;

/// Owns terminal interaction setup for the primary scrollback mode.
///
/// This driver must never enter the alternate screen because finalized transcript
/// belongs to primary terminal scrollback.
#[cfg(test)]
pub(crate) struct PrimaryTerminalDriver<W> {
    output: W,
    interaction_active: bool,
}

#[cfg(test)]
impl<W: Write> PrimaryTerminalDriver<W> {
    pub(crate) fn new(output: W) -> Self {
        Self {
            output,
            interaction_active: false,
        }
    }

    pub(crate) fn enter_interaction(&mut self) -> io::Result<()> {
        if self.interaction_active {
            return Ok(());
        }
        crossterm::execute!(
            self.output,
            crossterm::event::EnableBracketedPaste,
            crossterm::cursor::Hide
        )?;
        self.interaction_active = true;
        Ok(())
    }

    pub(crate) fn leave_interaction(&mut self) -> io::Result<()> {
        if !self.interaction_active {
            return Ok(());
        }
        crossterm::execute!(
            self.output,
            crossterm::event::DisableBracketedPaste,
            crossterm::cursor::Show
        )?;
        self.interaction_active = false;
        Ok(())
    }

    pub(crate) fn output_mut(&mut self) -> &mut W {
        &mut self.output
    }

    pub(crate) fn into_inner(self) -> W {
        self.output
    }
}

/// Interaction layer used by transcript writer in primary terminal mode.
#[derive(Debug, Default)]
pub(crate) struct PrimaryInteractionLayer;

impl InteractionLayer for PrimaryInteractionLayer {
    fn suspend(&mut self, output: &mut dyn Write) -> io::Result<()> {
        output.write_all(b"\r\x1b[2K")
    }

    fn repaint(&mut self, _output: &mut dyn Write) -> io::Result<()> {
        Ok(())
    }

    fn restore(&mut self, _output: &mut dyn Write) -> io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::PrimaryTerminalDriver;
    use crate::render_runtime::common::style::StyledLine;
    use crate::render_runtime::runtime::terminal::PrimaryInteractionLayer;
    use crate::render_runtime::transcript::writer::TerminalTranscriptWriter;

    #[test]
    fn primary_driver_never_emits_alternate_screen_sequences() {
        let mut driver = PrimaryTerminalDriver::new(Vec::new());

        driver.enter_interaction().unwrap();
        driver.leave_interaction().unwrap();
        let output = String::from_utf8(driver.into_inner()).unwrap();

        assert!(!output.contains("\u{1b}[?1049h"));
        assert!(!output.contains("\u{1b}[?1049l"));
    }

    #[test]
    fn enter_and_leave_are_idempotent() {
        let mut driver = PrimaryTerminalDriver::new(Vec::new());

        driver.enter_interaction().unwrap();
        let first_len = driver.output_mut().len();
        driver.enter_interaction().unwrap();
        let second_len = driver.output_mut().len();
        driver.leave_interaction().unwrap();
        let third_len = driver.output_mut().len();
        driver.leave_interaction().unwrap();
        let fourth_len = driver.output_mut().len();

        assert_eq!(first_len, second_len);
        assert_eq!(third_len, fourth_len);
    }

    #[test]
    fn primary_interaction_layer_flushes_transcript_without_alt_screen() {
        let mut writer = TerminalTranscriptWriter::new(Vec::new());
        writer.enqueue_lines([StyledLine::plain("persisted")]);
        writer
            .flush_with_interaction_layer(&mut PrimaryInteractionLayer)
            .unwrap();
        let output = String::from_utf8(writer.into_inner()).unwrap();

        assert!(output.contains("persisted"));
        assert!(!output.contains("\u{1b}[?1049h"));
        assert!(!output.contains("\u{1b}[?1049l"));
    }
}
