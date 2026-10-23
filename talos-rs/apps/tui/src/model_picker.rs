use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Clear, List, ListItem, ListState, Paragraph},
};
use talos_error::{TalosError, TalosResult};
use talos_sdk::{ModelInfo, ModelSelection, ReasoningControl, ReasoningEffort};

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
    current_selection: ModelSelection,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum ModelPickerStep {
    Model,
    Reasoning,
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
    pub(crate) const SUPPORTED: [Self; 5] = [
        Self {
            effort: None,
            label: "None",
            description: "Disable reasoning for this turn",
        },
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

    fn for_model(model: &ModelInfo) -> Vec<Self> {
        match model.selection_capabilities.reasoning {
            ReasoningControl::None => Self::NONE.to_vec(),
            ReasoningControl::Effort => Self::SUPPORTED.to_vec(),
            ReasoningControl::Toggle => vec![
                Self {
                    effort: None,
                    label: "Off",
                    description: "Disable reasoning",
                },
                Self {
                    effort: Some(model.model.reasoning_effort),
                    label: "On",
                    description: "Enable reasoning",
                },
            ],
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
        current_selection: &ModelSelection,
    ) -> TalosResult<Self> {
        let current_model = &current_selection.model_ref;
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
        current_selection.resolve_reasoning(model)?;
        let mut state = Self {
            step: ModelPickerStep::Model,
            models: models.to_vec(),
            selected_model,
            selected_reasoning: 0,
            current_selection: current_selection.clone(),
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

    pub(crate) fn confirm(&mut self) -> Option<ModelSelection> {
        if self.step == ModelPickerStep::Model {
            self.step = ModelPickerStep::Reasoning;
            self.sync_selected_reasoning();
            return None;
        }
        let model = &self.models[self.selected_model];
        Some(ModelSelection::new(
            talos_sdk::ModelRef {
                provider: model.provider.clone(),
                id: model.model.id.clone(),
            },
            self.reasoning_options()[self.selected_reasoning].effort,
        ))
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
        let items = self
            .models
            .iter()
            .enumerate()
            .map(|(index, model)| self.model_item(index, model, bg_style))
            .collect();
        render_options(
            f,
            area,
            bg_style,
            "Select Model Options".to_string(),
            items,
            self.selected_model,
        );
    }

    fn model_item(&self, index: usize, model: &ModelInfo, bg_style: Style) -> ListItem<'static> {
        let selected = index == self.selected_model;
        let current = model.provider == self.current_selection.model_ref.provider
            && model.model.id == self.current_selection.model_ref.id;
        let name = if current {
            format!(
                "{}. {} / {} (current)",
                index + 1,
                model.provider_name.as_deref().unwrap_or(&model.provider),
                model.model.display_name()
            )
        } else {
            format!(
                "{}. {} / {}",
                index + 1,
                model.provider_name.as_deref().unwrap_or(&model.provider),
                model.model.display_name()
            )
        };
        let description = if model.model.reasoning {
            "Reasoning model for complex work."
        } else {
            "General-purpose model."
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
        let model = &self.models[self.selected_model];
        let items = self
            .reasoning_options()
            .iter()
            .enumerate()
            .map(|(index, option)| {
                let same_effort =
                    if model.selection_capabilities.reasoning == ReasoningControl::Toggle {
                        option.effort.is_some() == self.current_selection.reasoning_effort.is_some()
                    } else {
                        option.effort == self.current_selection.reasoning_effort
                    };
                let current = model.provider == self.current_selection.model_ref.provider
                    && model.model.id == self.current_selection.model_ref.id
                    && same_effort;
                self.reasoning_item(index, option, current, bg_style)
            })
            .collect();
        render_options(
            f,
            area,
            bg_style,
            format!("Select Reasoning Level for {}", model.model.display_name()),
            items,
            self.selected_reasoning,
        );
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
        let effort = if model.provider == self.current_selection.model_ref.provider
            && model.model.id == self.current_selection.model_ref.id
        {
            self.current_selection.reasoning_effort
        } else {
            model.selection_capabilities.default_reasoning_effort
        };
        self.selected_reasoning =
            if model.selection_capabilities.reasoning == ReasoningControl::Toggle {
                usize::from(effort.is_some())
            } else {
                self.reasoning_options()
                    .iter()
                    .position(|option| option.effort == effort)
                    .expect("validated model reasoning state must have a picker option")
            };
    }

    fn reasoning_options(&self) -> Vec<ReasoningOption> {
        ReasoningOption::for_model(&self.models[self.selected_model])
    }
}

fn render_options(
    f: &mut impl RenderFrame,
    area: Rect,
    bg_style: Style,
    title: String,
    items: Vec<ListItem<'static>>,
    selected: usize,
) {
    f.render_widget(
        Paragraph::new(Line::from(Span::styled(
            title,
            bg_style
                .fg(Color::Rgb(242, 243, 245))
                .add_modifier(Modifier::BOLD),
        )))
        .style(bg_style),
        area,
    );
    let list_area = Rect::new(
        area.x,
        area.y.saturating_add(2),
        area.width,
        area.height.saturating_sub(3),
    );
    if list_area.height > 0 {
        let mut state = ListState::default();
        state.select(Some(selected));
        f.render_stateful_widget(
            List::new(items)
                .style(bg_style)
                .highlight_style(bg_style)
                .highlight_symbol("› ")
                .highlight_spacing(ratatui::widgets::HighlightSpacing::Always),
            list_area,
            &mut state,
        );
    }
    render_footer(f, area, bg_style);
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
