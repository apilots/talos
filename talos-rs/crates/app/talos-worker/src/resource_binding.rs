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
#[derive(Clone)]
struct BoundResources {
    invocation_id: String,
    node_id: String,
    product: TalosWorkerResources,
}

struct InvocationHistory {
    history: Arc<crate::execution_history::WorkerExecutionHistory>,
    node_id: String,
}
impl talos_agent::SessionHistoryReader for InvocationHistory {
    fn source_node_id(&self) -> Option<String> {
        Some(self.node_id.clone())
    }
    fn read(
        &self,
        params: talos_agent::SessionHistoryReadParams,
    ) -> talos_agent::AgentFuture<
        '_,
        talos_agent::AgentResult<Vec<talos_agent_api::SessionHistoryTurn>>,
    > {
        self.history.read(params)
    }
    fn append_supplement(
        &self,
        supplement: talos_agent_api::trajectory::SessionHistorySupplement,
    ) -> talos_agent::AgentFuture<'_, talos_agent::AgentResult<()>> {
        self.history.append_supplement(supplement)
    }
}

type LocalResolver = Arc<
    dyn Fn(
            AgentSessionContext,
        ) -> talos_agent::AgentFuture<'static, TalosResult<TalosWorkerResources>>
        + Send
        + Sync,
>;
#[derive(Clone)]
pub(crate) struct ResourceBindings {
    history: Arc<crate::execution_history::WorkerExecutionHistory>,
    active: Arc<RwLock<HashMap<Key, BoundResources>>>,
    local: Arc<RwLock<Option<LocalResolver>>>,
    remote: Arc<RwLock<Option<AgentSessionDataClient>>>,
}
impl ResourceBindings {
    pub(crate) fn new(history: Arc<crate::execution_history::WorkerExecutionHistory>) -> Self {
        Self {
            history,
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
                .cloned();
            Ok(resources.map(|resources| KernelExecutionResources {
                session_history_reader: Arc::new(InvocationHistory {
                    history: bindings.history.clone(),
                    node_id: resources.node_id,
                }),
                memory_progress_store: bindings.history.clone(),
                attachment_reader: resources.product.attachment_reader,
                tool_executor: Arc::new(ResourceTools {
                    registry: tools.clone(),
                    images: resources.product.tool_image_store,
                    control: resources.product.agent_control,
                    automation: resources.product.automation_control,
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
        let resources = if let Some(local) = local {
            local(context.clone()).await?
        } else {
            let remote = self
                .remote
                .read()
                .expect("Worker resource lock poisoned")
                .clone()
                .ok_or_else(|| {
                    TalosError::new(
                        ErrorCode::ServiceUnavailable,
                        "Worker product resources are unavailable.",
                    )
                })?;
            let client = remote.for_invocation(auth.clone(), context, caller.cloned())?;
            client_resources(client)
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
        active.insert(
            key.clone(),
            BoundResources {
                invocation_id: id.clone(),
                node_id: auth.node_id.clone(),
                product: resources,
            },
        );
        Ok(Box::new(ResourceGuard {
            active: self.active.clone(),
            key,
            id,
        }))
    }
}
struct ResourceGuard {
    active: Arc<RwLock<HashMap<Key, BoundResources>>>,
    key: Key,
    id: String,
}
impl Drop for ResourceGuard {
    fn drop(&mut self) {
        let mut active = self.active.write().expect("Worker resource lock poisoned");
        if active
            .get(&self.key)
            .is_some_and(|resources| resources.invocation_id == self.id)
        {
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
        let namespace = self._execution_namespace.clone();
        Arc::new(move |auth, session, caller| {
            let resources = resources.clone();
            let namespace = namespace.clone();
            let (auth, session, caller) = (auth.clone(), session.clone(), caller.cloned());
            Box::pin(async move {
                let resources = resources.bind(&auth, &session, caller.as_ref()).await?;
                Ok(Box::new((resources, namespace)) as Box<dyn Send>)
            })
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
        attachment_reader: shared.clone(),
        tool_image_store: shared.clone(),
        agent_control: Some(shared.clone()),
        automation_control: Some(shared),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use talos_agent::SessionHistoryReader;

    #[tokio::test]
    async fn invocation_history_keeps_each_authenticated_source_identity_on_one_store() {
        let directory = tempfile::tempdir().unwrap();
        let store = Arc::new(
            talos_store::execution::WorkerExecutionStore::open(
                &directory.path().join("worker.sqlite"),
            )
            .await
            .unwrap(),
        );
        let history = Arc::new(crate::execution_history::WorkerExecutionHistory(store));
        let embedded = InvocationHistory {
            history: history.clone(),
            node_id: "embedded".into(),
        };
        let registered = InvocationHistory {
            history,
            node_id: "registered-worker".into(),
        };
        assert_eq!(embedded.source_node_id().as_deref(), Some("embedded"));
        assert_eq!(
            registered.source_node_id().as_deref(),
            Some("registered-worker")
        );
        assert!(Arc::ptr_eq(&embedded.history, &registered.history));
    }
}
