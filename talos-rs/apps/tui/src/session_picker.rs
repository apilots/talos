use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Clear, List, ListItem, ListState, Paragraph},
    Frame,
};
use talos_sdk::SessionInfo;

use crate::format_session_timestamp;
use crate::utils::{compact_directory_path, text_display_width};

/// Owns the transient `/sessions` switcher state.
///
/// The picker is a full-screen filter surface. It may choose a session, but it
/// must not load history or mutate the active session itself.
#[derive(Debug, Clone)]
pub(crate) struct SessionPickerState {
    sessions: Vec<SessionInfo>,
    filtered_indices: Vec<usize>,
    selected: usize,
    scroll_top: usize,
    query: String,
    loading: bool,
    error: Option<String>,
}

#[derive(Debug, Clone)]
pub(crate) enum SessionPickerEvent {
    None,
    Cancel,
    Selected(Box<SessionInfo>),
}

impl SessionPickerState {
    pub(crate) fn loading() -> Self {
        Self {
            sessions: Vec::new(),
            filtered_indices: Vec::new(),
            selected: 0,
            scroll_top: 0,
            query: String::new(),
            loading: true,
            error: None,
        }
    }

    pub(crate) fn set_sessions(&mut self, mut sessions: Vec<SessionInfo>) {
        sessions.sort_by(|left, right| {
            right
                .last_activity
                .cmp(&left.last_activity)
                .then(right.created_at.cmp(&left.created_at))
        });
        self.sessions = sessions;
        self.loading = false;
        self.error = None;
        self.apply_filter();
    }

    pub(crate) fn set_error(&mut self, error: String) {
        self.loading = false;
        self.error = Some(error);
        self.filtered_indices.clear();
        self.selected = 0;
        self.scroll_top = 0;
    }

    pub(crate) fn handle_key(&mut self, key_event: KeyEvent) -> SessionPickerEvent {
        match key_event.code {
            KeyCode::Esc => SessionPickerEvent::Cancel,
            KeyCode::Enter => self
                .selected_session()
                .map(Box::new)
                .map(SessionPickerEvent::Selected)
                .unwrap_or(SessionPickerEvent::None),
            KeyCode::Up => {
                self.move_selection(-1);
                SessionPickerEvent::None
            }
            KeyCode::Down => {
                self.move_selection(1);
                SessionPickerEvent::None
            }
            KeyCode::PageUp => {
                self.page_selection(-1);
                SessionPickerEvent::None
            }
            KeyCode::PageDown => {
                self.page_selection(1);
                SessionPickerEvent::None
            }
            KeyCode::Backspace => {
                self.query.pop();
                self.apply_filter();
                SessionPickerEvent::None
            }
            KeyCode::Char(c)
                if !key_event
                    .modifiers
                    .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
            {
                self.query.push(c);
                self.apply_filter();
                SessionPickerEvent::None
            }
            _ => SessionPickerEvent::None,
        }
    }

    pub(crate) fn render(&mut self, f: &mut Frame, area: Rect) {
        f.render_widget(Clear, area);
        let bg_style = Style::default().bg(Color::Rgb(33, 38, 49));
        f.render_widget(Block::default().style(bg_style), area);

        let inner = Rect::new(
            area.x.saturating_add(2),
            area.y.saturating_add(1),
            area.width.saturating_sub(4),
            area.height.saturating_sub(2),
        );
        if inner.width == 0 || inner.height == 0 {
            return;
        }

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(2),
                Constraint::Length(2),
                Constraint::Min(1),
                Constraint::Length(2),
            ])
            .split(inner);

        self.render_header(f, chunks[0], bg_style);
        self.render_search(f, chunks[1], bg_style);
        self.render_list(f, chunks[2], bg_style);
        self.render_footer(f, chunks[3], bg_style);
    }

    fn render_header(&self, f: &mut Frame, area: Rect, bg_style: Style) {
        let lines = vec![
            Line::from(Span::styled(
                "Switch Session",
                bg_style
                    .fg(Color::Rgb(242, 243, 245))
                    .add_modifier(Modifier::BOLD),
            )),
            Line::from(Span::styled(
                "Type to filter recent sessions",
                bg_style.fg(Color::Rgb(168, 172, 184)),
            )),
        ];
        f.render_widget(Paragraph::new(lines).style(bg_style), area);
    }

    fn render_search(&self, f: &mut Frame, area: Rect, bg_style: Style) {
        let value = if self.query.is_empty() {
            "Search sessions"
        } else {
            self.query.as_str()
        };
        let style = if self.query.is_empty() {
            bg_style.fg(Color::Rgb(145, 149, 160))
        } else {
            bg_style.fg(Color::Rgb(242, 243, 245))
        };
        f.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled("› ", bg_style.fg(Color::Rgb(134, 233, 255))),
                Span::styled(value.to_string(), style),
            ]))
            .style(bg_style),
            area,
        );
    }

    fn render_list(&mut self, f: &mut Frame, area: Rect, bg_style: Style) {
        if area.height == 0 {
            return;
        }
        self.clamp_scroll(area.height as usize);
        let visible_indices = self
            .filtered_indices
            .iter()
            .skip(self.scroll_top)
            .take(area.height as usize)
            .copied()
            .collect::<Vec<_>>();

        let items = if self.loading {
            vec![ListItem::new(Line::from(Span::styled(
                "Loading sessions...",
                bg_style.fg(Color::Rgb(168, 172, 184)),
            )))]
        } else if let Some(error) = &self.error {
            vec![ListItem::new(Line::from(Span::styled(
                error.clone(),
                bg_style.fg(Color::Rgb(255, 130, 130)),
            )))]
        } else if visible_indices.is_empty() {
            vec![ListItem::new(Line::from(Span::styled(
                "No matching sessions",
                bg_style.fg(Color::Rgb(168, 172, 184)),
            )))]
        } else {
            visible_indices
                .iter()
                .map(|session_index| {
                    session_item(
                        &self.sessions[*session_index],
                        area.width as usize,
                        bg_style,
                    )
                })
                .collect()
        };

        let selected = self
            .selected
            .checked_sub(self.scroll_top)
            .filter(|_| !self.loading && self.error.is_none() && !visible_indices.is_empty());
        let mut list_state = ListState::default();
        list_state.select(selected);
        let list = List::new(items)
            .style(bg_style)
            .highlight_style(
                bg_style
                    .bg(Color::Rgb(70, 88, 118))
                    .fg(Color::Rgb(242, 245, 248)),
            )
            .highlight_symbol("› ")
            .highlight_spacing(ratatui::widgets::HighlightSpacing::Always);
        f.render_stateful_widget(list, area, &mut list_state);
    }

    fn render_footer(&self, f: &mut Frame, area: Rect, bg_style: Style) {
        let count = self.filtered_indices.len();
        let total = self.sessions.len();
        let progress = if self.loading {
            "loading".to_string()
        } else {
            format!("{count}/{total}")
        };
        let line = Line::from(vec![
            Span::styled(
                "Enter",
                bg_style
                    .fg(Color::Rgb(242, 243, 245))
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(" switch  ", bg_style.fg(Color::Rgb(168, 172, 184))),
            Span::styled(
                "Esc",
                bg_style
                    .fg(Color::Rgb(242, 243, 245))
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(" cancel  ", bg_style.fg(Color::Rgb(168, 172, 184))),
            Span::styled(
                "↑/↓",
                bg_style
                    .fg(Color::Rgb(242, 243, 245))
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(" navigate", bg_style.fg(Color::Rgb(168, 172, 184))),
            Span::styled(
                format!("  {progress}"),
                bg_style.fg(Color::Rgb(134, 233, 255)),
            ),
        ]);
        f.render_widget(Paragraph::new(line).style(bg_style), area);
    }

    fn selected_session(&self) -> Option<SessionInfo> {
        self.filtered_indices
            .get(self.selected)
            .and_then(|index| self.sessions.get(*index))
            .cloned()
    }

    fn move_selection(&mut self, direction: i32) {
        if self.filtered_indices.is_empty() {
            self.selected = 0;
            self.scroll_top = 0;
            return;
        }
        if direction < 0 {
            self.selected = self.selected.saturating_sub(1);
        } else {
            let max = self.filtered_indices.len().saturating_sub(1);
            if self.selected < max {
                self.selected += 1;
            }
        }
    }

    fn page_selection(&mut self, direction: i32) {
        let page = 8usize;
        if direction < 0 {
            self.selected = self.selected.saturating_sub(page);
        } else {
            self.selected = self
                .selected
                .saturating_add(page)
                .min(self.filtered_indices.len().saturating_sub(1));
        }
    }

    fn apply_filter(&mut self) {
        let query = self.query.trim().to_ascii_lowercase();
        self.filtered_indices = self
            .sessions
            .iter()
            .enumerate()
            .filter_map(|(index, session)| {
                (query.is_empty() || session_matches(session, &query)).then_some(index)
            })
            .collect();
        self.selected = self
            .selected
            .min(self.filtered_indices.len().saturating_sub(1));
        self.scroll_top = 0;
    }

    fn clamp_scroll(&mut self, height: usize) {
        if self.selected < self.scroll_top {
            self.scroll_top = self.selected;
        }
        let bottom = self.scroll_top.saturating_add(height);
        if self.selected >= bottom {
            self.scroll_top = self.selected.saturating_sub(height.saturating_sub(1));
        }
    }
}

fn session_matches(session: &SessionInfo, query: &str) -> bool {
    session.title.to_ascii_lowercase().contains(query)
        || session.id.to_ascii_lowercase().contains(query)
        || session.model_ref.id.to_ascii_lowercase().contains(query)
        || session
            .current_directory
            .as_ref()
            .map(|path| {
                path.display()
                    .to_string()
                    .to_ascii_lowercase()
                    .contains(query)
            })
            .unwrap_or(false)
}

fn session_item(session: &SessionInfo, width: usize, bg_style: Style) -> ListItem<'static> {
    let title = if session.title.trim().is_empty() {
        "Untitled Session"
    } else {
        session.title.trim()
    };
    let time_text = format_session_timestamp(session.last_activity);
    let meta = session
        .current_directory
        .as_ref()
        .map(|path| compact_directory_path(path))
        .unwrap_or_else(|| session.model_ref.id.clone());
    let available = width.saturating_sub(14).max(16);
    let title = truncate_to_width(title, available);
    let meta = truncate_to_width(&meta, available);
    ListItem::new(vec![
        Line::from(vec![
            Span::styled(title, bg_style.fg(Color::Rgb(242, 243, 245))),
            Span::styled(
                format!("  {time_text}"),
                bg_style.fg(Color::Rgb(168, 172, 184)),
            ),
        ]),
        Line::from(Span::styled(
            format!("  {meta}"),
            bg_style.fg(Color::Rgb(145, 149, 160)),
        )),
    ])
}

fn truncate_to_width(text: &str, max_width: usize) -> String {
    if text_display_width(text) <= max_width {
        return text.to_string();
    }
    let mut output = String::new();
    let mut width = 0usize;
    for ch in text.chars() {
        let next_width = width.saturating_add(crate::utils::char_display_width(ch));
        if next_width.saturating_add(1) > max_width {
            break;
        }
        output.push(ch);
        width = next_width;
    }
    output.push('…');
    output
}
