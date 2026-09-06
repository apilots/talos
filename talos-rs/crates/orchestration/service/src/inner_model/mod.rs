//! Routes Service-owned auxiliary model calls through one validated model and usage path.
//!
//! Business prompts, output parsing, and persistence remain owned by their callers. This module
//! only owns model resolution, Provider routing, and Profile usage recording.

mod client;
mod resolver;
mod types;

pub use client::{InnerModelClient, InnerModelClientBuilder};
pub use resolver::resolve_inner_model_ref;
pub use types::InnerModelChatParams;

#[cfg(test)]
#[path = "resolver_tests.rs"]
mod resolver_tests;

#[cfg(test)]
#[path = "client_tests.rs"]
mod client_tests;
