use std::collections::VecDeque;

const MAX_INPUT_HISTORY: usize = 100;

/// Owns composer input history for the main TUI surface.
///
/// Callers add submitted user messages and ask for previous/next draft text.
/// This type does not render UI and must not know about sessions, commands, or
/// terminal state.
#[derive(Debug, Clone, Default)]
pub(crate) struct InputHistoryState {
    entries: VecDeque<String>,
    selected: Option<usize>,
}

impl InputHistoryState {
    pub(crate) fn push(&mut self, message: String) {
        if message.trim().is_empty() {
            self.selected = None;
            return;
        }
        self.entries.push_back(message);
        if self.entries.len() > MAX_INPUT_HISTORY {
            self.entries.pop_front();
        }
        self.selected = None;
    }

    pub(crate) fn clear(&mut self) {
        self.entries.clear();
        self.selected = None;
    }

    pub(crate) fn previous(&mut self) -> Option<&str> {
        if self.entries.is_empty() {
            return None;
        }

        let index = match self.selected {
            Some(0) => 0,
            Some(index) => index.saturating_sub(1),
            None => self.entries.len().saturating_sub(1),
        };
        self.selected = Some(index);
        self.entries.get(index).map(String::as_str)
    }

    pub(crate) fn next(&mut self) -> InputHistoryNavigation<'_> {
        let Some(index) = self.selected else {
            return InputHistoryNavigation::Unchanged;
        };

        if index + 1 >= self.entries.len() {
            self.selected = None;
            return InputHistoryNavigation::Cleared;
        }

        let next_index = index + 1;
        self.selected = Some(next_index);
        self.entries
            .get(next_index)
            .map(String::as_str)
            .map(InputHistoryNavigation::Selected)
            .unwrap_or(InputHistoryNavigation::Unchanged)
    }

    pub(crate) fn selected_position(&self) -> Option<(usize, usize)> {
        self.selected
            .map(|index| (index + 1, self.entries.len()))
            .filter(|(_, total)| *total > 0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum InputHistoryNavigation<'a> {
    Selected(&'a str),
    Cleared,
    Unchanged,
}

#[cfg(test)]
mod tests {
    use super::{InputHistoryNavigation, InputHistoryState};

    #[test]
    fn navigates_submitted_inputs() {
        let mut history = InputHistoryState::default();
        history.push("first".to_string());
        history.push("second".to_string());

        assert_eq!(history.previous(), Some("second"));
        assert_eq!(history.previous(), Some("first"));
        assert_eq!(history.previous(), Some("first"));
        assert_eq!(history.next(), InputHistoryNavigation::Selected("second"));
        assert_eq!(history.next(), InputHistoryNavigation::Cleared);
        assert_eq!(history.next(), InputHistoryNavigation::Unchanged);
    }
}
