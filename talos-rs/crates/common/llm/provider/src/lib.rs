//! Talos LLM provider abstraction.

pub(crate) mod api;
mod attachment;
mod error;
pub use error::http_response_error;
pub mod protocols;
mod router;

pub use api::*;
pub use attachment::*;
pub use router::*;

#[cfg(test)]
#[path = "provider_tests.rs"]
mod tests;
