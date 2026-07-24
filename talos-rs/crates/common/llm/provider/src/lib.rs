//! Talos LLM provider abstraction.

pub(crate) mod api;
mod attachment;
pub mod protocols;
mod router;

pub use api::*;
pub use attachment::*;
pub use router::*;

#[cfg(test)]
#[path = "provider_tests.rs"]
mod tests;
