use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Clear, List, ListItem, ListState},
};

use crate::{render::frame::RenderFrame, TuiApp};

impl TuiApp {
    pub(crate) fn command_autocomplete_panel_height(&self) -> u16 {
        if !self.show_command_autocomplete {
            return 0;
        }
        self.command_suggestions.len().clamp(1, 7) as u16
    }

    pub(crate) fn render_command_autocomplete(&self, f: &mut impl RenderFrame, area: Rect) {
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

        let items = if self.command_suggestions.is_empty() {
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
                    let command_style = selected_style(is_selected);
                    let description_style = if is_selected {
                        selected_style(true)
                    } else {
                        Style::default().fg(Color::Rgb(186, 186, 186))
                    };
                    ListItem::new(vec![Line::from(vec![
                        Span::styled(cmd.display_text.clone(), command_style),
                        Span::raw("  "),
                        Span::styled(cmd.description.clone(), description_style),
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

fn selected_style(is_selected: bool) -> Style {
    if is_selected {
        Style::default()
            .fg(Color::Rgb(134, 233, 255))
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::Rgb(242, 243, 245))
    }
}
