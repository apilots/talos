//! Display time from canonical task and exact admitted Input facts, never scheduling.
use super::*;

impl TuiApp {
    pub(crate) fn automation_task_time(&self, task: &EditTarget, now: i64) -> String {
        match task {
            EditTarget::Goal(goal) => {
                let end = if goal.status == GoalStatus::Complete {
                    goal.updated_at
                } else {
                    now
                };
                duration_label(end.saturating_sub(goal.created_at))
            }
            EditTarget::Loop(task) => {
                if task.status == LoopStatus::Stopped {
                    return "ended".into();
                }
                if self.in_flight {
                    if let Some(started) = self
                        .active_stream_turn_id
                        .as_deref()
                        .and_then(|turn| self.render_runtime.loop_input_created_at(turn, &task.id))
                    {
                        return duration_label(now.saturating_sub(started));
                    }
                }
                if self.message_queue.messages.iter().any(|message| {
                    matches!(&message.source, QueuedMessageSource::Loop { loop_id, .. } if loop_id == &task.id)
                }) {
                    return "queued".into();
                }
                task.next_due_at
                    .and_then(|due| chrono::DateTime::from_timestamp(due, 0))
                    .map(|time| {
                        format!(
                            "next {}",
                            time.with_timezone(&chrono::Local).format("%m-%d %H:%M")
                        )
                    })
                    .unwrap_or_else(|| match task.schedule {
                        LoopSchedule::ModelDecided {} => "model-decided".into(),
                        LoopSchedule::FixedInterval { .. } => "no next time".into(),
                    })
            }
        }
    }
}

fn duration_label(seconds: i64) -> String {
    let seconds = seconds.max(0);
    if seconds >= 3600 {
        format!(
            "{}h {:02}m {:02}s",
            seconds / 3600,
            seconds / 60 % 60,
            seconds % 60
        )
    } else if seconds >= 60 {
        format!("{}m {:02}s", seconds / 60, seconds % 60)
    } else {
        format!("{seconds}s")
    }
}
