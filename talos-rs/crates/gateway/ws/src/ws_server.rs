use std::collections::HashMap;
use std::sync::Arc;

use futures_util::{SinkExt, StreamExt};
use log::{debug, error, info};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{mpsc, RwLock};
use tokio_tungstenite::{accept_async, tungstenite::Error as WsError};
use uuid::Uuid;

use talos_error::{TalosError, TalosResult};
use talos_gateway::{Gateway, GatewayRequest, TalosInputItem};
use talos_types::talos::TalosContext;

// WebSocket连接状态枚举
#[derive(Debug, Clone)]
pub enum WsConnectionStatus {
    Connected,
    Disconnected,
}

// WebSocket消息类型
#[derive(Debug, Clone)]
pub enum WsMessage {
    Text(String),
    Binary(Vec<u8>),
    Ping(Vec<u8>),
    Pong(Vec<u8>),
    Close(Option<(u16, String)>),
}

impl From<String> for WsMessage {
    fn from(s: String) -> Self {
        WsMessage::Text(s)
    }
}

impl From<&str> for WsMessage {
    fn from(s: &str) -> Self {
        WsMessage::Text(s.to_string())
    }
}

// WebSocket连接句柄
#[derive(Clone)]
pub struct WsConnection {
    id: String,
    sender: mpsc::UnboundedSender<WsMessage>,
    status: Arc<RwLock<WsConnectionStatus>>,
}

impl WsConnection {
    pub fn new(id: String, sender: mpsc::UnboundedSender<WsMessage>) -> Self {
        Self {
            id,
            sender,
            status: Arc::new(RwLock::new(WsConnectionStatus::Connected)),
        }
    }

    pub async fn send_message(&self, message: WsMessage) -> Result<(), WsError> {
        self.sender.send(message).map_err(|e| {
            WsError::Io(std::io::Error::new(
                std::io::ErrorKind::BrokenPipe,
                e.to_string(),
            ))
        })?;
        Ok(())
    }

    pub async fn close(&self) -> Result<(), WsError> {
        let _ = self.sender.send(WsMessage::Close(None));
        *self.status.write().await = WsConnectionStatus::Disconnected;
        Ok(())
    }

    pub async fn id(&self) -> String {
        self.id.clone()
    }

    pub async fn status(&self) -> WsConnectionStatus {
        self.status.read().await.clone()
    }
}

// WebSocket服务器
pub struct WsServer {
    gateway: Arc<dyn Gateway>,
    port: u16,
    connections: Arc<RwLock<HashMap<String, WsConnection>>>,
}

impl WsServer {
    pub fn new(gateway: Arc<dyn Gateway>, port: u16) -> Self {
        Self {
            gateway,
            port,
            connections: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn run(&self) -> TalosResult<()> {
        let addr = format!("0.0.0.0:{}", self.port);
        let listener = TcpListener::bind(&addr).await.map_err(|e| {
            TalosError::GatewayError(format!("Failed to bind to address {}: {}", addr, e))
        })?;

        info!("WebSocket server started on ws://{}", addr);

        loop {
            match listener.accept().await {
                Ok((stream, _)) => {
                    let gateway = self.gateway.clone();
                    let connections = self.connections.clone();
                    let _peer_addr = stream.peer_addr().ok();

                    tokio::spawn(async move {
                        if let Err(e) = Self::handle_connection(stream, gateway, connections).await
                        {
                            error!("Error handling WebSocket connection: {:?}", e);
                        }
                    });
                }
                Err(e) => {
                    error!("Error accepting connection: {}", e);
                }
            }
        }
    }

    async fn handle_connection(
        stream: TcpStream,
        gateway: Arc<dyn Gateway>,
        connections: Arc<RwLock<HashMap<String, WsConnection>>>,
    ) -> Result<(), WsError> {
        let ws_stream = accept_async(stream).await?;
        info!("New WebSocket connection established");

        // 生成唯一连接ID
        let conn_id = Uuid::new_v4().to_string();

        // 创建消息通道
        let (msg_tx, mut msg_rx) = mpsc::unbounded_channel::<WsMessage>();

        // 创建连接对象
        let connection = WsConnection::new(conn_id.clone(), msg_tx.clone());

        // 注册连接
        {
            let mut conns = connections.write().await;
            conns.insert(conn_id.clone(), connection.clone());
        }

        info!("WebSocket connection {} registered", conn_id);

        // 将WebSocket流分割为发送和接收部分
        let (ws_sender, mut ws_receiver) = ws_stream.split();

        // 消息发送任务 - 从内部队列获取消息并发送到WebSocket
        let send_task = {
            let mut ws_sender_clone = ws_sender;
            tokio::spawn(async move {
                while let Some(msg) = msg_rx.recv().await {
                    let ws_msg = match msg {
                        WsMessage::Text(text) => {
                            tokio_tungstenite::tungstenite::Message::Text(text)
                        }
                        WsMessage::Binary(data) => {
                            tokio_tungstenite::tungstenite::Message::Binary(data)
                        }
                        WsMessage::Ping(data) => {
                            tokio_tungstenite::tungstenite::Message::Ping(data)
                        }
                        WsMessage::Pong(data) => {
                            tokio_tungstenite::tungstenite::Message::Pong(data)
                        }
                        WsMessage::Close(opt_frame) => {
                            if let Some((code, reason)) = opt_frame {
                                tokio_tungstenite::tungstenite::Message::Close(Some(
                                    tokio_tungstenite::tungstenite::protocol::CloseFrame {
                                        code: code.into(),
                                        reason: reason.into(),
                                    },
                                ))
                            } else {
                                tokio_tungstenite::tungstenite::Message::Close(None)
                            }
                        }
                    };

                    if let Err(_) = ws_sender_clone.send(ws_msg).await {
                        break;
                    }
                }
            })
        };

        // 消息接收任务 - 从WebSocket接收消息并处理
        let recv_task = {
            let gateway_clone = gateway.clone();
            let conn_id_clone = conn_id.clone();
            let connections_clone = connections.clone();
            let msg_tx_clone = msg_tx.clone(); // 用于发送PONG消息

            tokio::spawn(async move {
                while let Some(result) = ws_receiver.next().await {
                    match result {
                        Ok(msg) => {
                            match msg {
                                // 文本消息
                                tokio_tungstenite::tungstenite::Message::Text(text) => {
                                    debug!(
                                        "Received text message from {}: {}",
                                        conn_id_clone, text
                                    );

                                    // 将消息转发给网关处理
                                    let request = GatewayRequest {
                                        session_id: format!("ws-session-{}", conn_id_clone),
                                        workspace_id: None,
                                        input: vec![TalosInputItem::Text { text: text.clone() }],
                                        context: TalosContext::ws(conn_id_clone.clone())
                                            .with_channel_id(format!("ws_{}", conn_id_clone)),
                                    };

                                    if let Err(e) = gateway_clone.handle_request(request).await {
                                        error!("Error handling WebSocket message: {:?}", e);
                                        break;
                                    }

                                    // 广播给其他连接
                                    Self::broadcast_to_others(
                                        &connections_clone,
                                        &conn_id_clone,
                                        WsMessage::Text(text),
                                    )
                                    .await;
                                }
                                // 二进制消息
                                tokio_tungstenite::tungstenite::Message::Binary(data) => {
                                    debug!("Received binary message from {}", conn_id_clone);

                                    // 将二进制消息转换为字符串并转发给网关处理
                                    let text_content =
                                        format!("binary_data_{}", String::from_utf8_lossy(&data));
                                    let request = GatewayRequest {
                                        session_id: format!("ws-binary-session-{}", conn_id_clone),
                                        workspace_id: None,
                                        input: vec![TalosInputItem::Text { text: text_content }],
                                        context: TalosContext::ws(conn_id_clone.clone())
                                            .with_channel_id(format!(
                                                "ws_{}_binary",
                                                conn_id_clone
                                            )),
                                    };

                                    if let Err(e) = gateway_clone.handle_request(request).await {
                                        error!("Error handling WebSocket binary message: {:?}", e);
                                        break;
                                    }

                                    // 广播给其他连接
                                    Self::broadcast_to_others(
                                        &connections_clone,
                                        &conn_id_clone,
                                        WsMessage::Binary(data),
                                    )
                                    .await;
                                }
                                // PING消息 - 自动响应PONG
                                tokio_tungstenite::tungstenite::Message::Ping(data) => {
                                    debug!("Received ping from {}", conn_id_clone);
                                    // 发送PONG响应
                                    if let Err(_) = msg_tx_clone.send(WsMessage::Pong(data)) {
                                        break;
                                    }
                                }
                                // PONG消息
                                tokio_tungstenite::tungstenite::Message::Pong(_) => {
                                    debug!("Received pong from {}", conn_id_clone);
                                }
                                // CLOSE消息
                                tokio_tungstenite::tungstenite::Message::Close(_) => {
                                    debug!("Received close frame from {}", conn_id_clone);
                                    break;
                                }
                                // 保留消息
                                tokio_tungstenite::tungstenite::Message::Frame(_) => {
                                    // 不应该到达这里，因为tungstenite会自动处理Frame消息
                                }
                            }
                        }
                        Err(e) => {
                            error!("WebSocket error for connection {}: {:?}", conn_id_clone, e);
                            break;
                        }
                    }
                }

                // 连接断开后清理
                {
                    let mut conns = connections_clone.write().await;
                    conns.remove(&conn_id_clone);
                }

                info!("WebSocket connection {} closed", conn_id_clone);
            })
        };

        // 等待任务完成
        tokio::try_join!(send_task, recv_task).map_err(|e| {
            WsError::Io(std::io::Error::new(
                std::io::ErrorKind::Other,
                e.to_string(),
            ))
        })?;

        Ok(())
    }

    async fn broadcast_to_others(
        connections: &Arc<RwLock<HashMap<String, WsConnection>>>,
        exclude_id: &str,
        message: WsMessage,
    ) {
        let conns = connections.read().await;
        for (id, conn) in conns.iter() {
            if id != exclude_id {
                if let Err(e) = conn.send_message(message.clone()).await {
                    error!("Error broadcasting message to {}: {:?}", id, e);
                }
            }
        }
    }

    pub async fn get_connection(&self, conn_id: &str) -> Option<WsConnection> {
        let conns = self.connections.read().await;
        conns.get(conn_id).cloned()
    }

    pub async fn get_connections_count(&self) -> usize {
        let conns = self.connections.read().await;
        conns.len()
    }

    pub async fn broadcast_message(&self, message: WsMessage) -> usize {
        let conns = self.connections.read().await;
        let mut sent_count = 0;

        for (id, conn) in conns.iter() {
            if let Err(e) = conn.send_message(message.clone()).await {
                error!("Error broadcasting message to {}: {:?}", id, e);
            } else {
                sent_count += 1;
            }
        }

        sent_count
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use talos_gateway::{Gateway, GatewayResponse};
    use talos_types::talos::{
        TalosEvent, TalosEventType, TalosInputItem, TalosOutputItem, TalosReasoning, TalosUsage,
    };

    struct MockGateway;

    #[async_trait::async_trait]
    impl Gateway for MockGateway {
        async fn handle_request(&self, request: GatewayRequest) -> TalosResult<GatewayResponse> {
            let text = extract_text_input(&request.input);
            println!("Mock gateway received: {}", text);
            Ok(GatewayResponse {
                content: format!("Echo: {}", text),
                output_items: vec![TalosOutputItem::OutputText {
                    text: format!("Echo: {}", text),
                }],
                reasoning: vec![TalosReasoning {
                    text: "mock reasoning".to_string(),
                }],
                stop_reason: "end_turn".to_string(),
                usage: TalosUsage::default(),
                success: true,
                model: "mock-model".to_string(),
                session_id: "session-mock".to_string(),
            })
        }

        async fn handle_request_stream(
            &self,
            request: GatewayRequest,
        ) -> TalosResult<tokio::sync::mpsc::Receiver<TalosEvent>> {
            let response = self.handle_request(request).await?;
            let (tx, rx) = tokio::sync::mpsc::channel(8);
            tokio::spawn(async move {
                let _ = tx
                    .send(TalosEvent {
                        event_type: TalosEventType::MessageStarted,
                        message_id: "msg_mock".to_string(),
                        session_id: response.session_id.clone(),
                        sequence: 0,
                        data: serde_json::json!({"model": response.model}),
                    })
                    .await;
            });
            Ok(rx)
        }

        async fn start(&self) -> TalosResult<()> {
            Ok(())
        }
    }

    fn extract_text_input(input: &[TalosInputItem]) -> String {
        input
            .iter()
            .filter_map(|item| match item {
                TalosInputItem::Text { text } => Some(text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[tokio::test]
    async fn test_ws_server_creation() {
        let gateway = Arc::new(MockGateway);
        let server = WsServer::new(gateway, 0);

        assert_eq!(server.get_connections_count().await, 0);
    }

    #[tokio::test]
    async fn test_websocket_connection() {
        let gateway = Arc::new(MockGateway);
        let server = WsServer::new(gateway, 8080);

        // Note: We can't fully test the server running since it would block
        // This test focuses on the connection management methods

        // Test connection count
        assert_eq!(server.get_connections_count().await, 0);
    }
}
