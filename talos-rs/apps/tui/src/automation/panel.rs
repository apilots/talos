//! One bounded composer surface for task definitions and the existing queue.
use super::*;
use ratatui::text::Line;
impl TuiApp {
    pub(crate) fn task_panel_height(&self) -> u16 {
        if self.automation_edit.is_some() {
            3
        } else if !self.automation_targets().is_empty()
            || !self.message_queue.messages.is_empty()
            || self.automation_error.is_some()
        {
            if self.tasks_expanded || self.selected_queue_message.is_some() {
                6
            } else {
                2
            }
        } else {
            0
        }
    }
    pub(crate) fn task_panel_lines(&self, max_lines: usize) -> Vec<Line<'static>> {
        if let Some(edit) = &self.automation_edit {
            return vec![Line::from(match &edit.target {
                EditTarget::Goal(_) => {
                    "Edit Goal · budget unchanged · Enter save / Esc cancel".into()
                }
                EditTarget::Loop(_) => format!(
                    "Edit Loop · {} · Tab mode / Ctrl+↑↓ minutes · Enter save / Esc cancel",
                    schedule_label(edit.schedule.as_ref().unwrap())
                ),
            })];
        }
        let targets = self.automation_targets();
        let header = format!(
            "{} Tasks {} · Waiting {}/{} · Ctrl+G toggle · Ctrl+Q queue",
            if self.tasks_expanded { "▾" } else { "▸" },
            targets.len(),
            self.message_queue.messages.len(),
            self.message_queue.max_messages
        );
        let mut lines = vec![Line::from(header)];
        if !self.tasks_expanded && self.selected_queue_message.is_none() {
            return lines;
        }
        if targets.is_empty()
            && self.automation_error.is_none()
            && self
                .message_queue
                .messages
                .iter()
                .all(|message| message.source == QueuedMessageSource::User)
        {
            return self.queue_panel_lines(max_lines);
        }
        let mut rows: Vec<(String, bool)> = targets
            .iter()
            .enumerate()
            .map(|(i, target)| {
                let text = match target {
                    EditTarget::Goal(g) => format!(
                        "◎ Goal · {} · {}",
                        g.objective,
                        self.automation_task_time(target, chrono::Utc::now().timestamp())
                    ),
                    EditTarget::Loop(l) => format!(
                        "↻ Loop · {} · {}",
                        l.prompt,
                        self.automation_task_time(target, chrono::Utc::now().timestamp())
                    ),
                };
                (text, self.selected_automation == Some(i))
            })
            .collect();
        rows.extend(
            self.message_queue
                .messages
                .iter()
                .enumerate()
                .filter(|(_, message)| match &message.source {
                    QueuedMessageSource::User => true,
                    QueuedMessageSource::Loop { loop_id, .. } => !targets
                        .iter()
                        .any(|target| matches!(target,EditTarget::Loop(l) if &l.id==loop_id)),
                })
                .map(|(i, message)| {
                    (
                        crate::message_queue::queued_message_summary(message),
                        self.selected_queue_message == Some(i),
                    )
                }),
        );
        let visible = max_lines.saturating_sub(2).max(1);
        let selected = rows.iter().position(|(_, s)| *s).unwrap_or(0);
        let start = selected.saturating_sub(visible - 1);
        lines.extend(
            rows.into_iter()
                .skip(start)
                .take(visible)
                .map(|(text, selected)| {
                    Line::from(format!(
                        "{} {}",
                        if selected { "❯" } else { " " },
                        text.replace('\n', " ")
                    ))
                }),
        );
        if let Some(error) = &self.automation_error {
            lines.push(Line::from(error.clone()));
        } else if self.selected_automation.is_some() {
            lines.push(Line::from(
                "Enter pause/resume/stop · e edit · d clear · Esc close",
            ));
        } else if self.selected_queue_message.is_some() {
            lines.push(Line::from(
                "Enter Steer/Play · e edit · d delete · Ctrl+↑↓ move · Esc close",
            ));
        }
        lines.truncate(max_lines);
        lines
    }
}
fn schedule_label(schedule: &LoopSchedule) -> String {
    match schedule {
        LoopSchedule::FixedInterval { minutes } => format!("every {minutes}m"),
        LoopSchedule::ModelDecided {} => "model-decided".into(),
    }
}
