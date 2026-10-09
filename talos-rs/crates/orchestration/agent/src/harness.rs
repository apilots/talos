use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use dashmap::mapref::entry::Entry;
use dashmap::DashMap;
use talos_agent_api::AgentTurnRef;
use talos_error::TalosError;
use talos_run_control::{ActiveRunControl, CancellationPolicy, Settlement};
use tokio::sync::{watch, Notify};

mod execution;
mod input;
mod maintenance;
mod port;
mod session_cache;
mod settlement;

use settlement::cleanup_task_count;

use crate::{
    event_sink::AgentEventPipeline, provider::SessionTurnInputSender, AgentCapabilities,
    AgentProvider, AgentResult, AgentSessionContext,
};

const RUN_COMPLETION_TIMEOUT: Duration = Duration::from_secs(5);
const RUN_CANCELLATION_POLICY: CancellationPolicy = CancellationPolicy {
    grace: Duration::from_secs(5),
};

#[derive(Clone)]
struct ActiveInvocation {
    identity: AgentTurnRef,
    control: Arc<ActiveRunControl>,
    input: SessionTurnInputSender,
    pipeline: AgentEventPipeline,
    input_failure: watch::Sender<Option<TalosError>>,
}

#[derive(Clone)]
enum ActiveOperation {
    Reserved {
        identity: AgentTurnRef,
        control: Arc<ActiveRunControl>,
    },
    Running(ActiveInvocation),
    CleanupPending {
        identity: AgentTurnRef,
        control: Arc<ActiveRunControl>,
        task_count: u32,
    },
    Maintenance {
        operation_id: String,
        control: Arc<ActiveRunControl>,
    },
    MaintenanceCleanupPending {
        control: Arc<ActiveRunControl>,
    },
}

#[derive(Debug, Clone, Copy)]
enum SessionOperationKind {
    Clear,
    Close,
}

struct SessionOperationOutcome {
    result: AgentResult<()>,
    settlement: Settlement,
}

struct CachedSession<S> {
    context: AgentSessionContext,
    session: Arc<S>,
    control: Arc<ActiveRunControl>,
}

impl<S> Clone for CachedSession<S> {
    fn clone(&self) -> Self {
        Self {
            context: self.context.clone(),
            session: self.session.clone(),
            control: self.control.clone(),
        }
    }
}

struct HarnessInner<P: AgentProvider> {
    agent_id: String,
    capabilities: AgentCapabilities,
    provider: P,
    accepting: AtomicBool,
    active: DashMap<String, ActiveOperation>,
    active_changed: Notify,
    sessions: DashMap<String, CachedSession<P::Session>>,
}

/// Provider-neutral Harness host composed with one concrete Agent Provider.
///
/// This host owns live admission, event settlement, input delivery, and exact
/// Turn cancellation. It must not acquire product queues, stores, or complete
/// Session aggregates.
#[derive(Clone)]
pub(crate) struct Harness<P: AgentProvider> {
    inner: Arc<HarnessInner<P>>,
}

impl<P> Harness<P>
where
    P: AgentProvider,
{
    /// Create one Harness host for an immutable Agent Definition binding.
    pub(crate) fn new(agent_id: String, provider: P) -> Self {
        let capabilities = provider.capabilities().clone();
        Self {
            inner: Arc::new(HarnessInner {
                agent_id,
                capabilities,
                provider,
                accepting: AtomicBool::new(true),
                active: DashMap::new(),
                active_changed: Notify::new(),
                sessions: DashMap::new(),
            }),
        }
    }

    fn validate_agent_id(&self, agent_id: &str) -> AgentResult<()> {
        if agent_id == self.inner.agent_id {
            return Ok(());
        }
        Err(TalosError::new(
            talos_error::ErrorCode::InvalidRequest,
            "AgentContext is bound to another Agent Harness",
        ))
    }

    fn reserve(&self, identity: &AgentTurnRef, control: Arc<ActiveRunControl>) -> AgentResult<()> {
        if !self.inner.accepting.load(Ordering::Acquire) {
            return Err(TalosError::new(
                talos_error::ErrorCode::InternalError,
                "Agent Harness is shutting down",
            ));
        }
        match self.inner.active.entry(identity.session_id.clone()) {
            Entry::Occupied(_) => Err(TalosError::new(
                talos_error::ErrorCode::ActiveTurnRunning,
                format!(
                    "session `{}` already has an active Agent operation",
                    identity.session_id
                ),
            )),
            Entry::Vacant(entry) => {
                entry.insert(ActiveOperation::Reserved {
                    identity: identity.clone(),
                    control: control.clone(),
                });
                if !self.inner.accepting.load(Ordering::Acquire)
                    && self.inner.active.get(&identity.session_id).is_some_and(|active| {
                        matches!(
                            active.value(),
                            ActiveOperation::Reserved {
                                identity: active_identity,
                                control: active_control,
                            } if active_identity == identity && Arc::ptr_eq(active_control, &control)
                        )
                    })
                {
                    self.inner.active.remove(&identity.session_id);
                    self.inner.active_changed.notify_waiters();
                    return Err(TalosError::new(talos_error::ErrorCode::InternalError, "Agent Harness is shutting down"));
                }
                Ok(())
            }
        }
    }

    fn activate(&self, invocation: ActiveInvocation) -> AgentResult<()> {
        match self
            .inner
            .active
            .entry(invocation.identity.session_id.clone())
        {
            Entry::Occupied(mut entry) => match entry.get() {
                ActiveOperation::Reserved { identity, .. } if identity == &invocation.identity => {
                    entry.insert(ActiveOperation::Running(invocation));
                    Ok(())
                }
                _ => Err(TalosError::new(
                    talos_error::ErrorCode::Conflict,
                    "Agent run reservation changed before Provider start",
                )),
            },
            Entry::Vacant(_) => Err(TalosError::new(
                talos_error::ErrorCode::ResourceNotFound,
                "Agent run reservation disappeared before Provider start",
            )),
        }
    }

    fn remove_exact(&self, identity: &AgentTurnRef) {
        if self
            .inner
            .active
            .remove_if(&identity.session_id, |_, active| match active {
                ActiveOperation::Reserved {
                    identity: current, ..
                } => current == identity,
                ActiveOperation::Running(invocation) => invocation.identity == *identity,
                ActiveOperation::CleanupPending { .. }
                | ActiveOperation::Maintenance { .. }
                | ActiveOperation::MaintenanceCleanupPending { .. } => false,
            })
            .is_some()
        {
            self.inner.active_changed.notify_waiters();
        }
    }

    fn mark_cleanup_pending(
        &self,
        identity: &AgentTurnRef,
        control: Arc<ActiveRunControl>,
        settlement: &talos_run_control::Settlement,
    ) {
        if let Entry::Occupied(mut entry) = self.inner.active.entry(identity.session_id.clone()) {
            entry.insert(ActiveOperation::CleanupPending {
                identity: identity.clone(),
                control,
                task_count: cleanup_task_count(settlement),
            });
            self.inner.active_changed.notify_waiters();
        }
    }
}

#[cfg(test)]
#[path = "harness_execution_tests.rs"]
mod execution_tests;
#[cfg(test)]
#[path = "harness_management_tests.rs"]
mod management_tests;
#[cfg(test)]
#[path = "harness/test_support.rs"]
mod test_support;

#[cfg(test)]
#[path = "rpc_tests.rs"]
mod rpc_tests;
