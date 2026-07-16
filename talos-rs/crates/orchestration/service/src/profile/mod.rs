mod retention;
mod service;

pub use retention::ProfileUsageRetention;
pub use service::ProfileService;

#[cfg(test)]
#[path = "profile_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "retention_tests.rs"]
mod retention_tests;
