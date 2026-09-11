use super::*;
use crate::{cleanup::Cleanup, DEFAULT_STARTUP_TIMEOUT};

impl Server {
    pub(super) async fn run(self: Arc<Self>, slots: Arc<Semaphore>, cleanup: Arc<Cleanup>) {
        if self.descriptor.server.credentials.is_some() && self.credentials.is_none() {
            return;
        }
        let slot = tokio::select! {
            _ = self.retire.cancelled() => return,
            slot = slots.acquire_owned() => match slot { Ok(slot) => slot, Err(_) => return },
        };
        *self.state.write().expect("MCP server lock poisoned") = State::Starting;
        let (credential_failure, receiver) = tokio::sync::oneshot::channel();
        let mut store_failure = Some(receiver);
        let connected = tokio::select! {
            biased;
            error = crate::oauth::store_failure(&mut store_failure) => Err(error),
            _ = self.retire.cancelled() => {
                if let Some(error) = crate::oauth::take_store_failure(&mut store_failure) {
                    cleanup.record(error);
                }
                return;
            },
            result = tokio::time::timeout(DEFAULT_STARTUP_TIMEOUT, async {
                let session = protocol::connect_service(&self.descriptor.server, self.credentials.as_ref(), &cleanup, credential_failure).await?;
                let subscription = if session.peer_info().is_some_and(|info| info.protocol_version == rmcp::model::ProtocolVersion::V_2026_07_28 && info.capabilities.tools.as_ref().is_some_and(|tools| tools.list_changed == Some(true))) {
                    Some(session.listen(rmcp::model::SubscriptionFilter::builder().tools_list_changed().build()).await.map_err(protocol::map_service_error)?)
                } else { None };
                let tools = protocol::list_tools(&self.descriptor.server.name, session.peer()).await?;
                Ok::<_, TalosError>((session, tools, subscription))
            }) => result.unwrap_or_else(|_| Err(TalosError::new(ErrorCode::ToolExecutionFailed, "The MCP server startup timed out."))),
        };
        drop(slot);
        let (session, tools, mut subscription) = match connected {
            Ok(connected) => connected,
            Err(error) => {
                self.fail(crate::oauth::take_store_failure(&mut store_failure).unwrap_or(error));
                return;
            }
        };
        let (service, mut process) = session.into_parts();
        let client = McpClient {
            peer: service.peer().clone(),
        };
        let changed = service.service().tools_changed.clone();
        *self.state.write().expect("MCP server lock poisoned") = State::Ready {
            client: client.clone(),
            tools: Arc::new(tools),
        };
        let cancel = service.cancellation_token();
        let waiting = service.waiting();
        tokio::pin!(waiting);
        // Discovery remains concurrent with process/transport observation. A server
        // can die during a tools/list request while a descendant holds stdout open.
        let refresh = async {
            loop {
                match &mut subscription {
                    Some(subscription) => {
                        if subscription
                            .next()
                            .await
                            .map_err(protocol::map_service_error)?
                            .is_none()
                        {
                            return Err(TalosError::new(
                                ErrorCode::ToolExecutionFailed,
                                "The MCP tool catalog subscription ended.",
                            ));
                        }
                    }
                    None => changed.notified().await,
                }
                let tools = tokio::time::timeout(
                    DEFAULT_TOOL_TIMEOUT,
                    protocol::list_tools(&self.descriptor.server.name, &client.peer),
                )
                .await
                .map_err(|_| {
                    TalosError::new(
                        ErrorCode::ToolExecutionFailed,
                        "The MCP tool catalog refresh timed out.",
                    )
                })??;
                *self.state.write().expect("MCP server lock poisoned") = State::Ready {
                    client: client.clone(),
                    tools: Arc::new(tools),
                };
            }
        };
        let mut transport_done = false;
        tokio::select! {
            biased;
            error = crate::oauth::store_failure(&mut store_failure) => self.fail(error),
            status = async { match &mut process { Some(process) => process.wait().await, None => std::future::pending().await } } => {
                match status { Ok(status) => self.exited(status), Err(error) => self.fail(error) }
            }
            _ = self.retire.cancelled() => {},
            result = &mut waiting => {
                let error = match result {
                    Ok(_) => TalosError::new(ErrorCode::ToolExecutionFailed, "The MCP connection closed unexpectedly."),
                    Err(error) => TalosError::internal_with_source("The MCP connection task failed.", error),
                };
                let error = crate::oauth::take_store_failure(&mut store_failure).unwrap_or(error);
                transport_done = true;
                match process.as_mut().map(|process| process.try_wait()) {
                    Some(Ok(Some(status))) => self.exited(status),
                    Some(Err(error)) => self.fail(error),
                    _ => self.fail(error),
                }
            }
            result = refresh => { let Err::<(), _>(error) = result else { unreachable!("catalog watch only ends on failure") }; self.fail(crate::oauth::take_store_failure(&mut store_failure).unwrap_or(error)); },
        }
        self.calls.close();
        if self.retire.is_cancelled() {
            let _ = tokio::time::timeout(DEFAULT_TOOL_TIMEOUT, self.calls.wait()).await;
        }
        cancel.cancel();
        if !transport_done {
            match tokio::time::timeout(CLEANUP_TIMEOUT, waiting).await {
                Ok(Ok(_)) => {}
                Ok(Err(error)) => cleanup.record(TalosError::from_source(
                    ErrorCode::RunCleanupUnconfirmed,
                    "MCP transport cleanup failed.",
                    error,
                )),
                Err(_) => cleanup.record(TalosError::new(
                    ErrorCode::RunCleanupUnconfirmed,
                    "MCP transport cleanup did not finish within the deadline.",
                )),
            }
        }
        if let Some(error) = crate::oauth::take_store_failure(&mut store_failure) {
            cleanup.record(error);
        }
        if let Some(process) = process {
            if let Err(error) = process.shutdown().await {
                cleanup.record(error);
            }
        }
    }
}
