use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
};
use talos_sdk::{
    PendingUserAction, QuestionAnswer, SandboxApprovalDecision, SandboxApprovalPresentation,
    UserActionParams, UserActionResponse, UserActionState,
};

use crate::render::frame::RenderFrame;
use crate::types::CommandUpdate;
use crate::TuiApp;
use tokio::sync::mpsc;
use tracing::error;

/// Owns TUI-only selection and text-entry state for one server-owned user action.
#[derive(Debug, Clone)]
pub(crate) struct PendingActionState {
    pub(crate) turn_id: String,
    pub(crate) action: PendingUserAction,
    selected: usize,
    editing_other: bool,
    other_input: String,
    pub(crate) submitting: bool,
    pub(crate) error: Option<String>,
}

impl PendingActionState {
    pub(crate) fn new(turn_id: String, action: PendingUserAction) -> Self {
        Self {
            turn_id,
            action,
            selected: 0,
            editing_other: false,
            other_input: String::new(),
            submitting: false,
            error: None,
        }
    }

    pub(crate) fn move_selection(&mut self, direction: i32) {
        if self.submitting || self.editing_other {
            return;
        }
        let count = self.option_count();
        if count == 0 {
            return;
        }
        self.selected = if direction < 0 {
            self.selected.saturating_sub(1)
        } else {
            (self.selected + 1).min(count - 1)
        };
    }

    pub(crate) fn insert_char(&mut self, value: char) {
        if self.editing_other && !self.submitting && self.other_input.chars().count() < 1_000 {
            self.other_input.push(value);
        }
    }

    pub(crate) fn is_editing_other(&self) -> bool {
        self.editing_other
    }

    pub(crate) fn backspace(&mut self) {
        if self.editing_other && !self.submitting {
            self.other_input.pop();
        }
    }

    pub(crate) fn confirm(&mut self) -> Option<UserActionResponse> {
        if self.submitting || self.action.state != UserActionState::Pending {
            return None;
        }
        match &self.action.params {
            UserActionParams::Question(params) => {
                if self.editing_other {
                    let text = self.other_input.trim();
                    if text.is_empty() {
                        return None;
                    }
                    return Some(UserActionResponse::Question(QuestionAnswer::FreeForm {
                        text: text.to_string(),
                    }));
                }
                if self.selected == params.options.len() {
                    self.editing_other = true;
                    return None;
                }
                params.options.get(self.selected).map(|option| {
                    UserActionResponse::Question(QuestionAnswer::SelectedOption {
                        option_id: option.id.clone(),
                    })
                })
            }
            UserActionParams::SandboxApproval(params) => params
                .available_decisions
                .get(self.selected)
                .copied()
                .map(UserActionResponse::SandboxApproval),
            UserActionParams::DynamicToolResult(_) => None,
        }
    }

    pub(crate) fn render(&self, f: &mut impl RenderFrame, area: Rect) {
        let width = area.width.min(84);
        let height = area.height.min(18);
        let popup = Rect::new(
            area.x + area.width.saturating_sub(width) / 2,
            area.y + area.height.saturating_sub(height) / 2,
            width,
            height,
        );
        f.render_widget(Clear, popup);
        let block = Block::default()
            .borders(Borders::ALL)
            .title(" Talos needs your input ")
            .style(Style::default().bg(Color::Rgb(33, 38, 49)).fg(Color::White));
        let inner = block.inner(popup);
        f.render_widget(block, popup);
        f.render_widget(
            Paragraph::new(self.lines()).wrap(Wrap { trim: false }),
            inner,
        );
    }

    fn option_count(&self) -> usize {
        match &self.action.params {
            UserActionParams::Question(params) => params.options.len() + 1,
            UserActionParams::SandboxApproval(params) => params.available_decisions.len(),
            UserActionParams::DynamicToolResult(_) => 0,
        }
    }

    fn lines(&self) -> Vec<Line<'static>> {
        let mut lines = match &self.action.params {
            UserActionParams::Question(params) => {
                let mut lines = vec![
                    Line::from(Span::styled(
                        params.header.clone(),
                        Style::default().add_modifier(Modifier::BOLD),
                    )),
                    Line::from(params.question.clone()),
                    Line::from(""),
                ];
                for (index, option) in params.options.iter().enumerate() {
                    lines.push(option_line(
                        index == self.selected,
                        &option.label,
                        &option.description,
                    ));
                }
                lines.push(option_line(
                    self.selected == params.options.len(),
                    "Other",
                    "Enter a custom response",
                ));
                if self.editing_other {
                    lines.push(Line::from(format!("> {}", self.other_input)));
                }
                lines
            }
            UserActionParams::SandboxApproval(params) => {
                let mut lines = vec![
                    Line::from(Span::styled(
                        format!("Permission required for {}", params.tool_name),
                        Style::default().add_modifier(Modifier::BOLD),
                    )),
                    Line::from(approval_scope(&params.presentation)),
                    Line::from(""),
                ];
                for (index, decision) in params.available_decisions.iter().enumerate() {
                    lines.push(option_line(
                        index == self.selected,
                        decision_label(*decision),
                        "",
                    ));
                }
                lines
            }
            UserActionParams::DynamicToolResult(params) => vec![Line::from(format!(
                "{} is waiting for an external tool result.",
                params.tool_name
            ))],
        };
        lines.push(Line::from(""));
        let footer = if self.submitting {
            "Submitting…"
        } else if self.editing_other {
            "Type response · Enter submit · Esc keeps the action pending"
        } else {
            "↑/↓ select · Enter submit · Esc keeps the action pending"
        };
        lines.push(Line::from(Span::styled(
            footer,
            Style::default().fg(Color::Gray),
        )));
        if let Some(error) = &self.error {
            lines.push(Line::from(Span::styled(
                error.clone(),
                Style::default().fg(Color::Red),
            )));
        }
        lines
    }
}

fn option_line(selected: bool, label: &str, description: &str) -> Line<'static> {
    let marker = if selected { "›" } else { " " };
    Line::from(format!("{marker} {label}  {description}"))
}

fn approval_scope(presentation: &SandboxApprovalPresentation) -> String {
    match presentation {
        SandboxApprovalPresentation::Command { command, cwd, .. } => format!("{cwd}\n$ {command}"),
        SandboxApprovalPresentation::FileSystem { paths, .. } => paths.join(", "),
        SandboxApprovalPresentation::Network { scope } => scope.clone(),
    }
}

fn decision_label(decision: SandboxApprovalDecision) -> &'static str {
    match decision {
        SandboxApprovalDecision::AllowOnce => "Allow once",
        SandboxApprovalDecision::Deny => "Deny",
        SandboxApprovalDecision::Cancel => "Cancel",
    }
}

impl TuiApp {
    pub(crate) fn submit_pending_user_action(&mut self) {
        let Some(response) = self
            .pending_user_action
            .as_mut()
            .and_then(PendingActionState::confirm)
        else {
            return;
        };
        let Some(client) = self.gateway_client.clone() else {
            return;
        };
        let Some(state) = self.pending_user_action.as_mut() else {
            return;
        };
        state.submitting = true;
        state.error = None;
        let action_id = state.action.action_id.clone();
        let request = talos_sdk::SubmitTurnInputRequest {
            identity: talos_sdk::AgentTurnRef::new(
                self.session_id.clone().unwrap_or_default(),
                state.turn_id.clone(),
            ),
            input: talos_sdk::SubmitTurnInput::UserActionResponse {
                action_id: action_id.clone(),
                response,
            },
            client_request_id: None,
        };
        let _ = self.spawn_command_task("submitting user action", async move {
            CommandUpdate::UserActionSubmitted {
                action_id,
                result: client.submit_turn_input(request).await,
            }
        });
    }

    pub(crate) fn reconnect_after_user_action(&mut self) {
        let (Some(client), Some(session_id)) =
            (self.gateway_client.clone(), self.session_id.clone())
        else {
            return;
        };
        self.abort_pending_stream_task();
        let since_event_id = self.last_stream_event_id.clone();
        self.last_stream_sequence = None;
        let (tx, rx) = mpsc::channel(1);
        self.pending_stream_connect = Some(rx);
        self.in_flight = true;
        let handle = tokio::spawn(async move {
            let result = client
                .reconnect_active_turn_stream(&session_id, since_event_id)
                .await;
            if tx.send(result).await.is_err() {
                error!("failed to deliver resumed user-action stream to TUI task");
            }
        });
        self.pending_stream_task = Some(handle);
    }
}

#[cfg(test)]
#[path = "user_action_tests.rs"]
mod tests;
