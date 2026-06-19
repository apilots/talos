mod installer;
mod manager;
mod scanner;
mod source;
mod state;

pub use manager::PluginManager;
pub use source::source_id_for;
pub use state::{MarketplaceSourcesFile, PluginStateStore};

#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_git;
