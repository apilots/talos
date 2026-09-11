use serde::{Deserialize, Serialize};
use std::{
    io::Read,
    path::{Path, PathBuf},
};
use talos_error::{ErrorCode, TalosError, TalosResult};
use talos_fs::{FilePermissions, FileSystem, WriteOptions};
use talos_types::{
    McpCredentialBinding, McpCredentialValues, McpDefinitionRef, McpServerDefinition,
    McpTransportDefinition,
};

mod oauth;

const MAX_CREDENTIAL_BYTES: u64 = 256 * 1024;

/// Instance-private credential bundles. Definitions hold references, never these records.
#[derive(Clone)]
pub struct McpCredentialStore {
    root: PathBuf,
}

#[derive(Serialize, Deserialize)]
struct CredentialRecord {
    definition: McpDefinitionRef,
    target: McpTransportDefinition,
    authentication: talos_types::McpAuthentication,
    values: CredentialValues,
}
#[derive(Serialize, Deserialize)]
enum CredentialValues {
    Token(McpCredentialValues),
    OAuth(Box<talos_mcp::OAuthCredentials>),
}

impl McpCredentialStore {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    pub(super) fn create(
        &self,
        definition: &McpDefinitionRef,
        server: &McpServerDefinition,
        values: McpCredentialValues,
    ) -> TalosResult<McpCredentialBinding> {
        super::credential_validation::validate(server, &values)?;
        self.create_record(
            definition,
            server,
            values.authentication.clone(),
            CredentialValues::Token(values),
        )
    }

    fn create_record(
        &self,
        definition: &McpDefinitionRef,
        server: &McpServerDefinition,
        authentication: talos_types::McpAuthentication,
        values: CredentialValues,
    ) -> TalosResult<McpCredentialBinding> {
        let binding = McpCredentialBinding {
            reference: uuid::Uuid::new_v4().to_string(),
            authentication: authentication.clone(),
        };
        let record = CredentialRecord {
            definition: definition.clone(),
            target: server.transport.clone(),
            authentication,
            values,
        };
        let bytes = serde_json::to_vec(&record).map_err(|error| {
            TalosError::internal_with_source("Failed to encode MCP credentials.", error)
        })?;
        if bytes.len() as u64 > MAX_CREDENTIAL_BYTES {
            return Err(TalosError::new(
                ErrorCode::InvalidRequest,
                "MCP credentials exceed 256 KiB.",
            ));
        }
        talos_fs::create_directory(&self.root, true)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&self.root, std::fs::Permissions::from_mode(0o700)).map_err(
                |error| {
                    TalosError::internal_with_source(
                        "Failed to protect the MCP credential directory.",
                        error,
                    )
                },
            )?;
        }
        let _lock = talos_fs::std_fs().with_file_lock(&self.root.join("credentials.lock"))?;
        talos_fs::write_bytes_with(
            &self.path(&binding.reference)?,
            &bytes,
            WriteOptions::create_new().with_permissions(FilePermissions::unix(0o600)),
        )?;
        Ok(binding)
    }

    /// Resolves only a bundle bound to this exact definition and execution target.
    pub fn resolve(
        &self,
        definition: &McpDefinitionRef,
        server: &McpServerDefinition,
    ) -> TalosResult<Option<talos_mcp::McpConnectionCredentials>> {
        let Some(record) = self.read_bound(definition, server)? else {
            return Ok(None);
        };
        match record.values {
            CredentialValues::Token(values) => {
                super::credential_validation::validate(server, &values)?;
                Ok(Some(talos_mcp::McpConnectionCredentials::Token(values)))
            }
            CredentialValues::OAuth(_) => Ok(Some(talos_mcp::McpConnectionCredentials::OAuth(
                oauth::source(self, definition, server),
            ))),
        }
    }

    fn read_bound(
        &self,
        definition: &McpDefinitionRef,
        server: &McpServerDefinition,
    ) -> TalosResult<Option<CredentialRecord>> {
        let Some(binding) = &server.credentials else {
            return Ok(None);
        };
        let Some(record) = read_record(&self.path(&binding.reference)?)? else {
            return Ok(None);
        };
        if record.definition != *definition
            || record.target != server.transport
            || record.authentication != binding.authentication
        {
            return Err(TalosError::new(ErrorCode::PermissionDenied, "MCP credentials are not bound to this definition and target. Configure new credentials."));
        }
        Ok(Some(record))
    }

    pub(super) fn authentication_state(
        &self,
        owner: &McpDefinitionRef,
        server: &McpServerDefinition,
    ) -> TalosResult<talos_types::McpAuthenticationState> {
        use talos_types::McpAuthenticationState;
        match self.read_bound(owner, server)?.map(|record| record.values) {
            None => Ok(McpAuthenticationState::Missing),
            Some(CredentialValues::Token(values)) => {
                super::credential_validation::validate(server, &values)?;
                Ok(McpAuthenticationState::CredentialsStored)
            }
            Some(CredentialValues::OAuth(values)) => Ok(values.state()),
        }
    }

    pub(super) fn remove_for(&self, owner: &McpDefinitionRef, reference: &str) -> TalosResult<()> {
        let path = self.path(reference)?;
        if !self.root.exists() {
            return Ok(());
        }
        let _lock = talos_fs::std_fs().with_file_lock(&self.root.join("credentials.lock"))?;
        if read_record(&path)?.is_some_and(|record| record.definition != *owner) {
            return Err(TalosError::new(
                ErrorCode::PermissionDenied,
                "The MCP credential belongs to another definition.",
            ));
        }
        remove(&path)
    }

    fn remove(&self, reference: &str) -> TalosResult<()> {
        let path = self.path(reference)?;
        if !self.root.exists() {
            return Ok(());
        }
        let _lock = talos_fs::std_fs().with_file_lock(&self.root.join("credentials.lock"))?;
        remove(&path)
    }

    /// Recovery checks each record's owner against committed definitions, including disabled installs.
    /// An unreadable definition is retained; it is never guessed to be an orphan.
    pub(super) fn recover(
        &self,
        referenced: impl Fn(&McpDefinitionRef, &str) -> TalosResult<bool>,
    ) -> TalosResult<()> {
        if !self.root.exists() {
            return Ok(());
        }
        let entries = std::fs::read_dir(&self.root).map_err(|error| {
            TalosError::internal_with_source("Failed to read the MCP credential directory.", error)
        })?;
        for entry in entries {
            let entry = entry.map_err(|error| {
                TalosError::internal_with_source("Failed to read an MCP credential entry.", error)
            })?;
            let path = entry.path();
            if path.extension().is_none_or(|extension| extension != "json") {
                continue;
            }
            let Some(reference) = path.file_stem().and_then(|stem| stem.to_str()) else {
                continue;
            };
            let result = (|| {
                let Some(record) = read_record(&self.path(reference)?)? else {
                    return Ok(());
                };
                if !referenced(&record.definition, reference)? {
                    self.remove(reference)?;
                }
                Ok(())
            })();
            if let Err(error) = result {
                talos_logging::report_error(&error);
            }
        }
        Ok(())
    }

    fn path(&self, reference: &str) -> TalosResult<PathBuf> {
        let id = uuid::Uuid::parse_str(reference).map_err(|error| {
            TalosError::new(
                ErrorCode::InvalidRequest,
                "The MCP credential reference is invalid.",
            )
            .with_source(error)
        })?;
        Ok(self.root.join(format!("{id}.json")))
    }
}

fn read_record(path: &Path) -> TalosResult<Option<CredentialRecord>> {
    let file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(TalosError::internal_with_source(
                "Failed to read MCP credentials.",
                error,
            ))
        }
    };
    let mut bytes = Vec::new();
    file.take(MAX_CREDENTIAL_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| {
            TalosError::internal_with_source("Failed to read MCP credentials.", error)
        })?;
    if bytes.len() as u64 > MAX_CREDENTIAL_BYTES {
        return Err(TalosError::new(
            ErrorCode::InvalidRequest,
            "The stored MCP credentials exceed their size limit.",
        ));
    }
    serde_json::from_slice(&bytes).map(Some).map_err(|error| {
        TalosError::internal_with_source("The stored MCP credentials are invalid.", error)
    })
}

fn remove(path: &Path) -> TalosResult<()> {
    match std::fs::remove_file(path) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => {
            return Err(TalosError::internal_with_source(
                "Failed to remove MCP credentials.",
                error,
            ))
        }
    }
    #[cfg(unix)]
    if let Some(parent) = path.parent() {
        std::fs::File::open(parent)
            .and_then(|file| file.sync_all())
            .map_err(|error| {
                TalosError::internal_with_source(
                    "MCP credential removal durability could not be confirmed.",
                    error,
                )
            })?;
    }
    Ok(())
}
