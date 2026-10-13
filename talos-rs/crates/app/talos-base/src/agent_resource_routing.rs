//! Live caller routes only; no business admission, facts or RPC replay live here.
use super::*;
use agent_client::{AgentExecutionConnection, AgentResourceRequests, AgentResourceResponses};
use std::collections::HashMap;

#[derive(Clone, Default)]
pub(super) struct ResourceRoutes(Arc<tokio::sync::Mutex<HashMap<String, CallerRoute>>>);
struct CallerRoute {
    peer_id: String,
    serves_files: bool,
    owner: String,
    computer_id: String,
    connection: std::sync::Weak<AgentExecutionConnection>,
    connected_at: tokio::time::Instant,
}
impl ResourceRoutes {
    pub(super) async fn install(
        &self,
        connection_id: String,
        peer_id: String,
        serves_files: bool,
        owner: String,
        computer_id: String,
        connection: &Arc<AgentExecutionConnection>,
    ) -> TalosResult<()> {
        let mut routes = self.0.lock().await;
        if routes
            .get(&connection_id)
            .and_then(|route| route.connection.upgrade())
            .is_some()
        {
            return Err(TalosError::new(
                ErrorCode::Conflict,
                "Coordinator connection identity is already active",
            ));
        }
        routes.insert(
            connection_id,
            CallerRoute {
                peer_id,
                serves_files,
                owner,
                computer_id,
                connection: Arc::downgrade(connection),
                connected_at: tokio::time::Instant::now(),
            },
        );
        Ok(())
    }
    pub(super) async fn remove(
        &self,
        connection_id: &str,
        connection: &Arc<AgentExecutionConnection>,
    ) {
        let mut routes = self.0.lock().await;
        if routes
            .get(connection_id)
            .is_some_and(|route| route.connection.ptr_eq(&Arc::downgrade(connection)))
        {
            routes.remove(connection_id);
        }
    }
    pub(super) async fn forward(
        &self,
        nodes: &AgentNodeService,
        auth: Option<pb::NodeAuth>,
        caller: pb::CoordinatorCallContext,
        requests: AgentResourceRequests,
    ) -> Result<AgentResourceResponses, Status> {
        let auth = node_pb::decode_auth(auth).map_err(status)?;
        let owner = nodes.authorize_lease_owner(&auth).await.map_err(status)?;
        let connection = {
            let routes = self.0.lock().await;
            let route = routes
                .get(&caller.connection_id)
                .filter(|route| route.owner == owner && route.computer_id == auth.node_id)
                .ok_or_else(|| status(unavailable()))?;
            route
                .connection
                .upgrade()
                .ok_or_else(|| status(unavailable()))?
        };
        // B's SDK checks the live call ID and original immutable Session scope.
        use futures_util::StreamExt;
        let mut requests = requests;
        let first = requests
            .next()
            .await
            .transpose()?
            .ok_or_else(|| status(unavailable()))?;
        let timeout = match &first {
            pb::worker_rpc_call::Request::WaitSubagent(request) => request
                .params
                .as_ref()
                .map_or(10_000, |params| params.timeout_ms.saturating_add(10_000)),
            _ => 10_000,
        };
        connection
            .invoke_resource(
                tokio_stream::once(Ok(first)).chain(requests),
                caller,
                timeout,
            )
            .await
    }
    pub(super) async fn forward_peer(
        &self,
        owner: &str,
        peer_id: &str,
        request: pb::worker_rpc_call::Request,
    ) -> Result<AgentResourceResponses, Status> {
        let (connection_id, connection) = {
            let routes = self.0.lock().await;
            routes
                .iter()
                .filter(|(_, route)| {
                    route.owner == owner && route.peer_id == peer_id && route.serves_files
                })
                .filter_map(|(id, route)| {
                    route
                        .connection
                        .upgrade()
                        .map(|connection| (route.connected_at, id.clone(), connection))
                })
                .max_by_key(|(connected_at, _, _)| *connected_at)
                .map(|(_, id, connection)| (id, connection))
                .ok_or_else(|| status(unavailable()))?
        };
        let caller = pb::CoordinatorCallContext {
            coordinator_id: String::new(),
            connection_id,
            rpc_call_id: 0,
            session: None,
            turn_id: None,
            project_id: None,
        };
        connection
            .invoke_resource(tokio_stream::once(Ok(request)), caller, 30_000)
            .await
    }
}
fn unavailable() -> TalosError {
    TalosError::new(
        ErrorCode::ServiceUnavailable,
        "Current coordinator resource route is unavailable",
    )
}
fn status(error: TalosError) -> Status {
    talos_logging::report_error(&error);
    error_pb::encode_status(&error)
}
