use chrono::Utc;
use sha2::{Digest, Sha256};
use std::fs;
use std::path::Path;
use talos_config::{
    PluginSourceConfig, PluginSourceKind as ConfigPluginSourceKind,
    PluginSourceType as ConfigPluginSourceType,
};
use talos_error::{TalosError, TalosResult};
use talos_types::{
    MarketplaceSource, MarketplaceSourceId, MarketplaceSourceInput, MarketplaceSourceKind,
    MarketplaceSourceLocation, MarketplaceSourceType, PluginId, PluginKind,
};

pub(crate) fn config_source(source: &PluginSourceConfig) -> Option<MarketplaceSource> {
    let ConfigPluginSourceKind::Git = source.kind;
    Some(MarketplaceSource {
        id: source_id_for(&source.url, &source.reference, &source.sparse_paths),
        source_type: match source.source_type {
            ConfigPluginSourceType::Official => MarketplaceSourceType::Official,
            ConfigPluginSourceType::User => MarketplaceSourceType::User,
        },
        kind: MarketplaceSourceKind::Git,
        display_name: source.display_name.clone(),
        location: MarketplaceSourceLocation::Git {
            url: source.url.clone(),
            reference: source.reference.clone(),
        },
        sparse_paths: source.sparse_paths.clone(),
        last_checked_commit: None,
        last_checked_at_ms: None,
        created_at_ms: 0,
        updated_at_ms: 0,
    })
}

pub(crate) fn source_from_input(input: MarketplaceSourceInput) -> TalosResult<MarketplaceSource> {
    let now_ms = Utc::now().timestamp_millis();
    match input {
        MarketplaceSourceInput::Git {
            url,
            reference,
            sparse_paths,
        } => {
            validate_git_url(&url)?;
            validate_reference(&reference)?;
            validate_sparse_paths(&sparse_paths)?;
            Ok(MarketplaceSource {
                id: source_id_for(&url, &reference, &sparse_paths),
                source_type: MarketplaceSourceType::User,
                kind: MarketplaceSourceKind::Git,
                display_name: display_name_for_git_url(&url),
                location: MarketplaceSourceLocation::Git { url, reference },
                sparse_paths,
                last_checked_commit: None,
                last_checked_at_ms: None,
                created_at_ms: now_ms,
                updated_at_ms: now_ms,
            })
        }
        MarketplaceSourceInput::LocalFolder { path, sparse_paths } => {
            validate_sparse_paths(&sparse_paths)?;
            let path = fs::canonicalize(&path).map_err(|err| {
                TalosError::public(
                    talos_error::ErrorCode::InvalidRequest,
                    format!("invalid local plugin source `{}`: {err}", path.display()),
                )
            })?;
            if !path.is_dir() {
                return Err(TalosError::public(
                    talos_error::ErrorCode::InvalidRequest,
                    format!(
                        "local plugin source `{}` is not a directory",
                        path.display()
                    ),
                ));
            }
            let path_key = path.display().to_string();
            Ok(MarketplaceSource {
                id: source_id_for(&path_key, "local", &sparse_paths),
                source_type: MarketplaceSourceType::User,
                kind: MarketplaceSourceKind::LocalFolder,
                display_name: path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or("Local Plugin Source")
                    .to_string(),
                location: MarketplaceSourceLocation::LocalFolder { path },
                sparse_paths,
                last_checked_commit: None,
                last_checked_at_ms: None,
                created_at_ms: now_ms,
                updated_at_ms: now_ms,
            })
        }
    }
}

pub fn source_id_for(url: &str, reference: &str, sparse_paths: &[String]) -> MarketplaceSourceId {
    let mut hasher = Sha256::new();
    hasher.update(url.trim().as_bytes());
    hasher.update(b"\0");
    hasher.update(reference.trim().as_bytes());
    for path in sparse_paths {
        hasher.update(b"\0");
        hasher.update(path.trim().as_bytes());
    }
    let digest = hasher.finalize();
    MarketplaceSourceId(format!("source_{}", short_hex(&digest[..12])))
}

pub(crate) fn plugin_id_for(
    source_id: &MarketplaceSourceId,
    kind: PluginKind,
    path_in_source: &str,
) -> PluginId {
    let mut hasher = Sha256::new();
    hasher.update(source_id.0.as_bytes());
    hasher.update(b"\0");
    hasher.update(format!("{kind:?}").as_bytes());
    hasher.update(b"\0");
    hasher.update(path_in_source.as_bytes());
    let digest = hasher.finalize();
    PluginId(format!("plugin_{}", short_hex(&digest[..12])))
}

fn short_hex(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push_str(&format!("{byte:02x}"));
    }
    output
}

fn validate_git_url(url: &str) -> TalosResult<()> {
    let trimmed = url.trim();
    if trimmed.starts_with("https://")
        || trimmed.starts_with("git@")
        || trimmed.starts_with("file://")
    {
        return Ok(());
    }
    Err(TalosError::public(
        talos_error::ErrorCode::InvalidRequest,
        format!("unsupported plugin source git URL `{url}`"),
    ))
}

fn validate_reference(reference: &str) -> TalosResult<()> {
    if reference.is_empty()
        || reference.starts_with('-')
        || reference.contains("..")
        || reference.contains('\\')
        || reference.chars().any(char::is_whitespace)
    {
        return Err(TalosError::public(
            talos_error::ErrorCode::InvalidRequest,
            format!("invalid plugin source git ref `{reference}`"),
        ));
    }
    Ok(())
}

fn validate_sparse_paths(paths: &[String]) -> TalosResult<()> {
    for path in paths {
        let candidate = Path::new(path);
        if path.is_empty()
            || candidate.is_absolute()
            || path.contains("..")
            || path.contains('\\')
            || path.split('/').any(|segment| segment.is_empty())
        {
            return Err(TalosError::public(
                talos_error::ErrorCode::InvalidRequest,
                format!("invalid plugin source sparse path `{path}`"),
            ));
        }
    }
    Ok(())
}

fn display_name_for_git_url(url: &str) -> String {
    url.trim_end_matches(".git")
        .rsplit(['/', ':'])
        .next()
        .filter(|name| !name.is_empty())
        .unwrap_or(url)
        .to_string()
}
