mod documents;
mod installer;
mod manager;
mod scanner;
mod source;
mod state;

pub use manager::{McpChangeHook, McpDiscovery, PluginManager};
pub use source::source_id_for;
pub use state::{MarketplaceSourcesFile, PluginStateStore};

#[cfg(test)]
mod test_support;
#[cfg(test)]
#[path = "plugin_tests.rs"]
mod tests;
#[cfg(test)]
#[path = "git_tests.rs"]
mod tests_git;
#[cfg(test)]
#[path = "lifecycle_tests.rs"]
mod tests_lifecycle;

#[cfg(test)]
mod manual_tests;
