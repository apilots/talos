//! Primary TUI render runtime.
//!
//! This module owns transcript projection, terminal scrollback synchronization,
//! live rendering, and frame scheduling for interactive TUI mode. It must stay
//! independent from CLI event rendering and must not reintroduce a second
//! transcript mirror or compatibility path for the removed chat-block renderer.

pub(crate) mod common;
pub(crate) mod render;
pub(crate) mod runtime;
pub(crate) mod transcript;
