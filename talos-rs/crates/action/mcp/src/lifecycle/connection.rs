use super::*;

impl McpConnection {
    async fn cancelled(&self, caller: impl Future<Output = ()> + Send) {
        tokio::select! { _ = caller => {}, _ = self.server.abort_calls.cancelled() => {} }
    }

    pub fn tools(&self) -> &[McpToolDescriptor] {
        &self.tools
    }
    pub fn name(&self) -> &str {
        &self.server.descriptor.server.name
    }

    pub fn supports_resources(&self) -> bool {
        self.client
            .peer
            .peer_info()
            .is_some_and(|info| info.capabilities.resources.is_some())
    }

    pub async fn list_resources(
        &self,
        cursor: Option<String>,
        cancellation: impl Future<Output = ()> + Send,
    ) -> TalosResult<talos_types::McpResourceListResponse> {
        let _call = self.admit()?;
        protocol::list_resources(
            self.name(),
            &self.client.peer,
            cursor,
            self.cancelled(cancellation),
        )
        .await
    }

    pub async fn list_resource_templates(
        &self,
        cursor: Option<String>,
        cancellation: impl Future<Output = ()> + Send,
    ) -> TalosResult<talos_types::McpResourceTemplateListResponse> {
        let _call = self.admit()?;
        protocol::list_resource_templates(
            self.name(),
            &self.client.peer,
            cursor,
            self.cancelled(cancellation),
        )
        .await
    }

    fn admit(&self) -> TalosResult<TaskTrackerToken> {
        let state = self.server.state.read().expect("MCP server lock poisoned");
        if self.server.retire.is_cancelled() {
            return Err(not_ready());
        }
        match &*state {
            State::Ready { tools, .. } if Arc::ptr_eq(tools, &self.tools) => {
                Ok(self.server.calls.token())
            }
            _ => Err(not_ready()),
        }
    }

    pub async fn call_tool(
        &self,
        request: McpCallToolRequest,
        cancellation: impl Future<Output = ()> + Send,
    ) -> TalosResult<McpCallToolResponse> {
        let _call = self.admit()?;
        if request.server_name != self.name()
            || !self.tools.iter().any(|tool| tool.name == request.tool_name)
        {
            return Err(not_ready());
        }
        self.client
            .call_tool(request, self.cancelled(cancellation))
            .await
    }

    pub async fn read_resource(
        &self,
        request: McpReadResourceRequest,
        cancellation: impl Future<Output = ()> + Send,
    ) -> TalosResult<McpReadResourceResponse> {
        let _call = self.admit()?;
        if request.server_name != self.name() {
            return Err(not_ready());
        }
        self.client
            .read_resource(request, self.cancelled(cancellation))
            .await
    }
}
