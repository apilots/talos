use std::path::{Path, PathBuf};

use chrono::Utc;
use talos_error::{TalosError, TalosResult};
use talos_knowledge_format::{KnowledgeFormatError, KnowledgeFormatErrorKind};
use talos_memory::{
    MemoryAgentId, MemoryScope, UserWorkspaceMemoryDocument, UserWorkspaceMemoryEntry,
    UserWorkspaceMemoryEntryQuery, UserWorkspaceMemoryEntryUpdate, UserWorkspaceMemoryProducer,
    UserWorkspaceMemoryReader, UserWorkspaceMemorySnapshot, UserWorkspaceMemoryWriter,
};
use talos_types::{DocUpdateRequest, ExpectedFileVersion};

use crate::workspace::file::{ensure_workspace_write_version, workspace_revision};

/// Shared filesystem boundary for every User Workspace Memory caller.
#[derive(Debug, Clone, Default)]
pub struct UserWorkspaceMemoryService;

impl UserWorkspaceMemoryService {
    /// Creates the stateless shared service.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }

    /// Reads raw and parsed state without creating a missing file.
    pub async fn read(&self, scope: &MemoryScope) -> TalosResult<UserWorkspaceMemorySnapshot> {
        snapshot_from_scope(scope).await
    }

    /// Reads one typed entry.
    pub async fn get_entry(
        &self,
        scope: &MemoryScope,
        key: &str,
    ) -> TalosResult<Option<UserWorkspaceMemoryEntry>> {
        let snapshot = self.read(scope).await?;
        valid_document(snapshot).map(|document| document.get_entry(key))
    }

    /// Lists typed entries.
    pub async fn list_entries(
        &self,
        scope: &MemoryScope,
        query: UserWorkspaceMemoryEntryQuery,
    ) -> TalosResult<Vec<UserWorkspaceMemoryEntry>> {
        let snapshot = self.read(scope).await?;
        valid_document(snapshot)?
            .list_entries(query)
            .map_err(map_format_error)
    }

    /// Creates one entry.
    pub async fn create_entry(
        &self,
        scope: &MemoryScope,
        entry: UserWorkspaceMemoryEntry,
    ) -> TalosResult<UserWorkspaceMemorySnapshot> {
        self.mutate(scope, move |document| document.create_entry(entry))
            .await
    }

    /// Updates fields of one entry.
    pub async fn update_entry(
        &self,
        scope: &MemoryScope,
        key: &str,
        update: UserWorkspaceMemoryEntryUpdate,
    ) -> TalosResult<UserWorkspaceMemorySnapshot> {
        let key = key.to_string();
        self.mutate(scope, move |document| document.update_entry(&key, update))
            .await
    }

    /// Creates or replaces one entry.
    pub async fn upsert_entry(
        &self,
        scope: &MemoryScope,
        entry: UserWorkspaceMemoryEntry,
    ) -> TalosResult<UserWorkspaceMemorySnapshot> {
        self.mutate(scope, move |document| document.upsert_entry(entry))
            .await
    }

    /// Replaces one existing entry.
    pub async fn replace_entry(
        &self,
        scope: &MemoryScope,
        entry: UserWorkspaceMemoryEntry,
    ) -> TalosResult<UserWorkspaceMemorySnapshot> {
        self.mutate(scope, move |document| document.replace_entry(entry))
            .await
    }

    /// Idempotently removes one entry.
    pub async fn remove_entry(
        &self,
        scope: &MemoryScope,
        key: &str,
    ) -> TalosResult<UserWorkspaceMemorySnapshot> {
        let key = key.to_string();
        self.mutate(scope, move |document| document.remove_entry(&key))
            .await
    }

    /// Replaces the complete file after profile validation and revision CAS.
    pub async fn replace_document(
        &self,
        scope: &MemoryScope,
        content: String,
        base_version: ExpectedFileVersion,
    ) -> TalosResult<UserWorkspaceMemorySnapshot> {
        let path = memory_path(scope);
        talos_fs::with_file_lock(&path, || async {
            let existing = talos_fs::read_file(&path).await?;
            ensure_workspace_write_version(
                existing.as_deref(),
                &DocUpdateRequest {
                    content: content.clone(),
                    base_version,
                },
            )?;
            let document =
                UserWorkspaceMemoryDocument::parse(&content).map_err(map_format_error)?;
            let rendered = document.render().map_err(map_format_error)?;
            talos_fs::write_file(&path, &rendered).await?;
            valid_snapshot(&path, rendered, document).await
        })
        .await
    }

    async fn mutate(
        &self,
        scope: &MemoryScope,
        operation: impl FnOnce(
            UserWorkspaceMemoryDocument,
        ) -> Result<UserWorkspaceMemoryDocument, KnowledgeFormatError>,
    ) -> TalosResult<UserWorkspaceMemorySnapshot> {
        let path = memory_path(scope);
        talos_fs::with_file_lock(&path, || async {
            let existing = talos_fs::read_file(&path).await?;
            let document = match existing.as_deref() {
                None => UserWorkspaceMemoryDocument::template(producer(scope)?)
                    .map_err(map_format_error)?,
                Some(content) => {
                    UserWorkspaceMemoryDocument::parse(content).map_err(map_format_error)?
                }
            };
            let document = operation(document).map_err(map_format_error)?;
            let rendered = document.render().map_err(map_format_error)?;
            talos_fs::write_file(&path, &rendered).await?;
            valid_snapshot(&path, rendered, document).await
        })
        .await
    }
}

impl UserWorkspaceMemoryReader for UserWorkspaceMemoryService {
    fn read<'a>(
        &'a self,
        scope: &'a MemoryScope,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = TalosResult<UserWorkspaceMemorySnapshot>> + Send + 'a>,
    > {
        Box::pin(Self::read(self, scope))
    }

    fn get_entry<'a>(
        &'a self,
        scope: &'a MemoryScope,
        key: &'a str,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<Output = TalosResult<Option<UserWorkspaceMemoryEntry>>>
                + Send
                + 'a,
        >,
    > {
        Box::pin(Self::get_entry(self, scope, key))
    }

    fn list_entries<'a>(
        &'a self,
        scope: &'a MemoryScope,
        query: UserWorkspaceMemoryEntryQuery,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<Output = TalosResult<Vec<UserWorkspaceMemoryEntry>>>
                + Send
                + 'a,
        >,
    > {
        Box::pin(Self::list_entries(self, scope, query))
    }
}

impl UserWorkspaceMemoryWriter for UserWorkspaceMemoryService {
    fn create_entry<'a>(
        &'a self,
        scope: &'a MemoryScope,
        entry: UserWorkspaceMemoryEntry,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = TalosResult<UserWorkspaceMemorySnapshot>> + Send + 'a>,
    > {
        Box::pin(Self::create_entry(self, scope, entry))
    }

    fn update_entry<'a>(
        &'a self,
        scope: &'a MemoryScope,
        key: &'a str,
        update: UserWorkspaceMemoryEntryUpdate,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = TalosResult<UserWorkspaceMemorySnapshot>> + Send + 'a>,
    > {
        Box::pin(Self::update_entry(self, scope, key, update))
    }

    fn upsert_entry<'a>(
        &'a self,
        scope: &'a MemoryScope,
        entry: UserWorkspaceMemoryEntry,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = TalosResult<UserWorkspaceMemorySnapshot>> + Send + 'a>,
    > {
        Box::pin(Self::upsert_entry(self, scope, entry))
    }

    fn replace_entry<'a>(
        &'a self,
        scope: &'a MemoryScope,
        entry: UserWorkspaceMemoryEntry,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = TalosResult<UserWorkspaceMemorySnapshot>> + Send + 'a>,
    > {
        Box::pin(Self::replace_entry(self, scope, entry))
    }

    fn remove_entry<'a>(
        &'a self,
        scope: &'a MemoryScope,
        key: &'a str,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = TalosResult<UserWorkspaceMemorySnapshot>> + Send + 'a>,
    > {
        Box::pin(Self::remove_entry(self, scope, key))
    }

    fn replace_document<'a>(
        &'a self,
        scope: &'a MemoryScope,
        content: String,
        base_version: ExpectedFileVersion,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = TalosResult<UserWorkspaceMemorySnapshot>> + Send + 'a>,
    > {
        Box::pin(Self::replace_document(self, scope, content, base_version))
    }
}

fn memory_path(scope: &MemoryScope) -> PathBuf {
    scope.workspace_root.join("MEMORY.md")
}

async fn snapshot_from_scope(scope: &MemoryScope) -> TalosResult<UserWorkspaceMemorySnapshot> {
    let path = memory_path(scope);
    let Some(content) = talos_fs::read_file(&path).await? else {
        return Ok(UserWorkspaceMemorySnapshot::Missing {
            template: UserWorkspaceMemoryDocument::template(producer(scope)?)
                .map_err(map_format_error)?,
        });
    };
    let updated_at = talos_fs::get_file_metadata(&path)
        .await?
        .and_then(|metadata| metadata.modified_at_ms);
    match UserWorkspaceMemoryDocument::parse(&content) {
        Ok(document) => Ok(UserWorkspaceMemorySnapshot::Valid {
            version_token: workspace_revision(&content),
            raw_content: content,
            updated_at,
            document,
        }),
        Err(error) => Ok(UserWorkspaceMemorySnapshot::Invalid {
            diagnostic: error.to_string(),
            version_token: workspace_revision(&content),
            raw_content: content,
            updated_at,
        }),
    }
}

fn producer(scope: &MemoryScope) -> TalosResult<UserWorkspaceMemoryProducer> {
    Ok(UserWorkspaceMemoryProducer::new(
        MemoryAgentId::parse(scope.workspace_id.clone()).map_err(|error| {
            TalosError::public(talos_error::ErrorCode::InvalidRequest, error.to_string())
        })?,
        Utc::now().timestamp(),
    ))
}

async fn valid_snapshot(
    path: &Path,
    raw_content: String,
    document: UserWorkspaceMemoryDocument,
) -> TalosResult<UserWorkspaceMemorySnapshot> {
    let updated_at = talos_fs::get_file_metadata(path)
        .await?
        .and_then(|metadata| metadata.modified_at_ms);
    Ok(UserWorkspaceMemorySnapshot::Valid {
        version_token: workspace_revision(&raw_content),
        raw_content,
        updated_at,
        document,
    })
}

fn valid_document(
    snapshot: UserWorkspaceMemorySnapshot,
) -> TalosResult<UserWorkspaceMemoryDocument> {
    match snapshot {
        UserWorkspaceMemorySnapshot::Missing { template } => Ok(template),
        UserWorkspaceMemorySnapshot::Valid { document, .. } => Ok(document),
        UserWorkspaceMemorySnapshot::Invalid { diagnostic, .. } => Err(TalosError::public(
            talos_error::ErrorCode::InvalidRequest,
            diagnostic,
        )),
    }
}

fn map_format_error(error: KnowledgeFormatError) -> TalosError {
    let code = match error.kind() {
        KnowledgeFormatErrorKind::AlreadyExists => talos_error::ErrorCode::AlreadyExists,
        KnowledgeFormatErrorKind::NotFound => talos_error::ErrorCode::ResourceNotFound,
        KnowledgeFormatErrorKind::InvalidOkf
        | KnowledgeFormatErrorKind::InvalidProfile
        | KnowledgeFormatErrorKind::InvalidDocument
        | KnowledgeFormatErrorKind::InvalidEntry
        | KnowledgeFormatErrorKind::LimitExceeded => talos_error::ErrorCode::InvalidRequest,
    };
    TalosError::public(code, error.to_string())
}
