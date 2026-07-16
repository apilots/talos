mod service;

pub use service::ProfileService;

#[cfg(test)]
#[path = "profile_tests.rs"]
mod tests;
