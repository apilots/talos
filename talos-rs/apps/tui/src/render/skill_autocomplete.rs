use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Clear, List, ListItem, ListState, Paragraph},
};

use crate::{render::frame::RenderFrame, TuiApp};

impl TuiApp {
    pub(crate) fn skill_autocomplete_panel_height(&self) -> u16 {
        if !self.show_skill_autocomplete {
            return 0;
        }
        self.skill_suggestions.len().clamp(1, 7) as u16 + 2
    }

    pub(crate) fn render_skill_autocomplete(&self, f: &mut impl RenderFrame, area: Rect) {
        let Some(input_area) = self.input_area else {
            return;
        };
        let popup_area = self.left_panel_area.unwrap_or(area);
        let popup_height = self.skill_autocomplete_panel_height();
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

        let list_height = popup_rect.height.saturating_sub(2).max(1);
        let list_area = Rect::new(popup_rect.x, popup_rect.y, popup_rect.width, list_height);
        let footer_area = Rect::new(
            popup_rect.x,
            popup_rect
                .y
                .saturating_add(popup_rect.height.saturating_sub(1)),
            popup_rect.width,
            1,
        );

        let items: Vec<ListItem> = self
            .skill_suggestions
            .iter()
            .take(list_height as usize)
            .enumerate()
            .map(|(index, skill)| {
                let is_selected = index == self.skill_autocomplete_selected;
                let name_style = if is_selected {
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
                let source_style = if is_selected {
                    Style::default().fg(Color::Rgb(134, 233, 255))
                } else {
                    Style::default().fg(Color::Rgb(170, 170, 170))
                };
                let name = fixed_width(&skill_display_name(&skill.name), 22);
                ListItem::new(vec![Line::from(vec![
                    Span::styled(name, name_style),
                    Span::styled(
                        format!("{} ", skill_scope_label(&skill.scope)),
                        source_style,
                    ),
                    Span::styled(&skill.description, description_style),
                ])])
            })
            .collect();

        let mut list_state = ListState::default();
        if !self.skill_suggestions.is_empty() {
            list_state.select(Some(self.skill_autocomplete_selected));
        }
        let list = List::new(items)
            .style(bg_style)
            .highlight_style(Style::default().bg(Color::Rgb(40, 44, 58)))
            .highlight_symbol("› ")
            .highlight_spacing(ratatui::widgets::HighlightSpacing::Always);

        f.render_stateful_widget(list, list_area, &mut list_state);

        if popup_rect.height > 1 {
            let footer = Paragraph::new(Line::from(vec![
                Span::styled("Press ", Style::default().fg(Color::Rgb(242, 243, 245))),
                Span::styled("enter", Style::default().fg(Color::Rgb(186, 186, 186))),
                Span::styled(
                    " to insert or ",
                    Style::default().fg(Color::Rgb(242, 243, 245)),
                ),
                Span::styled("esc", Style::default().fg(Color::Rgb(186, 186, 186))),
                Span::styled(" to close", Style::default().fg(Color::Rgb(242, 243, 245))),
            ]))
            .style(bg_style);
            f.render_widget(footer, footer_area);
        }
    }
}

fn skill_display_name(name: &str) -> String {
    name.split(['-', '_', ':'])
        .filter(|part| !part.is_empty())
        .map(|part| {
            let mut chars = part.chars();
            match chars.next() {
                Some(first) => format!("{}{}", first.to_ascii_uppercase(), chars.as_str()),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn skill_scope_label(scope: &talos_sdk::SkillScope) -> &'static str {
    match scope {
        talos_sdk::SkillScope::Project => "[Project]",
        talos_sdk::SkillScope::Plugin => "[Plugin]",
        talos_sdk::SkillScope::External => "[External]",
        talos_sdk::SkillScope::System => "[System]",
    }
}

fn fixed_width(value: &str, width: usize) -> String {
    let mut output = value.chars().take(width).collect::<String>();
    let count = output.chars().count();
    if count < width {
        output.push_str(&" ".repeat(width - count));
    }
    output
}
