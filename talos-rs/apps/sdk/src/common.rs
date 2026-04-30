use crate::{ExpectedFileVersion, WorkspaceDocKind};

pub(crate) fn workspace_doc_kind_token(kind: WorkspaceDocKind) -> &'static str {
    match kind {
        WorkspaceDocKind::Soul => "soul",
        WorkspaceDocKind::Identity => "identity",
        WorkspaceDocKind::User => "user",
        WorkspaceDocKind::Heartbeat => "heartbeat",
        WorkspaceDocKind::Memory => "memory",
    }
}

pub(crate) fn expected_file_version_token(expected_version: ExpectedFileVersion) -> Option<String> {
    match expected_version {
        ExpectedFileVersion::MustNotExist => None,
        ExpectedFileVersion::Match { version_token } => Some(version_token),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_workspace_doc_kind_to_transport_token() {
        assert_eq!(workspace_doc_kind_token(WorkspaceDocKind::Soul), "soul");
        assert_eq!(workspace_doc_kind_token(WorkspaceDocKind::Memory), "memory");
    }
}
