use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph},
};
use talos_sdk::UserPermissionMode;

use crate::render::frame::RenderFrame;

const OPTIONS: [(UserPermissionMode, &str, &str); 3] = [
    (
        UserPermissionMode::AskForApproval,
        "Ask for approval",
        "Ask when an action needs additional permissions",
    ),
    (
        UserPermissionMode::ApproveForMe,
        "Approve for me",
        "Run safe actions and ask before risky commands",
    ),
    (
        UserPermissionMode::FullAccess,
        "Full access",
        "Run without sandbox restrictions",
    ),
];

/// Owns transient `/permissions` selection and FullAccess confirmation state.
#[derive(Debug, Clone)]
pub(crate) struct PermissionPickerState {
    current: UserPermissionMode,
    selected: usize,
    confirming_full_access: bool,
}

#[cfg(test)]
#[path = "permission_picker_tests.rs"]
mod tests;

impl PermissionPickerState {
    pub(crate) fn new(current: UserPermissionMode) -> Self {
        let selected = OPTIONS
            .iter()
            .position(|(mode, _, _)| *mode == current)
            .unwrap_or_default();
        Self {
            current,
            selected,
            confirming_full_access: false,
        }
    }

    pub(crate) fn move_selection(&mut self, direction: i32) {
        self.confirming_full_access = false;
        self.selected = if direction < 0 {
            self.selected.saturating_sub(1)
        } else {
            (self.selected + 1).min(OPTIONS.len() - 1)
        };
    }

    pub(crate) fn confirm(&mut self) -> Option<UserPermissionMode> {
        let mode = OPTIONS[self.selected].0;
        if mode == UserPermissionMode::FullAccess
            && self.current != UserPermissionMode::FullAccess
            && !self.confirming_full_access
        {
            self.confirming_full_access = true;
            return None;
        }
        Some(mode)
    }

    pub(crate) fn render(&self, f: &mut impl RenderFrame, area: Rect) {
        let height = if self.confirming_full_access { 11 } else { 9 }.min(area.height);
        let width = area.width.min(72);
        let popup = Rect::new(
            area.x + area.width.saturating_sub(width) / 2,
            area.y + area.height.saturating_sub(height) / 2,
            width,
            height,
        );
        f.render_widget(Clear, popup);
        let block = Block::default()
            .borders(Borders::ALL)
            .title(" Choose what Talos is allowed to do ")
            .style(Style::default().bg(Color::Rgb(33, 38, 49)).fg(Color::White));
        let inner = block.inner(popup);
        f.render_widget(block, popup);

        let items = OPTIONS
            .iter()
            .map(|(mode, label, description)| {
                let current = if *mode == self.current {
                    " (current)"
                } else {
                    ""
                };
                ListItem::new(Line::from(vec![
                    Span::styled(
                        format!("{label}{current}"),
                        Style::default().add_modifier(Modifier::BOLD),
                    ),
                    Span::raw(format!("  {description}")),
                ]))
            })
            .collect::<Vec<_>>();
        let mut state = ListState::default();
        state.select(Some(self.selected));
        let list_height = OPTIONS.len() as u16;
        f.render_stateful_widget(
            List::new(items)
                .highlight_symbol("› ")
                .highlight_style(Style::default().fg(Color::Cyan)),
            Rect::new(inner.x, inner.y, inner.width, list_height),
            &mut state,
        );
        let footer_y = inner.y.saturating_add(list_height + 1);
        let footer = if self.confirming_full_access {
            "Full access removes sandbox restrictions. Press Enter again to confirm; Esc cancels."
        } else {
            "↑/↓ select · Enter confirm · Esc close"
        };
        f.render_widget(
            Paragraph::new(footer).style(Style::default().fg(Color::Gray)),
            Rect::new(
                inner.x,
                footer_y,
                inner.width,
                inner.height.saturating_sub(list_height + 1),
            ),
        );
    }
}

pub(crate) fn permission_mode_label(mode: UserPermissionMode) -> &'static str {
    match mode {
        UserPermissionMode::AskForApproval => "Ask for approval",
        UserPermissionMode::ApproveForMe => "Approve for me",
        UserPermissionMode::FullAccess => "Full access",
    }
}
