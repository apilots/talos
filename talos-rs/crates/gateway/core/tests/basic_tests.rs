#[cfg(test)]
mod basic_tests {
    use async_trait::async_trait;
    use std::sync::Arc;
    use talos_error::TalosError;
    use talos_error::TalosResult;
    use talos_gateway::{Gateway, GatewayImpl, GatewayRequest};
    use talos_types::RequestHandler;
    use tokio;

    struct MockHandler {
        should_error: bool,
    }

    #[async_trait]
    impl RequestHandler for MockHandler {
        async fn handle(
            &self,
            _request: talos_types::gateway::GatewayRequest,
        ) -> TalosResult<talos_types::gateway::GatewayResponse> {
            if self.should_error {
                Err(TalosError::InternalError("Test error".to_string()))
            } else {
                Ok(talos_types::gateway::GatewayResponse {
                    content: "Success response".to_string(),
                    output_items: vec![talos_types::canonical::CanonicalOutputItem::OutputText {
                        text: "Success response".to_string(),
                    }],
                    stop_reason: "end_turn".to_string(),
                    usage: talos_types::canonical::CanonicalUsage::default(),
                    success: true,
                    model: "test-model".to_string(),
                    session_id: Some("test-session".to_string()),
                })
            }
        }
    }

    #[tokio::test]
    async fn test_gateway_handle_request_success() {
        let mock_handler = MockHandler {
            should_error: false,
        };
        let gateway_impl = GatewayImpl::new(Arc::new(mock_handler));
        let request = GatewayRequest {
            channel: "test_channel".to_string(),
            user_id: "test_user".to_string(),
            content: "test content".to_string(),
            model: Some("test-model".to_string()),
            session_id: None,
            context: None,
            metadata: None,
        };

        let response = gateway_impl.handle_request(request).await.unwrap();
        assert_eq!(response.content, "Success response");
        assert_eq!(response.success, true);
    }

    #[tokio::test]
    async fn test_gateway_handle_request_error() {
        let mock_handler = MockHandler { should_error: true };
        let gateway_impl = GatewayImpl::new(Arc::new(mock_handler));
        let request = GatewayRequest {
            channel: "test_channel".to_string(),
            user_id: "test_user".to_string(),
            content: "test content".to_string(),
            model: Some("test-model".to_string()),
            session_id: None,
            context: None,
            metadata: None,
        };

        let result = gateway_impl.handle_request(request).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_gateway_start() {
        let mock_handler = MockHandler {
            should_error: false,
        };
        let gateway_impl = GatewayImpl::new(Arc::new(mock_handler));

        let result = gateway_impl.start().await;
        assert!(result.is_ok());
    }
}
