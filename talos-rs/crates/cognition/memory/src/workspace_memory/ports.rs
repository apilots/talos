use std::future::Future;
use std::pin::Pin;

use talos_error::TalosResult;
use talos_types::ExpectedFileVersion;

use crate::MemoryScope;

use super::{
    UserWorkspaceMemoryDocument, UserWorkspaceMemoryEntry, UserWorkspaceMemoryEntryQuery,
    UserWorkspaceMemoryEntryUpdate,
};

/// Consistent raw and parsed view of one User Workspace Memory file.
#[derive(Debug, Clone, PartialEq)]
pub enum UserWorkspaceMemorySnapshot {
    Missing {
        template: UserWorkspaceMemoryDocument,
    },
    Valid {
        raw_content: String,
        updated_at: Option<i64>,
        version_token: String,
        document: UserWorkspaceMemoryDocument,
    },
    Invalid {
        raw_content: String,
        updated_at: Option<i64>,
        version_token: String,
        diagnostic: String,
    },
}

impl UserWorkspaceMemorySnapshot {
    /// Returns whether the backing file exists.
    #[must_use]
    pub const fn exists(&self) -> bool {
        !matches!(self, Self::Missing { .. })
    }

    /// Returns the raw file content, or an empty string for a missing file.
    #[must_use]
    pub fn raw_content(&self) -> &str {
        match self {
            Self::Missing { .. } => "",
            Self::Valid { raw_content, .. } | Self::Invalid { raw_content, .. } => raw_content,
        }
    }

    /// Returns the file modification time when a file exists and metadata is available.
    #[must_use]
    pub const fn updated_at(&self) -> Option<i64> {
        match self {
            Self::Missing { .. } => None,
            Self::Valid { updated_at, .. } | Self::Invalid { updated_at, .. } => *updated_at,
        }
    }

    /// Returns the revision token for an existing file.
    #[must_use]
    pub fn version_token(&self) -> Option<&str> {
        match self {
            Self::Missing { .. } => None,
            Self::Valid { version_token, .. } | Self::Invalid { version_token, .. } => {
                Some(version_token)
            }
        }
    }

    /// Returns the usable document or missing-file template.
    #[must_use]
    pub const fn document(&self) -> Option<&UserWorkspaceMemoryDocument> {
        match self {
            Self::Missing { template } => Some(template),
            Self::Valid { document, .. } => Some(document),
            Self::Invalid { .. } => None,
        }
    }
}

/// Trusted, scoped read access to User Workspace Memory.
pub trait UserWorkspaceMemoryReader: Send + Sync {
    fn read<'a>(
        &'a self,
        scope: &'a MemoryScope,
    ) -> Pin<Box<dyn Future<Output = TalosResult<UserWorkspaceMemorySnapshot>> + Send + 'a>>;

    fn get_entry<'a>(
        &'a self,
        scope: &'a MemoryScope,
        key: &'a str,
    ) -> Pin<Box<dyn Future<Output = TalosResult<Option<UserWorkspaceMemoryEntry>>> + Send + 'a>>;

    fn list_entries<'a>(
        &'a self,
        scope: &'a MemoryScope,
        query: UserWorkspaceMemoryEntryQuery,
    ) -> Pin<Box<dyn Future<Output = TalosResult<Vec<UserWorkspaceMemoryEntry>>> + Send + 'a>>;
}

/// Trusted, scoped mutation access to User Workspace Memory.
pub trait UserWorkspaceMemoryWriter: Send + Sync {
    fn create_entry<'a>(
        &'a self,
        scope: &'a MemoryScope,
        entry: UserWorkspaceMemoryEntry,
    ) -> Pin<Box<dyn Future<Output = TalosResult<UserWorkspaceMemorySnapshot>> + Send + 'a>>;

    fn update_entry<'a>(
        &'a self,
        scope: &'a MemoryScope,
        key: &'a str,
        update: UserWorkspaceMemoryEntryUpdate,
    ) -> Pin<Box<dyn Future<Output = TalosResult<UserWorkspaceMemorySnapshot>> + Send + 'a>>;

    fn upsert_entry<'a>(
        &'a self,
        scope: &'a MemoryScope,
        entry: UserWorkspaceMemoryEntry,
    ) -> Pin<Box<dyn Future<Output = TalosResult<UserWorkspaceMemorySnapshot>> + Send + 'a>>;

    fn replace_entry<'a>(
        &'a self,
        scope: &'a MemoryScope,
        entry: UserWorkspaceMemoryEntry,
    ) -> Pin<Box<dyn Future<Output = TalosResult<UserWorkspaceMemorySnapshot>> + Send + 'a>>;

    fn remove_entry<'a>(
        &'a self,
        scope: &'a MemoryScope,
        key: &'a str,
    ) -> Pin<Box<dyn Future<Output = TalosResult<UserWorkspaceMemorySnapshot>> + Send + 'a>>;

    fn replace_document<'a>(
        &'a self,
        scope: &'a MemoryScope,
        content: String,
        base_version: ExpectedFileVersion,
    ) -> Pin<Box<dyn Future<Output = TalosResult<UserWorkspaceMemorySnapshot>> + Send + 'a>>;
}
