//! Talos LLM provider abstraction.

pub(crate) mod api;
pub mod protocols;

pub use api::*;

#[cfg(test)]
mod tests;
