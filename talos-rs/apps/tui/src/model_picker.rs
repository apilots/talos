use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Clear, List, ListItem, ListState, Paragraph},
};
use talos_sdk::{LlmReasoningEffort, ModelInfo};

use crate::render::frame::RenderFrame;

/// Owns the transient `/model` picker state and rendering contract.
///
/// The picker is a UI-only component: it may select a model and reasoning
/// effort, but it must not mutate sessions or write transcript blocks itself.
#[derive(Debug, Clone)]
pub(crate) struct ModelPickerState {
    step: ModelPickerStep,
    models: Vec<ModelInfo>,
    selected_model: usize,
    selected_reasoning: usize,
    current_model: String,
    reasoning_effort_override: Option<LlmReasoningEffort>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum ModelPickerStep {
    Model,
    Reasoning,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ModelPickerSelection {
    pub(crate) model: String,
    pub(crate) reasoning_effort: Option<LlmReasoningEffort>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ReasoningOption {
    pub(crate) effort: Option<LlmReasoningEffort>,
    label: &'static str,
    description: &'static str,
}

impl ReasoningOption {
    pub(crate) const ALL: [Self; 5] = [
        Self {
            effort: None,
            label: "Default",
            description: "Use the model's configured reasoning level",
        },
        Self {
            effort: Some(LlmReasoningEffort::Low),
            label: "Low",
            description: "Fast responses with lighter reasoning",
        },
        Self {
            effort: Some(LlmReasoningEffort::Medium),
            label: "Medium",
            description: "Balances speed and reasoning depth for everyday tasks",
        },
        Self {
            effort: Some(LlmReasoningEffort::High),
            label: "High",
            description: "Greater reasoning depth for complex problems",
        },
        Self {
            effort: Some(LlmReasoningEffort::XHigh),
            label: "Extra high",
            description: "Extra high reasoning depth for complex problems",
        },
    ];

    pub(crate) fn status_label_for(effort: LlmReasoningEffort) -> &'static str {
        match effort {
            LlmReasoningEffort::Low => "low",
            LlmReasoningEffort::Medium => "medium",
            LlmReasoningEffort::High => "high",
            LlmReasoningEffort::XHigh => "xhigh",
        }
    }
}

impl ModelPickerState {
    pub(crate) fn new(
        models: &[ModelInfo],
        current_model: &str,
        reasoning_effort_override: Option<LlmReasoningEffort>,
    ) -> Self {
        let selected_model = models
            .iter()
            .position(|model| model.id == current_model)
            .unwrap_or(0);
        let mut state = Self {
            step: ModelPickerStep::Model,
            models: models.to_vec(),
            selected_model,
            selected_reasoning: 0,
            current_model: current_model.to_string(),
            reasoning_effort_override,
        };
        state.sync_selected_reasoning();
        state
    }

    pub(crate) fn selected_model_id(&self) -> Option<&str> {
        self.models
            .get(self.selected_model)
            .map(|model| model.id.as_str())
    }

    pub(crate) fn move_selection(&mut self, direction: i32) {
        match self.step {
            ModelPickerStep::Model => {
                self.selected_model = move_selection(
                    self.selected_model,
                    self.models.len().saturating_sub(1),
                    direction,
                );
                self.sync_selected_reasoning();
            }
            ModelPickerStep::Reasoning => {
                self.selected_reasoning = move_selection(
                    self.selected_reasoning,
                    ReasoningOption::ALL.len().saturating_sub(1),
                    direction,
                );
            }
        }
    }

    pub(crate) fn back(&mut self) -> bool {
        match self.step {
            ModelPickerStep::Model => true,
            ModelPickerStep::Reasoning => {
                self.step = ModelPickerStep::Model;
                false
            }
        }
    }

    pub(crate) fn confirm(&mut self) -> Option<ModelPickerSelection> {
        let model = self.models.get(self.selected_model)?.clone();
        if model.reasoning && self.step == ModelPickerStep::Model {
            self.step = ModelPickerStep::Reasoning;
            self.sync_selected_reasoning();
            return None;
        }

        let reasoning_effort = if model.reasoning {
            ReasoningOption::ALL
                .get(self.selected_reasoning)
                .and_then(|option| option.effort)
        } else {
            None
        };
        Some(ModelPickerSelection {
            model: model.id,
            reasoning_effort,
        })
    }

    pub(crate) fn render(&self, f: &mut impl RenderFrame, area: Rect, input_area: Option<Rect>) {
        let content_len = match self.step {
            ModelPickerStep::Model => self.models.len(),
            ModelPickerStep::Reasoning => ReasoningOption::ALL.len(),
        };
        let popup_height = (content_len as u16 + 4).min(area.height.max(1));
        let y = input_area
            .map(|input_area| input_area.y.saturating_sub(popup_height))
            .unwrap_or(area.y)
            .max(area.y);
        let popup_rect = Rect::new(area.x, y, area.width, popup_height);

        f.render_widget(Clear, popup_rect);

        let bg_style = Style::default().bg(Color::Rgb(70, 74, 88));
        f.render_widget(Block::default().style(bg_style), popup_rect);
        match self.step {
            ModelPickerStep::Model => self.render_models(f, popup_rect, bg_style),
            ModelPickerStep::Reasoning => self.render_reasoning(f, popup_rect, bg_style),
        }
    }

    fn render_models(&self, f: &mut impl RenderFrame, area: Rect, bg_style: Style) {
        let header_lines = vec![
            Line::from(Span::styled(
                "Select Model and Effort",
                bg_style
                    .fg(Color::Rgb(242, 243, 245))
                    .add_modifier(Modifier::BOLD),
            )),
            Line::from(Span::styled(
                "Access legacy models by running talos -m <model_name> or in your config.toml",
                bg_style.fg(Color::Rgb(180, 182, 190)),
            )),
        ];
        f.render_widget(Paragraph::new(header_lines).style(bg_style), area);

        let list_area = Rect::new(
            area.x,
            area.y.saturating_add(3),
            area.width,
            area.height.saturating_sub(4),
        );
        if list_area.height > 0 {
            let items = self
                .models
                .iter()
                .enumerate()
                .map(|(index, model)| self.model_item(index, model, bg_style))
                .collect::<Vec<_>>();
            let mut list_state = ListState::default();
            list_state.select(Some(self.selected_model));
            let list = List::new(items)
                .style(bg_style)
                .highlight_style(bg_style)
                .highlight_symbol("› ")
                .highlight_spacing(ratatui::widgets::HighlightSpacing::Always);
            f.render_stateful_widget(list, list_area, &mut list_state);
        }

        render_footer(f, area, bg_style);
    }

    fn model_item(&self, index: usize, model: &ModelInfo, bg_style: Style) -> ListItem<'static> {
        let selected = index == self.selected_model;
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
            Span::styled(description.to_string(), desc_style),
        ]))
    }

    fn render_reasoning(&self, f: &mut impl RenderFrame, area: Rect, bg_style: Style) {
        let model = self
            .selected_model_id()
            .unwrap_or(self.current_model.as_str());
        let header = Line::from(Span::styled(
            format!("Select Reasoning Level for {model}"),
            bg_style
                .fg(Color::Rgb(242, 243, 245))
                .add_modifier(Modifier::BOLD),
        ));
        f.render_widget(Paragraph::new(vec![header]).style(bg_style), area);

        let list_area = Rect::new(
            area.x,
            area.y.saturating_add(2),
            area.width,
            area.height.saturating_sub(3),
        );
        if list_area.height > 0 {
            let items = ReasoningOption::ALL
                .iter()
                .enumerate()
                .map(|(index, option)| {
                    let is_current = model == self.current_model
                        && option.effort == self.reasoning_effort_override;
                    self.reasoning_item(index, option, is_current, bg_style)
                })
                .collect::<Vec<_>>();
            let mut list_state = ListState::default();
            list_state.select(Some(self.selected_reasoning));
            let list = List::new(items)
                .style(bg_style)
                .highlight_style(bg_style)
                .highlight_symbol("› ")
                .highlight_spacing(ratatui::widgets::HighlightSpacing::Always);
            f.render_stateful_widget(list, list_area, &mut list_state);
        }

        render_footer(f, area, bg_style);
    }

    fn reasoning_item(
        &self,
        index: usize,
        option: &ReasoningOption,
        is_current: bool,
        bg_style: Style,
    ) -> ListItem<'static> {
        let selected = index == self.selected_reasoning;
        let suffix = if is_current { " (current)" } else { "" };
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
            Span::styled(option.description.to_string(), desc_style),
        ]))
    }

    fn sync_selected_reasoning(&mut self) {
        let effort = self.models.get(self.selected_model).and_then(|model| {
            if model.id == self.current_model {
                self.reasoning_effort_override
            } else {
                None
            }
        });
        self.selected_reasoning = ReasoningOption::ALL
            .iter()
            .position(|option| option.effort == effort)
            .unwrap_or_default();
    }
}

fn render_footer(f: &mut impl RenderFrame, area: Rect, bg_style: Style) {
    let footer_area = Rect::new(
        area.x,
        area.y.saturating_add(area.height.saturating_sub(1)),
        area.width,
        1,
    );
    f.render_widget(
        Paragraph::new(Line::from(Span::styled(
            "Press enter to confirm or esc to go back",
            bg_style.fg(Color::Rgb(180, 182, 190)),
        )))
        .style(bg_style),
        footer_area,
    );
}

fn move_selection(current: usize, max: usize, direction: i32) -> usize {
    if direction < 0 {
        current.saturating_sub(1)
    } else if current < max {
        current + 1
    } else {
        current
    }
}
