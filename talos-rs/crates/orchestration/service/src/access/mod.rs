mod service;
mod types;

pub(crate) use service::ResourceAccessService;
pub(crate) use service::{embedded_access_denied, CreateSessionRequest};
pub use types::Actor;
