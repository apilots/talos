//! Per-call authorization, ephemeral file bindings and input dispatch fences.
use super::*;
use sha2::{Digest, Sha256};
use tokio::sync::OwnedMutexGuard;

impl BrowserRuntime {
    pub fn authorize(
        self: &Arc<Self>,
        identity: BrowserInvocationIdentity,
        action: &BrowserAction,
    ) -> TalosResult<BrowserInvocationLease> {
        if self.shutdown.is_cancelled() {
            return Err(unavailable("The browser runtime is shutting down."));
        }
        if !(self.enabled)()? {
            return Err(unavailable(
                "Browser is disabled. Enable it in MCP settings.",
            ));
        }
        require_private_ipc_support()?;
        if [
            &identity.user_id,
            &identity.session_id,
            &identity.turn_id,
            &identity.call_id,
        ]
        .iter()
        .any(|id| id.is_empty() || id.len() > 256)
        {
            return Err(TalosError::new(
                ErrorCode::PermissionDenied,
                "A trusted browser invocation identity is required.",
            ));
        }
        let mut s = self.state.lock().map_err(|_| poisoned())?;
        if s.revoked_users.contains(&identity.user_id) {
            return Err(denied());
        }
        if s.host == BrowserHostKind::Server {
            return Err(unavailable("Connect a browser-capable Computer first."));
        }
        if !action.is_read_only()
            && (s.preferences.manual_hold
                || matches!(s.owner, BrowserControlOwner::Suspended { .. }))
        {
            return Err(conflict(
                "Browser automation is paused. Explicitly resume it first.",
            ));
        }
        match &s.owner {
            BrowserControlOwner::Agent {
                session_id,
                turn_id,
            } if session_id != &identity.session_id
                || turn_id != &identity.turn_id
                || s.agent_user.as_ref() != Some(&identity.user_id) =>
            {
                return Err(conflict("Another task owns this browser."))
            }
            BrowserControlOwner::Human { .. } | BrowserControlOwner::HandingOver { .. }
                if !action.is_read_only() =>
            {
                return Err(conflict("The browser is under human control."))
            }
            _ => {}
        }
        if s.invocations.len() >= 16 {
            return Err(conflict("Browser invocation capacity is exhausted."));
        }
        if !action.is_read_only() && matches!(s.owner, BrowserControlOwner::Idle) {
            s.agent_user = Some(identity.user_id.clone());
            s.owner = BrowserControlOwner::Agent {
                session_id: identity.session_id.clone(),
                turn_id: identity.turn_id.clone(),
            };
            s.revision += 1;
        }
        let token = Uuid::new_v4().to_string();
        let record = Invocation {
            identity,
            digest: digest(action)?,
            epoch: s.epoch,
            target: s.preferences.selected,
            deadline: Instant::now() + Duration::from_secs(120),
            upload: None,
            downloads: Vec::new(),
            executing: false,
            cancellation: CancellationToken::new(),
        };
        s.invocations.insert(token.clone(), record);
        Ok(BrowserInvocationLease {
            runtime: self.clone(),
            token,
        })
    }

    pub fn attach_upload(&self, token: &str, file: talos_types::ToolInputFile) -> TalosResult<()> {
        let mut state = self.state.lock().map_err(|_| poisoned())?;
        let call = state.invocations.get_mut(token).ok_or_else(denied)?;
        if file.bytes.len() > 20 * 1024 * 1024 {
            return Err(TalosError::new(
                ErrorCode::UploadTooLarge,
                "Browser uploads are limited to 20 MiB.",
            ));
        }
        if call.executing || file.filename.is_empty() {
            return Err(denied());
        }
        call.upload = Some(file);
        Ok(())
    }
    pub fn take_downloads(&self, token: &str) -> TalosResult<Vec<talos_types::ToolOutputFile>> {
        let mut state = self.state.lock().map_err(|_| poisoned())?;
        let call = state.invocations.get_mut(token).ok_or_else(denied)?;
        Ok(std::mem::take(&mut call.downloads))
    }
    pub async fn downloads_saved(
        &self,
        token: &str,
        files: Vec<talos_types::ToolOutputFile>,
    ) -> TalosResult<()> {
        let (target, user) = {
            let state = self.state.lock().map_err(|_| poisoned())?;
            let call = state.invocations.get(token).ok_or_else(denied)?;
            if call.epoch != state.epoch || call.cancellation.is_cancelled() {
                return Err(denied());
            }
            (call.target, call.identity.user_id.clone())
        };
        self.backend()?.downloads_saved(user, target, files).await
    }
    pub(crate) async fn invoke(
        self: &Arc<Self>,
        token: &str,
        action: BrowserAction,
    ) -> TalosResult<McpCallToolResponse> {
        {
            let mut s = self.state.lock().map_err(|_| poisoned())?;
            let epoch = s.epoch;
            let record = s.invocations.get_mut(token).ok_or_else(denied)?;
            if record.executing
                || record.epoch != epoch
                || record.deadline <= Instant::now()
                || record.digest != digest(&action)?
            {
                return Err(denied());
            }
            record.executing = true;
        }
        let _mutation = (!action.is_read_only()).then(|| self.mutations.token());
        let dispatch = BrowserDispatch {
            runtime: self.clone(),
            token: token.into(),
        };
        dispatch.check()?;
        let cancellation = dispatch.cancellation()?;
        let backend = self.backend()?;
        tokio::select! {
            _=cancellation.cancelled()=>Err(TalosError::new(ErrorCode::Cancelled,"Browser invocation was cancelled.")),
            _=self.shutdown.cancelled()=>Err(unavailable("The browser runtime is shutting down.")),
            result=tokio::time::timeout(Duration::from_secs(120),backend.execute(action,dispatch))=>
                result.map_err(|error| TalosError::from_source(ErrorCode::Timeout,"Browser invocation timed out.",error))?,
        }
    }
}

pub struct BrowserInvocationLease {
    runtime: Arc<BrowserRuntime>,
    token: String,
}
impl std::fmt::Debug for BrowserInvocationLease {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("BrowserInvocationLease")
            .finish_non_exhaustive()
    }
}
impl BrowserInvocationLease {
    pub fn token(&self) -> &str {
        &self.token
    }
}
impl Drop for BrowserInvocationLease {
    fn drop(&mut self) {
        if let Ok(mut s) = self.runtime.state.lock() {
            if let Some(r) = s.invocations.remove(&self.token) {
                r.cancellation.cancel();
            }
        }
    }
}
#[derive(Clone)]
pub struct BrowserDispatch {
    runtime: Arc<BrowserRuntime>,
    token: String,
}
impl BrowserDispatch {
    pub fn upload(&self, file_id: &str) -> TalosResult<talos_types::ToolInputFile> {
        self.check()?;
        self.runtime
            .state
            .lock()
            .map_err(|_| poisoned())?
            .invocations
            .get(&self.token)
            .and_then(|call| call.upload.as_ref())
            .filter(|file| file.file_id == file_id)
            .cloned()
            .ok_or_else(denied)
    }
    pub fn stage_downloads(&self, files: Vec<talos_types::ToolOutputFile>) -> TalosResult<()> {
        self.check()?;
        if files.len() > 5
            || files.iter().map(|file| file.bytes.len()).sum::<usize>() > 20 * 1024 * 1024
        {
            return Err(TalosError::new(
                ErrorCode::UploadTooLarge,
                "Browser downloads exceeded their limit.",
            ));
        }
        let mut state = self.runtime.state.lock().map_err(|_| poisoned())?;
        let call = state.invocations.get_mut(&self.token).ok_or_else(denied)?;
        call.downloads = files;
        Ok(())
    }
    pub fn check(&self) -> TalosResult<()> {
        if !(self.runtime.enabled)()? {
            return Err(denied());
        }
        let s = self.runtime.state.lock().map_err(|_| poisoned())?;
        let r = s.invocations.get(&self.token).ok_or_else(denied)?;
        if r.deadline <= Instant::now() || r.epoch != s.epoch || r.cancellation.is_cancelled() {
            return Err(denied());
        }
        Ok(())
    }
    pub fn epoch(&self) -> TalosResult<u64> {
        self.check()?;
        self.runtime
            .state
            .lock()
            .map_err(|_| poisoned())?
            .invocations
            .get(&self.token)
            .map(|record| record.epoch)
            .ok_or_else(denied)
    }

    pub fn target(&self) -> TalosResult<BrowserTargetKind> {
        self.check()?;
        self.runtime
            .state
            .lock()
            .map_err(|_| poisoned())?
            .invocations
            .get(&self.token)
            .map(|r| r.target)
            .ok_or_else(denied)
    }
    pub fn identity(&self) -> TalosResult<BrowserInvocationIdentity> {
        self.check()?;
        self.runtime
            .state
            .lock()
            .map_err(|_| poisoned())?
            .invocations
            .get(&self.token)
            .map(|r| r.identity.clone())
            .ok_or_else(denied)
    }
    pub fn cancellation(&self) -> TalosResult<CancellationToken> {
        self.runtime
            .state
            .lock()
            .map_err(|_| poisoned())?
            .invocations
            .get(&self.token)
            .map(|r| r.cancellation.clone())
            .ok_or_else(denied)
    }
    /// Hold this guard across each actual transport send, not the whole action.
    pub async fn input_permit(&self) -> TalosResult<OwnedMutexGuard<()>> {
        let guard = self.runtime.input.clone().lock_owned().await;
        self.check()?;
        Ok(guard)
    }
}
fn digest(action: &BrowserAction) -> TalosResult<[u8; 32]> {
    Ok(Sha256::digest(serde_json::to_vec(action).map_err(|error| {
        TalosError::internal_with_source("Failed to bind browser arguments.", error)
    })?)
    .into())
}
