use crate::BrowserActionExt;
use crate::{BrowserAction, BrowserRuntime};
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use std::{
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    sync::Arc,
};
use talos_error::{ApiError, ErrorCode, TalosError, TalosResult};
use talos_types::McpCallToolResponse;
use tokio::net::{UnixListener, UnixStream};
use tokio_util::codec::{Framed, LengthDelimitedCodec};
use uuid::Uuid;

const MAX_BYTES: usize = 12 * 1024 * 1024;
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct Request {
    capability: String,
    tool: String,
    arguments: serde_json::Value,
}
#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "camelCase")]
enum Response {
    Success(McpCallToolResponse),
    Failure(ApiError),
}

impl BrowserRuntime {
    /// OS-private, per-boot socket; no listener on host/container TCP interfaces.
    pub async fn start_ipc(self: &Arc<Self>, home: &Path) -> TalosResult<PathBuf> {
        let root = if home.as_os_str().len() < 45 {
            home.join("browser/ipc")
                .join(Uuid::new_v4().simple().to_string())
        } else {
            PathBuf::from("/tmp").join(format!(".talos-browser-{}", Uuid::new_v4().simple()))
        };
        let parent = root
            .parent()
            .ok_or_else(|| TalosError::internal("Browser IPC directory has no parent."))?;
        tokio::fs::create_dir_all(parent).await.map_err(|error| {
            TalosError::internal_with_source("Failed to prepare Browser IPC parent.", error)
        })?;
        use std::os::unix::fs::DirBuilderExt;
        std::fs::DirBuilder::new()
            .mode(0o700)
            .create(&root)
            .map_err(|error| {
                TalosError::internal_with_source(
                    "Failed to prepare private Browser IPC directory.",
                    error,
                )
            })?;
        let path = root.join("mcp.sock");
        let listener = UnixListener::bind(&path).map_err(|error| {
            TalosError::internal_with_source("Failed to bind Browser IPC.", error)
        })?;
        tokio::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))
            .await
            .map_err(|error| {
                TalosError::internal_with_source("Failed to protect Browser IPC socket.", error)
            })?;
        let runtime = self.clone();
        let cleanup = path.clone();
        let shutdown = self.shutdown_token();
        self.ipc_tasks.spawn(async move {
            let mut tasks = tokio::task::JoinSet::new();
            loop {
                tokio::select! {
                    _=shutdown.cancelled()=>break,
                    accepted=listener.accept()=>match accepted {
                        Ok((stream,_)) if tasks.len()<16=>{ let runtime=runtime.clone();tasks.spawn(async move {if let Err(error)=serve(runtime,stream).await {talos_logging::report_error(&error);}}); },
                        Ok(_)=>{}, Err(_)=>break,
                    },
                    _=tasks.join_next(),if !tasks.is_empty()=>{},
                }
            }
            tasks.abort_all();
            while tasks.join_next().await.is_some() {}
            let _ = tokio::fs::remove_file(cleanup).await;
            let _ = tokio::fs::remove_dir(root).await;
        });
        Ok(path)
    }
}
fn framed(stream: UnixStream) -> Framed<UnixStream, LengthDelimitedCodec> {
    Framed::new(
        stream,
        LengthDelimitedCodec::builder()
            .max_frame_length(MAX_BYTES)
            .new_codec(),
    )
}
async fn serve(runtime: Arc<BrowserRuntime>, stream: UnixStream) -> TalosResult<()> {
    let mut stream = framed(stream);
    let request = tokio::time::timeout(std::time::Duration::from_secs(5), stream.next())
        .await
        .map_err(|error| {
            TalosError::from_source(
                ErrorCode::Timeout,
                "Browser IPC handshake timed out.",
                error,
            )
        })?
        .ok_or_else(|| {
            TalosError::new(ErrorCode::InvalidRequest, "Browser IPC request is missing.")
        })?
        .map_err(|error| {
            TalosError::internal_with_source("Failed to read Browser IPC request.", error)
        })?;
    let request: Request = serde_json::from_slice(&request).map_err(|error| {
        TalosError::from_source(
            ErrorCode::InvalidRequest,
            "Invalid Browser IPC request.",
            error,
        )
    })?;
    let result = match BrowserAction::parse(&request.tool, request.arguments) {
        Ok(action) => runtime.invoke(&request.capability, action).await,
        Err(error) => Err(error),
    };
    let response = match result {
        Ok(value) => Response::Success(value),
        Err(error) => {
            talos_logging::report_error(&error);
            Response::Failure(error.to_api_error())
        }
    };
    let bytes = serde_json::to_vec(&response).map_err(|error| {
        TalosError::internal_with_source("Failed to encode Browser IPC response.", error)
    })?;
    if bytes.len() > MAX_BYTES {
        return Err(TalosError::new(
            ErrorCode::ToolExecutionFailed,
            "Browser response exceeds its size limit.",
        ));
    }
    stream.send(bytes.into()).await.map_err(|error| {
        TalosError::internal_with_source("Failed to send Browser IPC response.", error)
    })
}

pub(crate) async fn call(
    path: &Path,
    capability: String,
    tool: String,
    arguments: serde_json::Value,
) -> TalosResult<McpCallToolResponse> {
    let socket = UnixStream::connect(path).await.map_err(|error| {
        TalosError::from_source(
            ErrorCode::ServiceUnavailable,
            "Browser Worker bridge is unavailable.",
            error,
        )
    })?;
    let mut socket = framed(socket);
    let bytes = serde_json::to_vec(&Request {
        capability,
        tool,
        arguments,
    })
    .map_err(|error| {
        TalosError::internal_with_source("Failed to encode Browser invocation.", error)
    })?;
    socket.send(bytes.into()).await.map_err(|error| {
        TalosError::internal_with_source("Failed to send Browser invocation.", error)
    })?;
    let bytes = socket
        .next()
        .await
        .ok_or_else(|| {
            TalosError::new(
                ErrorCode::ServiceUnavailable,
                "Browser Worker bridge closed.",
            )
        })?
        .map_err(|error| {
            TalosError::internal_with_source("Failed to read Browser result.", error)
        })?;
    let response: Response = serde_json::from_slice(&bytes).map_err(|error| {
        TalosError::internal_with_source("Invalid Browser result envelope.", error)
    })?;
    match response {
        Response::Success(value) => Ok(value),
        Response::Failure(error) => Err(error.into()),
    }
}
