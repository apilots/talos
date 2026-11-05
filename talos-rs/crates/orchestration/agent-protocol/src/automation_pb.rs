//! Closed Goal/Loop codecs for both local and reverse resource RPC.
use agent_pb as pb;
use talos_error::{ErrorCode, TalosError, TalosResult};
use talos_types::{
    AgentCaller, AutomationControlResult, AutomationInputOrigin, AutomationModelRequest,
    AutomationObjectRef,
};

#[path = "automation_pb/actions.rs"]
mod actions;
#[path = "automation_pb/facts.rs"]
mod facts;

pub fn encode_model_params(
    value: AutomationModelRequest,
) -> TalosResult<pb::AutomationModelParams> {
    validate_identity(&value.caller, &value.call_id)?;
    Ok(pb::AutomationModelParams {
        caller: Some(pb::AgentCaller {
            session_id: value.caller.session_id,
            turn_id: value.caller.turn_id,
            agent_id: value.caller.agent_id,
        }),
        call_id: value.call_id,
        action: Some(actions::encode_action(value.action)),
    })
}

pub fn decode_model_params(
    value: Option<pb::AutomationModelParams>,
) -> TalosResult<AutomationModelRequest> {
    let value = value.ok_or_else(invalid)?;
    let caller = value.caller.ok_or_else(invalid)?;
    let caller = AgentCaller {
        session_id: caller.session_id,
        turn_id: caller.turn_id,
        agent_id: caller.agent_id,
    };
    validate_identity(&caller, &value.call_id)?;
    Ok(AutomationModelRequest {
        caller,
        call_id: value.call_id,
        action: actions::decode_action(value.action.ok_or_else(invalid)?)?,
    })
}

/// Validate transport identity only; authorization and business state belong to Service.
fn validate_identity(caller: &AgentCaller, call_id: &str) -> TalosResult<()> {
    for value in [
        &caller.session_id,
        &caller.turn_id,
        &caller.agent_id,
        call_id,
    ] {
        required(value)?;
    }
    Ok(())
}

/// Shared exact Input metadata codec; identity validation remains closed and typed.
pub fn encode_input_origin(value: AutomationInputOrigin) -> pb::AutomationInputOrigin {
    pb::AutomationInputOrigin {
        message_id: value.message_id,
        origin: Some(facts::encode_origin(value.origin)),
    }
}
pub fn decode_input_origin(value: pb::AutomationInputOrigin) -> TalosResult<AutomationInputOrigin> {
    required(&value.message_id)?;
    Ok(AutomationInputOrigin {
        message_id: value.message_id,
        origin: facts::decode_origin(value.origin.ok_or_else(invalid)?)?,
    })
}

pub fn encode_result(value: AutomationControlResult) -> pb::AutomationControlResult {
    pb::AutomationControlResult {
        created_object: value.created_object.map(encode_created_object),
        effect: facts::encode_effect(value.effect),
        bound_origin: value.bound_origin.map(facts::encode_origin),
        snapshot: Some(facts::encode_snapshot(value.snapshot)),
    }
}

pub fn decode_result(value: pb::AutomationControlResult) -> TalosResult<AutomationControlResult> {
    Ok(AutomationControlResult {
        created_object: value
            .created_object
            .map(decode_created_object)
            .transpose()?,
        effect: facts::decode_effect(value.effect)?,
        bound_origin: value.bound_origin.map(facts::decode_origin).transpose()?,
        snapshot: facts::decode_snapshot(value.snapshot.ok_or_else(invalid)?)?,
    })
}

fn encode_created_object(value: AutomationObjectRef) -> pb::AutomationObjectRef {
    use pb::automation_object_ref::Value;
    pb::AutomationObjectRef {
        value: Some(match value {
            AutomationObjectRef::Goal { goal_id } => Value::GoalId(goal_id),
            AutomationObjectRef::Loop { loop_id } => Value::LoopId(loop_id),
        }),
    }
}

fn decode_created_object(value: pb::AutomationObjectRef) -> TalosResult<AutomationObjectRef> {
    use pb::automation_object_ref::Value;
    match value.value.ok_or_else(invalid)? {
        Value::GoalId(goal_id) => {
            required(&goal_id)?;
            Ok(AutomationObjectRef::Goal { goal_id })
        }
        Value::LoopId(loop_id) => {
            required(&loop_id)?;
            Ok(AutomationObjectRef::Loop { loop_id })
        }
    }
}

pub fn encode_response(
    value: TalosResult<AutomationControlResult>,
) -> pb::ExecuteAutomationResponse {
    use pb::execute_automation_response::Result;
    pb::ExecuteAutomationResponse {
        result: Some(match value {
            Ok(value) => Result::Accepted(Box::new(encode_result(value))),
            Err(error) => Result::Failure(Box::new(crate::error_pb::encode_failure(&error))),
        }),
    }
}

pub fn decode_response(
    value: pb::ExecuteAutomationResponse,
) -> TalosResult<AutomationControlResult> {
    match value.result.ok_or_else(invalid)? {
        pb::execute_automation_response::Result::Accepted(value) => decode_result(*value),
        pb::execute_automation_response::Result::Failure(value) => {
            Err(crate::error_pb::decode_failure(*value)?)
        }
    }
}

fn required(value: &str) -> TalosResult<()> {
    if value.trim().is_empty() {
        Err(invalid())
    } else {
        Ok(())
    }
}

fn invalid() -> TalosError {
    TalosError::new(
        ErrorCode::InvalidRequest,
        "Automation RPC payload is malformed",
    )
}
