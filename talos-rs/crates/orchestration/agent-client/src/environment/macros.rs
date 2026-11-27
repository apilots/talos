//! Typed environment RPC expansion and boxed protobuf carrier handling.

macro_rules! result_value {
    (boxed, $value:expr) => {
        *$value
    };
    ($value:expr) => {
        $value
    };
}
macro_rules! invoke {
    ($client:expr, $method:ident, $call:ident, $request:expr, $result:ident, $variant:ident, $decode:ident $(, $boxed:ident)?) => {{
        if $client.is_closed() {
            return Err(crate::connection_writer::closed());
        }
        let mut request = Request::new($request);
        let timeout = match stringify!($method) {
            "add_plugin_source"
            | "refresh_plugin_source"
            | "search_plugins"
            | "start_mcp_authentication"
            | "complete_mcp_authentication" => 60,
            "install_plugin" | "update_plugin" => 300,
            "prepare_plugin_change" => 600,
            _ => 10,
        };
        request.set_timeout(Duration::from_secs(timeout));
        let response = match &$client.transport {
            HarnessTransport::Direct(channel) => {
                pb::worker_environment_rpc_client::WorkerEnvironmentRpcClient::new(channel.clone())
                    .$method(request)
                    .await
                    .map_err(crate::decode_rpc_status)?
                    .into_inner()
            }
            HarnessTransport::Outbound(connection) => {
                let mut stream = connection
                    .invoke(
                        tokio_stream::once(pb::worker_rpc_call::Request::$call(Box::new(
                            request.into_inner(),
                        ))),
                        timeout * 1_000,
                        RequestMode::Unary,
                    )
                    .await
                    .map_err(crate::decode_rpc_status)?;
                let reply = stream
                    .message()
                    .await
                    .map_err(crate::decode_rpc_status)?
                    .ok_or_else(wire::invalid)?;
                let pb::worker_rpc_result::Response::$call(response) = reply else {
                    return Err(wire::invalid());
                };
                if stream
                    .message()
                    .await
                    .map_err(crate::decode_rpc_status)?
                    .is_some()
                {
                    return Err(wire::invalid());
                }
                *response
            }
        };
        match response.result.ok_or_else(wire::invalid)? {
            pb::$result::Result::$variant(value) => wire::$decode(result_value!($($boxed,)? value)),
            pb::$result::Result::Failure(failure) => Err(error_pb::decode_failure(result_value!($($boxed,)? failure))?),
        }
    }};
}
macro_rules! environment_method {
    ($method:ident, $params:ty, $response:ty, $request:ident, $call:ident, $result:ident, $variant:ident, $encode:ident, $decode:ident $(, $boxed:ident)?) => {
        fn $method(
            &self,
            context: WorkerEnvironmentContext,
            params: $params,
        ) -> AgentFuture<'_, TalosResult<$response>> {
            Box::pin(async move {
                invoke!(
                    self,
                    $method,
                    $call,
                    pb::$request {
                        auth: Some(node_pb::encode_auth(self.auth.clone())),
                        context: Some(wire::encode_context(context)),
                        params: Some(wire::$encode(params)?),
                    },
                    $result,
                    $variant,
                    $decode $(, $boxed)?
                )
            })
        }
    };
}
