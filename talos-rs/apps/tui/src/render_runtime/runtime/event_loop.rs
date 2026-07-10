//! Event loop contracts for the render runtime.

use std::io;
use std::time::Duration;

use crossterm::event::{self, Event, KeyEvent, KeyEventKind, MouseEvent};

/// Events consumed by the render runtime event loop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TuiRuntimeEvent {
    Input,
    Mouse,
    Paste,
    Resize,
    Stream,
    HistoryLoaded,
    LiveCommitTick,
}

/// Terminal event consumed by the top-level TUI runtime loop.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum TerminalRuntimeEvent {
    Tick,
    Key(KeyEvent),
    Paste(String),
    Mouse(MouseEvent),
    Resize(u16, u16),
    Ignored,
}

impl TerminalRuntimeEvent {
    pub(crate) fn runtime_event(&self) -> Option<TuiRuntimeEvent> {
        match self {
            Self::Key(_) => Some(TuiRuntimeEvent::Input),
            Self::Paste(_) => Some(TuiRuntimeEvent::Paste),
            Self::Mouse(_) => Some(TuiRuntimeEvent::Mouse),
            Self::Resize(_, _) => Some(TuiRuntimeEvent::Resize),
            Self::Tick | Self::Ignored => None,
        }
    }
}

/// Reads terminal events and emits periodic ticks for async TUI housekeeping.
#[derive(Debug, Clone, Copy)]
pub(crate) struct CrosstermRuntimeEventSource {
    tick_interval: Duration,
}

impl Default for CrosstermRuntimeEventSource {
    fn default() -> Self {
        Self {
            tick_interval: Duration::from_millis(50),
        }
    }
}

impl CrosstermRuntimeEventSource {
    pub(crate) fn next_event(&mut self) -> io::Result<TerminalRuntimeEvent> {
        if !event::poll(self.tick_interval)? {
            return Ok(TerminalRuntimeEvent::Tick);
        }
        event::read().map(terminal_event_from_crossterm)
    }
}

fn terminal_event_from_crossterm(event: Event) -> TerminalRuntimeEvent {
    match event {
        Event::Key(key) if key.kind == KeyEventKind::Press => TerminalRuntimeEvent::Key(key),
        Event::Paste(text) => TerminalRuntimeEvent::Paste(text),
        Event::Mouse(mouse) => TerminalRuntimeEvent::Mouse(mouse),
        Event::Resize(width, height) => TerminalRuntimeEvent::Resize(width, height),
        Event::FocusGained | Event::FocusLost | Event::Key(_) => TerminalRuntimeEvent::Ignored,
    }
}

#[cfg(test)]
mod tests {
    use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

    use super::{terminal_event_from_crossterm, TerminalRuntimeEvent, TuiRuntimeEvent};

    #[test]
    fn maps_key_press_to_input_runtime_event() {
        let terminal_event = terminal_event_from_crossterm(Event::Key(KeyEvent::new(
            KeyCode::Char('a'),
            KeyModifiers::NONE,
        )));

        assert!(matches!(terminal_event, TerminalRuntimeEvent::Key(_)));
        assert_eq!(terminal_event.runtime_event(), Some(TuiRuntimeEvent::Input));
    }

    #[test]
    fn ignores_key_release_events() {
        let terminal_event = terminal_event_from_crossterm(Event::Key(KeyEvent {
            code: KeyCode::Char('a'),
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Release,
            state: crossterm::event::KeyEventState::NONE,
        }));

        assert_eq!(terminal_event, TerminalRuntimeEvent::Ignored);
        assert_eq!(terminal_event.runtime_event(), None);
    }

    #[test]
    fn maps_resize_to_runtime_resize_event() {
        let terminal_event = terminal_event_from_crossterm(Event::Resize(120, 40));

        assert_eq!(terminal_event, TerminalRuntimeEvent::Resize(120, 40));
        assert_eq!(
            terminal_event.runtime_event(),
            Some(TuiRuntimeEvent::Resize)
        );
    }
}
