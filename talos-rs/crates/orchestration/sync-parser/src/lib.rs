//! Talos owns the synchronization payload and its projection into native facts.
mod codec;
mod header;
mod metadata;
mod projection;
mod records;
mod types;

pub use codec::{codec_error, sync_error};
pub use metadata::{decode_changes, encode_change, resource};
pub use projection::SessionProjection;
pub use types::*;

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
