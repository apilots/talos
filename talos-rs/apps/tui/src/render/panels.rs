use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Clear, List, ListItem, ListState, Paragraph},
    Frame,
};

use crate::session_list_label;
use crate::types::{ModelPickerStep, ReasoningOption};
use crate::TuiApp;
impl TuiApp {
    pub(crate) fn render_model_picker(&self, f: &mut Frame, area: Rect) {
        let popup_area = self.left_panel_area.unwrap_or(area);

        let content_len = match self.model_picker_step {
            ModelPickerStep::Model => self.available_models.len(),
            ModelPickerStep::Reasoning => ReasoningOption::ALL.len(),
        };
        let popup_height = (content_len as u16 + 4).min(popup_area.height.max(1));
        let popup_width = popup_area.width;
        let y = self
            .input_area
            .map(|input_area| input_area.y.saturating_sub(popup_height))
            .unwrap_or(popup_area.y);
        let popup_rect = Rect::new(popup_area.x, y.max(popup_area.y), popup_width, popup_height);

        f.render_widget(Clear, popup_rect);

        let bg_style = Style::default().bg(Color::Rgb(70, 74, 88));
        f.render_widget(Block::default().style(bg_style), popup_rect);
        match self.model_picker_step {
            ModelPickerStep::Model => self.render_model_picker_models(f, popup_rect, bg_style),
            ModelPickerStep::Reasoning => {
                self.render_model_picker_reasoning(f, popup_rect, bg_style)
            }
        }
    }

    fn render_model_picker_models(&self, f: &mut Frame, area: Rect, bg_style: Style) {
        if area.height == 0 {
            return;
        }
        let header_lines = vec![
            Line::from(vec![Span::styled(
                "Select Model and Effort",
                bg_style
                    .fg(Color::Rgb(242, 243, 245))
                    .add_modifier(Modifier::BOLD),
            )]),
            Line::from(vec![Span::styled(
                "Access legacy models by running talos -m <model_name> or in your config.toml",
                bg_style.fg(Color::Rgb(180, 182, 190)),
            )]),
        ];
        f.render_widget(Paragraph::new(header_lines).style(bg_style), area);

        let list_area = Rect::new(
            area.x,
            area.y.saturating_add(3),
            area.width,
            area.height.saturating_sub(4),
        );
        if list_area.height > 0 {
            let items: Vec<ListItem> = self
                .available_model_info
                .iter()
                .enumerate()
                .map(|(index, model)| {
                    let selected = index == self.model_picker_selected;
                    let current = model.id == self.current_model;
                    let name = if current {
                        format!("{}. {} (current)", index + 1, model.id)
                    } else {
                        format!("{}. {}", index + 1, model.id)
                    };
                    let description = if model.reasoning {
                        "Reasoning model for complex work."
                    } else {
                        "Fast model for everyday work."
                    };
                    let selected_style = bg_style
                        .fg(Color::Rgb(134, 233, 255))
                        .add_modifier(Modifier::BOLD);
                    let normal_style = bg_style.fg(Color::Rgb(242, 243, 245));
                    let desc_style = if selected {
                        selected_style
                    } else {
                        bg_style.fg(Color::Rgb(178, 180, 188))
                    };
                    ListItem::new(Line::from(vec![
                        Span::styled(
                            name,
                            if selected {
                                selected_style
                            } else {
                                normal_style
                            },
                        ),
                        Span::raw("    "),
                        Span::styled(description, desc_style),
                    ]))
                })
                .collect();
            let mut list_state = ListState::default();
            list_state.select(Some(self.model_picker_selected));
            let list = List::new(items)
                .style(bg_style)
                .highlight_style(bg_style)
                .highlight_symbol("› ")
                .highlight_spacing(ratatui::widgets::HighlightSpacing::Always);
            f.render_stateful_widget(list, list_area, &mut list_state);
        }

        self.render_model_picker_footer(f, area, bg_style);
    }

    fn render_model_picker_reasoning(&self, f: &mut Frame, area: Rect, bg_style: Style) {
        if area.height == 0 {
            return;
        }
        let model = self
            .selected_model_id()
            .unwrap_or(self.current_model.as_str());
        let header = Line::from(vec![Span::styled(
            format!("Select Reasoning Level for {model}"),
            bg_style
                .fg(Color::Rgb(242, 243, 245))
                .add_modifier(Modifier::BOLD),
        )]);
        f.render_widget(Paragraph::new(vec![header]).style(bg_style), area);

        let list_area = Rect::new(
            area.x,
            area.y.saturating_add(2),
            area.width,
            area.height.saturating_sub(3),
        );
        if list_area.height > 0 {
            let current_effort = if model == self.current_model {
                self.current_reasoning_effort
            } else {
                None
            };
            let items: Vec<ListItem> = ReasoningOption::ALL
                .iter()
                .enumerate()
                .map(|(index, option)| {
                    let selected = index == self.reasoning_picker_selected;
                    let is_default = option.effort == Default::default();
                    let is_current = current_effort == Some(option.effort);
                    let suffix = if is_current {
                        " (current)"
                    } else if is_default {
                        " (default)"
                    } else {
                        ""
                    };
                    let label = format!("{}. {}{}", index + 1, option.label, suffix);
                    let selected_style = bg_style
                        .fg(Color::Rgb(134, 233, 255))
                        .add_modifier(Modifier::BOLD);
                    let normal_style = bg_style.fg(Color::Rgb(242, 243, 245));
                    let desc_style = if selected {
                        selected_style
                    } else {
                        bg_style.fg(Color::Rgb(178, 180, 188))
                    };
                    ListItem::new(Line::from(vec![
                        Span::styled(
                            label,
                            if selected {
                                selected_style
                            } else {
                                normal_style
                            },
                        ),
                        Span::raw("    "),
                        Span::styled(option.description, desc_style),
                    ]))
                })
                .collect();
            let mut list_state = ListState::default();
            list_state.select(Some(self.reasoning_picker_selected));
            let list = List::new(items)
                .style(bg_style)
                .highlight_style(bg_style)
                .highlight_symbol("› ")
                .highlight_spacing(ratatui::widgets::HighlightSpacing::Always);
            f.render_stateful_widget(list, list_area, &mut list_state);
        }

        self.render_model_picker_footer(f, area, bg_style);
    }

    fn render_model_picker_footer(&self, f: &mut Frame, area: Rect, bg_style: Style) {
        if area.height == 0 {
            return;
        }
        let footer_area = Rect::new(
            area.x,
            area.y.saturating_add(area.height.saturating_sub(1)),
            area.width,
            1,
        );
        f.render_widget(
            Paragraph::new(Line::from(vec![Span::styled(
                "Press enter to confirm or esc to go back",
                bg_style.fg(Color::Rgb(180, 182, 190)),
            )]))
            .style(bg_style),
            footer_area,
        );
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
