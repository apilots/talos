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
        talos_agent::AgentResult<Vec<talos_agent::SessionHistoryReadTurn>>,
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
    use crate::execution_history::tests::{admit_turn, scope};
    use crate::rpc_host::tests::UnusedFiles;
    use talos_agent::{SessionHistoryReadParams, SessionHistoryWindow};

    #[tokio::test]
    async fn bound_sources_resolve_their_identity_and_share_evolving_worker_history() {
        let directory = tempfile::tempdir().unwrap();
        let store = Arc::new(
            talos_store::execution::WorkerExecutionStore::open(
                &directory.path().join("worker.sqlite"),
            )
            .await
            .unwrap(),
        );
        let context = scope();
        admit_turn(&store, &context, 0, 16, true).await;
        let bindings = ResourceBindings::new(Arc::new(
            crate::execution_history::WorkerExecutionHistory(store.clone()),
        ));
        *bindings.local.write().unwrap() = Some(Arc::new(|_| {
            Box::pin(async {
                Ok(TalosWorkerResources {
                    attachment_reader: Arc::new(UnusedFiles),
                    tool_image_store: Arc::new(UnusedFiles),
                    agent_control: None,
                    automation_control: None,
                })
            })
        }));
        let resolve = bindings.resolver(Arc::new(talos_tools::ToolRegistry::default()));
        let read = SessionHistoryReadParams {
            session_id: context.session_id.clone(),
            window: SessionHistoryWindow::Latest,
            limit: 10,
            message_limit: 64,
        };
        let embedded_guard = bindings
            .bind(
                &NodeAuth {
                    node_id: "embedded".into(),
                    token: "embedded-boundary".into(),
                },
                &context,
                None,
            )
            .await
            .unwrap();
        let embedded = resolve(&context).unwrap().unwrap();
        assert_eq!(
            embedded.session_history_reader.source_node_id().as_deref(),
            Some("embedded")
        );
        let initial = embedded
            .session_history_reader
            .read(read.clone())
            .await
            .unwrap();
        assert_eq!(initial.len(), 1);
        assert_eq!(
            initial[0].turn.identity,
            talos_agent_api::AgentTurnRef::new(&context.session_id, "turn-0")
        );
        assert_eq!(initial[0].turn.completed_event_seq, 4);
        let talos_agent_api::AgentRunResult::Completed(output) = &initial[0].turn.result else {
            panic!("expected the seeded completed Turn")
        };
        assert_eq!(
            output
                .messages
                .iter()
                .map(|message| message.message_id.as_str())
                .collect::<Vec<_>>(),
            ["message-0", "tool-message-0"]
        );
        assert_eq!(
            output.messages,
            store
                .checkpoint(&initial[0].turn.identity)
                .await
                .unwrap()
                .unwrap()
                .messages
        );
        embedded
            .memory_progress_store
            .advance_processed_until_event_seq(&context.session_id, 0, 4)
            .await
            .unwrap();
        drop(embedded_guard);
        assert!(resolve(&context).unwrap().is_none());

        let registered_guard = bindings
            .bind(
                &NodeAuth {
                    node_id: "registered-worker".into(),
                    token: "registered-boundary".into(),
                },
                &context,
                None,
            )
            .await
            .unwrap();
        let registered = resolve(&context).unwrap().unwrap();
        assert_eq!(
            registered
                .session_history_reader
                .source_node_id()
                .as_deref(),
            Some("registered-worker")
        );
        assert_eq!(
            registered
                .session_history_reader
                .read(read.clone())
                .await
                .unwrap(),
            initial
        );
        assert_eq!(
            registered
                .memory_progress_store
                .processed_until_event_seq(&context.session_id)
                .await
                .unwrap(),
            4
        );
        admit_turn(&store, &context, 1, 24, true).await;
        let evolved = registered
            .session_history_reader
            .read(read.clone())
            .await
            .unwrap();
        assert_eq!(evolved.len(), 2);
        assert_eq!(
            evolved[1].turn.identity,
            talos_agent_api::AgentTurnRef::new(&context.session_id, "turn-1")
        );
        assert_eq!(evolved[1].turn.completed_event_seq, 8);
        registered
            .memory_progress_store
            .advance_processed_until_event_seq(&context.session_id, 4, 8)
            .await
            .unwrap();
        assert_eq!(
            embedded.session_history_reader.read(read).await.unwrap(),
            evolved
        );
        assert_eq!(
            embedded
                .memory_progress_store
                .processed_until_event_seq(&context.session_id)
                .await
                .unwrap(),
            8
        );
        assert_eq!(
            embedded.session_history_reader.source_node_id().as_deref(),
            Some("embedded")
        );
        drop(registered_guard);
        assert!(resolve(&context).unwrap().is_none());
    }
}
