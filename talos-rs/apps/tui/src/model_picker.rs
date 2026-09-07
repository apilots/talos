use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Clear, List, ListItem, ListState, Paragraph},
};
use talos_error::{TalosError, TalosResult};
use talos_sdk::{ModelInfo, ModelRef, ReasoningEffort};

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
    current_model: ModelRef,
    reasoning_effort_override: Option<ReasoningEffort>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum ModelPickerStep {
    Model,
    Reasoning,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ModelPickerSelection {
    pub(crate) model_ref: ModelRef,
    pub(crate) reasoning_effort: Option<ReasoningEffort>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ReasoningOption {
    pub(crate) effort: Option<ReasoningEffort>,
    label: &'static str,
    description: &'static str,
}

impl ReasoningOption {
    pub(crate) const NONE: [Self; 1] = [Self {
        effort: None,
        label: "None",
        description: "Reasoning is not enabled for this model",
    }];
    pub(crate) const SUPPORTED: [Self; 4] = [
        Self {
            effort: Some(ReasoningEffort::Light),
            label: "Light",
            description: "Fast responses with lighter reasoning",
        },
        Self {
            effort: Some(ReasoningEffort::Medium),
            label: "Medium",
            description: "Balances speed and reasoning depth for everyday tasks",
        },
        Self {
            effort: Some(ReasoningEffort::High),
            label: "High",
            description: "Greater reasoning depth for complex problems",
        },
        Self {
            effort: Some(ReasoningEffort::XHigh),
            label: "Extra high",
            description: "Extra high reasoning depth for complex problems",
        },
    ];

    fn for_model(model: &ModelInfo) -> &'static [Self] {
        if model.model.reasoning {
            &Self::SUPPORTED
        } else {
            &Self::NONE
        }
    }

    pub(crate) fn status_label_for(effort: ReasoningEffort) -> &'static str {
        match effort {
            ReasoningEffort::Light => "light",
            ReasoningEffort::Medium => "medium",
            ReasoningEffort::High => "high",
            ReasoningEffort::XHigh => "xhigh",
        }
    }
}

impl ModelPickerState {
    pub(crate) fn new(
        models: &[ModelInfo],
        current_model: &ModelRef,
        reasoning_effort_override: Option<ReasoningEffort>,
    ) -> TalosResult<Self> {
        let selected_model = models
            .iter()
            .position(|model| {
                model.provider == current_model.provider && model.model.id == current_model.id
            })
            .ok_or_else(|| {
                TalosError::internal("Current model is missing from the model picker catalog.")
                    .context(format!(
                        "current model '{}:{}' is missing from the model picker catalog",
                        current_model.provider, current_model.id
                    ))
            })?;
        let model = &models[selected_model];
        if model.model.reasoning != reasoning_effort_override.is_some() {
            return Err(
                TalosError::internal("Current model has inconsistent reasoning state.").context(
                    format!(
                        "current model '{}:{}' has inconsistent reasoning state",
                        current_model.provider, current_model.id
                    ),
                ),
            );
        }
        let mut state = Self {
            step: ModelPickerStep::Model,
            models: models.to_vec(),
            selected_model,
            selected_reasoning: 0,
            current_model: current_model.clone(),
            reasoning_effort_override,
        };
        state.sync_selected_reasoning();
        Ok(state)
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
                let option_count = self.reasoning_options().len();
                self.selected_reasoning = move_selection(
                    self.selected_reasoning,
                    option_count.saturating_sub(1),
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
        let model = self.models[self.selected_model].clone();
        if self.step == ModelPickerStep::Model {
            self.step = ModelPickerStep::Reasoning;
            self.sync_selected_reasoning();
            return None;
        }

        let reasoning_effort = if model.model.reasoning {
            ReasoningOption::for_model(&model)[self.selected_reasoning].effort
        } else {
            None
        };
        Some(ModelPickerSelection {
            model_ref: ModelRef {
                provider: model.provider,
                id: model.model.id,
            },
            reasoning_effort,
        })
    }

    pub(crate) fn render(&self, f: &mut impl RenderFrame, area: Rect, input_area: Option<Rect>) {
        let content_len = match self.step {
            ModelPickerStep::Model => self.models.len(),
            ModelPickerStep::Reasoning => self.reasoning_options().len(),
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
        let header_lines = vec![Line::from(Span::styled(
            "Select Model and Effort",
            bg_style
                .fg(Color::Rgb(242, 243, 245))
                .add_modifier(Modifier::BOLD),
        ))];
        f.render_widget(Paragraph::new(header_lines).style(bg_style), area);

        let list_area = Rect::new(
            area.x,
            area.y.saturating_add(2),
            area.width,
            area.height.saturating_sub(3),
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
        let current = model.provider == self.current_model.provider
            && model.model.id == self.current_model.id;
        let name = if current {
            format!(
                "{}. {} / {} (current)",
                index + 1,
                model.provider,
                model.model.id
            )
        } else {
            format!("{}. {} / {}", index + 1, model.provider, model.model.id)
        };
        let description = if model.model.reasoning {
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
        let model = self.models[self.selected_model].model.id.as_str();
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
            let items = self
                .reasoning_options()
                .iter()
                .enumerate()
                .map(|(index, option)| {
                    let selected_model = &self.models[self.selected_model];
                    let is_current = selected_model.provider == self.current_model.provider
                        && model == self.current_model.id
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
        let model = &self.models[self.selected_model];
        let effort = if model.provider == self.current_model.provider
            && model.model.id == self.current_model.id
        {
            self.reasoning_effort_override
        } else if model.model.reasoning {
            Some(model.model.reasoning_effort)
        } else {
            None
        };
        self.selected_reasoning = self
            .reasoning_options()
            .iter()
            .position(|option| option.effort == effort)
            .expect("validated model reasoning state must have a picker option");
    }

    fn reasoning_options(&self) -> &'static [ReasoningOption] {
        ReasoningOption::for_model(&self.models[self.selected_model])
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

#[cfg(test)]
#[path = "model_picker_tests.rs"]
mod tests;
