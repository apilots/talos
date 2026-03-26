pub mod gateway;
pub mod r#trait;
pub mod types;

pub use gateway::GatewayImpl;
pub use r#trait::Gateway;
pub use talos_types::talos::TalosInputItem;
pub use types::{GatewayRequest, GatewayResponse};
