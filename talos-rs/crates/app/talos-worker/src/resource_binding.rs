//! Immutable product capability handles for each accepted Worker invocation.
use crate::{resource_tools::ResourceTools, TalosWorker, TalosWorkerResources};
use agent_client::AgentSessionDataClient;
use agent_protocol::{AgentSessionContext, NodeAuth};
use std::{
    collections::HashMap,
    sync::{Arc, RwLock},
};
use talos_agent_provider_kernel::{KernelExecutionResources, KernelResourceResolver};
use talos_error::{ErrorCode, TalosError, TalosResult};

type Key = (String, String);
type LocalResolver = Arc<
    dyn Fn(
            AgentSessionContext,
        ) -> talos_agent::AgentFuture<'static, TalosResult<TalosWorkerResources>>
        + Send
        + Sync,
>;
#[derive(Clone)]
pub(crate) struct ResourceBindings {
    active: Arc<RwLock<HashMap<Key, (String, TalosWorkerResources)>>>,
    local: Arc<RwLock<Option<LocalResolver>>>,
    remote: Arc<RwLock<Option<AgentSessionDataClient>>>,
}
impl ResourceBindings {
    pub(crate) fn new() -> Self {
        Self {
            active: Arc::default(),
            local: Arc::default(),
            remote: Arc::default(),
        }
    }
    pub(crate) fn resolver(&self, tools: Arc<talos_tools::ToolRegistry>) -> KernelResourceResolver {
        let bindings = self.clone();
        Arc::new(move |context| {
            let resources = bindings
                .active
                .read()
                .expect("Worker resource lock poisoned")
                .get(&(context.agent_id.clone(), context.session_id.clone()))
                .map(|(_, resources)| resources.clone());
            Ok(resources.map(|resources| KernelExecutionResources {
                session_history_reader: resources.session_history_reader,
                memory_progress_store: resources.memory_progress_store,
                attachment_reader: resources.attachment_reader,
                tool_executor: Arc::new(ResourceTools {
                    registry: tools.clone(),
                    images: resources.tool_image_store,
                    control: resources.agent_control,
                    automation: resources.automation_control,
                }),
            }))
        })
    }
    async fn bind(
        &self,
        auth: &NodeAuth,
        context: &AgentSessionContext,
        caller: Option<&agent_pb::CoordinatorCallContext>,
    ) -> TalosResult<Box<dyn Send>> {
        let local = self
            .local
            .read()
            .expect("Worker resource lock poisoned")
            .clone();
        let resources = if let Some(caller) = caller {
            let remote = self
                .remote
                .read()
                .expect("Worker resource lock poisoned")
                .clone()
                .ok_or_else(|| {
                    TalosError::new(
                        ErrorCode::ServiceUnavailable,
                        "Worker resource Gateway is unavailable",
                    )
                })?;
            let client = remote.for_invocation(auth.clone(), context, Some(caller.clone()))?;
            client_resources(client)
        } else if let Some(local) = local {
            local(context.clone()).await?
        } else {
            // The original standalone registration owner supplies default ports
            // before its listener is published. No remote failure falls back here.
            return Ok(Box::new(()));
        };
        let key = (context.agent_id.clone(), context.session_id.clone());
        let id = uuid::Uuid::new_v4().to_string();
        let mut active = self.active.write().expect("Worker resource lock poisoned");
        if active.contains_key(&key) {
            return Err(TalosError::new(
                ErrorCode::Conflict,
                "Session resource invocation is already active",
            ));
        }
        active.insert(key.clone(), (id.clone(), resources));
        Ok(Box::new(ResourceGuard {
            active: self.active.clone(),
            key,
            id,
        }))
    }
}
struct ResourceGuard {
    active: Arc<RwLock<HashMap<Key, (String, TalosWorkerResources)>>>,
    key: Key,
    id: String,
}
impl Drop for ResourceGuard {
    fn drop(&mut self) {
        let mut active = self.active.write().expect("Worker resource lock poisoned");
        if active.get(&self.key).is_some_and(|(id, _)| id == &self.id) {
            active.remove(&self.key);
        }
    }
}
impl TalosWorker {
    /// Connect media only to the current registered ingress. Host composition may
    /// select this connector after excluding its own local reservations; no local
    /// or arbitrary endpoint fallback is performed here.
    pub fn registered_desktop_connector(&self) -> talos_browser::DesktopConnector {
        let bindings = self.resource_bindings.clone();
        Arc::new(move |request| {
            let client = bindings
                .remote
                .read()
                .map_err(|_| TalosError::internal("Worker resource state is poisoned."))
                .and_then(|client| {
                    client.clone().ok_or_else(|| {
                        TalosError::new(
                            ErrorCode::ServiceUnavailable,
                            "The registered desktop ingress is unavailable.",
                        )
                    })
                });
            Box::pin(async move { client?.open_desktop_media(request).await })
        })
    }

    pub(crate) fn resource_binder(&self) -> talos_agent::AgentInvocationResourceBinder {
        let resources = self.resource_bindings.clone();
        Arc::new(move |auth, session, caller| {
            let resources = resources.clone();
            let (auth, session, caller) = (auth.clone(), session.clone(), caller.cloned());
            Box::pin(async move { resources.bind(&auth, &session, caller.as_ref()).await })
        })
    }
    /// Install the original product ports with their persisted invocation-scope
    /// check. Local history and callbacks never cross a transport boundary.
    pub fn use_local_resources(
        &self,
        resolver: impl Fn(
                AgentSessionContext,
            ) -> talos_agent::AgentFuture<'static, TalosResult<TalosWorkerResources>>
            + Send
            + Sync
            + 'static,
    ) {
        *self
            .resource_bindings
            .local
            .write()
            .expect("Worker resource lock poisoned") = Some(Arc::new(resolver));
    }
    pub(crate) fn use_remote_resource_rpc(&self, client: AgentSessionDataClient) {
        *self
            .resource_bindings
            .remote
            .write()
            .expect("Worker resource lock poisoned") = Some(client);
    }
}
fn client_resources(client: AgentSessionDataClient) -> TalosWorkerResources {
    let shared = Arc::new(client.clone());
    TalosWorkerResources {
        session_history_reader: shared.clone(),
        memory_progress_store: Arc::new(crate::standalone::RemoteMemoryProgress(client)),
        attachment_reader: shared.clone(),
        tool_image_store: shared.clone(),
        agent_control: Some(shared.clone()),
        automation_control: Some(shared),
    }
}
