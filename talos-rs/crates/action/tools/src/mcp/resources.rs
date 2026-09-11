use base64::Engine;
use serde::{Deserialize, Serialize};

use super::*;

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
enum ResourceKind {
    Resources,
    Templates,
}

#[derive(Serialize, Deserialize)]
struct Cursor {
    kind: ResourceKind,
    server: String,
    remote: Option<String>,
}

struct Position {
    connection: McpConnection,
    remote: Option<String>,
    next_server: Option<String>,
}

impl McpConnectionManager {
    pub async fn list_resources(
        &self,
        server: Option<&str>,
        cursor: Option<String>,
        context: &ToolContext,
    ) -> TalosResult<Value> {
        let Some(position) = self.resource_position(server, cursor, ResourceKind::Resources)?
        else {
            return to_json(talos_types::McpResourceListResponse {
                data: vec![],
                next_cursor: None,
            });
        };
        let mut page = position
            .connection
            .list_resources(position.remote.clone(), cancelled(context))
            .await?;
        page.next_cursor = position.next_cursor(ResourceKind::Resources, page.next_cursor)?;
        to_json(page)
    }

    pub async fn list_resource_templates(
        &self,
        server: Option<&str>,
        cursor: Option<String>,
        context: &ToolContext,
    ) -> TalosResult<Value> {
        let Some(position) = self.resource_position(server, cursor, ResourceKind::Templates)?
        else {
            return to_json(talos_types::McpResourceTemplateListResponse {
                data: vec![],
                next_cursor: None,
            });
        };
        let mut page = position
            .connection
            .list_resource_templates(position.remote.clone(), cancelled(context))
            .await?;
        page.next_cursor = position.next_cursor(ResourceKind::Templates, page.next_cursor)?;
        to_json(page)
    }

    fn resource_position(
        &self,
        server: Option<&str>,
        cursor: Option<String>,
        kind: ResourceKind,
    ) -> TalosResult<Option<Position>> {
        let cursor: Option<Cursor> = cursor
            .map(|cursor| {
                if cursor.len() > 8 * 1024 * 1024 {
                    return Err(invalid_cursor());
                }
                let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
                    .decode(cursor)
                    .map_err(|error| invalid_cursor().with_source(error))?;
                serde_json::from_slice(&bytes).map_err(|error| invalid_cursor().with_source(error))
            })
            .transpose()?;
        if cursor.as_ref().is_some_and(|cursor| {
            cursor.kind != kind || server.is_some_and(|server| server != cursor.server)
        }) {
            return Err(invalid_cursor());
        }
        let connections: Vec<_> = self
            .runtime
            .connections()
            .into_iter()
            .filter(|connection| {
                connection.supports_resources()
                    && server.is_none_or(|server| server == connection.name())
            })
            .collect();
        let index = match &cursor {
            Some(cursor) => connections
                .iter()
                .position(|connection| connection.name() == cursor.server)
                .ok_or_else(invalid_cursor)?,
            None => 0,
        };
        let Some(connection) = connections.get(index).cloned() else {
            if server.is_some() {
                return Err(TalosError::new(
                    talos_error::ErrorCode::ToolExecutionFailed,
                    "The MCP resource service is not ready.",
                ));
            }
            return Ok(None);
        };
        Ok(Some(Position {
            connection,
            remote: cursor.and_then(|cursor| cursor.remote),
            next_server: connections
                .get(index + 1)
                .map(|connection| connection.name().to_owned()),
        }))
    }
}

impl Position {
    fn next_cursor(
        &self,
        kind: ResourceKind,
        remote: Option<String>,
    ) -> TalosResult<Option<String>> {
        if remote.is_some() && remote == self.remote {
            return Err(invalid_cursor());
        }
        let server = if remote.is_some() {
            Some(self.connection.name().to_owned())
        } else {
            self.next_server.clone()
        };
        server
            .map(|server| {
                serde_json::to_vec(&Cursor {
                    kind,
                    server,
                    remote,
                })
                .map(|bytes| base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes))
                .map_err(|error| {
                    TalosError::internal_with_source(
                        "Failed to encode the MCP resource cursor.",
                        error,
                    )
                })
            })
            .transpose()
    }
}

fn invalid_cursor() -> TalosError {
    TalosError::new(
        talos_error::ErrorCode::InvalidCursor,
        "The MCP resource cursor is invalid or no longer available.",
    )
}
fn to_json(value: impl Serialize) -> TalosResult<Value> {
    serde_json::to_value(value).map_err(|error| {
        TalosError::internal_with_source("Failed to encode the MCP resource page.", error)
    })
}
