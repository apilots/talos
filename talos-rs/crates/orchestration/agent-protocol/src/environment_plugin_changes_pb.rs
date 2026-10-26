use crate::{
    environment_pb as wire, WorkerPluginChange, WorkerPluginInstall, WorkerPluginPrepared,
    WorkerPluginUpdate,
};
use agent_pb as pb;
use talos_error::TalosResult;

pub fn encode_prepare_plugin_change_params(
    value: WorkerPluginChange,
) -> TalosResult<pb::PluginPrepareParams> {
    use pb::plugin_prepare_params::Change;
    Ok(pb::PluginPrepareParams {
        change: Some(match value {
            WorkerPluginChange::Install(request) => {
                Change::Install(wire::encode_install_plugin_params(request)?)
            }
            WorkerPluginChange::Update(request) => {
                Change::Update(wire::encode_update_plugin_params(request)?)
            }
        }),
    })
}
pub fn decode_prepare_plugin_change_params(
    value: Option<pb::PluginPrepareParams>,
) -> TalosResult<WorkerPluginChange> {
    use pb::plugin_prepare_params::Change;
    match value
        .and_then(|value| value.change)
        .ok_or_else(wire::invalid)?
    {
        Change::Install(request) => Ok(WorkerPluginChange::Install(
            wire::decode_install_plugin_params(Some(request))?,
        )),
        Change::Update(request) => Ok(WorkerPluginChange::Update(
            wire::decode_update_plugin_params(Some(request))?,
        )),
    }
}
pub fn encode_prepare_plugin_change_result(
    value: WorkerPluginPrepared,
) -> TalosResult<pb::PluginPreparedResult> {
    Ok(pb::PluginPreparedResult {
        preparation_id: value.preparation_id,
    })
}
pub fn decode_prepare_plugin_change_result(
    value: pb::PluginPreparedResult,
) -> TalosResult<WorkerPluginPrepared> {
    if value.preparation_id.is_empty() {
        return Err(wire::invalid());
    }
    Ok(WorkerPluginPrepared {
        preparation_id: value.preparation_id,
    })
}
pub fn encode_worker_install_plugin_params(
    value: WorkerPluginInstall,
) -> TalosResult<pb::WorkerPluginInstallParams> {
    Ok(pb::WorkerPluginInstallParams {
        request: Some(wire::encode_install_plugin_params(value.request)?),
        preparation_id: value.preparation_id,
    })
}

pub fn decode_cancel_plugin_change_params(
    value: Option<pb::PluginPreparedResult>,
) -> TalosResult<WorkerPluginPrepared> {
    decode_prepare_plugin_change_result(value.ok_or_else(wire::invalid)?)
}

pub fn encode_cancel_plugin_change_result(_: ()) -> TalosResult<pb::Empty> {
    Ok(pb::Empty {})
}

pub fn decode_cancel_plugin_change_result(_: pb::Empty) -> TalosResult<()> {
    Ok(())
}
pub fn decode_worker_install_plugin_params(
    value: Option<pb::WorkerPluginInstallParams>,
) -> TalosResult<WorkerPluginInstall> {
    let value = value.ok_or_else(wire::invalid)?;
    if value.preparation_id.is_empty() {
        return Err(wire::invalid());
    }
    Ok(WorkerPluginInstall {
        request: wire::decode_install_plugin_params(value.request)?,
        preparation_id: value.preparation_id,
    })
}
pub fn encode_worker_update_plugin_params(
    value: WorkerPluginUpdate,
) -> TalosResult<pb::WorkerPluginUpdateParams> {
    Ok(pb::WorkerPluginUpdateParams {
        request: Some(wire::encode_update_plugin_params(value.request)?),
        preparation_id: value.preparation_id,
    })
}
pub fn decode_worker_update_plugin_params(
    value: Option<pb::WorkerPluginUpdateParams>,
) -> TalosResult<WorkerPluginUpdate> {
    let value = value.ok_or_else(wire::invalid)?;
    if value.preparation_id.is_empty() {
        return Err(wire::invalid());
    }
    Ok(WorkerPluginUpdate {
        request: wire::decode_update_plugin_params(value.request)?,
        preparation_id: value.preparation_id,
    })
}
