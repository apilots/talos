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

mod request_parameters;
pub use request_parameters::{
    apply_thinking_selection, has_boolean_thinking_control, serialize_with_parameters,
};

mod protocol_provider;
pub use protocol_provider::ProtocolProvider;
