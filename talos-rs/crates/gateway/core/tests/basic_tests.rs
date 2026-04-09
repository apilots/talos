#[cfg(test)]
mod basic_tests {
    use async_trait::async_trait;
    use std::sync::Arc;
    use talos_error::TalosError;
    use talos_error::TalosResult;
    use talos_gateway::{Gateway, GatewayImpl, GatewayRequest};
    use talos_types::talos::{
        StopReason, TalosContext, TalosInputItem, TalosOutputItem, TalosProvider, TalosReasoning,
        TalosSourceType, TalosUsage,
    };
    use talos_types::RequestHandler;
    use tokio;
    use tokio::sync::mpsc;

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
                    output_items: vec![TalosOutputItem::OutputText {
                        text: "Success response".to_string(),
                    }],
                    reasoning: vec![TalosReasoning {
                        text: "mock reasoning".to_string(),
                    }],
                    stop_reason: StopReason::EndTurn,
                    usage: TalosUsage::default(),
                    success: true,
                    model: "test-model".to_string(),
                    session_id: "test-session".to_string(),
                })
            }
        }

        async fn handle_stream(
            &self,
            request: talos_types::gateway::GatewayRequest,
        ) -> TalosResult<mpsc::Receiver<talos_types::talos::TalosEvent>> {
            let response = self.handle(request).await?;
            let (tx, rx) = mpsc::channel(1);
            tokio::spawn(async move {
                let _ = tx
                    .send(talos_types::talos::TalosEvent {
                        event_type: talos_types::talos::TalosEventType::MessageStarted,
                        message_id: "msg-mock".to_string(),
                        session_id: response.session_id,
                        sequence: 0,
                        payload: talos_types::talos::TalosEventPayload::MessageStarted(
                            talos_types::talos::TalosMessageStartedEvent {
                                request_id: "req-mock".to_string(),
                                model: "test-model".to_string(),
                                usage: TalosUsage::default(),
                                iteration: 0,
                                step_id: "start".to_string(),
                            },
                        ),
                    })
                    .await;
            });
            Ok(rx)
        }

        async fn cancel_active_turn(&self, _session_id: &str) -> TalosResult<()> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn test_gateway_handle_request_success() {
        let mock_handler = MockHandler {
            should_error: false,
        };
        let gateway_impl = GatewayImpl::new(Arc::new(mock_handler));
        let request = GatewayRequest {
            session_id: "test-session".to_string(),
            workspace_id: None,
            input: vec![TalosInputItem::Text {
                text: "test content".to_string(),
            }],
            context: TalosContext {
                source_type: TalosSourceType::Http,
                provider: TalosProvider::Http,
                user_id: "test_user".to_string(),
                message_id: "message-1".to_string(),
                trace_id: "trace-1".to_string(),
                channel_id: Some("test_channel".to_string()),
                thread_id: None,
                environment: None,
            },
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
            session_id: "test-session".to_string(),
            workspace_id: None,
            input: vec![TalosInputItem::Text {
                text: "test content".to_string(),
            }],
            context: TalosContext {
                source_type: TalosSourceType::Http,
                provider: TalosProvider::Http,
                user_id: "test_user".to_string(),
                message_id: "message-1".to_string(),
                trace_id: "trace-1".to_string(),
                channel_id: Some("test_channel".to_string()),
                thread_id: None,
                environment: None,
            },
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
