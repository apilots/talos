//! Talos Gateway HTTP Module
//!
//! Provides HTTP interface for the Talos gateway system.

mod server;

pub use server::{HttpServer, ServerLifecycle, ServerLifecycleState};
