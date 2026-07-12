use chrono::{Local, TimeZone};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Clear, List, ListItem, ListState, Paragraph},
};
use talos_sdk::SessionInfo;

use crate::{render::frame::RenderFrame, utils::text_display_width};

const PICKER_CHROME_HEIGHT: u16 = 8;
const LIST_HORIZONTAL_INSET: u16 = 4;
const SESSION_AGE_WIDTH: usize = 9;
const SESSION_COLUMN_GAP: usize = 2;

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

    pub(crate) fn render(&mut self, f: &mut impl RenderFrame, area: Rect) {
        f.render_widget(Clear, area);
        let bg_style = Style::default().bg(Color::Rgb(33, 38, 49));
        f.render_widget(Block::default().style(bg_style), area);

        let inner = Rect::new(area.x, area.y, area.width, area.height);
        if inner.width == 0 || inner.height == 0 {
            return;
        }

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1),
                Constraint::Length(1),
                Constraint::Length(1),
                Constraint::Length(1),
                Constraint::Min(inner.height.saturating_sub(PICKER_CHROME_HEIGHT)),
                Constraint::Length(4),
            ])
            .split(inner);

        self.render_header(f, chunks[0], bg_style);
        self.render_search(f, chunks[2], bg_style);
        self.render_list(f, chunks[4], bg_style);
        self.render_footer(f, chunks[5], bg_style);
    }

    fn render_header(&self, f: &mut impl RenderFrame, area: Rect, bg_style: Style) {
        let title_area = inset(area, 1);
        let line = Line::from(Span::styled(
            "Resume a previous session",
            bg_style
                .fg(Color::Rgb(134, 233, 255))
                .add_modifier(Modifier::BOLD),
        ));
        f.render_widget(Paragraph::new(line).style(bg_style), title_area);
    }

    fn render_search(&self, f: &mut impl RenderFrame, area: Rect, bg_style: Style) {
        let area = inset(area, 1);
        let search = if self.query.is_empty() {
            Span::styled("Type to search", bg_style.fg(Color::Rgb(168, 172, 184)))
        } else {
            Span::styled(
                format!("Search: {}", self.query),
                bg_style.fg(Color::Rgb(242, 243, 245)),
            )
        };
        let toolbar = Line::from(vec![
            Span::styled("Filter: ", bg_style.fg(Color::Rgb(168, 172, 184))),
            Span::styled(
                "[Cwd]",
                bg_style
                    .fg(Color::Rgb(255, 128, 210))
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(" All    Sort: ", bg_style.fg(Color::Rgb(168, 172, 184))),
            Span::styled(
                "[Updated]",
                bg_style
                    .fg(Color::Rgb(242, 243, 245))
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(" Created", bg_style.fg(Color::Rgb(168, 172, 184))),
        ]);
        let search_width = text_display_width(search.content.as_ref());
        let toolbar_width = toolbar.width();
        let spacer = area
            .width
            .saturating_sub((search_width + toolbar_width) as u16)
            .max(1) as usize;
        let mut spans = vec![search, Span::raw(" ".repeat(spacer))];
        spans.extend(toolbar.spans);
        f.render_widget(Paragraph::new(Line::from(spans)).style(bg_style), area);
    }

    fn render_list(&mut self, f: &mut impl RenderFrame, area: Rect, bg_style: Style) {
        if area.height == 0 {
            return;
        }
        let list_area = Rect::new(
            area.x.saturating_add(2),
            area.y,
            area.width.saturating_sub(LIST_HORIZONTAL_INSET),
            area.height,
        );
        self.clamp_scroll(list_area.height as usize);
        let visible_indices = self
            .filtered_indices
            .iter()
            .skip(self.scroll_top)
            .take(list_area.height as usize)
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
                        list_area.width as usize,
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
        f.render_stateful_widget(list, list_area, &mut list_state);
    }

    fn render_footer(&self, f: &mut impl RenderFrame, area: Rect, bg_style: Style) {
        if area.height == 0 {
            return;
        }
        let separator_area = Rect::new(area.x, area.y, area.width, 1);
        let progress = self.footer_progress(area.height.saturating_sub(1) as usize);
        let separator = "─".repeat(area.width as usize);
        f.render_widget(
            Paragraph::new(Line::from(Span::styled(
                separator,
                bg_style.fg(Color::Rgb(168, 172, 184)),
            )))
            .style(bg_style),
            separator_area,
        );
        let progress_width = text_display_width(&progress) as u16;
        if progress_width < area.width {
            let progress_area = Rect::new(
                area.x + area.width - progress_width - 1,
                area.y,
                progress_width,
                1,
            );
            f.render_widget(
                Paragraph::new(Line::from(Span::styled(
                    progress,
                    bg_style.fg(Color::Rgb(168, 172, 184)),
                )))
                .style(bg_style),
                progress_area,
            );
        }

        let first_line = Line::from(vec![
            Span::styled(
                "enter",
                bg_style
                    .fg(Color::Rgb(242, 243, 245))
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(" resume    ", bg_style.fg(Color::Rgb(168, 172, 184))),
            Span::styled(
                "esc",
                bg_style
                    .fg(Color::Rgb(242, 243, 245))
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(" exit    ", bg_style.fg(Color::Rgb(168, 172, 184))),
            Span::styled(
                "ctrl+c",
                bg_style
                    .fg(Color::Rgb(242, 243, 245))
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(" exit    ", bg_style.fg(Color::Rgb(168, 172, 184))),
            Span::styled(
                "↑/↓",
                bg_style
                    .fg(Color::Rgb(242, 243, 245))
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(" browse", bg_style.fg(Color::Rgb(168, 172, 184))),
        ]);
        let second_line = Line::from(vec![
            Span::styled(
                "type",
                bg_style
                    .fg(Color::Rgb(242, 243, 245))
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(" search    ", bg_style.fg(Color::Rgb(168, 172, 184))),
            Span::styled(
                "backspace",
                bg_style
                    .fg(Color::Rgb(242, 243, 245))
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(" edit search", bg_style.fg(Color::Rgb(168, 172, 184))),
        ]);
        let hint_area = Rect::new(
            area.x.saturating_add(1),
            area.y.saturating_add(1),
            area.width,
            2,
        );
        f.render_widget(
            Paragraph::new(vec![first_line, second_line]).style(bg_style),
            hint_area,
        );
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

    fn footer_progress(&self, list_height: usize) -> String {
        if self.loading {
            return " loading ".to_string();
        }
        let total = self.filtered_indices.len();
        let position = if total == 0 { 0 } else { self.selected + 1 };
        let percent = if total <= list_height || total <= 1 {
            100
        } else {
            ((self.scroll_top as f32 / (total.saturating_sub(list_height).max(1)) as f32) * 100.0)
                .round()
                .clamp(0.0, 100.0) as u8
        };
        format!(" {position} / {total} · {percent}% ")
    }
}

fn session_matches(session: &SessionInfo, query: &str) -> bool {
    session.title.to_ascii_lowercase().contains(query)
        || session.id.to_ascii_lowercase().contains(query)
        || session
            .model_selection
            .model_ref
            .id
            .to_ascii_lowercase()
            .contains(query)
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
    let age = format_session_age(session.last_activity);
    let available = width
        .saturating_sub(SESSION_AGE_WIDTH)
        .saturating_sub(SESSION_COLUMN_GAP)
        .max(16);
    let title = truncate_to_width(title, available);
    ListItem::new(Line::from(vec![
        Span::styled(
            format!("{age:>SESSION_AGE_WIDTH$}"),
            bg_style.fg(Color::Rgb(168, 172, 184)),
        ),
        Span::raw("  "),
        Span::styled(title, bg_style.fg(Color::Rgb(242, 243, 245))),
    ]))
}

fn format_session_age(timestamp_ms: u64) -> String {
    let Some(local_dt) = timestamp_ms
        .try_into()
        .ok()
        .and_then(|millis| Local.timestamp_millis_opt(millis).single())
    else {
        return "unknown".to_string();
    };
    let duration = Local::now().signed_duration_since(local_dt);
    let minutes = duration.num_minutes().max(0);
    if minutes < 1 {
        "now".to_string()
    } else if minutes < 60 {
        format!("{minutes}m ago")
    } else {
        let hours = duration.num_hours();
        if hours < 24 {
            format!("{hours}h ago")
        } else {
            let days = duration.num_days();
            format!("{days}d ago")
        }
    }
}

fn inset(area: Rect, horizontal: u16) -> Rect {
    Rect::new(
        area.x.saturating_add(horizontal),
        area.y,
        area.width.saturating_sub(horizontal.saturating_mul(2)),
        area.height,
    )
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
