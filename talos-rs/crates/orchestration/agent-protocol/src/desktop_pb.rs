//! Closed desktop-media projections; no endpoints, raw CDP or untyped control payloads.
use crate::environment_pb::invalid;
use agent_pb as pb;
use talos_error::TalosResult;
use talos_types::{BrowserInputMode, DesktopStreamBinding, WorkerDesktopOpen};

pub fn validate_binding(value: &DesktopStreamBinding) -> TalosResult<()> {
    for id in [&value.stream_id, &value.viewer_id, &value.generation] {
        if uuid::Uuid::parse_str(id).is_err() {
            return Err(invalid());
        }
    }
    if value.target_id != "managed" {
        return Err(invalid());
    }
    match value.input_mode {
        BrowserInputMode::ReadOnly
            if value.controller_id.is_none() && value.input_epoch.is_none() =>
        {
            Ok(())
        }
        BrowserInputMode::Control
            if value
                .controller_id
                .as_ref()
                .is_some_and(|id| uuid::Uuid::parse_str(id).is_ok())
                && value
                    .input_epoch
                    .is_some_and(|epoch| epoch <= 9_007_199_254_740_991) =>
        {
            Ok(())
        }
        _ => Err(invalid()),
    }
}

pub fn encode_binding(value: DesktopStreamBinding) -> TalosResult<pb::DesktopBinding> {
    validate_binding(&value)?;
    Ok(pb::DesktopBinding {
        stream_id: value.stream_id,
        target_id: value.target_id,
        viewer_id: value.viewer_id,
        generation: value.generation,
        input_mode: match value.input_mode {
            BrowserInputMode::ReadOnly => pb::DesktopInputMode::ReadOnly,
            BrowserInputMode::Control => pb::DesktopInputMode::Control,
        } as i32,
        controller_id: value.controller_id,
        input_epoch: value.input_epoch,
    })
}
pub fn decode_binding(value: Option<pb::DesktopBinding>) -> TalosResult<DesktopStreamBinding> {
    let value = value.ok_or_else(invalid)?;
    let value = DesktopStreamBinding {
        stream_id: value.stream_id,
        target_id: value.target_id,
        viewer_id: value.viewer_id,
        generation: value.generation,
        input_mode: match pb::DesktopInputMode::try_from(value.input_mode).map_err(|_| invalid())? {
            pb::DesktopInputMode::ReadOnly => BrowserInputMode::ReadOnly,
            pb::DesktopInputMode::Control => BrowserInputMode::Control,
            _ => return Err(invalid()),
        },
        controller_id: value.controller_id,
        input_epoch: value.input_epoch,
    };
    validate_binding(&value)?;
    Ok(value)
}
pub fn encode_desktop_open_params(
    value: WorkerDesktopOpen,
) -> TalosResult<pb::WorkerDesktopOpenParams> {
    if uuid::Uuid::parse_str(&value.nonce).is_err() {
        return Err(invalid());
    }
    Ok(pb::WorkerDesktopOpenParams {
        binding: Some(encode_binding(value.binding)?),
        nonce: value.nonce,
    })
}
pub fn decode_desktop_open_params(
    value: Option<pb::WorkerDesktopOpenParams>,
) -> TalosResult<WorkerDesktopOpen> {
    let value = value.ok_or_else(invalid)?;
    if uuid::Uuid::parse_str(&value.nonce).is_err() {
        return Err(invalid());
    }
    Ok(WorkerDesktopOpen {
        binding: decode_binding(value.binding)?,
        nonce: value.nonce,
    })
}
pub fn encode_desktop_open_result(_: ()) -> TalosResult<pb::Empty> {
    Ok(pb::Empty {})
}
pub fn decode_desktop_open_result(_: pb::Empty) -> TalosResult<()> {
    Ok(())
}
