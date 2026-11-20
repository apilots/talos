//! Shared Session list/count predicates and parameter ordering.

use super::{DatabaseSessionStore, SessionQuery, SessionTreeScope};

impl DatabaseSessionStore {
    pub(super) fn append_computer_scope(
        query: &SessionQuery,
        clauses: &mut Vec<&'static str>,
        params: &mut Vec<String>,
    ) {
        if let Some(node_id) = query.execution_node_id.as_deref() {
            clauses.push(if query.include_embedded_execution {
                "execution_node_id IN (?, 'embedded')"
            } else {
                "execution_node_id = ?"
            });
            params.push(node_id.to_string());
        }
        if let Some(user_id) = query.user_id.as_deref() {
            clauses.push("user_id = ?");
            params.push(user_id.to_string());
        }
        if let Some(scope) = &query.unassigned_computer {
            clauses.push(crate::session::UNASSIGNED_SESSION_COMPUTER);
            params.push(scope.embedded_owner_id.clone().unwrap_or_default());
        }
    }

    pub(super) fn append_tree_scope(
        scope: &SessionTreeScope,
        clauses: &mut Vec<&'static str>,
        params: &mut Vec<String>,
    ) {
        match scope {
            SessionTreeScope::Primary => clauses.push("NULLIF(parent_session_id, '') IS NULL"),
            SessionTreeScope::SubagentTree {
                tree_root_session_id,
            } => {
                clauses.push("tree_root_session_id = ?");
                params.push(tree_root_session_id.clone());
            }
            SessionTreeScope::All => {}
        }
    }
}
