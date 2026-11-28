//! Resolve the existing packaged WebUI without changing startup policy.
#[cfg(feature = "server")]
use super::TALOS_WEBUI_DIR_ENV;
use std::path::PathBuf;
#[cfg(feature = "server")]
use tracing::{info, warn};

#[cfg(feature = "server")]
pub(super) fn resolve_packaged_webui_dir() -> Option<PathBuf> {
    let resolution = resolve_webui_dir(WebuiLookupInput {
        env_dir: std::env::var_os(TALOS_WEBUI_DIR_ENV).map(PathBuf::from),
        current_exe: std::env::current_exe().ok(),
        current_dir: std::env::current_dir().ok(),
    });
    match resolution {
        WebuiLookupResult::Found(path) => {
            info!("Serving Talos WebUI from {}", path.display());
            Some(path)
        }
        WebuiLookupResult::InvalidEnv(path) => {
            let index_path = path.join("index.html");
            warn!(
                "{} is set but {} was not found; starting API-only server.",
                TALOS_WEBUI_DIR_ENV,
                index_path.display()
            );
            None
        }
        WebuiLookupResult::Missing => {
            warn!(
                "Talos WebUI bundle not found; starting API-only server. Set {} to serve a WebUI bundle.",
                TALOS_WEBUI_DIR_ENV
            );
            None
        }
    }
}

#[cfg(any(feature = "server", test))]
#[derive(Debug)]
pub(super) struct WebuiLookupInput {
    pub(super) env_dir: Option<PathBuf>,
    pub(super) current_exe: Option<PathBuf>,
    pub(super) current_dir: Option<PathBuf>,
}

#[cfg(any(feature = "server", test))]
#[derive(Debug, PartialEq, Eq)]
pub(super) enum WebuiLookupResult {
    Found(PathBuf),
    InvalidEnv(PathBuf),
    Missing,
}

#[cfg(any(feature = "server", test))]
pub(super) fn resolve_webui_dir(input: WebuiLookupInput) -> WebuiLookupResult {
    if let Some(path) = input.env_dir {
        return if is_webui_dir(&path) {
            WebuiLookupResult::Found(canonical_or_original(path))
        } else {
            WebuiLookupResult::InvalidEnv(path)
        };
    }

    for candidate in webui_dir_candidates(&input) {
        if is_webui_dir(&candidate) {
            return WebuiLookupResult::Found(canonical_or_original(candidate));
        }
    }

    WebuiLookupResult::Missing
}

#[cfg(any(feature = "server", test))]
fn webui_dir_candidates(input: &WebuiLookupInput) -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(exe_dir) = input
        .current_exe
        .as_ref()
        .and_then(|path| path.parent().map(PathBuf::from))
    {
        candidates.push(exe_dir.join("webui"));
        candidates.push(exe_dir.join("..").join("webui"));
    }
    if let Some(current_dir) = input.current_dir.as_ref() {
        candidates.push(current_dir.join("webui"));
    }
    candidates
}

#[cfg(any(feature = "server", test))]
fn is_webui_dir(path: &std::path::Path) -> bool {
    path.join("index.html").is_file()
}

pub(super) fn canonical_or_original(path: PathBuf) -> PathBuf {
    path.canonicalize().unwrap_or(path)
}
