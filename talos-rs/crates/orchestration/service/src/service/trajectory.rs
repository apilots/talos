//! Authorized terminal-history projections, shared by HTTP, RPC and SDKs.
use crate::{Actor, TalosService};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use serde::{Deserialize, Serialize};
use talos_agent_api::{trajectory::*, AgentTurnRef};
use talos_error::{ErrorCode, TalosError, TalosResult};

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Cursor {
    identity: AgentTurnRef,
    offset: usize,
    payload_id: Option<String>,
}
fn invalid_cursor() -> TalosError {
    TalosError::new(
        ErrorCode::InvalidCursor,
        "The execution history cursor is invalid.",
    )
}
fn not_found() -> TalosError {
    TalosError::new(
        ErrorCode::ResourceNotFound,
        "The requested execution history content was not found in this Turn.",
    )
}
fn encode_cursor(value: &Cursor) -> TalosResult<String> {
    serde_json::to_vec(value)
        .map(|v| URL_SAFE_NO_PAD.encode(v))
        .map_err(|e| {
            TalosError::internal_with_source(
                "Encode execution history cursor; technical details were withheld.",
                e,
            )
        })
}
fn offset(
    value: Option<&str>,
    identity: &AgentTurnRef,
    payload_id: Option<&str>,
) -> TalosResult<usize> {
    let Some(value) = value else {
        return Ok(0);
    };
    if value.len() > 2048 {
        return Err(invalid_cursor());
    }
    let cursor: Cursor = serde_json::from_slice(
        &URL_SAFE_NO_PAD
            .decode(value)
            .map_err(|_| invalid_cursor())?,
    )
    .map_err(|_| invalid_cursor())?;
    if &cursor.identity != identity || cursor.payload_id.as_deref() != payload_id {
        return Err(invalid_cursor());
    }
    Ok(cursor.offset)
}
impl TalosService {
    pub async fn trajectory_capabilities(
        &self,
        _actor: &Actor,
    ) -> TalosResult<TrajectoryCapabilitiesResponse> {
        Ok(TrajectoryCapabilitiesResponse {
            supported: self.history.supports_history_supplements(),
        })
    }
    async fn terminal_supplement(
        &self,
        actor: &Actor,
        identity: &AgentTurnRef,
    ) -> TalosResult<Option<SessionHistorySupplement>> {
        self.access
            .load_session(actor, &identity.session_id)
            .await?;
        if identity.turn_id.trim().is_empty() {
            return Err(TalosError::new(
                ErrorCode::InvalidRequest,
                "Turn ID is required.",
            ));
        }
        self.history
            .read_terminal_history_supplement(identity)
            .await
    }
    pub async fn read_turn_trajectory(
        &self,
        actor: &Actor,
        request: TrajectoryReadTurnRequest,
    ) -> TalosResult<TrajectoryReadTurnResponse> {
        let value = self.terminal_supplement(actor, &request.identity).await?;
        let start = offset(request.cursor.as_deref(), &request.identity, None)?;
        let Some(value) = value else {
            if request.cursor.is_some() {
                return Err(invalid_cursor());
            }
            return Ok(TrajectoryReadTurnResponse {
                identity: request.identity,
                clock_segments: vec![],
                data: None,
                next_cursor: None,
            });
        };
        let entries = value
            .requests
            .into_iter()
            .map(|r| TrajectoryEntry::ModelRequest { request: r.request })
            .chain(
                value
                    .tools
                    .into_iter()
                    .map(|t| TrajectoryEntry::Tool { tool: t.tool }),
            )
            .collect::<Vec<_>>();
        let total = entries.len();
        if start > total {
            return Err(invalid_cursor());
        }
        let limit = request.limit.unwrap_or(32).clamp(1, 128) as usize;
        let mut bytes = 0usize;
        let mut data = Vec::new();
        for entry in entries.into_iter().skip(start).take(limit) {
            let size = serde_json::to_vec(&entry)
                .map_err(|e| {
                    TalosError::internal_with_source(
                        "Encode execution history metadata; technical details were withheld.",
                        e,
                    )
                })?
                .len();
            if !data.is_empty() && bytes + size > 2 * 1024 * 1024 {
                break;
            }
            bytes += size;
            data.push(entry);
        }
        let end = start + data.len();
        let next_cursor = if end < total {
            Some(encode_cursor(&Cursor {
                identity: request.identity.clone(),
                offset: end,
                payload_id: None,
            })?)
        } else {
            None
        };
        Ok(TrajectoryReadTurnResponse {
            identity: request.identity,
            clock_segments: value.clock_segments,
            data: Some(data),
            next_cursor,
        })
    }
    pub async fn read_request_trajectory(
        &self,
        actor: &Actor,
        request: TrajectoryReadModelRequestRequest,
    ) -> TalosResult<TrajectoryReadModelRequestResponse> {
        self.terminal_supplement(actor, &request.identity)
            .await?
            .and_then(|v| {
                v.requests
                    .into_iter()
                    .find(|r| r.request.model_request_id == request.model_request_id)
            })
            .ok_or_else(not_found)
    }
    pub async fn read_tool_trajectory(
        &self,
        actor: &Actor,
        request: TrajectoryReadToolRequest,
    ) -> TalosResult<TrajectoryReadToolResponse> {
        self.terminal_supplement(actor, &request.identity)
            .await?
            .and_then(|v| {
                v.tools
                    .into_iter()
                    .find(|t| t.tool.call_id == request.call_id)
            })
            .ok_or_else(not_found)
    }
    pub async fn read_trajectory_payload(
        &self,
        actor: &Actor,
        request: TrajectoryReadPayloadRequest,
    ) -> TalosResult<TrajectoryReadPayloadResponse> {
        self.access
            .load_session(actor, &request.identity.session_id)
            .await?;
        let body = self
            .history
            .read_execution_payload(&request.identity, &request.payload_id)
            .await?
            .ok_or_else(not_found)?;
        let start = offset(
            request.cursor.as_deref(),
            &request.identity,
            Some(&request.payload_id),
        )?;
        if start > body.len() || !body.is_char_boundary(start) {
            return Err(invalid_cursor());
        }
        let mut end = start
            .saturating_add(
                request
                    .limit_bytes
                    .unwrap_or(256 * 1024)
                    .clamp(4, 256 * 1024) as usize,
            )
            .min(body.len());
        while !body.is_char_boundary(end) {
            end -= 1;
        }
        let next_cursor = if end < body.len() {
            Some(encode_cursor(&Cursor {
                identity: request.identity.clone(),
                offset: end,
                payload_id: Some(request.payload_id.clone()),
            })?)
        } else {
            None
        };
        Ok(TrajectoryReadPayloadResponse {
            payload_id: request.payload_id,
            encoding: TrajectoryPayloadEncoding::Utf8Json,
            data: body[start..end].to_owned(),
            next_cursor,
        })
    }
}
