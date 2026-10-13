use serde::{Deserialize, Serialize};

/// Management data synchronization. Pure Worker hosts do not consume this.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SyncConfig {
    pub enabled: Option<bool>,
    pub hub_endpoint: String,
    pub trusted_ca_file: Option<std::path::PathBuf>,
}

impl Default for SyncConfig {
    fn default() -> Self {
        Self {
            enabled: None,
            hub_endpoint: "wss://sync.talos.love/v1/sync".into(),
            trusted_ca_file: None,
        }
    }
}

impl SyncConfig {
    pub(crate) fn validate(&self) -> talos_error::TalosResult<()> {
        let endpoint = url::Url::parse(&self.hub_endpoint).map_err(|_| {
            talos_error::TalosError::new(
                talos_error::ErrorCode::InvalidRequest,
                "sync.hub_endpoint must be a valid WSS URL.",
            )
        })?;
        if endpoint.scheme() != "wss"
            || endpoint.host_str().is_none()
            || !endpoint.username().is_empty()
            || endpoint.password().is_some()
            || endpoint.query().is_some()
            || endpoint.fragment().is_some()
        {
            return Err(talos_error::TalosError::new(
                talos_error::ErrorCode::InvalidRequest,
                "Synchronization requires a WSS endpoint without credentials, query or fragment.",
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "sync_tests.rs"]
mod tests;
