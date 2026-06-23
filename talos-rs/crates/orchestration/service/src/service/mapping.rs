use talos_store::project::ProjectRecord;
use talos_store::session::IdempotencyStatus;
use talos_types::{ProjectInfo, TurnCommandStatus};

use crate::TimelineCursorStatus;

pub(super) fn project_info_from_record(record: ProjectRecord) -> ProjectInfo {
    ProjectInfo {
        project_id: record.project_id,
        name: record.name,
        directory: record.directory,
        status: record.status,
        created_at: record.created_at,
        updated_at: record.updated_at,
        session_count: record.session_count,
    }
}

pub(super) fn turn_command_status(status: IdempotencyStatus) -> TurnCommandStatus {
    match status {
        IdempotencyStatus::Pending => TurnCommandStatus::Pending,
        IdempotencyStatus::Accepted => TurnCommandStatus::Accepted,
        IdempotencyStatus::Failed => TurnCommandStatus::Failed,
        IdempotencyStatus::FailedNoFirstEvent => TurnCommandStatus::FailedNoFirstEvent,
    }
}

pub(super) fn map_timeline_cursor_status(
    status: TimelineCursorStatus,
) -> talos_types::TimelineCursorStatus {
    match status {
        TimelineCursorStatus::Current => talos_types::TimelineCursorStatus::Current,
        TimelineCursorStatus::FromBeginning => talos_types::TimelineCursorStatus::FromBeginning,
    }
}
