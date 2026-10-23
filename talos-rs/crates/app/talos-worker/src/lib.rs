//! Shared Talos Agent execution runtime.
//!
//! Control-plane, Desktop and standalone hosts compose this same runtime.
//! Network hosts wrap its existing Harnesses with authenticated direct or outbound gRPC.

mod config;
mod harness;
mod mcp;
mod output_logs;
mod provider;
mod registration;
mod resource_binding;
mod resource_tools;
mod rpc_host;
mod runtime;
mod standalone;

pub use mcp::prepare as prepare_mcp;
pub use provider::create_llm_provider;
pub use registration::RegisteredWorker;
pub use rpc_host::WorkerRpcHost;
pub use runtime::{TalosWorker, TalosWorkerResources};
pub use standalone::run as run_standalone;

pub use talos_browser::run_stdio as run_browser_mcp;
