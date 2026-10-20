//! Desktop website execution consumes an injected typed host port.
use crate::{BrowserAction, BrowserActionExt, BrowserBackend, BrowserDispatch, BrowserFuture};
use base64::Engine;
use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
};
use talos_error::{ErrorCode, TalosError, TalosResult};
use talos_types::*;
use uuid::Uuid;
/// Injected website-host capability. The gateway owns transport correlation/lifecycle.
pub trait BrowserHostPort: Send + Sync {
    fn available(&self) -> bool;
    fn request(
        self: Arc<Self>,
        request: BrowserHostRequest,
    ) -> BrowserFuture<'static, BrowserHostResponse>;
}
#[derive(Default)]
struct NativeState {
    target: Option<String>,
    owner: Option<(String, String, String, u64)>,
    elements: HashMap<String, BrowserHostElement>,
}
pub struct NativeBrowser {
    bridge: Arc<dyn BrowserHostPort>,
    generation: String,
    connected: AtomicBool,
    state: tokio::sync::Mutex<NativeState>,
    user: Mutex<Option<String>>,
}
impl NativeBrowser {
    pub fn new(bridge: Arc<dyn BrowserHostPort>) -> Arc<Self> {
        Arc::new(Self {
            bridge,
            generation: Uuid::new_v4().to_string(),
            connected: AtomicBool::new(false),
            state: Default::default(),
            user: Mutex::new(None),
        })
    }
    async fn control(
        &self,
        operation: BrowserHostOperation,
        user: Option<String>,
    ) -> TalosResult<()> {
        if let Some(user) = user {
            *self
                .user
                .lock()
                .map_err(|_| TalosError::internal("Browser host lock failed."))? = Some(user);
        }
        let user = self
            .user
            .lock()
            .map_err(|_| TalosError::internal("Browser host lock failed."))?
            .clone();
        let Some(user_id) = user else {
            return Ok(());
        };
        let response = self
            .bridge
            .clone()
            .request(BrowserHostRequest {
                generation: self.generation.clone(),
                epoch: 0,
                user_id,
                target: BrowserTargetKind::Connected,
                operation,
            })
            .await?;
        match response {
            BrowserHostResponse::Done => Ok(()),
            _ => Err(TalosError::internal("Invalid Desktop control response.")),
        }
    }
}
impl BrowserBackend for NativeBrowser {
    fn refresh(&self, user_id: String) -> BrowserFuture<'_, ()> {
        Box::pin(async move {
            match self
                .bridge
                .clone()
                .request(BrowserHostRequest {
                    generation: self.generation.clone(),
                    epoch: 0,
                    user_id,
                    target: BrowserTargetKind::Connected,
                    operation: BrowserHostOperation::Probe,
                })
                .await?
            {
                BrowserHostResponse::Readiness { connected } => {
                    self.connected.store(connected, Ordering::Release);
                    Ok(())
                }
                _ => Err(TalosError::internal(
                    "Invalid Desktop browser readiness response.",
                )),
            }
        })
    }

    fn readiness(&self, target: BrowserTargetKind) -> BrowserReadiness {
        if !self.bridge.available() {
            BrowserReadiness::Unavailable {
                error: unavailable("Desktop browser host disconnected.").to_api_error(),
            }
        } else if target == BrowserTargetKind::Managed || self.connected.load(Ordering::Acquire) {
            BrowserReadiness::Ready
        } else {
            BrowserReadiness::NeedsConnection
        }
    }
    fn execute(
        &self,
        action: BrowserAction,
        dispatch: BrowserDispatch,
    ) -> BrowserFuture<'_, McpCallToolResponse> {
        Box::pin(async move {
            let mut state = self.state.lock().await;
            let identity = dispatch.identity()?;
            *self
                .user
                .lock()
                .map_err(|_| TalosError::internal("Browser host lock failed."))? =
                Some(identity.user_id.clone());
            let owner = (
                identity.user_id.clone(),
                identity.session_id,
                identity.turn_id,
                dispatch.epoch()?,
            );
            if state.owner.as_ref() != Some(&owner) {
                state.elements.clear();
                state.owner = Some(owner);
            }
            let element = match &action {
                BrowserAction::Click { element }
                | BrowserAction::Fill { element, .. }
                | BrowserAction::Upload { element, .. } => {
                    Some(state.elements.get(element).cloned().ok_or_else(|| {
                        TalosError::new(
                            ErrorCode::Conflict,
                            "Run browser_state to get a fresh element reference.",
                        )
                    })?)
                }
                _ => None,
            };
            let upload = if let BrowserAction::Upload { file_id, .. } = &action {
                let file = dispatch.upload(file_id)?;
                Some(BrowserHostUpload {
                    filename: file.filename,
                    data_base64: base64::engine::general_purpose::STANDARD.encode(&file.bytes),
                })
            } else {
                None
            };
            let _input = dispatch.input_permit().await?;
            let response = self
                .bridge
                .clone()
                .request(BrowserHostRequest {
                    generation: self.generation.clone(),
                    epoch: dispatch.epoch()?,
                    user_id: identity.user_id,
                    target: dispatch.target()?,
                    operation: BrowserHostOperation::Execute {
                        action: action.clone(),
                        target_id: state.target.clone(),
                        element,
                        upload,
                    },
                })
                .await?;
            dispatch.check()?;
            let BrowserHostResponse::Observation {
                page,
                loader_id,
                elements,
                screenshot,
                downloads,
            } = response
            else {
                return Err(TalosError::internal("Invalid Desktop browser observation."));
            };
            if elements.len() > 150
                || page.tabs.len() > 32
                || screenshot
                    .as_ref()
                    .is_some_and(|s| s.len() > 6 * 1024 * 1024)
            {
                return Err(TalosError::internal(
                    "Desktop browser result exceeded its limit.",
                ));
            }
            let mut size = 0usize;
            let mut completed = Vec::new();
            if downloads.len() > 5 {
                return Err(TalosError::new(
                    ErrorCode::UploadTooLarge,
                    "Too many browser downloads.",
                ));
            }
            for file in downloads {
                let bytes = base64::engine::general_purpose::STANDARD
                    .decode(&file.data_base64)
                    .map_err(|e| TalosError::internal_with_source("Decode browser download.", e))?;
                size += bytes.len();
                if size > 20 * 1024 * 1024 {
                    return Err(TalosError::new(
                        ErrorCode::UploadTooLarge,
                        "Browser downloads exceed 20 MiB.",
                    ));
                }
                completed.push(talos_types::ToolOutputFile {
                    filename: file.filename,
                    bytes: bytes.into(),
                });
            }
            dispatch.stage_downloads(completed)?;
            state.target = Some(page.target_id.clone());
            let mut payload = serde_json::to_value(page)
                .map_err(|e| TalosError::internal_with_source("Encode Desktop page.", e))?;
            payload["documentRevision"] = serde_json::json!(loader_id);
            if matches!(action, BrowserAction::State) {
                state.elements.clear();
                payload["elements"]=serde_json::json!(elements.into_iter().map(|element|{
                    let reference=Uuid::new_v4().to_string(); let item=serde_json::json!({"element":reference,"tag":element.tag,"attributes":element.attributes});state.elements.insert(reference,element);item
                }).collect::<Vec<_>>());
            } else if !action.is_read_only() {
                state.elements.clear();
            }
            let mut content = vec![McpContent::Text {
                text: payload.to_string(),
            }];
            if let Some(data) = screenshot {
                content.push(McpContent::Image {
                    data,
                    mime_type: "image/png".into(),
                });
            }
            Ok(McpCallToolResponse {
                content,
                is_error: false,
                structured_content: Some(payload),
            })
        })
    }
    fn connect(&self, user: String) -> BrowserFuture<'_, ()> {
        Box::pin(async move {
            self.control(BrowserHostOperation::Connect, Some(user))
                .await?;
            self.connected.store(true, Ordering::Release);
            Ok(())
        })
    }
    fn disconnect(&self, _user: String) -> BrowserFuture<'_, ()> {
        Box::pin(async move {
            self.control(BrowserHostOperation::Disconnect, None).await?;
            self.connected.store(false, Ordering::Release);
            Ok(())
        })
    }
    fn open_local(&self, user: String) -> BrowserFuture<'_, ()> {
        Box::pin(async move { self.control(BrowserHostOperation::Show, Some(user)).await })
    }
    fn downloads_saved(
        &self,
        user_id: String,
        target: BrowserTargetKind,
        files: Vec<talos_types::ToolOutputFile>,
    ) -> BrowserFuture<'_, ()> {
        Box::pin(async move {
            use sha2::{Digest, Sha256};
            let files = files
                .into_iter()
                .map(|file| BrowserDownloadReceipt {
                    filename: file.filename,
                    sha256: format!("{:x}", Sha256::digest(&file.bytes)),
                })
                .collect();
            match self
                .bridge
                .clone()
                .request(BrowserHostRequest {
                    generation: self.generation.clone(),
                    epoch: 0,
                    user_id,
                    target,
                    operation: BrowserHostOperation::AcknowledgeDownloads { files },
                })
                .await?
            {
                BrowserHostResponse::Done => Ok(()),
                _ => Err(TalosError::internal(
                    "Invalid browser file acknowledgement.",
                )),
            }
        })
    }
    fn release_inputs(&self) -> BrowserFuture<'_, ()> {
        Box::pin(async move {
            self.control(BrowserHostOperation::ReleaseInputs, None)
                .await
        })
    }
    fn shutdown(&self) -> BrowserFuture<'_, ()> {
        Box::pin(async move {
            if !self.bridge.available() {
                return Ok(());
            }
            self.control(BrowserHostOperation::Shutdown, None).await
        })
    }
}
fn unavailable(message: &'static str) -> TalosError {
    TalosError::new(ErrorCode::ServiceUnavailable, message)
}
