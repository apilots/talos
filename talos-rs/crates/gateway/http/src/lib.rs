//! Talos Gateway HTTP Module
//!
//! Provides HTTP interface for the Talos gateway system.

mod server;

pub use server::{HttpServer, PersonalPreviewRoute, ServerLifecycle, ServerLifecycleState};

pub use server::product_routing::{
    ProductRequestRouter, ProductRoute, ProductRouteFuture, SelectedComputer,
};
