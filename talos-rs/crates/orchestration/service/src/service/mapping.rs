use talos_store::project::ProjectRecord;
use talos_types::ProjectInfo;

pub(super) fn project_info_from_record(record: ProjectRecord) -> ProjectInfo {
    ProjectInfo {
        node_id: record.execution_node_id,
        project_id: record.project_id,
        name: record.name,
        directory: record.directory,
        status: record.status,
        created_at: record.created_at,
        updated_at: record.updated_at,
        session_count: record.session_count,
    }
}
