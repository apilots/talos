//! Typed projections between the canonical Harness contract and protobuf.

mod action;
mod collaboration;
mod compaction;
mod context;
mod event;
mod input;
mod message;
mod notice;
mod run;
mod tool_content;

#[cfg(test)]
mod tests;

pub use context::{
    decode_context, decode_metrics, decode_session_context, decode_turn_ref, encode_context,
    encode_metrics, encode_session_context, encode_turn_ref,
};
pub use event::{decode_event, encode_event};
pub use input::{decode_user_content, encode_user_content};
pub use message::{
    decode_block, decode_message, decode_phase, decode_tool_progress, encode_block, encode_message,
    encode_phase, encode_tool_progress,
};
pub use notice::{decode_system_notice, encode_system_notice};
pub use run::{
    decode_admission_events, decode_input_params, decode_interrupt_outcome,
    decode_interrupt_params, decode_run_params, decode_settlement, encode_admission_events,
    encode_input_params, encode_interrupt_outcome, encode_interrupt_params, encode_run_params,
    encode_settlement,
};
pub use tool_content::{decode_tool_content, encode_tool_content};

use talos_error::{ErrorCode, TalosError};

fn invalid() -> TalosError {
    TalosError::new(
        ErrorCode::InvalidRequest,
        "Agent Harness RPC payload is invalid",
    )
}
pub use action::{decode_pending_input, encode_pending_input};
pub use collaboration::{decode_collaboration, encode_collaboration};
pub use compaction::{
    decode_compaction, decode_compaction_delta, encode_compaction, encode_compaction_delta,
};
