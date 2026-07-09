use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, ListState},
    Frame,
};

use crate::session_list_label;
use crate::TuiApp;
impl TuiApp {
    pub(crate) fn render_model_picker(&self, f: &mut Frame, area: Rect) {
        let popup_area = self.left_panel_area.unwrap_or(area);

        let popup_height = (self.available_models.len() as u16 + 4).min(20);
        let popup_width = 52u16;
        let x = popup_area.x + popup_area.width.saturating_sub(popup_width) / 2;
        let y = popup_area.y + popup_area.height.saturating_sub(popup_height) / 2;
        let popup_rect = Rect::new(
            x.min(popup_area.x + popup_area.width.saturating_sub(popup_width)),
            y.min(popup_area.y + popup_area.height.saturating_sub(popup_height)),
            popup_width.min(popup_area.width),
            popup_height.min(popup_area.height),
        );

        f.render_widget(Clear, popup_rect);

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Rgb(100, 180, 240)))
            .title(" Switch Model  [Enter] confirm  [Esc] cancel ");
        let inner = block.inner(popup_rect);
        f.render_widget(block, popup_rect);

        let items: Vec<ListItem> = self
            .available_models
            .iter()
            .map(|m| {
                if m == &self.current_model {
                    ListItem::new(format!(" {m}  ✓"))
                } else {
                    ListItem::new(format!(" {m}"))
                }
            })
            .collect();

        let mut list_state = ListState::default();
        list_state.select(Some(self.model_picker_selected));

        let list = List::new(items)
            .highlight_style(
                Style::default()
                    .bg(Color::Rgb(40, 80, 140))
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            )
            .highlight_symbol("▶ ");

        f.render_stateful_widget(list, inner, &mut list_state);
    }

    pub(crate) fn render_sessions_picker(&self, f: &mut Frame, area: Rect) {
        let popup_area = self.left_panel_area.unwrap_or(area);

        let popup_height = (self.history_sessions.len() as u16 + 4).min(20);
        let popup_width = 70u16;
        let x = popup_area.x + popup_area.width.saturating_sub(popup_width) / 2;
        let y = popup_area.y + popup_area.height.saturating_sub(popup_height) / 2;
        let popup_rect = Rect::new(
            x.min(popup_area.x + popup_area.width.saturating_sub(popup_width)),
            y.min(popup_area.y + popup_area.height.saturating_sub(popup_height)),
            popup_width.min(popup_area.width),
            popup_height.min(popup_area.height),
        );

        f.render_widget(Clear, popup_rect);

        let block = Block::default()
            .style(Style::default().bg(Color::Rgb(28, 32, 40)))
            .title(" Recent Sessions  [↑↓] navigate  [Enter] switch  [Esc] cancel ");
        let inner = block.inner(popup_rect);
        f.render_widget(block, popup_rect);

        let items: Vec<ListItem> = self
            .history_sessions
            .iter()
            .map(|s| ListItem::new(session_list_label(s)))
            .collect();

        let mut list_state = ListState::default();
        list_state.select(Some(self.sessions_picker_selected));

        let list = List::new(items)
            .style(
                Style::default()
                    .bg(Color::Rgb(28, 32, 40))
                    .fg(Color::Rgb(220, 225, 233)),
            )
            .highlight_style(
                Style::default()
                    .bg(Color::Rgb(70, 88, 118))
                    .fg(Color::Rgb(242, 245, 248)),
            )
            .highlight_symbol("▶ ");

        f.render_stateful_widget(list, inner, &mut list_state);
    }

    pub(crate) fn command_autocomplete_panel_height(&self) -> u16 {
        if !self.show_command_autocomplete {
            return 0;
        }
        self.command_suggestions.len().clamp(1, 7) as u16
    }

    pub(crate) fn render_command_autocomplete(&self, f: &mut Frame, area: Rect) {
        let Some(input_area) = self.input_area else {
            return;
        };
        let popup_area = self.left_panel_area.unwrap_or(area);
        let popup_height = self.command_autocomplete_panel_height();
        if popup_height == 0 || input_area.width == 0 {
            return;
        }

        let max_width = popup_area.width.saturating_sub(2).max(1);
        let popup_width = input_area.width.min(max_width);
        let popup_x = input_area.x;
        let popup_y = input_area.y.saturating_add(input_area.height);
        let available_height = popup_area
            .y
            .saturating_add(popup_area.height)
            .saturating_sub(popup_y);
        let popup_rect = Rect::new(
            popup_x,
            popup_y,
            popup_width,
            popup_height.min(available_height),
        );
        if popup_rect.width == 0 || popup_rect.height == 0 {
            return;
        }

        let bg_style = Style::default().bg(Color::Rgb(40, 44, 58));
        f.render_widget(Clear, popup_rect);
        f.render_widget(Block::default().style(bg_style), popup_rect);

        let items: Vec<ListItem> = if self.command_suggestions.is_empty() {
            vec![ListItem::new(Line::from(vec![Span::styled(
                "No matching commands",
                Style::default().fg(Color::Rgb(150, 154, 165)),
            )]))]
        } else {
            self.command_suggestions
                .iter()
                .enumerate()
                .map(|(index, cmd)| {
                    let is_selected = index == self.command_autocomplete_selected;
                    let command_style = if is_selected {
                        Style::default()
                            .fg(Color::Rgb(134, 233, 255))
                            .add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(Color::Rgb(242, 243, 245))
                    };
                    let description_style = if is_selected {
                        Style::default()
                            .fg(Color::Rgb(134, 233, 255))
                            .add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(Color::Rgb(186, 186, 186))
                    };
                    ListItem::new(vec![Line::from(vec![
                        Span::styled(&cmd.display_text, command_style),
                        Span::raw("  "),
                        Span::styled(&cmd.description, description_style),
                    ])])
                })
                .collect()
        };

        let mut list_state = ListState::default();
        if !self.command_suggestions.is_empty() {
            list_state.select(Some(self.command_autocomplete_selected));
        }

        let list = List::new(items)
            .style(bg_style)
            .highlight_style(Style::default().bg(Color::Rgb(40, 44, 58)))
            .highlight_symbol("› ")
            .highlight_spacing(ratatui::widgets::HighlightSpacing::Always);

        f.render_stateful_widget(list, popup_rect, &mut list_state);
    }
}
