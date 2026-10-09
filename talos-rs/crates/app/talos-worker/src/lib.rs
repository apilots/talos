//! Shared Talos Agent execution runtime.
//!
//! The control plane embeds this runtime and registers its Harnesses directly.
//! The standalone host wraps the same Harnesses with authenticated gRPC.

mod harness;
mod mcp;
mod provider;
mod runtime;
mod standalone;

pub use mcp::prepare as prepare_mcp;
pub use provider::create_llm_provider;
pub use runtime::{TalosWorker, TalosWorkerResources};
pub use standalone::run as run_standalone;
