//! Transcript source-of-truth types for the render runtime.

pub(crate) mod block;
pub(crate) mod line;
pub(crate) mod projection;
pub(crate) mod store;
#[cfg(test)]
pub(crate) mod sync;
#[cfg(test)]
pub(crate) mod writer;
