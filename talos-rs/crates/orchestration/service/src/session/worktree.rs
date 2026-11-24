use super::SessionService;
use agent_protocol::WorkerWorktreeRequest;
use std::path::PathBuf;
use talos_error::{ErrorCode, TalosError, TalosResult};
use talos_store::session::{
    PatchSessionParams, SessionRecord, SessionRecordPatch, SessionTreeIdentity,
};
use talos_types::{AgentUserContent, AgentUserInput, SessionWorktreeBinding};

/// Only a leading request in the user's message can change the execution directory.
pub(crate) enum WorktreeDirective {
    CreateFromOriginMain,
    Bind(PathBuf),
}

impl WorktreeDirective {
    pub(crate) fn from_input(input: &AgentUserInput) -> TalosResult<Option<Self>> {
        let Some(text) = input.content.iter().find_map(|item| match item {
            AgentUserContent::Text { text } => Some(text.trim_start()),
            _ => None,
        }) else {
            return Ok(None);
        };
        let mut text = text;
        // Accept the common polite prefixes in English and Chinese.
        for prefix in [
            "please ",
            "\u{8bf7}",
            "\u{5e2e}\u{6211}",
            "\u{6307}\u{5b9a}",
            "\u{5148}",
        ] {
            if text.to_ascii_lowercase().starts_with(prefix) {
                text = text[prefix.len()..].trim_start();
            }
        }
        let lower = text.to_ascii_lowercase();
        for prefix in [
            "/worktree bind ",
            "bind existing worktree ",
            "use existing worktree ",
            "switch to existing worktree ",
            "\u{7ed1}\u{5b9a}\u{5df2}\u{6709} worktree",
            "\u{4f7f}\u{7528}\u{5df2}\u{6709} worktree",
            "\u{5207}\u{5230}\u{5df2}\u{6709} worktree",
        ] {
            if lower.starts_with(prefix) {
                return checkout_path(text[prefix.len()..].trim())
                    .map(Self::Bind)
                    .map(Some);
            }
        }
        let first_clause = lower
            .split(['\n', ';', '\u{3002}', '\u{ff1b}', '?', '\u{ff1f}'])
            .next()
            .unwrap_or_default();
        let english = [
            "create a new worktree ",
            "create new worktree ",
            "create a worktree ",
            "start a new worktree ",
        ]
        .iter()
        .find_map(|prefix| first_clause.strip_prefix(prefix))
        .and_then(|tail| {
            ["from ", "based on ", "using "]
                .iter()
                .find_map(|prefix| tail.strip_prefix(prefix))
        })
        .is_some_and(|tail| {
            let tail = tail.trim_start_matches('\x60');
            ["origin/main", "remote main"].iter().any(|reference| {
                tail.strip_prefix(reference).is_some_and(|suffix| {
                    suffix.is_empty()
                        || suffix == "."
                        || suffix.starts_with(['\x60', ' ', ',', '\u{ff0c}'])
                })
            })
        });
        let clause = first_clause.replace(['\x60', ' '], "");
        let chinese = ["\u{57fa}\u{4e8e}", "\u{4ece}"]
            .iter()
            .find_map(|prefix| clause.strip_prefix(prefix))
            .and_then(|tail| {
                ["origin/main", "\u{8fdc}\u{7aef}main"]
                    .iter()
                    .find_map(|reference| tail.strip_prefix(reference))
            })
            .is_some_and(|action| {
                [
                    "\u{65b0}\u{5efa}worktree",
                    "\u{5f00}\u{4e2a}\u{65b0}worktree",
                    "\u{5f00}\u{4e00}\u{4e2a}\u{65b0}worktree",
                    "\u{5f00}\u{65b0}worktree",
                    "\u{5f00}\u{65b0}\u{7684}worktree",
                ]
                .iter()
                .any(|prefix| action.starts_with(prefix))
            });
        Ok(
            (first_clause == "/worktree create origin/main" || english || chinese)
                .then_some(Self::CreateFromOriginMain),
        )
    }
}

fn checkout_path(input: &str) -> TalosResult<PathBuf> {
    let input = input.trim_start_matches([':', '\u{ff1a}']).trim_start();
    let input = input.strip_prefix("at ").unwrap_or(input).trim_start();
    let path = match input.chars().next() {
        Some(delimiter @ ('`' | '"' | '\'')) => input[delimiter.len_utf8()..]
            .split_once(delimiter)
            .map(|(path, _)| path),
        _ => input
            .split(|ch: char| {
                ch.is_whitespace() || [',', ';', '\u{ff0c}', '\u{3002}'].contains(&ch)
            })
            .next(),
    }
    .map(PathBuf::from);
    match path {
        Some(path) if path.is_absolute() => Ok(path),
        _ => Err(TalosError::new(
            ErrorCode::InvalidRequest,
            "Provide an absolute path to the existing worktree. Quote paths that contain spaces.",
        )),
    }
}

impl SessionService {
    pub(super) async fn verify_worktree(
        &self,
        record: &SessionRecord,
        binding: SessionWorktreeBinding,
    ) -> TalosResult<SessionWorktreeBinding> {
        self.worktree_operation(record, WorkerWorktreeRequest::Verify(binding))
            .await
    }

    pub(super) async fn apply_worktree_directive(
        &self,
        record: &SessionRecord,
        directive: WorktreeDirective,
        operation_id: uuid::Uuid,
    ) -> TalosResult<(SessionRecord, PathBuf)> {
        if !matches!(record.tree_identity, SessionTreeIdentity::Primary)
            || self.has_active_subagent_turn(&record.id).await?
        {
            return Err(TalosError::new(
                ErrorCode::ActiveTurnRunning,
                "The working directory can only be changed in the main Session after its agents finish.",
            ));
        }
        self.ensure_worker_idle(record).await?;
        let repository = self.resolve_unbound_directory(record).await?;
        let operation = match directive {
            WorktreeDirective::Bind(checkout) => WorkerWorktreeRequest::Bind { repository, checkout },
            WorktreeDirective::CreateFromOriginMain => WorkerWorktreeRequest::CreateFromOriginMain {
                repository: repository.or_else(|| record.worktree_binding.as_ref().map(|binding| binding.checkout_root.clone()))
                    .ok_or_else(|| TalosError::new(
                        ErrorCode::InvalidRequest, "Select a Git project or provide its directory before creating a worktree.",
                    ))?,
                operation_id,
                expected_common_directory: record.worktree_binding.as_ref().map(|binding| binding.git_common_directory.clone()),
            },
        };
        let binding = self.worktree_operation(record, operation).await?;
        if record
            .worktree_binding
            .as_ref()
            .is_some_and(|previous| previous.git_common_directory != binding.git_common_directory)
        {
            return Err(TalosError::authorization_denied());
        }
        let checkout_root = binding.checkout_root.clone();
        let updated = self
            .session_store
            .patch_session(PatchSessionParams {
                session_id: record.id.clone(),
                change: SessionRecordPatch::Worktree(binding),
                last_activity: chrono::Utc::now()
                    .timestamp_millis()
                    .try_into()
                    .unwrap_or_default(),
            })
            .await?
            .ok_or_else(|| {
                TalosError::new(ErrorCode::ResourceNotFound, "The Session no longer exists.")
            })?;
        Ok((updated, checkout_root))
    }

    async fn worktree_operation(
        &self,
        record: &SessionRecord,
        request: WorkerWorktreeRequest,
    ) -> TalosResult<SessionWorktreeBinding> {
        self.worker_environment(&record.execution_node_id, &record.user_id)
            .await?
            .session_worktree(
                Self::environment_context(&record.user_id, Some(record.workspace_id.clone())),
                request,
            )
            .await
    }
}

#[cfg(test)]
#[path = "worktree_tests.rs"]
mod tests;
