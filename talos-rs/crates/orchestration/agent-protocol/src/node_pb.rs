//! Lossless node-domain projections at the protobuf boundary.

use std::collections::HashSet;

use agent_pb as pb;
use talos_error::{ErrorCode, TalosError, TalosResult};

use crate::{
    CreateNodeRequest, CreatedNode, Deployment, GetNodeRequest, HeartbeatParams, HeartbeatResult,
    LeaseValidity, ListNodesRequest, NodeAuth, NodeCredential, NodeHealth, NodeInfo, NodeMode,
    NodePage, NodeStatus, RegisterParams, Registration, RotateCredentialRequest, UpdateNodeRequest,
};

pub fn decode_auth(auth: Option<pb::NodeAuth>) -> TalosResult<NodeAuth> {
    let auth = auth.ok_or_else(invalid_request)?;
    if auth.node_id.trim().is_empty() || auth.token.is_empty() {
        return Err(invalid_request());
    }
    Ok(NodeAuth {
        node_id: auth.node_id,
        token: auth.token,
    })
}

pub fn encode_auth(auth: NodeAuth) -> pb::NodeAuth {
    pb::NodeAuth {
        node_id: auth.node_id,
        token: auth.token,
    }
}

pub fn decode_register(params: Option<pb::RegisterParams>) -> TalosResult<RegisterParams> {
    let params = params.ok_or_else(invalid_request)?;
    let mut deployments = Vec::with_capacity(params.deployments.len());
    let mut ids = HashSet::new();
    for deployment in params.deployments {
        if deployment.agent_id.trim().is_empty() || !ids.insert(deployment.agent_id.clone()) {
            return Err(invalid_request());
        }
        deployments.push(Deployment {
            agent_id: deployment.agent_id,
        });
    }
    Ok(RegisterParams { deployments })
}

pub fn encode_register(params: RegisterParams) -> pb::RegisterParams {
    pb::RegisterParams {
        deployments: params
            .deployments
            .into_iter()
            .map(|deployment| pb::Deployment {
                agent_id: deployment.agent_id,
            })
            .collect(),
    }
}

pub fn decode_health(value: i32) -> TalosResult<NodeHealth> {
    match pb::NodeHealth::try_from(value) {
        Ok(pb::NodeHealth::Unknown) => Ok(NodeHealth::Unknown),
        Ok(pb::NodeHealth::Healthy) => Ok(NodeHealth::Healthy),
        Ok(pb::NodeHealth::Degraded) => Ok(NodeHealth::Degraded),
        Ok(pb::NodeHealth::Unspecified) | Err(_) => Err(invalid_request()),
    }
}

pub fn encode_health(health: NodeHealth) -> i32 {
    match health {
        NodeHealth::Unknown => pb::NodeHealth::Unknown as i32,
        NodeHealth::Healthy => pb::NodeHealth::Healthy as i32,
        NodeHealth::Degraded => pb::NodeHealth::Degraded as i32,
    }
}

pub fn decode_status(value: i32) -> TalosResult<NodeStatus> {
    match pb::NodeStatus::try_from(value) {
        Ok(pb::NodeStatus::Enabled) => Ok(NodeStatus::Enabled),
        Ok(pb::NodeStatus::Disabled) => Ok(NodeStatus::Disabled),
        Ok(pb::NodeStatus::Unspecified) | Err(_) => Err(invalid_request()),
    }
}

pub fn encode_status(status: NodeStatus) -> i32 {
    match status {
        NodeStatus::Enabled => pb::NodeStatus::Enabled as i32,
        NodeStatus::Disabled => pb::NodeStatus::Disabled as i32,
    }
}

pub fn encode_registration(registration: Registration) -> pb::Registration {
    pb::Registration {
        lease_token: registration.lease_token,
        validity: Some(encode_validity(registration.validity)),
        owner_user_id: registration.owner_user_id,
    }
}

pub fn decode_registration(value: pb::Registration) -> TalosResult<Registration> {
    if value.lease_token.is_empty() || value.owner_user_id.is_empty() {
        return Err(invalid_response());
    }
    Ok(Registration {
        lease_token: value.lease_token,
        validity: decode_validity(value.validity.ok_or_else(invalid_response)?)?,
        owner_user_id: value.owner_user_id,
    })
}

pub fn encode_validity(validity: LeaseValidity) -> pb::LeaseValidity {
    pb::LeaseValidity {
        expires_at: validity.expires_at,
        server_at: validity.server_at,
    }
}

pub fn decode_validity(value: pb::LeaseValidity) -> TalosResult<LeaseValidity> {
    if value.expires_at <= value.server_at {
        return Err(invalid_response());
    }
    Ok(LeaseValidity {
        expires_at: value.expires_at,
        server_at: value.server_at,
    })
}

pub fn encode_heartbeat(result: HeartbeatResult) -> pb::HeartbeatResult {
    pb::HeartbeatResult {
        received_at: result.received_at,
        registration_required: result.registration_required,
    }
}

pub fn decode_heartbeat(value: pb::HeartbeatResult) -> HeartbeatResult {
    HeartbeatResult {
        received_at: value.received_at,
        registration_required: value.registration_required,
    }
}

pub fn decode_heartbeat_params(
    params: Option<pb::HeartbeatParams>,
) -> TalosResult<HeartbeatParams> {
    Ok(HeartbeatParams {
        health: decode_health(params.ok_or_else(invalid_request)?.health)?,
    })
}

pub fn encode_info(info: NodeInfo) -> pb::NodeInfo {
    pb::NodeInfo {
        node_id: info.node_id,
        display_name: info.display_name,
        mode: match info.mode {
            NodeMode::Embedded => pb::NodeMode::Embedded as i32,
            NodeMode::Remote => pb::NodeMode::Remote as i32,
        },
        endpoint: info.endpoint,
        status: encode_status(info.status),
        health: encode_health(info.health),
        lease_expires_at: info.lease_expires_at,
        last_heartbeat_at: info.last_heartbeat_at,
        created_at: info.created_at,
        updated_at: info.updated_at,
        owner_user_id: info.owner_user_id,
        available: info.available,
        credential_expires_at: info.credential_expires_at,
    }
}

pub fn encode_credential(credential: NodeCredential) -> pb::NodeCredential {
    pb::NodeCredential {
        node_id: credential.node_id,
        token: credential.token,
        expires_at: credential.expires_at,
    }
}

pub fn encode_created(created: CreatedNode) -> pb::CreatedNode {
    pb::CreatedNode {
        node: Some(encode_info(created.node)),
        credential: Some(encode_credential(created.credential)),
    }
}

pub fn encode_page(page: NodePage) -> pb::NodePage {
    pb::NodePage {
        data: page.data.into_iter().map(encode_info).collect(),
        next_cursor: page.next_cursor,
    }
}

pub fn decode_create(value: pb::CreateNodeRequest) -> CreateNodeRequest {
    CreateNodeRequest {
        display_name: value.display_name,
        endpoint: value.endpoint,
        owner_user_id: value.owner_user_id,
        credential_expires_at: value.credential_expires_at,
    }
}

pub fn decode_get(value: pb::GetNodeRequest) -> GetNodeRequest {
    GetNodeRequest {
        node_id: value.node_id,
    }
}

pub fn decode_list(value: pb::ListNodesRequest) -> ListNodesRequest {
    ListNodesRequest {
        cursor: value.cursor,
        limit: value.limit,
    }
}

pub fn decode_update(value: pb::UpdateNodeRequest) -> TalosResult<UpdateNodeRequest> {
    Ok(UpdateNodeRequest {
        node_id: value.node_id,
        display_name: value.display_name,
        endpoint: value.endpoint,
        status: decode_status(value.status)?,
    })
}

pub fn decode_rotate(value: pb::RotateCredentialRequest) -> RotateCredentialRequest {
    RotateCredentialRequest {
        node_id: value.node_id,
        expires_at: value.expires_at,
    }
}

pub fn require_empty(params: Option<pb::Empty>) -> TalosResult<()> {
    params.map(|_| ()).ok_or_else(invalid_request)
}

fn invalid_request() -> TalosError {
    TalosError::new(ErrorCode::InvalidRequest, "Agent node request is malformed")
}

fn invalid_response() -> TalosError {
    TalosError::internal("Agent node response is malformed")
}
