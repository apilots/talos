//! Talos LLM provider abstraction.

pub(crate) mod api;
pub mod protocols;
mod router;

pub use api::*;
pub use router::*;

#[cfg(test)]
#[path = "provider_tests.rs"]
mod tests;
