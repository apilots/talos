use std::path::Path;
use talos_types::BrowserHostKind;

/// Packaged resources extend both embedded and registered hosts. Desktop keeps
/// its native backend; the Browser owner validates the selected package.
pub(crate) fn select_host(
    fallback: BrowserHostKind,
    resource_dir: Option<&Path>,
) -> BrowserHostKind {
    match fallback {
        BrowserHostKind::Desktop | BrowserHostKind::Docker => fallback,
        BrowserHostKind::Native | BrowserHostKind::Server => {
            if resource_dir.is_some_and(|root| root.join("browser/runtime.json").is_file()) {
                BrowserHostKind::Docker
            } else {
                fallback
            }
        }
    }
}

#[cfg(test)]
#[path = "browser_host_tests.rs"]
mod tests;
