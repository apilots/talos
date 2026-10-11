//! Lossless wire projections of the existing talos-memory domain objects.
use agent_pb as pb;
use talos_error::{ErrorCode, TalosError, TalosResult};
use talos_memory::*;
use talos_types::{ExpectedFileVersion, MemoryKind};

fn invalid() -> TalosError {
    TalosError::new(ErrorCode::InvalidRequest, "Invalid Agent memory payload")
}
fn source<E: std::error::Error + Send + Sync + 'static>(error: E) -> TalosError {
    TalosError::from_source(
        ErrorCode::InvalidRequest,
        "Invalid Agent memory payload",
        error,
    )
}

pub fn encode_snapshot(
    snapshot: UserWorkspaceMemorySnapshot,
) -> TalosResult<pb::WorkspaceMemorySnapshot> {
    use pb::workspace_memory_snapshot::State;
    let state = match snapshot {
        UserWorkspaceMemorySnapshot::Missing { template } => {
            State::MissingTemplate(template.render().map_err(source)?)
        }
        UserWorkspaceMemorySnapshot::Valid {
            raw_content,
            updated_at,
            version_token,
            ..
        } => State::Valid(pb::WorkspaceMemoryFile {
            content: raw_content,
            updated_at,
            version_token,
        }),
        UserWorkspaceMemorySnapshot::Invalid {
            raw_content,
            updated_at,
            version_token,
            diagnostic,
        } => State::Invalid(pb::InvalidWorkspaceMemoryFile {
            file: Some(pb::WorkspaceMemoryFile {
                content: raw_content,
                updated_at,
                version_token,
            }),
            diagnostic,
        }),
    };
    Ok(pb::WorkspaceMemorySnapshot { state: Some(state) })
}
pub fn decode_snapshot(
    value: pb::WorkspaceMemorySnapshot,
) -> TalosResult<UserWorkspaceMemorySnapshot> {
    use pb::workspace_memory_snapshot::State;
    Ok(match value.state.ok_or_else(invalid)? {
        State::MissingTemplate(content) => UserWorkspaceMemorySnapshot::Missing {
            template: UserWorkspaceMemoryDocument::parse(&content).map_err(source)?,
        },
        State::Valid(file) => UserWorkspaceMemorySnapshot::Valid {
            document: UserWorkspaceMemoryDocument::parse(&file.content).map_err(source)?,
            raw_content: file.content,
            updated_at: file.updated_at,
            version_token: file.version_token,
        },
        State::Invalid(value) => {
            let file = value.file.ok_or_else(invalid)?;
            UserWorkspaceMemorySnapshot::Invalid {
                raw_content: file.content,
                updated_at: file.updated_at,
                version_token: file.version_token,
                diagnostic: value.diagnostic,
            }
        }
    })
}

fn encode_entry(value: UserWorkspaceMemoryEntry) -> pb::WorkspaceMemoryEntry {
    pb::WorkspaceMemoryEntry {
        key: value.key,
        section: encode_section(value.section),
        title: value.title,
        kind: encode_kind(value.kind),
        summary: value.summary,
        why: value.why,
        apply: value.apply,
        status: value.status,
        next: value.next,
    }
}
fn decode_entry(value: pb::WorkspaceMemoryEntry) -> TalosResult<UserWorkspaceMemoryEntry> {
    let entry = UserWorkspaceMemoryEntry {
        key: value.key,
        section: decode_section(value.section)?,
        title: value.title,
        kind: decode_kind(value.kind)?,
        summary: value.summary,
        why: value.why,
        apply: value.apply,
        status: value.status,
        next: value.next,
    };
    entry.validate().map_err(source)?;
    Ok(entry)
}
fn encode_field(value: OptionalField<String>) -> Option<pb::OptionalMemoryField> {
    use pb::optional_memory_field::Action;
    Some(pb::OptionalMemoryField {
        action: Some(match value {
            OptionalField::Unchanged => Action::Unchanged(pb::Empty {}),
            OptionalField::Set(value) => Action::Set(value),
            OptionalField::Clear => Action::Clear(pb::Empty {}),
        }),
    })
}
fn decode_field(value: Option<pb::OptionalMemoryField>) -> TalosResult<OptionalField<String>> {
    use pb::optional_memory_field::Action;
    Ok(
        match value.ok_or_else(invalid)?.action.ok_or_else(invalid)? {
            Action::Unchanged(_) => OptionalField::Unchanged,
            Action::Set(value) => OptionalField::Set(value),
            Action::Clear(_) => OptionalField::Clear,
        },
    )
}
pub fn encode_update(value: UserWorkspaceMemoryUpdate) -> pb::WorkspaceMemoryUpdate {
    use pb::workspace_memory_update::Operation;
    let operation = match value {
        UserWorkspaceMemoryUpdate::CreateEntry { entry } => {
            Operation::CreateEntry(encode_entry(entry))
        }
        UserWorkspaceMemoryUpdate::UpsertEntry { entry } => {
            Operation::UpsertEntry(encode_entry(entry))
        }
        UserWorkspaceMemoryUpdate::ReplaceEntry { entry } => {
            Operation::ReplaceEntry(encode_entry(entry))
        }
        UserWorkspaceMemoryUpdate::RemoveEntry { key } => Operation::RemoveEntry(key),
        UserWorkspaceMemoryUpdate::UpdateEntry { key, update } => {
            Operation::UpdateEntry(pb::UpdateMemoryEntry {
                key,
                update: Some(pb::WorkspaceMemoryEntryUpdate {
                    section: update.section.map(encode_section),
                    title: update.title,
                    kind: update.kind.map(encode_kind),
                    summary: update.summary,
                    why: encode_field(update.why),
                    apply: encode_field(update.apply),
                    status: encode_field(update.status),
                    next: encode_field(update.next),
                }),
            })
        }
        UserWorkspaceMemoryUpdate::ReplaceDocument {
            content,
            base_version,
        } => Operation::ReplaceDocument(pb::ReplaceMemoryDocument {
            content,
            base_version: Some(match base_version {
                ExpectedFileVersion::MustNotExist => {
                    pb::replace_memory_document::BaseVersion::MustNotExist(pb::Empty {})
                }
                ExpectedFileVersion::Match { version_token } => {
                    pb::replace_memory_document::BaseVersion::MatchVersion(version_token)
                }
            }),
        }),
    };
    pb::WorkspaceMemoryUpdate {
        operation: Some(operation),
    }
}
pub fn decode_update(value: pb::WorkspaceMemoryUpdate) -> TalosResult<UserWorkspaceMemoryUpdate> {
    use pb::workspace_memory_update::Operation;
    let update = match value.operation.ok_or_else(invalid)? {
        Operation::CreateEntry(value) => UserWorkspaceMemoryUpdate::CreateEntry {
            entry: decode_entry(value)?,
        },
        Operation::UpsertEntry(value) => UserWorkspaceMemoryUpdate::UpsertEntry {
            entry: decode_entry(value)?,
        },
        Operation::ReplaceEntry(value) => UserWorkspaceMemoryUpdate::ReplaceEntry {
            entry: decode_entry(value)?,
        },
        Operation::RemoveEntry(key) => UserWorkspaceMemoryUpdate::RemoveEntry { key },
        Operation::UpdateEntry(value) => {
            let update = value.update.ok_or_else(invalid)?;
            UserWorkspaceMemoryUpdate::UpdateEntry {
                key: value.key,
                update: UserWorkspaceMemoryEntryUpdate {
                    section: update.section.map(decode_section).transpose()?,
                    title: update.title,
                    kind: update.kind.map(decode_kind).transpose()?,
                    summary: update.summary,
                    why: decode_field(update.why)?,
                    apply: decode_field(update.apply)?,
                    status: decode_field(update.status)?,
                    next: decode_field(update.next)?,
                },
            }
        }
        Operation::ReplaceDocument(value) => UserWorkspaceMemoryUpdate::ReplaceDocument {
            content: value.content,
            base_version: match value.base_version.ok_or_else(invalid)? {
                pb::replace_memory_document::BaseVersion::MustNotExist(_) => {
                    ExpectedFileVersion::MustNotExist
                }
                pb::replace_memory_document::BaseVersion::MatchVersion(version_token) => {
                    ExpectedFileVersion::Match { version_token }
                }
            },
        },
    };
    update.validate().map_err(source)?;
    Ok(update)
}

pub(crate) fn encode_kind(value: MemoryKind) -> i32 {
    match value {
        MemoryKind::Preference => pb::MemoryKind::Preference as i32,
        MemoryKind::Fact => pb::MemoryKind::Fact as i32,
        MemoryKind::Decision => pb::MemoryKind::Decision as i32,
        MemoryKind::Task => pb::MemoryKind::Task as i32,
        MemoryKind::Episode => pb::MemoryKind::Episode as i32,
    }
}
pub(crate) fn decode_kind(value: i32) -> TalosResult<MemoryKind> {
    match pb::MemoryKind::try_from(value) {
        Ok(pb::MemoryKind::Preference) => Ok(MemoryKind::Preference),
        Ok(pb::MemoryKind::Fact) => Ok(MemoryKind::Fact),
        Ok(pb::MemoryKind::Decision) => Ok(MemoryKind::Decision),
        Ok(pb::MemoryKind::Task) => Ok(MemoryKind::Task),
        Ok(pb::MemoryKind::Episode) => Ok(MemoryKind::Episode),
        _ => Err(invalid()),
    }
}

fn encode_section(value: UserWorkspaceMemorySection) -> i32 {
    match value {
        UserWorkspaceMemorySection::InteractionGuidance => {
            pb::WorkspaceMemorySection::InteractionGuidance as i32
        }
        UserWorkspaceMemorySection::KeyKnowledge => pb::WorkspaceMemorySection::KeyKnowledge as i32,
        UserWorkspaceMemorySection::ActiveContext => {
            pb::WorkspaceMemorySection::ActiveContext as i32
        }
    }
}
fn decode_section(value: i32) -> TalosResult<UserWorkspaceMemorySection> {
    match pb::WorkspaceMemorySection::try_from(value) {
        Ok(pb::WorkspaceMemorySection::InteractionGuidance) => {
            Ok(UserWorkspaceMemorySection::InteractionGuidance)
        }
        Ok(pb::WorkspaceMemorySection::KeyKnowledge) => {
            Ok(UserWorkspaceMemorySection::KeyKnowledge)
        }
        Ok(pb::WorkspaceMemorySection::ActiveContext) => {
            Ok(UserWorkspaceMemorySection::ActiveContext)
        }
        _ => Err(invalid()),
    }
}
