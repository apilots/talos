use super::*;
use futures_util::future::BoxFuture;
use std::sync::Arc;
use talos_mcp::{OAuthCredentialSource, OAuthCredentialStore, OAuthCredentials};

impl McpCredentialStore {
    pub(in crate::plugin) fn create_oauth(
        &self,
        owner: &McpDefinitionRef,
        server: &McpServerDefinition,
        config: talos_types::McpOAuthConfig,
        values: OAuthCredentials,
    ) -> TalosResult<McpCredentialBinding> {
        self.create_record(
            owner,
            server,
            talos_types::McpAuthentication::OAuth {
                config: Box::new(config),
            },
            CredentialValues::OAuth(Box::new(values)),
        )
    }
}
pub(super) fn source(
    store: &McpCredentialStore,
    owner: &McpDefinitionRef,
    server: &McpServerDefinition,
) -> OAuthCredentialSource {
    OAuthCredentialSource {
        reference: server
            .credentials
            .as_ref()
            .expect("resolved credential binding")
            .reference
            .clone(),
        store: Arc::new(BoundStore {
            store: store.clone(),
            owner: owner.clone(),
            server: server.clone(),
        }),
    }
}
struct BoundStore {
    store: McpCredentialStore,
    owner: McpDefinitionRef,
    server: McpServerDefinition,
}
impl BoundStore {
    fn reference(&self) -> &str {
        &self
            .server
            .credentials
            .as_ref()
            .expect("resolved credential binding")
            .reference
    }
}
impl OAuthCredentialStore for BoundStore {
    fn load(&self) -> BoxFuture<'_, TalosResult<Option<OAuthCredentials>>> {
        Box::pin(async move {
            match self
                .store
                .read_bound(&self.owner, &self.server)?
                .map(|record| record.values)
            {
                None => Ok(None),
                Some(CredentialValues::OAuth(values)) => Ok(Some(*values)),
                Some(_) => Err(TalosError::internal(
                    "The MCP credential record is not OAuth.",
                )),
            }
        })
    }
    fn save(&self, credentials: OAuthCredentials) -> BoxFuture<'_, TalosResult<()>> {
        Box::pin(async move {
            let _lock =
                talos_fs::std_fs().with_file_lock(&self.store.root.join("credentials.lock"))?;
            let mut record = self
                .store
                .read_bound(&self.owner, &self.server)?
                .ok_or_else(|| {
                    TalosError::new(
                        ErrorCode::Unauthenticated,
                        "MCP credentials were removed during refresh.",
                    )
                })?;
            record.values = CredentialValues::OAuth(Box::new(credentials));
            let bytes = serde_json::to_vec(&record).map_err(|error| {
                TalosError::internal_with_source("Failed to encode MCP credentials.", error)
            })?;
            if bytes.len() as u64 > MAX_CREDENTIAL_BYTES {
                return Err(TalosError::new(
                    ErrorCode::InvalidRequest,
                    "MCP credentials exceed 256 KiB.",
                ));
            }
            talos_fs::write_bytes_with(
                &self.store.path(self.reference())?,
                &bytes,
                WriteOptions::default().with_permissions(FilePermissions::unix(0o600)),
            )
        })
    }
    fn clear(&self) -> BoxFuture<'_, TalosResult<()>> {
        Box::pin(async move { self.store.remove_for(&self.owner, self.reference()) })
    }
    fn refresh_guard(&self) -> BoxFuture<'_, TalosResult<Box<dyn Send>>> {
        Box::pin(async move {
            let path = self
                .store
                .path(self.reference())?
                .with_extension("refresh.lock");
            loop {
                if let Some(lock) = talos_fs::std_fs().try_file_lock(&path)? {
                    return Ok(Box::new(lock) as Box<dyn Send>);
                }
                tokio::time::sleep(std::time::Duration::from_millis(25)).await;
            }
        })
    }
}
