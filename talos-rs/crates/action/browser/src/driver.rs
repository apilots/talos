//! Fixed-operation subprocess adapter. Model values never select scripts, endpoints or filesystem paths.
use crate::process::{BrowserProcess, ProcessCleanup};
use crate::BrowserActionExt;
use crate::{proxy::InvocationProxy, BrowserAction, BrowserDispatch};
use serde::Deserialize;
use std::sync::Arc;
use std::{collections::HashMap, path::PathBuf, process::Stdio};
use talos_error::{ErrorCode, TalosError, TalosResult};
use talos_types::{
    BrowserHostElement as Element, BrowserHostPage as Page, McpCallToolResponse, McpContent,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    process::Command,
};

#[derive(Clone)]
pub(crate) struct Driver {
    pub python: PathBuf,
    pub script: PathBuf,
    pub downloads: PathBuf,
    pub pressed: crate::pressed::PressedInputs,
    pub cleanup: Arc<ProcessCleanup>,
}
#[derive(Default)]
pub(crate) struct DriverState {
    target: Option<String>,
    owner: Option<(String, String, String, u64)>,
    elements: HashMap<String, Element>,
}
#[derive(Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "camelCase")]
enum ResultEnvelope {
    Success(Observation),
    Failure { reason: FailureReason },
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
enum FailureReason {
    StaleElement,
    OperationFailed,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Observation {
    page: Page,
    #[serde(default)]
    loader_id: Option<String>,
    elements: Vec<Element>,
    #[serde(default)]
    screenshot: Option<String>,
}

impl Driver {
    pub async fn execute(
        &self,
        endpoint: String,
        action: BrowserAction,
        dispatch: BrowserDispatch,
        state: &mut DriverState,
    ) -> TalosResult<McpCallToolResponse> {
        if matches!(action, BrowserAction::Download) {
            dispatch.stage_downloads(crate::files::completed_downloads(&self.downloads)?)?;
            return Ok(McpCallToolResponse {
                content: Vec::new(),
                is_error: false,
                structured_content: None,
            });
        }
        let upload = if let BrowserAction::Upload { file_id, .. } = &action {
            Some(crate::files::prepare_upload(
                &self.downloads,
                &dispatch.upload(file_id)?,
            )?)
        } else {
            None
        };
        let identity = dispatch.identity()?;
        let owner = (
            identity.user_id,
            identity.session_id,
            identity.turn_id,
            dispatch.epoch()?,
        );
        if state.owner.as_ref() != Some(&owner) {
            state.owner = Some(owner);
            state.elements.clear();
        }
        let bound = match &action {
            BrowserAction::Click { element }
            | BrowserAction::Fill { element, .. }
            | BrowserAction::Upload { element, .. } => {
                Some(state.elements.get(element).cloned().ok_or_else(stale)?)
            }
            _ => None,
        };
        let proxy = InvocationProxy::start(
            endpoint,
            dispatch.clone(),
            action.is_read_only(),
            self.pressed.clone(),
        )
        .await?;
        let request = serde_json::json!({"action":action,"cdp":proxy.url,"targetId":state.target,"element":bound,"downloads":self.downloads,"uploadPath":upload.as_ref().map(|(_,path)|path)});
        let mut command = Command::new(&self.python);
        command
            .arg("-I")
            .arg(&self.script)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .env_clear()
            .env("PATH", "/usr/local/bin:/usr/bin:/bin")
            .env("ANONYMIZED_TELEMETRY", "false")
            .env("BH_UPDATE_CHECK", "false")
            .env("BROWSER_USE_LOGGING_LEVEL", "error")
            .env("PYTHONUNBUFFERED", "1");
        let mut child = BrowserProcess::spawn(command, self.cleanup.clone())?;
        let mut stdin = child
            .inner_mut()
            .stdin()
            .take()
            .ok_or_else(|| TalosError::internal("Browser driver stdin is missing."))?;
        let output = child
            .inner_mut()
            .stdout()
            .take()
            .ok_or_else(|| TalosError::internal("Browser driver stdout is missing."))?;
        let mut bytes = serde_json::to_vec(&request).map_err(|error| {
            TalosError::internal_with_source("Failed to encode browser operation.", error)
        })?;
        if bytes.len() > 64 * 1024 {
            return Err(TalosError::new(
                ErrorCode::InvalidRequest,
                "Browser invocation exceeds its private transport limit.",
            ));
        }
        bytes.push(b'\n');
        stdin.write_all(&bytes).await.map_err(|error| {
            TalosError::internal_with_source("Failed to dispatch browser operation.", error)
        })?;
        drop(stdin);
        let mut bytes = Vec::new();
        output
            .take(6 * 1024 * 1024 + 1)
            .read_to_end(&mut bytes)
            .await
            .map_err(|error| {
                TalosError::internal_with_source("Failed to receive browser result.", error)
            })?;
        if bytes.len() > 6 * 1024 * 1024 {
            return Err(TalosError::new(
                ErrorCode::ToolExecutionFailed,
                "Browser result exceeds its transport limit.",
            ));
        }
        let status = child.wait().await.map_err(|error| {
            TalosError::internal_with_source("Browser driver exit could not be observed.", error)
        })?;
        if !status.success() {
            return Err(TalosError::new(
                ErrorCode::ToolExecutionFailed,
                "The browser driver exited unexpectedly.",
            ));
        }
        dispatch.check()?;
        let result: ResultEnvelope = serde_json::from_slice(&bytes).map_err(|error| {
            TalosError::internal_with_source("Browser driver returned an invalid result.", error)
        })?;
        let value = match result {
            ResultEnvelope::Success(value) => value,
            ResultEnvelope::Failure {
                reason: FailureReason::StaleElement,
            } => return Err(stale()),
            ResultEnvelope::Failure {
                reason: FailureReason::OperationFailed,
            } => {
                return Err(TalosError::new(
                    ErrorCode::ToolExecutionFailed,
                    "Browser operation failed. Inspect the current page before retrying.",
                ))
            }
        };
        if value.elements.len() > 150 {
            return Err(TalosError::internal(
                "Browser driver returned too many elements.",
            ));
        }
        if value.page.tabs.len() > 32 || value.page.target_id.is_empty() {
            return Err(TalosError::internal(
                "Browser driver returned invalid page state.",
            ));
        }
        state.target = Some(value.page.target_id.clone());
        let mut payload = serde_json::to_value(value.page).map_err(|error| {
            TalosError::internal_with_source("Failed to encode browser page state.", error)
        })?;
        payload["documentRevision"] = serde_json::json!(value.loader_id);
        if matches!(action, BrowserAction::State) {
            state.elements.clear();
            let elements=value.elements.into_iter().map(|element|{
                let reference=uuid::Uuid::new_v4().to_string();
                let info=serde_json::json!({"element":reference,"tag":element.tag,"attributes":element.attributes});
                state.elements.insert(reference,element);info
            }).collect::<Vec<_>>();
            payload["elements"] = serde_json::json!(elements);
        } else if !action.is_read_only() {
            state.elements.clear();
        }
        let mut content = vec![McpContent::Text {
            text: serde_json::to_string(&payload).map_err(|error| {
                TalosError::internal_with_source("Failed to encode browser page state.", error)
            })?,
        }];
        if let Some(data) = value.screenshot {
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
    }
}
fn stale() -> TalosError {
    TalosError::new(
        ErrorCode::Conflict,
        "This element reference is stale. Run browser_state and use a fresh reference.",
    )
}
